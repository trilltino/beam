// Client-side consistency guard for a separately authenticated creator/vault pair.
// It does not derive PDAs, authenticate chain data, or authorize a fee claim.
import { decodeBase58Pubkey } from './beam-native.mjs';
import { buildCollectCreatorFee } from './beam-creator.mjs';

function expectedPair(value) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)
      || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) {
    throw new TypeError('expected recipient must be a plain record');
  }
  const fields = ['creator', 'creatorVault'];
  const keys = Reflect.ownKeys(value);
  if (keys.length !== fields.length || keys.some((key) => !fields.includes(key))) {
    throw new TypeError('expected recipient must contain exactly creator and creatorVault');
  }
  return Object.fromEntries(fields.map((key) => {
    const field = Object.getOwnPropertyDescriptor(value, key);
    if (!field || !Object.hasOwn(field, 'value')) {
      throw new TypeError(`expected recipient ${key} must be an own data field`);
    }
    decodeBase58Pubkey(field.value);
    return [key, field.value];
  }));
}

export function buildCollectCreatorFeeForExpectedRecipient(config, accounts, expected) {
  if (arguments.length !== 3) throw new TypeError('expected config, accounts and recipient pair');
  const pair = expectedPair(expected);
  const instruction = buildCollectCreatorFee(config, accounts);
  if (instruction.keys[0].pubkey !== pair.creator
      || instruction.keys[1].pubkey !== pair.creatorVault) {
    throw new RangeError('creator or vault differs from expected recipient pair');
  }
  return instruction;
}
