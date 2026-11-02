// Disabled, offline batch-allocation research. Rational claims are NOT
// executable token or quote transfers and do not define a production launch.
const U64_MAX = (1n << 64n) - 1n;
const MAX_ORDERS = 128; // Research input-work bound, not a launch parameter.
const states = new WeakSet();

function u64(value, name, positive = false) {
  if (typeof value !== 'bigint' || value < 0n || value > U64_MAX || (positive && value === 0n)) {
    throw new TypeError(`${name} must be ${positive ? 'a positive' : 'a'} u64 bigint`);
  }
  return value;
}

function ratio(numerator, denominator) {
  return Object.freeze({
    numerator: numerator.toString(), denominator: denominator.toString(),
    floor: (numerator / denominator).toString(),
  });
}

function record(value, fields, name) {
  if (value === null || typeof value !== 'object' ||
      ![Object.prototype, null].includes(Object.getPrototypeOf(value)) ||
      Reflect.ownKeys(value).length !== fields.length) {
    throw new TypeError(`${name} has an invalid shape`);
  }
  for (const field of fields) {
    const descriptor = Object.getOwnPropertyDescriptor(value, field);
    if (!descriptor || !Object.hasOwn(descriptor, 'value') || !descriptor.enumerable) {
      throw new TypeError(`${name}.${field} must be an own data field`);
    }
  }
}

function label(value, name) {
  if (typeof value !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(value)) {
    throw new TypeError(`${name} must be a bounded test label`);
  }
  return value;
}

function makeState(plan, phase, asOfSlot, claimed) {
  const state = Object.freeze({ plan, phase, asOfSlot, claimed: Object.freeze(claimed) });
  states.add(state);
  return state;
}

/**
 * Fixed-supply proportional allocation with exact rational accounting.
 * All budgets are synthetic maximum deposits. No submitted order, custody,
 * on-chain state, fee, oracle, token mint, or wallet identity is modeled.
 */
export function planBatch(input) {
  record(input, ['batchId', 'tokenCap', 'quoteTarget', 'openSlot', 'closeSlot',
    'settlementDeadlineSlot', 'orders'], 'batch');
  const batchId = label(input.batchId, 'batchId');
  const tokenCap = u64(input.tokenCap, 'tokenCap', true);
  const quoteTarget = u64(input.quoteTarget, 'quoteTarget', true);
  const openSlot = u64(input.openSlot, 'openSlot');
  const closeSlot = u64(input.closeSlot, 'closeSlot');
  const deadline = u64(input.settlementDeadlineSlot, 'settlementDeadlineSlot');
  if (!(openSlot < closeSlot && closeSlot < deadline)) throw new RangeError('invalid close/deadline order');
  if (!Array.isArray(input.orders) || Object.getPrototypeOf(input.orders) !== Array.prototype ||
      input.orders.length === 0 || input.orders.length > MAX_ORDERS ||
      Reflect.ownKeys(input.orders).length !== input.orders.length + 1) {
    throw new TypeError('orders must be a bounded dense array');
  }
  const seen = new Set();
  let totalBudget = 0n;
  const orders = input.orders.map((order, index) => {
    if (!Object.hasOwn(input.orders, index)) throw new TypeError('orders must be dense');
    record(order, ['id', 'budget'], `orders[${index}]`);
    const id = label(order.id, `orders[${index}].id`);
    if (seen.has(id)) throw new TypeError('duplicate order label');
    seen.add(id);
    const budget = u64(order.budget, `orders[${index}].budget`, true);
    totalBudget += budget;
    return { id, budget };
  }).sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0);

  const oversubscribed = totalBudget > quoteTarget;
  const denominator = oversubscribed ? totalBudget : quoteTarget;
  const collectedTarget = oversubscribed ? quoteTarget : totalBudget;
  const rows = orders.map(({ id, budget }) => Object.freeze({
    id, budget: budget.toString(),
    // The denominator is the same across all orders, so splitting a budget
    // cannot increase its exact proportional entitlement. Integer floors can
    // leave additional dust; no rule for assigning it is selected.
    tokens: ratio(tokenCap * budget, denominator),
    quoteSpend: ratio(collectedTarget * budget, totalBudget),
    quoteRefund: ratio((totalBudget - collectedTarget) * budget, totalBudget),
  }));
  const allocatedFloor = rows.reduce((sum, row) => sum + BigInt(row.tokens.floor), 0n);
  const quoteSpendFloor = rows.reduce((sum, row) => sum + BigInt(row.quoteSpend.floor), 0n);
  const quoteRefundFloor = rows.reduce((sum, row) => sum + BigInt(row.quoteRefund.floor), 0n);
  const plan = Object.freeze({
    kind: 'disabled-rational-batch-research', executable: false,
    batchId, tokenCap: tokenCap.toString(), quoteTarget: quoteTarget.toString(),
    totalBudget: totalBudget.toString(), oversubscribed,
    openSlot: openSlot.toString(), closeSlot: closeSlot.toString(),
    settlementDeadlineSlot: deadline.toString(), rows: Object.freeze(rows),
    exactRationalTokenDenominator: denominator.toString(),
    integerTokenDust: (tokenCap - allocatedFloor).toString(),
    uncollectedQuoteTarget: (quoteTarget - quoteSpendFloor).toString(),
    unresolvedQuoteRounding: (totalBudget - quoteSpendFloor - quoteRefundFloor).toString(),
  });
  return makeState(plan, 'open', openSlot, []);
}

/** Only tracks mock claim eligibility. It never executes/refunds assets. */
export function stepBatch(state, action) {
  if (!states.has(state)) throw new TypeError('state was not created by this model');
  if (action === null || typeof action !== 'object') throw new TypeError('invalid action');
  const kindField = Object.getOwnPropertyDescriptor(action, 'kind');
  if (!kindField || !Object.hasOwn(kindField, 'value')) throw new TypeError('invalid action kind');
  const kind = kindField.value;
  if (!['settle', 'cancel', 'claim'].includes(kind)) throw new TypeError('invalid action kind');
  record(action, kind === 'claim' ? ['kind', 'slot', 'id'] : ['kind', 'slot'], 'action');
  const slot = u64(action.slot, 'action.slot');
  if (slot < state.asOfSlot) throw new RangeError('slot cannot move backward');
  const closeSlot = BigInt(state.plan.closeSlot);
  const deadline = BigInt(state.plan.settlementDeadlineSlot);
  if (kind === 'settle') {
    if (state.phase !== 'open' || slot < closeSlot || slot > deadline) {
      throw new RangeError('settlement outside open close window');
    }
    return makeState(state.plan, 'settled', slot, state.claimed);
  }
  if (kind === 'cancel') {
    if (state.phase !== 'open' || slot <= deadline) throw new RangeError('cancellation requires expired open batch');
    return makeState(state.plan, 'cancelled', slot, state.claimed);
  }
  if (!['settled', 'cancelled'].includes(state.phase)) throw new RangeError('claim requires settled or cancelled batch');
  const id = label(action.id, 'action.id');
  const row = state.plan.rows.find((entry) => entry.id === id);
  if (!row) throw new RangeError('unknown order');
  if (state.claimed.includes(id)) throw new RangeError('duplicate claim');
  return Object.freeze({
    state: makeState(state.plan, state.phase, slot, [...state.claimed, id].sort()),
    // Cancellation makes the full synthetic deposit refundable. Settled
    // rational values still require an unchosen integer settlement policy.
    claim: Object.freeze(state.phase === 'cancelled'
      ? { id, tokens: ratio(0n, 1n), quoteRefund: ratio(BigInt(row.budget), 1n) }
      : { id, tokens: row.tokens, quoteRefund: row.quoteRefund }),
  });
}
