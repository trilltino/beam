import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { encodeBase58Pubkey } from './beam-native.mjs';
import {
  CREATOR_SOURCE_COMMIT, CREATOR_PROGRAM_ID, buildCollectCreatorFee,
} from './beam-creator.mjs';
import { buildCollectCreatorFeeForExpectedRecipient } from './beam-creator-recipient.mjs';

const fixture = JSON.parse(readFileSync(new URL('./fixtures/creator-native-sol.json', import.meta.url)));
const accounts = fixture.collectCreatorFee.accounts;
const config = { sourceCommit: CREATOR_SOURCE_COMMIT, programId: CREATOR_PROGRAM_ID };
const expected = { creator: accounts.creator, creatorVault: accounts.creatorVault };
const other = encodeBase58Pubkey(new Uint8Array(32).fill(9));

test('a verified recipient pair keeps the exact permissionless claim ABI', () => {
  const bound = buildCollectCreatorFeeForExpectedRecipient(config, accounts, expected);
  assert.deepEqual(bound, buildCollectCreatorFee(config, accounts));
  assert.equal(bound.keys.length, 5);
  assert.equal(bound.keys[0].isSigner, false);
});

test('recipient or vault substitution fails before constructing a claim', () => {
  for (const changed of [
    { creator: other }, { creatorVault: other },
    { creator: other, creatorVault: other },
  ]) {
    assert.throws(
      () => buildCollectCreatorFeeForExpectedRecipient(config, { ...accounts, ...changed }, expected),
      /differs from expected recipient pair/,
    );
  }
  assert.throws(
    () => buildCollectCreatorFeeForExpectedRecipient(config, accounts, { ...expected, creator: other }),
    /differs from expected recipient pair/,
  );
});

test('expected recipient must contain exact own data fields', () => {
  let called = false;
  const accessor = { creatorVault: expected.creatorVault };
  Object.defineProperty(accessor, 'creator', { enumerable: true, get() { called = true; return expected.creator; } });
  for (const bad of [null, Object.create(expected), { ...expected, payoutWallet: other }, accessor]) {
    assert.throws(() => buildCollectCreatorFeeForExpectedRecipient(config, accounts, bad));
  }
  assert.equal(called, false);
  assert.throws(() => buildCollectCreatorFeeForExpectedRecipient(config, accounts, { ...expected, creator: '0' }), /base58/);
  assert.throws(() => buildCollectCreatorFeeForExpectedRecipient(config, accounts), /expected config/);
});
