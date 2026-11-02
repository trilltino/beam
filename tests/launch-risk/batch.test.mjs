import assert from 'node:assert/strict';
import test from 'node:test';
import { planBatch, stepBatch } from './batch.mjs';
import { buyExactTokens, curveState } from './model.mjs';

const input = (orders, overrides = {}) => ({
  batchId: 'synthetic-1', tokenCap: 100n, quoteTarget: 100n,
  openSlot: 1n, closeSlot: 2n, settlementDeadlineSlot: 4n,
  orders, ...overrides,
});

function conservation(plan) {
  const rows = plan.rows;
  const sum = (field) => rows.reduce((total, row) => total + BigInt(row[field].floor), 0n);
  const budgets = rows.reduce((total, row) => total + BigInt(row.budget), 0n);
  assert.equal(sum('tokens') + BigInt(plan.integerTokenDust), BigInt(plan.tokenCap));
  assert.equal(sum('quoteSpend') + BigInt(plan.uncollectedQuoteTarget), BigInt(plan.quoteTarget));
  assert.equal(sum('quoteSpend') + sum('quoteRefund') + BigInt(plan.unresolvedQuoteRounding), budgets);
  for (const row of rows) {
    const spent = BigInt(row.quoteSpend.numerator) * BigInt(row.quoteRefund.denominator);
    const refund = BigInt(row.quoteRefund.numerator) * BigInt(row.quoteSpend.denominator);
    const budget = BigInt(row.budget) * BigInt(row.quoteSpend.denominator)
      * BigInt(row.quoteRefund.denominator);
    assert.equal(spent + refund, budget);
  }
}

test('order permutations and identical-budget ties have the same plan', () => {
  const orders = [{ id: 'z', budget: 80n }, { id: 'a', budget: 80n }];
  const a = planBatch(input(orders));
  const b = planBatch(input([...orders].reverse()));
  assert.deepEqual(a.plan, b.plan);
  assert.deepEqual(a.plan.rows.map((row) => row.id), ['a', 'z']);
  assert.deepEqual(a.plan.rows.map((row) => row.tokens.floor), ['50', '50']);
  assert.equal(a.plan.kind, 'disabled-rational-batch-research');
  assert.equal(a.plan.executable, false);
  conservation(a.plan);
});

test('oversubscription has exact rational refunds without hidden allocation', () => {
  const plan = planBatch(input([{ id: 'a', budget: 80n }, { id: 'b', budget: 80n }])).plan;
  assert.equal(plan.oversubscribed, true);
  for (const row of plan.rows) {
    assert.equal(row.tokens.floor, '50');
    assert.equal(row.quoteSpend.floor, '50');
    assert.equal(row.quoteRefund.floor, '30');
  }
  assert.equal(plan.integerTokenDust, '0');
  assert.equal(plan.uncollectedQuoteTarget, '0');
  assert.equal(plan.unresolvedQuoteRounding, '0');
  conservation(plan);
});

test('undersubscription leaves unallocated tokens and an unmet quote target', () => {
  const plan = planBatch(input([{ id: 'a', budget: 40n }, { id: 'b', budget: 40n }])).plan;
  assert.equal(plan.oversubscribed, false);
  assert.equal(plan.integerTokenDust, '20');
  assert.equal(plan.uncollectedQuoteTarget, '20');
  assert.equal(plan.unresolvedQuoteRounding, '0');
  assert.deepEqual(plan.rows.map((row) => row.quoteRefund.floor), ['0', '0']);
  conservation(plan);
});

test('split wallets preserve exact rational share but cannot gain floor tokens', () => {
  const whole = planBatch(input([{ id: 'a', budget: 6n }, { id: 'b', budget: 6n }],
    { tokenCap: 7n, quoteTarget: 10n })).plan;
  const split = planBatch(input([{ id: 'a1', budget: 3n }, { id: 'a2', budget: 3n },
    { id: 'b', budget: 6n }], { tokenCap: 7n, quoteTarget: 10n })).plan;
  const wholeA = whole.rows.find((row) => row.id === 'a');
  const splitA = split.rows.filter((row) => row.id.startsWith('a'));
  assert.equal(splitA.reduce((sum, row) => sum + BigInt(row.tokens.numerator), 0n),
    BigInt(wholeA.tokens.numerator));
  assert.equal(splitA[0].tokens.denominator, wholeA.tokens.denominator);
  assert.ok(splitA.reduce((sum, row) => sum + BigInt(row.tokens.floor), 0n)
    <= BigInt(wholeA.tokens.floor));
  assert.equal(whole.integerTokenDust, '1');
  assert.equal(split.integerTokenDust, '2');
  assert.equal(split.unresolvedQuoteRounding, '2');
  conservation(whole);
  conservation(split);
});

test('identical equal budgets remove the modeled first-order price difference only', () => {
  const start = curveState({ virtualToken: 1000n, virtualQuote: 1000n,
    realToken: 500n, realQuote: 0n });
  const first = buyExactTokens(start, 100n);
  const later = buyExactTokens(first.state, 100n);
  assert.deepEqual([first.quoteIn, later.quoteIn], [112n, 140n]);
  // The 224-unit quote target is chosen solely for this comparison. It is
  // not a Beam price, an allowed launch parameter, or an equal-budget proof
  // for the continuous curve (the later buyer needs 140 units there).
  const batch = planBatch(input([{ id: 'early', budget: 112n },
    { id: 'later', budget: 112n }], { tokenCap: 200n, quoteTarget: 224n })).plan;
  assert.deepEqual(batch.rows.map((row) => row.tokens.floor), ['100', '100']);
  assert.deepEqual(batch.rows.map((row) => row.quoteSpend.floor), ['112', '112']);
  conservation(batch);
});

test('duplicate claims fail and stale operators cannot settle a stalled batch', () => {
  const open = planBatch(input([{ id: 'a', budget: 60n }, { id: 'b', budget: 40n }]));
  assert.throws(() => stepBatch(open, { kind: 'settle', slot: 1n }), /outside/);
  assert.throws(() => stepBatch(open, { kind: 'claim', slot: 2n, id: 'a' }), /requires/);
  const settled = stepBatch(open, { kind: 'settle', slot: 2n });
  assert.equal(settled.phase, 'settled');
  const first = stepBatch(settled, { kind: 'claim', slot: 3n, id: 'a' });
  assert.equal(first.claim.tokens.floor, '60');
  assert.throws(() => stepBatch(first.state, { kind: 'claim', slot: 3n, id: 'a' }), /duplicate/);
  assert.throws(() => stepBatch(first.state, { kind: 'cancel', slot: 5n }), /expired open/);
  assert.throws(() => stepBatch(open, { kind: 'settle', slot: 5n }), /outside/);
  const cancelled = stepBatch(open, { kind: 'cancel', slot: 5n });
  assert.equal(cancelled.phase, 'cancelled');
  const refund = stepBatch(cancelled, { kind: 'claim', slot: 5n, id: 'a' });
  assert.equal(refund.claim.tokens.floor, '0');
  assert.equal(refund.claim.quoteRefund.floor, '60');
  assert.throws(() => stepBatch(refund.state, { kind: 'claim', slot: 5n, id: 'a' }), /duplicate/);
});

test('malformed and unbounded synthetic inputs fail before a plan exists', () => {
  assert.throws(() => planBatch(input([{ id: 'a', budget: 1n }, { id: 'a', budget: 2n }])), /duplicate/);
  assert.throws(() => planBatch(input([{ id: 'a', budget: 0n }])), /positive/);
  assert.throws(() => planBatch(input([{ id: 'a', budget: 1n }], { closeSlot: 4n })), /deadline/);
  assert.throws(() => planBatch(input([{ id: 'a', budget: 1n }], { tokenCap: 0n })), /positive/);
  assert.throws(() => planBatch(input(Array.from({ length: 129 }, (_, i) =>
    ({ id: `x${i}`, budget: 1n })))), /bounded/);
  const sparse = new Array(2);
  sparse[0] = { id: 'a', budget: 1n };
  assert.throws(() => planBatch(input(sparse)), /dense/);
  const open = planBatch(input([{ id: 'a', budget: 1n }]));
  assert.throws(() => stepBatch(open, { kind: 'settle', slot: 2n, extra: true }), /shape/);
  assert.throws(() => stepBatch(open, { kind: 'cancel', slot: 4n }), /expired/);
  assert.throws(() => stepBatch(open, { kind: 'cancel', slot: 0n }), /backward/);
  assert.throws(() => stepBatch({}, { kind: 'cancel', slot: 5n }), /not created/);
});
