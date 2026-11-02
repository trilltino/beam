import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { classifyBeamLaunchPayload, decodeBeamLaunchEvent } from './beam-launch-events.mjs';
import { decodeBeamEvent } from './beam-events.mjs';
import { decodeCompleteEvent } from './beam-native.mjs';

const events = JSON.parse(readFileSync(new URL('./fixtures/events.json', import.meta.url)));
const native = JSON.parse(readFileSync(new URL('./fixtures/native-sol.json', import.meta.url)));
const bytes = (hex) => Buffer.from(hex, 'hex');

test('routes three independent Anchor golden payloads without changing decoded fields', () => {
  assert.equal(native.fixtureOnly, true);
  assert.equal(events.fixtureOnly, true);
  const complete = bytes(native.completeEvent.dataHex);
  assert.equal(complete.subarray(0, 8).toString('hex'),
    createHash('sha256').update('event:CompleteEvent').digest('hex').slice(0, 16));
  assert.deepEqual(decodeBeamLaunchEvent(complete), {
    eventName: 'CompleteEvent', data: decodeCompleteEvent(complete),
  });
  for (const fixture of events.cases) {
    const payload = bytes(fixture.dataHex);
    assert.deepEqual(decodeBeamLaunchEvent(payload), decodeBeamEvent(payload), fixture.id);
  }
});

test('CompleteEvent means curve completion and requires its exact 144-byte payload', () => {
  const complete = bytes(native.completeEvent.dataHex);
  assert.equal(decodeBeamLaunchEvent(complete).eventName, 'CompleteEvent');
  for (let length = 0; length < complete.length; length += 1) {
    assert.throws(() => decodeBeamLaunchEvent(complete.subarray(0, length)), RangeError);
  }
  assert.throws(() => decodeBeamLaunchEvent(Buffer.concat([complete, Buffer.from([0])])),
    /CompleteEvent/);
  const wrong = Buffer.from(complete); wrong[0] ^= 1;
  assert.throws(() => decodeBeamLaunchEvent(wrong), /unknown event discriminator/);
  // The older gated CPMM event and a CPI envelope are not AMM v4 migration proof.
  const cpmmTag = createHash('sha256').update('event:BeamRaydiumMigration').digest().subarray(0, 8);
  assert.throws(() => decodeBeamLaunchEvent(Buffer.concat([cpmmTag, complete.subarray(8)])),
    /unknown event discriminator/);
  assert.throws(() => decodeBeamLaunchEvent(Buffer.concat([
    bytes('e445a52e51cb9a1d'), complete,
  ])), /unknown event discriminator/);
});

test('accepts byte views, rejects other inputs, and does not retain the input', () => {
  const complete = bytes(native.completeEvent.dataHex);
  const storage = Buffer.alloc(complete.length + 24, 0xaa);
  complete.copy(storage, 11);
  const view = new Uint8Array(storage.buffer, storage.byteOffset + 11, complete.length);
  const decoded = decodeBeamLaunchEvent(view);
  assert.deepEqual(decoded.data, decodeCompleteEvent(complete));
  storage.fill(0);
  assert.deepEqual(decoded.data, decodeCompleteEvent(complete));
  for (const bad of [undefined, null, 'base64', [], {}, new ArrayBuffer(144),
    new DataView(new ArrayBuffer(144)), new Uint8ClampedArray(144)]) {
    assert.throws(() => decodeBeamLaunchEvent(bad), TypeError);
  }
  assert.throws(() => decodeBeamLaunchEvent(new Uint8Array(1024 * 1024 + 1)),
    /SDK payload limit/);
});

test('local event kinds match the read-only feed vocabulary without inventing migration', () => {
  const expected = new Map([
    ['create-unicode', 'create'], ['create-empty', 'create'],
    ['trade-mixed', 'buy'], ['trade-empty', 'sell'], ['trade-sequential', 'sell'],
  ]);
  for (const fixture of events.cases) {
    const payload = bytes(fixture.dataHex);
    const decoded = decodeBeamLaunchEvent(payload);
    assert.deepEqual(classifyBeamLaunchPayload(payload), {
      kind: expected.get(fixture.id), mint: decoded.data.mint,
    }, fixture.id);
  }
  const complete = bytes(native.completeEvent.dataHex);
  assert.deepEqual(classifyBeamLaunchPayload(complete), {
    kind: 'curve_complete', mint: decodeCompleteEvent(complete).mint,
  });
  assert.throws(() => classifyBeamLaunchPayload(new Uint8Array(8)),
    /unknown event discriminator/);
});
