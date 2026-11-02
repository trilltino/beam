// Deterministic research model for reserve depth and entry concentration.
// No network, wallet, transaction builder, production parameter or bot logic.
const U64_MAX = (1n << 64n) - 1n;
const U128_MAX = (1n << 128n) - 1n;

function u64(value, name, positive = false) {
  if (typeof value !== 'bigint' || value < 0n || value > U64_MAX || (positive && value === 0n)) {
    throw new TypeError(`${name} must be ${positive ? 'a positive' : 'a'} u64 bigint`);
  }
  return value;
}

function checkedU64(value, name) {
  if (value > U64_MAX) throw new RangeError(`${name} exceeds u64`);
  return value;
}

export function curveState({ virtualToken, virtualQuote, realToken, realQuote }) {
  const state = {
    virtualToken: u64(virtualToken, 'virtualToken', true),
    virtualQuote: u64(virtualQuote, 'virtualQuote', true),
    realToken: u64(realToken, 'realToken'),
    realQuote: u64(realQuote, 'realQuote'),
  };
  if (state.realToken > state.virtualToken || state.realQuote > state.virtualQuote) {
    throw new RangeError('real reserves cannot exceed corresponding virtual reserves');
  }
  return Object.freeze(state);
}

// Mirrors the retained pure curve quote arithmetic, excluding every fee.
export function buyExactTokens(state, amount) {
  amount = u64(amount, 'amount', true);
  if (amount > state.realToken || amount >= state.virtualToken) {
    throw new RangeError('buy exceeds real tokens or virtual denominator');
  }
  const product = amount * state.virtualQuote;
  if (product > U128_MAX) throw new RangeError('buy quote exceeds u128');
  const quoteIn = checkedU64(product / (state.virtualToken - amount) + 1n, 'buy quote');
  return Object.freeze({ quoteIn, state: curveState({
    virtualToken: state.virtualToken - amount,
    virtualQuote: checkedU64(state.virtualQuote + quoteIn, 'virtual quote reserve'),
    realToken: state.realToken - amount,
    realQuote: checkedU64(state.realQuote + quoteIn, 'real quote reserve'),
  }) });
}

// Gross quote before fees. An actual holder's token balance is external input.
export function sellExactTokens(state, amount) {
  amount = u64(amount, 'amount', true);
  const virtualToken = checkedU64(state.virtualToken + amount, 'virtual token reserve');
  const realToken = checkedU64(state.realToken + amount, 'real token reserve');
  const product = amount * state.virtualQuote;
  if (product > U128_MAX) throw new RangeError('sell quote exceeds u128');
  const quoteOut = product / virtualToken;
  if (quoteOut > state.realQuote) throw new RangeError('sell exceeds real quote reserves');
  return Object.freeze({ quoteOut, state: curveState({
    virtualToken,
    virtualQuote: state.virtualQuote - quoteOut,
    realToken,
    realQuote: state.realQuote - quoteOut,
  }) });
}

// Indicative virtual spot valuation only; it is neither actual vault liquidity
// nor a quoted amount a holder could receive by selling totalSupply tokens.
export function nominalSpotCapProxy(state, totalSupply) {
  totalSupply = u64(totalSupply, 'totalSupply', true);
  if (totalSupply < state.realToken) throw new RangeError('supply cannot be below real token reserves');
  return totalSupply * state.virtualQuote / state.virtualToken;
}

// Research-only common-clearing-price baseline. Every budget has equal weight
// per unit of capital, with no per-wallet bonus or assigned rounding dust.
// It does not encode an order submission, settlement or refund mechanism.
export function proRataBatch(totalTokens, orders) {
  totalTokens = u64(totalTokens, 'totalTokens', true);
  if (!Array.isArray(orders) || orders.length === 0) throw new TypeError('orders must be nonempty');
  const seen = new Set();
  let totalBudget = 0n;
  const checked = orders.map(({ id, budget }) => {
    if (typeof id !== 'string' || id.length === 0 || seen.has(id)) throw new TypeError('distinct labels required');
    seen.add(id);
    budget = u64(budget, 'budget', true);
    totalBudget += budget;
    if (totalBudget > U128_MAX) throw new RangeError('total budget exceeds u128');
    return { id, budget };
  });
  const allocations = checked.map(({ id, budget }) => Object.freeze({
    id, budget, tokens: budget * totalTokens / totalBudget,
  })).sort((a, b) => a.id.localeCompare(b.id));
  const allocated = allocations.reduce((sum, row) => sum + row.tokens, 0n);
  return Object.freeze({
    allocations: Object.freeze(allocations),
    unallocatedDust: totalTokens - allocated,
    illustrativeCommonPrice: Object.freeze({ numerator: totalBudget, denominator: totalTokens }),
  });
}
