import assert from 'node:assert/strict';
import test from 'node:test';
import { genesisJournal, planIntegerBatch, replayJournal, ResearchJournalStore } from './integer-batch.mjs';
import { buyExactTokens, curveState } from './model.mjs';

const fixture = (orders, override = {}) => ({
  batchId: 'test-batch', tokenCapLots: 5n, lotSizeTokenUnits: 100n,
  lotPriceQuoteUnits: 10n, openSlot: 1n, closeSlot: 2n,
  settlementDeadlineSlot: 5n, orders, ...override,
});
const budget = (id, budgetQuoteUnits) => ({ id, budgetQuoteUnits });
const sums = (plan, field) => plan.rows.reduce((sum, row) => sum + BigInt(row[field]), 0n);

function conserved(plan) {
  assert.equal(sums(plan, 'allocatedLots') + BigInt(plan.unallocatedLots), BigInt(plan.tokenCapLots));
  assert.equal(sums(plan, 'tokenUnits'), sums(plan, 'allocatedLots') * BigInt(plan.lotSizeTokenUnits));
  assert.equal(sums(plan, 'quoteSpentUnits'), sums(plan, 'allocatedLots') * BigInt(plan.lotPriceQuoteUnits));
  assert.equal(sums(plan, 'quoteSpentUnits') + sums(plan, 'quoteRefundUnits'),
    BigInt(plan.totalBudgetQuoteUnits));
  assert.equal(BigInt(plan.unsoldDemandGapLots) + BigInt(plan.prorataRoundingLots),
    BigInt(plan.unallocatedLots));
  for (const row of plan.rows) {
    assert.equal(BigInt(row.quoteSpentUnits) + BigInt(row.quoteRefundUnits),
      BigInt(row.budgetQuoteUnits));
  }
}

test('oversubscribed whole-lot allocation conserves quote and leaves rounding lots unassigned', () => {
  const plan = planIntegerBatch(fixture([budget('a', 50n), budget('b', 50n)]));
  assert.equal(plan.oversubscribed, true);
  assert.deepEqual(plan.rows.map((row) => row.allocatedLots), ['2', '2']);
  assert.deepEqual(plan.rows.map((row) => row.quoteSpentUnits), ['20', '20']);
  assert.deepEqual(plan.rows.map((row) => row.quoteRefundUnits), ['30', '30']);
  assert.equal(plan.prorataRoundingLots, '1');
  assert.equal(plan.unsoldDemandGapLots, '0');
  assert.equal(plan.kind, 'disabled-exact-integer-batch-research');
  assert.equal(plan.executable, false);
  conserved(plan);
});

test('undersubscription and sub-lot budgets return unused quote without free units', () => {
  const under = planIntegerBatch(fixture([budget('a', 25n), budget('b', 31n)],
    { tokenCapLots: 10n }));
  assert.deepEqual(under.rows.map((row) => row.allocatedLots), ['2', '3']);
  assert.deepEqual(under.rows.map((row) => row.quoteRefundUnits), ['5', '1']);
  assert.equal(under.unsoldDemandGapLots, '5');
  assert.equal(under.prorataRoundingLots, '0');
  conserved(under);
  const tiny = planIntegerBatch(fixture([budget('a', 9n), budget('b', 9n)]));
  assert.equal(tiny.allocatedLots, '0');
  assert.equal(tiny.quoteSpentUnits, '0');
  assert.equal(tiny.quoteRefundUnits, '18');
  conserved(tiny);
});

test('order permutations and equal-demand ties do not assign a bonus or dust', () => {
  const orders = [budget('z', 50n), budget('a', 50n)];
  const first = planIntegerBatch(fixture(orders));
  const reversed = planIntegerBatch(fixture([...orders].reverse()));
  assert.deepEqual(first, reversed);
  assert.deepEqual(first.rows.map((row) => row.id), ['a', 'z']);
  assert.deepEqual(first.rows.map((row) => row.allocatedLots), ['2', '2']);
  assert.equal(first.unallocatedLots, '1');
});

test('splitting a fixed budget never increases acquired lots over a bounded adversarial grid', () => {
  let cases = 0;
  for (let cap = 1n; cap <= 4n; cap++) {
    for (let price = 1n; price <= 4n; price++) {
      for (let actor = 2n; actor <= 8n; actor++) {
        for (let other = 1n; other <= 8n; other++) {
          const whole = planIntegerBatch(fixture([budget('actor', actor), budget('other', other)],
            { tokenCapLots: cap, lotPriceQuoteUnits: price }));
          const unsplit = BigInt(whole.rows.find((row) => row.id === 'actor').allocatedLots);
          conserved(whole);
          for (let part = 1n; part < actor; part++) {
            const split = planIntegerBatch(fixture([budget('a1', part), budget('a2', actor - part),
              budget('other', other)], { tokenCapLots: cap, lotPriceQuoteUnits: price }));
            const acquired = split.rows.filter((row) => row.id.startsWith('a'))
              .reduce((sum, row) => sum + BigInt(row.allocatedLots), 0n);
            assert.ok(acquired <= unsplit);
            conserved(split);
            cases++;
          }
        }
      }
    }
  }
  assert.equal(cases, 3584);
});

test('equal-budget comparison exposes changed ordering, not a production price choice', () => {
  const start = curveState({ virtualToken: 1000n, virtualQuote: 1000n,
    realToken: 500n, realQuote: 0n });
  const first = buyExactTokens(start, 100n);
  assert.equal(first.quoteIn, 112n);
  assert.equal(buyExactTokens(first.state, 100n).quoteIn, 140n);
  assert.equal(buyExactTokens(first.state, 82n).quoteIn, 112n);
  assert.ok(buyExactTokens(first.state, 83n).quoteIn > 112n);
  const batch = planIntegerBatch(fixture([budget('early', 112n), budget('later', 112n)],
    { tokenCapLots: 2n, lotPriceQuoteUnits: 112n }));
  assert.deepEqual(batch.rows.map((row) => row.tokenUnits), ['100', '100']);
  assert.deepEqual(batch.rows.map((row) => row.quoteSpentUnits), ['112', '112']);
  conserved(batch);
});

test('journal replays from JSON, rejects stale CAS and duplicate claims', () => {
  const config = fixture([budget('a', 50n), budget('b', 50n)]);
  const plan = planIntegerBatch(config);
  const store = new ResearchJournalStore(plan);
  const initialHead = store.snapshot().head;
  assert.throws(() => store.commit(initialHead, { kind: 'settle', slot: 1n }), /window/);
  assert.equal(store.snapshot().head, initialHead);
  store.commit(initialHead, { kind: 'settle', slot: 2n });
  assert.throws(() => store.commit(initialHead, { kind: 'claim', slot: 3n, id: 'a' }), /stale/);
  const claim = store.commit(store.snapshot().head, { kind: 'claim', slot: 3n, id: 'a' });
  assert.deepEqual(claim.voucher, { kind: 'mock-claim-only', id: 'a', tokenUnits: '200',
    quoteSpentUnits: '20', quoteRefundUnits: '30' });
  const saved = JSON.parse(JSON.stringify(store.snapshot()));
  const restored = new ResearchJournalStore(planIntegerBatch(config), saved);
  assert.deepEqual(restored.status(), store.status());
  saved.events[0].slot = '999';
  assert.deepEqual(restored.status(), store.status());
  assert.throws(() => restored.commit(restored.snapshot().head,
    { kind: 'claim', slot: 3n, id: 'a' }), /duplicate/);
  const second = restored.commit(restored.snapshot().head,
    { kind: 'claim', slot: 3n, id: 'b' });
  assert.equal(second.voucher.quoteRefundUnits, '30');
  assert.deepEqual(restored.status().claimed, ['a', 'b']);
});

test('stalled batch cancels after deadline and mock claims return full budgets once', () => {
  const plan = planIntegerBatch(fixture([budget('a', 50n), budget('b', 50n)]));
  const store = new ResearchJournalStore(plan);
  assert.throws(() => store.commit(store.snapshot().head,
    { kind: 'cancel', slot: 5n }), /window/);
  assert.throws(() => store.commit(store.snapshot().head,
    { kind: 'settle', slot: 6n }), /window/);
  store.commit(store.snapshot().head, { kind: 'cancel', slot: 6n });
  const a = store.commit(store.snapshot().head, { kind: 'claim', slot: 6n, id: 'a' });
  const b = store.commit(store.snapshot().head, { kind: 'claim', slot: 7n, id: 'b' });
  assert.equal(a.voucher.quoteRefundUnits, '50');
  assert.equal(b.voucher.quoteRefundUnits, '50');
  assert.equal(a.voucher.tokenUnits, '0');
  assert.throws(() => store.commit(store.snapshot().head,
    { kind: 'claim', slot: 7n, id: 'b' }), /duplicate/);
});

test('separate stores can fork from one history without a durable atomic CAS', () => {
  const plan = planIntegerBatch(fixture([budget('a', 50n)]));
  const original = new ResearchJournalStore(plan);
  const restored = new ResearchJournalStore(plan,
    JSON.parse(JSON.stringify(original.snapshot())));
  const sameOldHead = original.snapshot().head;
  original.commit(sameOldHead, { kind: 'settle', slot: 2n });
  restored.commit(sameOldHead, { kind: 'settle', slot: 2n });
  const claimA = original.commit(original.snapshot().head,
    { kind: 'claim', slot: 3n, id: 'a' });
  const claimB = restored.commit(restored.snapshot().head,
    { kind: 'claim', slot: 3n, id: 'a' });
  assert.deepEqual(claimA.voucher, claimB.voucher);
  // A real service must serialize commits in a durable store; this class
  // cannot arbitrate independent stores or authenticate who may claim.
});

test('journal tampering, wrong plan and malformed input fail closed', () => {
  const plan = planIntegerBatch(fixture([budget('a', 50n)]));
  const store = new ResearchJournalStore(plan);
  store.commit(store.snapshot().head, { kind: 'settle', slot: 2n });
  const saved = JSON.parse(JSON.stringify(store.snapshot()));
  saved.events[0].slot = '3';
  assert.throws(() => replayJournal(plan, saved), /hash/);
  const otherPlan = planIntegerBatch(fixture([budget('a', 51n)]));
  assert.throws(() => replayJournal(otherPlan, store.snapshot()), /plan digest/);
  assert.throws(() => planIntegerBatch(fixture([budget('a', 1n), budget('a', 2n)])), /duplicate/);
  assert.throws(() => planIntegerBatch(fixture([budget('a', 1n)],
    { tokenCapLots: (1n << 64n) - 1n, lotSizeTokenUnits: 2n })), /exceed/);
  assert.throws(() => planIntegerBatch(fixture([budget('a', 1n)],
    { lotPriceQuoteUnits: 0n })), /positive/);
  assert.throws(() => planIntegerBatch(fixture([budget('a', 1n)],
    { closeSlot: 5n })), /deadline/);
  assert.throws(() => planIntegerBatch(fixture([budget('a', 1n)],
    { settlementDeadlineSlot: (1n << 64n) - 1n })), /cancellation slot/);
  const sparse = new Array(2);
  sparse[0] = budget('a', 1n);
  assert.throws(() => planIntegerBatch(fixture(sparse)), /dense/);
  assert.throws(() => planIntegerBatch(fixture(Array.from({ length: 129 }, (_, i) =>
    budget(`id${i}`, 1n)))), /bounded/);
  assert.throws(() => replayJournal(plan, { planHash: 'x', head: 'x', events: [] }), /digest/);
});

test('bounded journal permits a terminal event plus every one of 128 claims', () => {
  const orders = Array.from({ length: 128 }, (_, i) => budget(`id${i}`, 1n));
  const plan = planIntegerBatch(fixture(orders, { tokenCapLots: 128n, lotPriceQuoteUnits: 1n }));
  const store = new ResearchJournalStore(plan);
  store.commit(store.snapshot().head, { kind: 'settle', slot: 2n });
  for (const row of plan.rows) {
    store.commit(store.snapshot().head, { kind: 'claim', slot: 3n, id: row.id });
  }
  assert.equal(store.snapshot().events.length, 129);
  assert.equal(store.status().claimed.length, 128);
  conserved(plan);
});
