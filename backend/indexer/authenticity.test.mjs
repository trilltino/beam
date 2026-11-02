import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { PROGRAM_ID, encodeBase58Pubkey } from '../../sdk/beam-native.mjs';
import { BLOCK_VERSION, IndexerError, createFixtureIndexer } from './index.mjs';
import { LOCAL_PROOF_VERSION, simulateSourceProofBoundary } from './authenticity.mjs';

const fixture = JSON.parse(readFileSync(new URL('../../sdk/fixtures/native-sol.json', import.meta.url)));
const SOURCE = 'beam.native-sol.synthetic.v1';
const pk = (number) => encodeBase58Pubkey(new Uint8Array(32).fill(number));
const sha = (hex) => createHash('sha256').update(Buffer.from(hex, 'hex')).digest('hex');
const code = (expected) => (error) => error instanceof IndexerError && error.code === expected;

function setup({ finalized = true, forgedTimestamp = false } = {}) {
  const indexer = createFixtureIndexer({ sourceId: SOURCE });
  const eventHex = forgedTimestamp ? (() => {
    const bytes = Buffer.from(fixture.completeEvent.dataHex, 'hex');
    new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).setBigInt64(104, 999n, true);
    return bytes.toString('hex');
  })() : fixture.completeEvent.dataHex;
  indexer.replay({ blocks: [{ version: BLOCK_VERSION, sourceId: SOURCE, slot: '100',
    hash: pk(100), parent: null, commitment: finalized ? 'finalized' : 'processed', records: [
      { kind: 'complete-event', transactionIndex: 0, recordIndex: 0, programId: PROGRAM_ID, dataHex: eventHex },
      { kind: 'curve-account', transactionIndex: 0, recordIndex: 1, owner: PROGRAM_ID,
        address: fixture.completeEvent.expected.bondingCurve, dataHex: fixture.currentBondingCurve.dataHex },
    ] }] });
  const recordId = indexer.query({ kind: 'complete-event' }).items[0].id;
  const evidence = { kind: 'synthetic-fixture', sourceId: SOURCE, indexedAtMs: 1_800_000_000_000,
    metadata: { mint: fixture.completeEvent.expected.mint,
      curveAccount: fixture.completeEvent.expected.bondingCurve, quoteMint: null,
      tokenProgram: 'spl_token', creator: fixture.currentBondingCurve.expected.creator,
      name: 'Beam sample', symbol: 'BEAM', metadataUri: 'https://example.invalid/sample.json' },
    event: { recordId, signature: '1'.repeat(64), eventIndex: 0 } };
  const proof = { version: LOCAL_PROOF_VERSION,
    root: { genesisHash: pk(10), slot: '100', hash: pk(100), commitment: 'finalized' },
    transaction: { signature: evidence.event.signature, eventIndex: 0, slot: '100',
      blockHash: pk(100), programId: PROGRAM_ID, dataHash: sha(eventHex) },
    account: { address: evidence.metadata.curveAccount, ownerProgram: PROGRAM_ID,
      slot: '100', blockHash: pk(100), dataHash: sha(fixture.currentBondingCurve.dataHex) },
    association: { mint: evidence.metadata.mint, curveAccount: evidence.metadata.curveAccount, quoteMint: null } };
  return { indexer, evidence, proof };
}

function localHooks(overrides = {}) {
  return { verifyRoot: async () => true, verifyTransaction: async () => true,
    verifyAccount: async () => true, verifyAssociation: async () => true, ...overrides };
}

test('all local proof hooks run yet never issue chain-backed status', async () => {
  const { indexer, evidence, proof } = setup({ forgedTimestamp: true });
  const called = [];
  const hooks = Object.fromEntries(['Root', 'Transaction', 'Account', 'Association'].map((name) => [
    `verify${name}`, async () => { called.push(name); return true; },
  ]));
  const result = await simulateSourceProofBoundary(indexer, evidence, proof, hooks);
  assert.deepEqual(called, ['Root', 'Transaction', 'Account', 'Association']);
  assert.deepEqual([result.kind, result.live, result.verifiedChain], ['local-proof-simulation', false, false]);
  assert.equal(Object.isFrozen(result), true);
  assert.throws(() => { result.verifiedChain = true; }, TypeError);
});

test('proof substitution and missing fields fail before any verifier hook runs', async () => {
  const { indexer, evidence, proof } = setup();
  let calls = 0;
  const hooks = localHooks({ verifyRoot: async () => { calls += 1; return true; } });
  const mutations = [
    (x) => { x.root.hash = pk(11); },
    (x) => { x.root.commitment = 'processed'; },
    (x) => { x.transaction.signature = '2'.repeat(64); },
    (x) => { x.transaction.programId = pk(12); },
    (x) => { x.transaction.dataHash = '0'.repeat(64); },
    (x) => { x.account.ownerProgram = pk(13); },
    (x) => { x.account.dataHash = '0'.repeat(64); },
    (x) => { x.association.mint = pk(14); },
    (x) => { delete x.account.blockHash; },
  ];
  for (const mutate of mutations) {
    const candidate = structuredClone(proof); mutate(candidate);
    await assert.rejects(simulateSourceProofBoundary(indexer, evidence, candidate, hooks), (error) => error instanceof IndexerError);
  }
  assert.equal(calls, 0);
});

test('each required proof hook must affirm; fixture finality claim alone is insufficient', async () => {
  const { indexer, evidence, proof } = setup();
  for (const name of ['verifyRoot', 'verifyTransaction', 'verifyAccount', 'verifyAssociation']) {
    await assert.rejects(simulateSourceProofBoundary(indexer, evidence, proof,
      localHooks({ [name]: async () => false })), code('PROOF_REJECTED'));
  }
  await assert.rejects(simulateSourceProofBoundary(indexer, evidence, proof,
    { ...localHooks(), verifyAccount: undefined }), code('INVALID_PROOF'));
  const unfinalized = setup({ finalized: false });
  await assert.rejects(simulateSourceProofBoundary(unfinalized.indexer, unfinalized.evidence,
    unfinalized.proof, localHooks()), code('STALE_SOURCE'));
});

test('source status changes during asynchronous proof hooks invalidate the result', async () => {
  const { indexer, evidence, proof } = setup();
  const hooks = localHooks({ verifyRoot: async () => {
    indexer.markSource({ state: 'stale', detail: 'Fixture changed during proof check.' });
    return true;
  } });
  await assert.rejects(simulateSourceProofBoundary(indexer, evidence, proof, hooks), code('STALE_SOURCE'));
});
