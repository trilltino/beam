import test from 'node:test';
import assert from 'node:assert/strict';
import { encodeBase58Pubkey } from '../../sdk/beam-native.mjs';
import { summarizeObservedConcentration } from './concentration.mjs';

const pk = (number) => encodeBase58Pubkey(new Uint8Array(32).fill(number));
const MINT = pk(1);
const HASH = pk(2);
const SOURCE = 'beam.synthetic.balances.v1';
const row = (number, amount, overrides = {}) => ({
  address: pk(number), mint: MINT, slot: '100', blockHash: HASH, amount, ...overrides,
});
const snapshot = (balances, overrides = {}) => ({
  mint: MINT, addressKind: 'token_account', totalSupply: '100',
  source: { sourceId: SOURCE, slot: '100', blockHash: HASH, finality: 'finalized' },
  balances, ...overrides,
});
const context = (overrides = {}) => ({
  sourceId: SOURCE, asOfSlot: '105', expectedBlockHash: HASH,
  maxSlotLag: '5', topK: 2, ...overrides,
});

test('reports exact top-K and known-observed fractions without ownership inference', () => {
  const input = snapshot([row(4, '10'), row(3, '40'), row(5, '20')]);
  const result = summarizeObservedConcentration(input, context());
  assert.deepEqual(result.topK.addresses.map(({ amount }) => amount), ['40', '20']);
  assert.deepEqual(result.topK.share, { numerator: '60', denominator: '100' });
  assert.deepEqual(result.knownObservedShare, { numerator: '70', denominator: '100' });
  assert.equal(result.unobservedSupply, '30');
  assert.equal(result.observedAddressCount, 3);
  assert.equal(result.kind, 'caller-asserted-observation');
  assert.equal(result.verifiedChain, false);
  assert.equal(result.source.claimedFinality, 'finalized');
  assert.equal(result.source.observedSlotLag, '5');
  assert.equal(Object.hasOwn(result, 'bot'), false);
  assert.equal(Object.isFrozen(result.topK.addresses[0]), true);
  input.balances[0].amount = '99';
  assert.deepEqual(result.knownObservedShare, { numerator: '70', denominator: '100' });
});

test('keeps u64 precision, deterministic ties, and zero-supply shares explicit', () => {
  const maximum = (1n << 64n) - 1n;
  const amount = ((maximum - 1n) / 2n).toString();
  const tied = summarizeObservedConcentration(snapshot([row(8, amount), row(7, amount)], {
    totalSupply: maximum.toString(), addressKind: 'claimed_owner',
  }), context({ topK: 1 }));
  assert.equal(tied.topK.addresses[0].address, pk(7));
  assert.deepEqual(tied.topK.share, { numerator: amount, denominator: maximum.toString() });
  assert.equal(tied.knownObservedBalance, (BigInt(amount) * 2n).toString());
  const zero = summarizeObservedConcentration(snapshot([], { totalSupply: '0' }), context());
  assert.equal(zero.topK.share, null);
  assert.equal(zero.knownObservedShare, null);
  assert.equal(zero.unobservedSupply, '0');
});

test('rejects duplicate, malformed, mixed-fork and over-supply observations', () => {
  const cases = [
    snapshot([row(3, '10'), row(3, '20')]),
    snapshot([row(3, '101')]),
    snapshot([row(3, '01')]),
    snapshot([row(3, '1', { mint: pk(9) })]),
    snapshot([row(3, '1', { slot: '99' })]),
    snapshot([row(3, '1', { blockHash: pk(9) })]),
    snapshot([row(3, '1', { address: 'not-a-pubkey' })]),
    snapshot([row(3, '1')], { authenticated: true }),
  ];
  for (const candidate of cases) {
    assert.throws(() => summarizeObservedConcentration(candidate, context()), TypeError);
  }
  const sparse = snapshot(new Array(1));
  assert.throws(() => summarizeObservedConcentration(sparse, context()), TypeError);
  const accessor = [row(3, '1')];
  Object.defineProperty(accessor, 0, { get: () => { throw new Error('getter ran'); }, enumerable: true });
  assert.throws(() => summarizeObservedConcentration(snapshot(accessor), context()), TypeError);
});

test('rejects stale, future, source and reorg-inconsistent snapshots', () => {
  const observed = snapshot([row(3, '1')]);
  for (const badContext of [
    context({ asOfSlot: '106' }),
    context({ asOfSlot: '99' }),
    context({ sourceId: 'other.source' }),
    context({ expectedBlockHash: pk(10) }),
    context({ maxSlotLag: '01' }),
    context({ topK: 0 }),
  ]) assert.throws(() => summarizeObservedConcentration(observed, badContext), TypeError);
  const competingFork = snapshot([row(3, '1', { blockHash: pk(10) })], {
    source: { ...observed.source, blockHash: pk(10) },
  });
  assert.throws(() => summarizeObservedConcentration(competingFork, context()), TypeError);
  const finalizedClaim = summarizeObservedConcentration(observed, context());
  assert.equal(finalizedClaim.verifiedChain, false);
});
