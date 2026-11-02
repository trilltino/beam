import assert from 'node:assert/strict';
import test from 'node:test';
import {
  buyExactTokens, curveState, nominalSpotCapProxy, proRataBatch, sellExactTokens,
} from './model.mjs';

const start = (virtualToken = 1000n, virtualQuote = 1000n, realToken = 500n) =>
  curveState({ virtualToken, virtualQuote, realToken, realQuote: 0n });

test('virtual spot value is not actual quote exit liquidity', () => {
  const curve = start();
  assert.equal(nominalSpotCapProxy(curve, 1000n), 1000n);
  assert.equal(curve.realQuote, 0n);
  assert.throws(() => sellExactTokens(curve, 100n), /real quote reserves/);
});

test('same nominal spot proxy can have different reserve depth and price impact', () => {
  const shallow = start();
  const deep = start(10000n, 10000n, 5000n);
  assert.equal(nominalSpotCapProxy(shallow, 10000n), nominalSpotCapProxy(deep, 10000n));
  const a = buyExactTokens(shallow, 100n);
  const b = buyExactTokens(deep, 100n);
  assert.equal(a.quoteIn, 112n);
  assert.equal(b.quoteIn, 102n);
  assert.ok(a.state.virtualQuote * b.state.virtualToken >
    b.state.virtualQuote * a.state.virtualToken);
});

test('a higher nominal quote reserve charges honest and coordinated buys alike', () => {
  const base = start();
  const raised = start(1000n, 2000n);
  assert.ok(nominalSpotCapProxy(raised, 1000n) > nominalSpotCapProxy(base, 1000n));
  assert.ok(buyExactTokens(raised, 100n).quoteIn > buyExactTokens(base, 100n).quoteIn);
  // There is no wallet identity input that could selectively exempt one buyer.
  assert.deepEqual(Object.keys(raised), Object.keys(base));
});

test('the first identical buy is cheaper regardless of actor, slot or bundle label', () => {
  const first = buyExactTokens(start(), 100n);
  const second = buyExactTokens(first.state, 100n);
  assert.equal(first.quoteIn, 112n);
  assert.equal(second.quoteIn, 140n);
  // The model and retained quote equation have no slot input. Calling a pair a
  // same-slot bundle does not change reserve ordering or remove the advantage.
  const bundled = [first.quoteIn, second.quoteIn];
  const ordinary = [buyExactTokens(start(), 100n).quoteIn,
    buyExactTokens(buyExactTokens(start(), 100n).state, 100n).quoteIn];
  assert.deepEqual(bundled, ordinary);
});

test('later buy provides quote liquidity for an earlier holder to exit', () => {
  const first = buyExactTokens(start(), 100n);
  const later = buyExactTokens(first.state, 100n);
  const exitFirst = sellExactTokens(later.state, 100n);
  const exitLater = sellExactTokens(exitFirst.state, 100n);
  assert.equal(first.quoteIn, 112n);
  assert.equal(later.quoteIn, 140n);
  assert.equal(exitFirst.quoteOut, 139n);
  assert.equal(exitLater.quoteOut, 111n);
  assert.equal(exitLater.state.realQuote,
    first.quoteIn + later.quoteIn - exitFirst.quoteOut - exitLater.quoteOut);
  assert.equal(exitLater.state.realToken, start().realToken);
  assert.equal(exitLater.state.virtualToken, start().virtualToken);
});

test('splitting the same requested token quantity preserves final reserves', () => {
  const single = buyExactTokens(start(), 100n);
  const split1 = buyExactTokens(start(), 50n);
  const split2 = buyExactTokens(split1.state, 50n);
  assert.equal(single.state.realToken, split2.state.realToken);
  assert.equal(single.state.virtualToken, split2.state.virtualToken);
  assert.ok(split1.quoteIn + split2.quoteIn >= single.quoteIn);
  assert.ok(split1.quoteIn + split2.quoteIn <= single.quoteIn + 2n);
});

test('research batch allocation is order independent; exact budget share is split invariant', () => {
  const a = proRataBatch(101n, [{ id: 'creator', budget: 60n }, { id: 'public', budget: 40n }]);
  const b = proRataBatch(101n, [{ id: 'public', budget: 40n }, { id: 'creator', budget: 60n }]);
  assert.deepEqual(a, b);
  assert.equal(a.allocations[0].tokens, 60n);
  assert.equal(a.unallocatedDust, 1n);
  const split = proRataBatch(101n, [
    { id: 'c1', budget: 30n }, { id: 'c2', budget: 30n }, { id: 'public', budget: 40n },
  ]);
  assert.equal(split.allocations[0].budget + split.allocations[1].budget, 60n);
  assert.equal(split.allocations[0].tokens + split.allocations[1].tokens, 60n);
  assert.equal(split.illustrativeCommonPrice.numerator, a.illustrativeCommonPrice.numerator);
  const dustSplit = proRataBatch(2n, [
    { id: 'a1', budget: 1n }, { id: 'a2', budget: 1n }, { id: 'b', budget: 1n },
  ]);
  assert.equal(dustSplit.unallocatedDust, 2n);
  assert.equal(proRataBatch(2n, [{ id: 'a', budget: 2n }, { id: 'b', budget: 1n }]).unallocatedDust, 1n);
});

test('invalid or insolvent synthetic states fail closed', () => {
  assert.throws(() => start(1000n, 1000n, 1001n), /real reserves/);
  assert.throws(() => nominalSpotCapProxy(start(1000n, 1000n, 500n), 499n), /supply/);
  assert.throws(() => buyExactTokens(start(), 0n), /positive/);
  assert.throws(() => buyExactTokens(start(), 501n), /exceeds/);
  assert.throws(() => buyExactTokens(start(), 500.5), /u64 bigint/);
  // The retained pure quote can round a dust sale to zero; proposed launch
  // rules must decide separately whether such a sale should be rejected.
  assert.equal(sellExactTokens(start(), 1n).quoteOut, 0n);
  assert.throws(() => proRataBatch(2n, [{ id: 'a', budget: 1n }, { id: 'a', budget: 1n }]), /distinct/);
  assert.throws(() => proRataBatch(2n, [{ id: 'a', budget: 0n }]), /positive/);
  assert.throws(() => proRataBatch(0n, [{ id: 'a', budget: 1n }]), /positive/);
});
