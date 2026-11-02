import { decodeBase58Pubkey, encodeBase58Pubkey } from '../../sdk/beam-native.mjs';

const U64_MAX = (1n << 64n) - 1n;
const MAX_ROWS = 10_000; // Input-work bound, not a launch or trading rule.

function invalid(message) { throw new TypeError(message); }

function exact(value, fields, name) {
  if (value === null || typeof value !== 'object'
    || ![Object.prototype, null].includes(Object.getPrototypeOf(value))
    || Reflect.ownKeys(value).length !== fields.length) invalid(`${name} has an invalid shape`);
  for (const field of fields) {
    const descriptor = Object.getOwnPropertyDescriptor(value, field);
    if (!descriptor || !Object.hasOwn(descriptor, 'value') || !descriptor.enumerable) {
      invalid(`${name}.${field} must be an own data field`);
    }
  }
}

function u64(value, name) {
  if (typeof value !== 'string' || value.length > 20
    || !/^(0|[1-9][0-9]*)$/.test(value)) invalid(`${name} must be a canonical u64 string`);
  const number = BigInt(value);
  if (number > U64_MAX) invalid(`${name} exceeds u64`);
  return number;
}

function key(value, name) {
  try {
    if (encodeBase58Pubkey(decodeBase58Pubkey(value)) !== value) throw new Error('noncanonical');
  } catch { invalid(`${name} must be a canonical 32-byte public key`); }
  return value;
}

function sourceId(value) {
  if (typeof value !== 'string' || !/^[a-z0-9][a-z0-9._-]{0,63}$/.test(value)) {
    invalid('sourceId must be a bounded local identifier');
  }
  return value;
}

function ratio(numerator, denominator) {
  return denominator === 0n ? null : Object.freeze({
    numerator: numerator.toString(), denominator: denominator.toString(),
  });
}

/**
 * Summarize caller-asserted balances without authenticating Solana state.
 * The context is an explicit comparison point, not a trusted root or policy.
 */
export function summarizeObservedConcentration(snapshot, context) {
  exact(snapshot, ['mint', 'addressKind', 'totalSupply', 'source', 'balances'], 'snapshot');
  exact(snapshot.source, ['sourceId', 'slot', 'blockHash', 'finality'], 'snapshot.source');
  exact(context, ['sourceId', 'asOfSlot', 'expectedBlockHash', 'maxSlotLag', 'topK'], 'context');
  const mint = key(snapshot.mint, 'snapshot.mint');
  if (!['token_account', 'claimed_owner'].includes(snapshot.addressKind)) invalid('invalid address kind');
  const totalSupply = u64(snapshot.totalSupply, 'totalSupply');
  const id = sourceId(snapshot.source.sourceId);
  const slot = u64(snapshot.source.slot, 'source.slot');
  const blockHash = key(snapshot.source.blockHash, 'source.blockHash');
  if (!['processed', 'confirmed', 'finalized'].includes(snapshot.source.finality)) invalid('invalid claimed finality');
  if (sourceId(context.sourceId) !== id) invalid('source identity differs');
  const asOfSlot = u64(context.asOfSlot, 'context.asOfSlot');
  const maxSlotLag = u64(context.maxSlotLag, 'context.maxSlotLag');
  if (key(context.expectedBlockHash, 'context.expectedBlockHash') !== blockHash) invalid('block hash differs from expected observation');
  if (asOfSlot < slot || asOfSlot - slot > maxSlotLag) invalid('observation is future-dated or stale by supplied slot policy');
  if (typeof context.topK !== 'number' || !Number.isSafeInteger(context.topK)
    || context.topK < 1 || context.topK > MAX_ROWS) invalid('topK must be an explicit bounded integer');
  if (!Array.isArray(snapshot.balances) || Object.getPrototypeOf(snapshot.balances) !== Array.prototype
    || snapshot.balances.length > MAX_ROWS) invalid('balances exceed the input bound');
  if (Reflect.ownKeys(snapshot.balances).length !== snapshot.balances.length + 1) {
    invalid('balances has unsupported properties');
  }
  for (let index = 0; index < snapshot.balances.length; index += 1) {
    const descriptor = Object.getOwnPropertyDescriptor(snapshot.balances, String(index));
    if (!descriptor || !Object.hasOwn(descriptor, 'value') || !descriptor.enumerable) {
      invalid('balances must contain only dense data elements');
    }
  }

  const addresses = new Set();
  let observed = 0n;
  const rows = snapshot.balances.map((input, index) => {
    exact(input, ['address', 'mint', 'slot', 'blockHash', 'amount'], `balances[${index}]`);
    const address = key(input.address, `balances[${index}].address`);
    if (addresses.has(address)) invalid('duplicate observed address');
    addresses.add(address);
    if (key(input.mint, `balances[${index}].mint`) !== mint
      || u64(input.slot, `balances[${index}].slot`) !== slot
      || key(input.blockHash, `balances[${index}].blockHash`) !== blockHash) {
      invalid('balance belongs to another mint, slot, or fork');
    }
    const amount = u64(input.amount, `balances[${index}].amount`);
    observed += amount;
    if (observed > totalSupply) invalid('observed balances exceed stated supply');
    return { address, amount };
  });
  rows.sort((left, right) => left.amount === right.amount
    ? left.address < right.address ? -1 : left.address > right.address ? 1 : 0
    : left.amount > right.amount ? -1 : 1);
  const selected = rows.slice(0, context.topK);
  const topBalance = selected.reduce((sum, row) => sum + row.amount, 0n);
  return Object.freeze({
    kind: 'caller-asserted-observation', verifiedChain: false,
    mint, addressKind: snapshot.addressKind, totalSupply: totalSupply.toString(),
    source: Object.freeze({ sourceId: id, slot: slot.toString(), blockHash,
      claimedFinality: snapshot.source.finality, asOfSlot: asOfSlot.toString(),
      observedSlotLag: (asOfSlot - slot).toString() }),
    observedAddressCount: rows.length,
    knownObservedBalance: observed.toString(),
    unobservedSupply: (totalSupply - observed).toString(),
    knownObservedShare: ratio(observed, totalSupply),
    topK: Object.freeze({ requested: context.topK, count: selected.length,
      balance: topBalance.toString(), share: ratio(topBalance, totalSupply),
      addresses: Object.freeze(selected.map((row) => Object.freeze({
        address: row.address, amount: row.amount.toString(),
      }))) }),
  });
}
