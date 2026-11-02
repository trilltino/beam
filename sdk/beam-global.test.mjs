import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import {
  GLOBAL_DISCRIMINATOR, GLOBAL_LENGTH, decodeGlobal,
  isAuthorizedFeeRecipient, isAuthorizedBuybackFeeRecipient, authorizedBuybackFeeRecipients,
} from './beam-global.mjs';

const fixtureBytes = readFileSync(new URL('./fixtures/global.json', import.meta.url));
const fixture = JSON.parse(fixtureBytes);
const receipt = JSON.parse(readFileSync(new URL('./fixtures/global.receipt.json', import.meta.url)));
const bytes = (name) => Uint8Array.from(Buffer.from(fixture.cases.find((c) => c.name === name).dataHex, 'hex'));
const expected = (name) => fixture.cases.find((c) => c.name === name).fields;
const plain = (value) => JSON.parse(JSON.stringify(value, (_, v) => (typeof v === 'bigint' ? v.toString() : v)));
const ZERO = '11111111111111111111111111111111';

test('fixture bytes match the recorded Rust producer receipt', () => {
  assert.equal(createHash('sha256').update(fixtureBytes).digest('hex'), receipt.stdoutSha256);
  assert.equal(fixtureBytes.length, receipt.stdoutBytes);
  const source = readFileSync(new URL('./tools/generate-global-fixtures.rs', import.meta.url));
  assert.equal(createHash('sha256').update(source).digest('hex'), receipt.sourceSha256);
});

test('layout constants match Anchor serialization', () => {
  assert.equal(Buffer.from(GLOBAL_DISCRIMINATOR).toString('hex'), fixture.discriminatorHex);
  assert.equal(GLOBAL_LENGTH, fixture.serializedLength);
});

test('decodeGlobal matches Anchor for every fixture case', () => {
  for (const c of fixture.cases) {
    const decoded = decodeGlobal(bytes(c.name));
    assert.deepEqual(plain(decoded), c.fields, c.name);
    for (const field of ['initialVirtualTokenReserves', 'buybackBasisPoints', 'maxConfigurableCreatorFeeBps']) {
      assert.equal(typeof decoded[field], 'bigint');
    }
  }
  assert.equal(decodeGlobal(bytes('distinct')).initialVirtualQuoteReserves, (1n << 64n) - 1n);
  const view = bytes('trailingPadding');
  const offset = new Uint8Array(view.length + 3);
  offset.set(view, 3);
  assert.deepEqual(plain(decodeGlobal(offset.subarray(3))), expected('trailingPadding'), 'nonzero-offset views decode');
});

test('decodeGlobal rejects malformed input', () => {
  const data = bytes('distinct');
  assert.throws(() => decodeGlobal(data.subarray(0, GLOBAL_LENGTH - 1)), /at least/);
  const wrong = data.slice(); wrong[0] ^= 1;
  assert.throws(() => decodeGlobal(wrong), /not a Global/);
  const badBool = data.slice(); badBool[8] = 2;
  assert.throws(() => decodeGlobal(badBool), /Borsh boolean/);
  assert.throws(() => decodeGlobal('00'), TypeError);
});

test('fee-recipient membership mirrors validate_fee_recipient', () => {
  const global = decodeGlobal(bytes('distinct'));
  assert.equal(isAuthorizedFeeRecipient(global, global.feeRecipient), true);
  for (const key of global.feeRecipients) assert.equal(isAuthorizedFeeRecipient(global, key), true);
  assert.equal(isAuthorizedFeeRecipient(global, global.authority), false);
  assert.equal(isAuthorizedFeeRecipient(global, global.reservedFeeRecipient), false);
  assert.equal(isAuthorizedFeeRecipient(global, global.reservedFeeRecipient, { isMayhemMode: true }), true);
  assert.equal(isAuthorizedFeeRecipient(global, global.reservedFeeRecipients[6], { isMayhemMode: true }), true);
  assert.equal(isAuthorizedFeeRecipient(global, global.feeRecipient, { isMayhemMode: true }), false);

  const unsorted = decodeGlobal(bytes('unsortedFeeRecipients'));
  assert.equal(isAuthorizedFeeRecipient(unsorted, unsorted.feeRecipient), true, 'the primary recipient is checked first');
  assert.equal(isAuthorizedFeeRecipient(unsorted, unsorted.authority), false, 'an absent key can never be found');
  assert.throws(() => isAuthorizedFeeRecipient(unsorted, unsorted.feeRecipients[1]), /unsorted/);
});

test('buyback membership mirrors validate_buyback_fee_recipient', () => {
  const global = decodeGlobal(bytes('distinct'));
  assert.equal(isAuthorizedBuybackFeeRecipient(global, global.buybackFeeRecipients[7]), true);
  assert.equal(isAuthorizedBuybackFeeRecipient(global, global.feeRecipient), false);
  assert.deepEqual(authorizedBuybackFeeRecipients(global), global.buybackFeeRecipients);
  const zero = decodeGlobal(bytes('zero'));
  assert.equal(isAuthorizedBuybackFeeRecipient(zero, ZERO), false, 'the zero key is never a buyback recipient');
  assert.deepEqual(authorizedBuybackFeeRecipients(zero), []);
});
