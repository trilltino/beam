import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const webAdapter = fileURLToPath(new URL('../../web/src/indexerAdapter.ts', import.meta.url));

test('synthetic fixture stays a sample or unverified candidate at the actual web boundary', (context) => {
  // PR #16 predates merged web adapter #17; the combined tree runs this case.
  if (!existsSync(webAdapter)) return context.skip('web adapter is absent from this stacked base');
  const script = `
    import assert from 'node:assert/strict';
    import { readFileSync } from 'node:fs';
    import { PROGRAM_ID, encodeBase58Pubkey } from './sdk/beam-native.mjs';
    import { BLOCK_VERSION, createFixtureIndexer, buildSyntheticWebFixture } from './backend/indexer/index.mjs';
    import { parseIndexerPage, sampleState, dataLabel } from './web/src/indexerAdapter.ts';
    const fixture = JSON.parse(readFileSync('sdk/fixtures/native-sol.json', 'utf8'));
    const sourceId = 'beam.native-sol.synthetic.v1';
    const indexer = createFixtureIndexer({ sourceId });
    indexer.replay({ blocks: [{ version: BLOCK_VERSION, sourceId, slot: '100',
      hash: encodeBase58Pubkey(new Uint8Array(32).fill(100)), parent: null,
      commitment: 'finalized', records: [
        { kind: 'complete-event', transactionIndex: 0, recordIndex: 0,
          programId: PROGRAM_ID, dataHex: fixture.completeEvent.dataHex },
        { kind: 'curve-account', transactionIndex: 0, recordIndex: 1,
          owner: PROGRAM_ID, address: fixture.completeEvent.expected.bondingCurve,
          dataHex: fixture.currentBondingCurve.dataHex },
      ] }] });
    const recordId = indexer.query({ kind: 'complete-event' }).items[0].id;
    const wrapped = buildSyntheticWebFixture(indexer, {
      kind: 'synthetic-fixture', sourceId, indexedAtMs: 1_800_000_000_000,
      metadata: { mint: fixture.completeEvent.expected.mint,
        curveAccount: fixture.completeEvent.expected.bondingCurve,
        quoteMint: null, tokenProgram: 'spl_token',
        creator: fixture.currentBondingCurve.expected.creator,
        name: 'Beam sample', symbol: 'BEAM', metadataUri: 'https://example.invalid/sample.json' },
      event: { recordId, signature: '1'.repeat(64), eventIndex: 0 },
    });
    const options = { expectedProgramId: PROGRAM_ID, nowMs: 1_800_000_000_000, maxAgeMs: 30_000 };
    assert.throws(() => parseIndexerPage(wrapped, options), TypeError);
    const candidate = parseIndexerPage(wrapped.page, options);
    assert.equal(candidate.kind, 'unverified_indexer');
    assert.match(dataLabel(candidate, options.nowMs, options.maxAgeMs), /^Unverified indexer payload/);
    assert.equal(candidate.page.source.finality, 'processed');
    assert.equal(candidate.capabilities.canTrade, false);
    assert.equal(candidate.capabilities.canSign, false);
    const sample = sampleState(wrapped.page.tokens);
    assert.equal(dataLabel(sample, options.nowMs, options.maxAgeMs), 'Sample data — no chain source');
    assert.equal(sample.capabilities.canTrade, false);
    assert.equal(wrapped.live, false);
    assert.equal(wrapped.verifiedChain, false);
  `;
  const run = spawnSync(process.execPath,
    ['--experimental-strip-types', '--input-type=module', '--eval', script],
    { cwd: root, encoding: 'utf8', timeout: 10_000 });
  assert.equal(run.status, 0, `web boundary subprocess failed: ${run.error?.message ?? run.stderr}`);
});
