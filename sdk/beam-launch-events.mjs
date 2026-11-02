// Local routing for three raw Anchor event payloads. This does not establish
// that bytes came from Beam, a successful transaction, or a chain at all.
import { decodeCompleteEvent } from './beam-native.mjs';
import { decodeBeamEvent, MAX_EVENT_BYTES } from './beam-events.mjs';

// SHA-256("event:CompleteEvent")[0..8], checked against the Anchor fixture.
const COMPLETE_EVENT_DISCRIMINATOR = Uint8Array.from([95, 114, 97, 156, 212, 46, 152, 8]);
const typedArrayPrototype = Object.getPrototypeOf(Uint8Array.prototype);
const typedArrayTag = Object.getOwnPropertyDescriptor(typedArrayPrototype, Symbol.toStringTag).get;
const typedArrayByteLength = Object.getOwnPropertyDescriptor(typedArrayPrototype, 'byteLength').get;

/** Decode a complete raw CreateEvent, TradeEvent, or curve CompleteEvent. */
export function decodeBeamLaunchEvent(input) {
  if (typedArrayTag.call(input) !== 'Uint8Array') {
    throw new TypeError('event payload must be a Uint8Array');
  }
  const length = typedArrayByteLength.call(input);
  if (length < 8) throw new RangeError('truncated event discriminator');
  if (length > MAX_EVENT_BYTES) throw new RangeError('event exceeds SDK payload limit');
  const bytes = new Uint8Array(input);
  if (COMPLETE_EVENT_DISCRIMINATOR.every((byte, index) => bytes[index] === byte)) {
    return { eventName: 'CompleteEvent', data: decodeCompleteEvent(bytes) };
  }
  return decodeBeamEvent(bytes);
}

/** Local event meaning only; an IndexedEvent also needs authenticated context. */
export function classifyBeamLaunchPayload(input) {
  const event = decodeBeamLaunchEvent(input);
  const kind = event.eventName === 'CreateEvent' ? 'create'
    : event.eventName === 'CompleteEvent' ? 'curve_complete'
      : event.data.isBuy ? 'buy' : 'sell';
  return { kind, mint: event.data.mint };
}
