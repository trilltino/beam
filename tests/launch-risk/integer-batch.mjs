import { createHash } from 'node:crypto';

// Offline, test-only fixed-price whole-lot reference. No funds, signing,
// network, token program, or production launch parameter is implemented.
const U64_MAX = (1n << 64n) - 1n;
const MAX_ORDERS = 128;
const MAX_EVENTS = MAX_ORDERS + 1; // One terminal transition plus one claim per order.
const plans = new WeakSet();

function digest(value) { return createHash('sha256').update(value).digest('hex'); }

function exactRecord(value, fields, name) {
  if (value === null || typeof value !== 'object' ||
    ![Object.prototype, null].includes(Object.getPrototypeOf(value)) ||
    Reflect.ownKeys(value).length !== fields.length) throw new TypeError(`${name} shape`);
  for (const field of fields) {
    const descriptor = Object.getOwnPropertyDescriptor(value, field);
    if (!descriptor || !Object.hasOwn(descriptor, 'value') || !descriptor.enumerable) {
      throw new TypeError(`${name}.${field} must be an own data field`);
    }
  }
}

function denseArray(value, name, maximum = MAX_ORDERS) {
  if (!Array.isArray(value) || Object.getPrototypeOf(value) !== Array.prototype ||
    value.length > maximum || Reflect.ownKeys(value).length !== value.length + 1) {
    throw new TypeError(`${name} must be a bounded dense array`);
  }
  for (let index = 0; index < value.length; index += 1) {
    const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
    if (!descriptor || !Object.hasOwn(descriptor, 'value') || !descriptor.enumerable) {
      throw new TypeError(`${name} must contain only data elements`);
    }
  }
}

function u64(value, name, positive = false) {
  if (typeof value !== 'bigint' || value < 0n || value > U64_MAX ||
    (positive && value === 0n)) throw new TypeError(`${name} must be a ${positive ? 'positive ' : ''}u64 bigint`);
  return value;
}

function decimal(value, name) {
  if (typeof value !== 'string' || value.length > 20 ||
    !/^(0|[1-9][0-9]*)$/.test(value)) throw new TypeError(`${name} must be canonical u64 decimal`);
  const result = BigInt(value);
  if (result > U64_MAX) throw new RangeError(`${name} exceeds u64`);
  return result;
}

function label(value, name) {
  if (typeof value !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(value)) {
    throw new TypeError(`${name} must be a bounded test label`);
  }
  return value;
}

function immutableRows(rows) { return Object.freeze(rows.map((row) => Object.freeze(row))); }

/**
 * Demand is floor(budget/lotPrice). Oversubscribed demand receives
 * floor(capacity*demand/totalDemand) lots. Remaining lots are unassigned.
 * For the same total budget and other orders, splitting cannot increase
 * demand or the attacker's combined floored lot allocation.
 */
export function planIntegerBatch(input) {
  exactRecord(input, ['batchId', 'tokenCapLots', 'lotSizeTokenUnits',
    'lotPriceQuoteUnits', 'openSlot', 'closeSlot', 'settlementDeadlineSlot',
    'orders'], 'batch');
  const batchId = label(input.batchId, 'batchId');
  const cap = u64(input.tokenCapLots, 'tokenCapLots', true);
  const lotSize = u64(input.lotSizeTokenUnits, 'lotSizeTokenUnits', true);
  const price = u64(input.lotPriceQuoteUnits, 'lotPriceQuoteUnits', true);
  if (cap * lotSize > U64_MAX) throw new RangeError('token cap units exceed u64');
  const open = u64(input.openSlot, 'openSlot');
  const close = u64(input.closeSlot, 'closeSlot');
  const deadline = u64(input.settlementDeadlineSlot, 'settlementDeadlineSlot');
  if (!(open < close && close < deadline)) throw new RangeError('invalid deadline order');
  denseArray(input.orders, 'orders');
  if (input.orders.length === 0) throw new TypeError('orders cannot be empty');
  const ids = new Set();
  const checked = input.orders.map((order, index) => {
    exactRecord(order, ['id', 'budgetQuoteUnits'], `orders[${index}]`);
    const id = label(order.id, `orders[${index}].id`);
    if (ids.has(id)) throw new TypeError('duplicate order');
    ids.add(id);
    const budget = u64(order.budgetQuoteUnits, `orders[${index}].budgetQuoteUnits`, true);
    return { id, budget, demand: budget / price };
  }).sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  const totalBudget = checked.reduce((sum, order) => sum + order.budget, 0n);
  const totalDemand = checked.reduce((sum, order) => sum + order.demand, 0n);
  const oversubscribed = totalDemand > cap;
  const rows = immutableRows(checked.map(({ id, budget, demand }) => {
    const allocated = oversubscribed ? cap * demand / totalDemand : demand;
    const spend = allocated * price;
    if (spend > budget) throw new RangeError('spend exceeds budget');
    return {
      id, budgetQuoteUnits: budget.toString(), demandLots: demand.toString(),
      allocatedLots: allocated.toString(), tokenUnits: (allocated * lotSize).toString(),
      quoteSpentUnits: spend.toString(), quoteRefundUnits: (budget - spend).toString(),
    };
  }));
  const allocated = rows.reduce((sum, row) => sum + BigInt(row.allocatedLots), 0n);
  const spent = rows.reduce((sum, row) => sum + BigInt(row.quoteSpentUnits), 0n);
  const unsoldDemandGap = totalDemand < cap ? cap - totalDemand : 0n;
  const plan = Object.freeze({
    kind: 'disabled-exact-integer-batch-research', executable: false,
    batchId, tokenCapLots: cap.toString(), lotSizeTokenUnits: lotSize.toString(),
    lotPriceQuoteUnits: price.toString(),
    openSlot: open.toString(), closeSlot: close.toString(),
    settlementDeadlineSlot: deadline.toString(),
    totalBudgetQuoteUnits: totalBudget.toString(), totalDemandLots: totalDemand.toString(),
    oversubscribed, rows,
    allocatedLots: allocated.toString(), unallocatedLots: (cap - allocated).toString(),
    unsoldDemandGapLots: unsoldDemandGap.toString(),
    prorataRoundingLots: (cap - allocated - unsoldDemandGap).toString(),
    quoteSpentUnits: spent.toString(),
    quoteRefundUnits: (totalBudget - spent).toString(),
  });
  plans.add(plan);
  return plan;
}

function planHash(plan) {
  if (!plans.has(plan)) throw new TypeError('plan must be built by this module');
  return digest(JSON.stringify(plan));
}

function genesis(plan) { return digest(`beam-integer-batch-v1:${planHash(plan)}`); }

export function genesisJournal(plan) {
  const head = genesis(plan);
  return Object.freeze({ planHash: planHash(plan), head, events: Object.freeze([]) });
}

function applyEvent(plan, state, kind, slot, id) {
  if (slot < state.asOfSlot) throw new RangeError('slot moved backward');
  const close = BigInt(plan.closeSlot);
  const deadline = BigInt(plan.settlementDeadlineSlot);
  if (kind === 'settle') {
    if (state.phase !== 'open' || slot < close || slot > deadline) throw new RangeError('invalid settlement window');
    return { phase: 'settled', asOfSlot: slot, claimed: state.claimed };
  }
  if (kind === 'cancel') {
    if (state.phase !== 'open' || slot <= deadline) throw new RangeError('invalid cancellation window');
    return { phase: 'cancelled', asOfSlot: slot, claimed: state.claimed };
  }
  if (kind !== 'claim' || !['settled', 'cancelled'].includes(state.phase)) {
    throw new RangeError('claim requires a terminal batch');
  }
  if (!plan.rows.some((row) => row.id === id)) throw new RangeError('unknown order');
  if (state.claimed.has(id)) throw new RangeError('duplicate claim');
  return { phase: state.phase, asOfSlot: slot, claimed: new Set([...state.claimed, id]) };
}

function eventHash(previous, seq, kind, slot, id) {
  return digest(JSON.stringify([previous, seq, kind, slot, id]));
}

/** Replay a JSON-serializable journal; hash chaining checks consistency, not authenticity. */
export function replayJournal(plan, journal) {
  exactRecord(journal, ['planHash', 'head', 'events'], 'journal');
  if (journal.planHash !== planHash(plan)) throw new RangeError('plan digest mismatch');
  denseArray(journal.events, 'events', MAX_EVENTS);
  let head = genesis(plan);
  let state = { phase: 'open', asOfSlot: BigInt(plan.openSlot), claimed: new Set() };
  for (let index = 0; index < journal.events.length; index += 1) {
    const event = journal.events[index];
    exactRecord(event, ['seq', 'kind', 'slot', 'id', 'previous', 'hash'], `events[${index}]`);
    if (event.seq !== index + 1 || event.previous !== head ||
      !['settle', 'cancel', 'claim'].includes(event.kind)) throw new RangeError('journal sequence or previous head mismatch');
    const slot = decimal(event.slot, 'event.slot');
    const id = event.kind === 'claim' ? label(event.id, 'event.id') : null;
    if (event.id !== id || event.hash !== eventHash(head, event.seq, event.kind, event.slot, id)) {
      throw new RangeError('journal event hash or shape mismatch');
    }
    state = applyEvent(plan, state, event.kind, slot, id);
    head = event.hash;
  }
  if (journal.head !== head) throw new RangeError('journal head mismatch');
  return Object.freeze({ phase: state.phase, asOfSlot: state.asOfSlot.toString(),
    claimed: Object.freeze([...state.claimed].sort()), head });
}

/** In-memory compare-and-swap model. A real store would need durable atomic CAS. */
export class ResearchJournalStore {
  #plan;
  #journal;

  constructor(plan, journal = genesisJournal(plan)) {
    replayJournal(plan, journal);
    this.#plan = plan;
    this.#journal = journal;
  }

  snapshot() { return this.#journal; }
  status() { return replayJournal(this.#plan, this.#journal); }

  commit(expectedHead, command) {
    const before = this.status();
    if (expectedHead !== before.head) throw new RangeError('stale expected head');
    if (command === null || typeof command !== 'object') throw new TypeError('invalid command');
    const kindField = Object.getOwnPropertyDescriptor(command, 'kind');
    if (!kindField || !Object.hasOwn(kindField, 'value')) throw new TypeError('invalid command kind');
    const kind = kindField.value;
    exactRecord(command, kind === 'claim' ? ['kind', 'slot', 'id'] : ['kind', 'slot'], 'command');
    if (!['settle', 'cancel', 'claim'].includes(kind)) throw new TypeError('invalid command kind');
    const slot = u64(command.slot, 'command.slot');
    const id = kind === 'claim' ? label(command.id, 'command.id') : null;
    // Validate transition before modifying the store.
    applyEvent(this.#plan, { phase: before.phase, asOfSlot: BigInt(before.asOfSlot),
      claimed: new Set(before.claimed) }, kind, slot, id);
    const seq = this.#journal.events.length + 1;
    const event = Object.freeze({ seq, kind, slot: slot.toString(), id,
      previous: before.head,
      hash: eventHash(before.head, seq, kind, slot.toString(), id) });
    const candidate = Object.freeze({ planHash: this.#journal.planHash, head: event.hash,
      events: Object.freeze([...this.#journal.events, event]) });
    replayJournal(this.#plan, candidate);
    this.#journal = candidate;
    if (kind !== 'claim') return Object.freeze({ journal: candidate, voucher: null });
    const row = this.#plan.rows.find((entry) => entry.id === id);
    const voucher = before.phase === 'cancelled'
      ? { id, tokenUnits: '0', quoteSpentUnits: '0', quoteRefundUnits: row.budgetQuoteUnits }
      : { id, tokenUnits: row.tokenUnits, quoteSpentUnits: row.quoteSpentUnits,
        quoteRefundUnits: row.quoteRefundUnits };
    return Object.freeze({ journal: candidate,
      voucher: Object.freeze({ kind: 'mock-claim-only', ...voucher }) });
  }
}
