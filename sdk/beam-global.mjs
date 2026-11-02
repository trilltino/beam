// Development-only decoder for Beam's `Global` account (programs/beam/src/lib.rs)
// and the recipient checks that legacy `buy`/`sell` apply to it. Pure bytes in,
// values out: it does not verify the account owner, address or freshness.

import { decodeBase58Pubkey, encodeBase58Pubkey } from './beam-native.mjs';

export const GLOBAL_DISCRIMINATOR = Uint8Array.from([167, 232, 232, 177, 200, 108, 114, 127]);
export const GLOBAL_LENGTH = 8 + 1080;
const ZERO = '11111111111111111111111111111111';

function reader(data) {
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  let offset = 8;
  const take = (n) => { const out = data.subarray(offset, offset + n); offset += n; return out; };
  return {
    bool() {
      const value = data[offset++];
      if (value > 1) throw new RangeError('invalid Borsh boolean');
      return value === 1;
    },
    u8() { return data[offset++]; },
    u64() { const value = view.getBigUint64(offset, true); offset += 8; return value; },
    key() { return encodeBase58Pubkey(take(32)); },
    keys(n) { return Array.from({ length: n }, () => encodeBase58Pubkey(take(32))); },
    get offset() { return offset; },
  };
}

// Anchor accepts trailing bytes after the serialized fields, so does this.
export function decodeGlobal(input) {
  if (!(input instanceof Uint8Array)) throw new TypeError('Global account must be bytes');
  if (input.length < GLOBAL_LENGTH) throw new RangeError(`expected at least ${GLOBAL_LENGTH} Global bytes`);
  if (!GLOBAL_DISCRIMINATOR.every((byte, i) => input[i] === byte)) throw new RangeError('not a Global account');
  const r = reader(input);
  const global = {
    initialized: r.bool(),
    authority: r.key(),
    feeRecipient: r.key(),
    initialVirtualTokenReserves: r.u64(),
    initialVirtualSolReserves: r.u64(),
    initialRealTokenReserves: r.u64(),
    tokenTotalSupply: r.u64(),
    feeBasisPoints: r.u64(),
    withdrawAuthority: r.key(),
    enableMigrate: r.bool(),
    poolMigrationFee: r.u64(),
    creatorFeeBasisPoints: r.u64(),
    feeRecipients: r.keys(7),
    setCreatorAuthority: r.key(),
    adminSetCreatorAuthority: r.key(),
    createV2Enabled: r.bool(),
    whitelistPda: r.key(),
    reservedFeeRecipient: r.key(),
    mayhemModeEnabled: r.bool(),
    reservedFeeRecipients: r.keys(7),
    isCashbackEnabled: r.bool(),
    buybackFeeRecipients: r.keys(8),
    buybackBasisPoints: r.u64(),
    initialVirtualQuoteReserves: r.u64(),
    whitelistedQuoteMints: r.keys(1),
    creatorFeeConfigurable: r.bool(),
    maxConfigurableCreatorFeeBps: r.u64(),
    holderRewardClaimAuthority: r.key(),
    isHolderRewardEnabled: r.bool(),
    maxCurveDepth: r.u8(),
  };
  if (r.offset !== GLOBAL_LENGTH) throw new Error('Global layout length mismatch');
  return global;
}

// Pubkey ordering is lexicographic over its 32 bytes, as in Rust's derived Ord.
function compareKeys(a, b) {
  const x = decodeBase58Pubkey(a); const y = decodeBase58Pubkey(b);
  for (let i = 0; i < 32; i++) if (x[i] !== y[i]) return x[i] - y[i];
  return 0;
}

function requireKey(value, name) {
  if (typeof value !== 'string' || !value) throw new TypeError(`${name} must be a base58 pubkey`);
}

// Mirrors `validate_fee_recipient`. A non-Mayhem curve accepts
// `feeRecipient` or a member of `feeRecipients` found by Rust's
// `binary_search`. That search is only predictable on a sorted list, so a
// listed key in an unsorted list throws instead of guessing.
export function isAuthorizedFeeRecipient(global, key, { isMayhemMode = false } = {}) {
  requireKey(key, 'key');
  if (isMayhemMode) return key === global.reservedFeeRecipient || global.reservedFeeRecipients.includes(key);
  if (key === global.feeRecipient) return true;
  if (!global.feeRecipients.includes(key)) return false;
  const sorted = global.feeRecipients.every((k, i, all) => i === 0 || compareKeys(all[i - 1], k) <= 0);
  if (!sorted) throw new RangeError('feeRecipients is unsorted; the program\'s binary_search result cannot be predicted');
  return true;
}

// Mirrors `Global::validate_buyback_fee_recipient`.
export function isAuthorizedBuybackFeeRecipient(global, key) {
  requireKey(key, 'key');
  return key !== ZERO && global.buybackFeeRecipients.includes(key);
}

export function authorizedBuybackFeeRecipients(global) {
  return [...new Set(global.buybackFeeRecipients.filter((k) => k !== ZERO))];
}
