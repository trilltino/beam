// Development-only, dependency-free derivation of the program-derived and
// associated-token addresses that sdk/beam-native.mjs builders need. Seeds
// mirror the Anchor constraints on `Create`/`LegacyBuy`/`LegacySell`
// (programs/beam/src/lib.rs) and `CreateV2` (programs/beam/src/create_v2.rs).
// Derivation reads no chain state: it does not prove that an account exists,
// is owned by the expected program, or holds valid data. Nothing here signs,
// sends, or contacts an RPC endpoint.

import { PROGRAM_ID, decodeBase58Pubkey, encodeBase58Pubkey } from './beam-native.mjs';

export const SYSTEM_PROGRAM_ID = '11111111111111111111111111111111';
export const TOKEN_PROGRAM_ID = 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA';
export const TOKEN_2022_PROGRAM_ID = 'TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb';
export const ASSOCIATED_TOKEN_PROGRAM_ID = 'ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL';
export const METADATA_PROGRAM_ID = 'metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s';
export const MAYHEM_PROGRAM_ID = 'MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e';
export const RENT_SYSVAR_ID = 'SysvarRent111111111111111111111111111111111';

const MAX_SEEDS = 16;
const MAX_SEED_LEN = 32;
const PDA_MARKER = new TextEncoder().encode('ProgramDerivedAddress');

// ---------------------------------------------------------------- SHA-256

const K = Uint32Array.from([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

// Synchronous so that derivation works identically in Node and browsers
// without WebCrypto's async API. Tests compare it with node:crypto.
export function sha256(input) {
  if (!(input instanceof Uint8Array)) throw new TypeError('sha256 input must be bytes');
  const bitLength = BigInt(input.length) * 8n;
  const padded = new Uint8Array((((input.length + 9 + 63) >> 6) << 6));
  padded.set(input);
  padded[input.length] = 0x80;
  new DataView(padded.buffer).setBigUint64(padded.length - 8, bitLength, false);
  const h = Uint32Array.from([
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ]);
  const w = new Uint32Array(64);
  const view = new DataView(padded.buffer);
  const rotr = (x, n) => (x >>> n) | (x << (32 - n));
  for (let block = 0; block < padded.length; block += 64) {
    for (let i = 0; i < 16; i++) w[i] = view.getUint32(block + i * 4, false);
    for (let i = 16; i < 64; i++) {
      const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
    }
    let [a, b, c, d, e, f, g, hh] = h;
    for (let i = 0; i < 64; i++) {
      const s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const t1 = (hh + s1 + ch + K[i] + w[i]) >>> 0;
      const s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = (s0 + maj) >>> 0;
      hh = g; g = f; f = e; e = (d + t1) >>> 0;
      d = c; c = b; b = a; a = (t1 + t2) >>> 0;
    }
    h[0] += a; h[1] += b; h[2] += c; h[3] += d;
    h[4] += e; h[5] += f; h[6] += g; h[7] += hh;
  }
  const out = new Uint8Array(32);
  const outView = new DataView(out.buffer);
  for (let i = 0; i < 8; i++) outView.setUint32(i * 4, h[i], false);
  return out;
}

// ------------------------------------------------- ed25519 point validity

const P = (1n << 255n) - 19n;

function mod(a) {
  const r = a % P;
  return r >= 0n ? r : r + P;
}

function pow(base, exponent) {
  let result = 1n;
  let b = mod(base);
  let e = exponent;
  while (e > 0n) {
    if (e & 1n) result = (result * b) % P;
    b = (b * b) % P;
    e >>= 1n;
  }
  return result;
}

const D = mod(-121665n * pow(121666n, P - 2n));

// Matches curve25519-dalek `CompressedEdwardsY::decompress().is_some()`, which
// Solana uses to reject PDAs: the high (sign) bit is ignored for validity, a
// non-canonical y is reduced mod p, and the point is valid iff
// (y^2 - 1) / (d*y^2 + 1) is a square (zero included).
export function isOnCurve(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length !== 32) throw new TypeError('point must be 32 bytes');
  let y = 0n;
  for (let i = 31; i >= 0; i--) y = (y << 8n) | BigInt(i === 31 ? bytes[i] & 0x7f : bytes[i]);
  y = mod(y);
  const y2 = (y * y) % P;
  const u = mod(y2 - 1n);
  const v = mod(D * y2 + 1n);
  const ratio = (u * pow(v, P - 2n)) % P;
  return ratio === 0n || pow(ratio, (P - 1n) / 2n) === 1n;
}

// ------------------------------------------------- program-derived address

function concat(parts) {
  const out = new Uint8Array(parts.reduce((length, part) => length + part.length, 0));
  let offset = 0;
  for (const part of parts) { out.set(part, offset); offset += part.length; }
  return out;
}

function checkSeeds(seeds) {
  if (!Array.isArray(seeds)) throw new TypeError('seeds must be an array of byte arrays');
  if (seeds.length > MAX_SEEDS) throw new RangeError(`at most ${MAX_SEEDS} seeds are allowed`);
  for (const seed of seeds) {
    if (!(seed instanceof Uint8Array)) throw new TypeError('each seed must be a Uint8Array');
    if (seed.length > MAX_SEED_LEN) throw new RangeError(`each seed must be at most ${MAX_SEED_LEN} bytes`);
  }
}

// Mirrors `Pubkey::create_program_address`; throws if the hash is a curve point.
export function createProgramAddress(seeds, programId) {
  checkSeeds(seeds);
  const hash = sha256(concat([...seeds, decodeBase58Pubkey(programId), PDA_MARKER]));
  if (isOnCurve(hash)) throw new RangeError('seeds produce an on-curve address');
  return encodeBase58Pubkey(hash);
}

// Mirrors `Pubkey::try_find_program_address`: bumps 255 down to 1 (Solana
// never tries bump 0), returning the first off-curve address.
export function findProgramAddress(seeds, programId) {
  checkSeeds(seeds);
  if (seeds.length >= MAX_SEEDS) throw new RangeError(`at most ${MAX_SEEDS - 1} seeds leave room for a bump`);
  for (let bump = 255; bump > 0; bump--) {
    try {
      return { address: createProgramAddress([...seeds, Uint8Array.of(bump)], programId), bump };
    } catch (error) {
      if (!(error instanceof RangeError) || !/on-curve/.test(error.message)) throw error;
    }
  }
  throw new RangeError('no viable bump seed');
}

const text = (value) => new TextEncoder().encode(value);
const key = (value, name) => {
  try { return decodeBase58Pubkey(value); } catch (error) { throw new TypeError(`${name}: ${error.message}`); }
};

function tokenProgramOf(tokenProgram) {
  if (tokenProgram !== TOKEN_PROGRAM_ID && tokenProgram !== TOKEN_2022_PROGRAM_ID) {
    throw new RangeError('tokenProgram must be SPL Token or Token-2022');
  }
  return tokenProgram;
}

// Address only; it neither checks nor creates the token account.
export function getAssociatedTokenAddress({ owner, mint, tokenProgram }) {
  const program = tokenProgramOf(tokenProgram);
  return findProgramAddress(
    [key(owner, 'owner'), key(program, 'tokenProgram'), key(mint, 'mint')],
    ASSOCIATED_TOKEN_PROGRAM_ID,
  ).address;
}

// ------------------------------------------------------- Beam program PDAs

const beam = (seeds) => findProgramAddress(seeds, PROGRAM_ID);

export const deriveGlobal = () => beam([text('global')]);
export const deriveMintAuthority = () => beam([text('mint-authority')]);
export const deriveEventAuthority = () => beam([text('__event_authority')]);
export const deriveGlobalVolumeAccumulator = () => beam([text('global_volume_accumulator')]);
export const deriveBondingCurve = (mint) => beam([text('bonding-curve'), key(mint, 'mint')]);
export const deriveCreatorVault = (creator) => beam([text('creator-vault'), key(creator, 'creator')]);
export const deriveUserVolumeAccumulator = (user) => beam([text('user_volume_accumulator'), key(user, 'user')]);

// `Create` uses `seeds::program = mpl_token_metadata`.
export const deriveMetadata = (mint) => findProgramAddress(
  [text('metadata'), key(METADATA_PROGRAM_ID, 'metadata program'), key(mint, 'mint')],
  METADATA_PROGRAM_ID,
);

// `CreateV2` requires these retained Mayhem slots even though Beam rejects
// Mayhem creation; the program checks them with `seeds::program = MAYHEM_ID`.
export const deriveMayhemGlobalParams = () => findProgramAddress([text('global-params')], MAYHEM_PROGRAM_ID);
export const deriveMayhemSolVault = () => findProgramAddress([text('sol-vault')], MAYHEM_PROGRAM_ID);
export const deriveMayhemState = (mint) => findProgramAddress([text('mayhem-state'), key(mint, 'mint')], MAYHEM_PROGRAM_ID);

// ---------------------------------------------------- account resolution
// Each resolver returns an `accounts` object accepted by the matching
// beam-native.mjs builder. Inputs the program does not derive are required
// arguments; nothing is defaulted from a guess.

export function resolveCreateAccounts({ mint, user }) {
  key(user, 'user');
  const bondingCurve = deriveBondingCurve(mint).address;
  return {
    mint,
    mintAuthority: deriveMintAuthority().address,
    bondingCurve,
    associatedBondingCurve: getAssociatedTokenAddress({ owner: bondingCurve, mint, tokenProgram: TOKEN_PROGRAM_ID }),
    global: deriveGlobal().address,
    mplTokenMetadata: METADATA_PROGRAM_ID,
    metadata: deriveMetadata(mint).address,
    user,
    systemProgram: SYSTEM_PROGRAM_ID,
    tokenProgram: TOKEN_PROGRAM_ID,
    associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    rent: RENT_SYSVAR_ID,
    eventAuthority: deriveEventAuthority().address,
    program: PROGRAM_ID,
  };
}

// `mayhem_token_vault` is an unconstrained account in Beam's `CreateV2`, so
// the caller must choose it explicitly; this module does not invent one.
export function resolveCreateV2Accounts({ mint, user, mayhemTokenVault }) {
  key(user, 'user');
  key(mayhemTokenVault, 'mayhemTokenVault');
  const bondingCurve = deriveBondingCurve(mint).address;
  return {
    mint,
    mintAuthority: deriveMintAuthority().address,
    bondingCurve,
    associatedBondingCurve: getAssociatedTokenAddress({ owner: bondingCurve, mint, tokenProgram: TOKEN_2022_PROGRAM_ID }),
    global: deriveGlobal().address,
    user,
    systemProgram: SYSTEM_PROGRAM_ID,
    tokenProgram: TOKEN_2022_PROGRAM_ID,
    associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    mayhemProgramId: MAYHEM_PROGRAM_ID,
    globalParams: deriveMayhemGlobalParams().address,
    solVault: deriveMayhemSolVault().address,
    mayhemState: deriveMayhemState(mint).address,
    mayhemTokenVault,
    eventAuthority: deriveEventAuthority().address,
    program: PROGRAM_ID,
  };
}

// Inherited Pump fee accounts (`feeRecipient`, `feeConfig`, `feeProgram`) are
// unresolved for Beam and must be passed explicitly. `creator` must be the
// `creator` field of the decoded bonding curve, and `tokenProgram` the mint's
// owning token program; both are chain state this module cannot verify.
// `associatedUser` defaults to the user's ATA, which must already exist.
function resolveTrade({ mint, user, creator, tokenProgram, feeRecipient, feeConfig, feeProgram, associatedUser }) {
  key(user, 'user');
  key(creator, 'creator');
  for (const [name, value] of Object.entries({ feeRecipient, feeConfig, feeProgram })) {
    if (value === undefined) throw new TypeError(`${name} is unresolved for Beam and must be supplied explicitly`);
    key(value, name);
  }
  const program = tokenProgramOf(tokenProgram);
  const bondingCurve = deriveBondingCurve(mint).address;
  const userAta = associatedUser ?? getAssociatedTokenAddress({ owner: user, mint, tokenProgram: program });
  key(userAta, 'associatedUser');
  return {
    global: deriveGlobal().address,
    feeRecipient,
    mint,
    bondingCurve,
    associatedBondingCurve: getAssociatedTokenAddress({ owner: bondingCurve, mint, tokenProgram: program }),
    associatedUser: userAta,
    user,
    systemProgram: SYSTEM_PROGRAM_ID,
    tokenProgram: program,
    creatorVault: deriveCreatorVault(creator).address,
    eventAuthority: deriveEventAuthority().address,
    program: PROGRAM_ID,
    feeConfig,
    feeProgram,
  };
}

export function resolveLegacyBuyAccounts(input) {
  const accounts = resolveTrade(input);
  return {
    ...accounts,
    globalVolumeAccumulator: deriveGlobalVolumeAccumulator().address,
    userVolumeAccumulator: deriveUserVolumeAccumulator(input.user).address,
  };
}

export function resolveLegacySellAccounts(input) {
  return resolveTrade(input);
}

// ------------------------------------------------ legacy remaining accounts

// `check_bonding_curve_v2` (programs/beam/src/sell.rs) compares
// remaining_accounts[0] with this PDA when a creator fee is charged.
export const deriveBondingCurveV2 = (mint) => beam([text('bonding-curve-v2'), key(mint, 'mint')]);

// Legacy `buy` and `sell` on a non-cashback curve read remaining_accounts as
// [bonding-curve-v2 PDA, buyback fee recipient]; the buyback recipient
// receives lamports and must be listed in `Global.buyback_fee_recipients`.
// Choosing which listed recipient to use is the caller's decision; pass a
// decoded Global (sdk/beam-global.mjs) to have membership checked here.
// Beam rejects cashback creation; cashback sells use a different layout and
// are not modelled.
export function legacyTradeRemainingAccounts({ mint, buybackFeeRecipient, global }) {
  key(buybackFeeRecipient, 'buybackFeeRecipient');
  if (buybackFeeRecipient === SYSTEM_PROGRAM_ID) throw new RangeError('buybackFeeRecipient must be nonzero');
  if (global !== undefined && !global.buybackFeeRecipients?.includes(buybackFeeRecipient)) {
    throw new RangeError('buybackFeeRecipient is not listed in Global.buyback_fee_recipients');
  }
  return [
    { pubkey: deriveBondingCurveV2(mint).address, isSigner: false, isWritable: false },
    { pubkey: buybackFeeRecipient, isSigner: false, isWritable: true },
  ];
}

// -------------------------------------------------- V2 trade accounts

export const WSOL_MINT = 'So11111111111111111111111111111111111111112';
// `buy_v2.rs` / `sell_v2.rs` derive `sharing_config` under this hard-coded
// inherited Pump fee program ID, independently of the `fee_program` account.
export const INHERITED_FEE_PROGRAM_ID = 'pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ';

export const deriveSharingConfig = (mint) => findProgramAddress(
  [text('sharing-config'), key(mint, 'mint')], INHERITED_FEE_PROGRAM_ID,
);

// Native-SOL curves only: the V2 handlers then require `quoteMint` = WSOL and
// derive every quote ATA with SPL Token, while paying in lamports. The
// returned object uses the Anchor field names that the draft
// `beam-v2-trades.mjs` builders take. As with the legacy resolvers,
// `creator`, `baseTokenProgram` (the mint's owner), the fee recipient, the
// buyback recipient and the inherited fee accounts are caller inputs. Pass a
// decoded Global to have recipient membership checked here.
function resolveV2({
  mint, user, creator, baseTokenProgram, feeRecipient, buybackFeeRecipient, feeConfig, feeProgram, global,
}) {
  key(user, 'user');
  key(creator, 'creator');
  key(buybackFeeRecipient, 'buybackFeeRecipient');
  key(feeRecipient, 'feeRecipient');
  for (const [name, value] of Object.entries({ feeConfig, feeProgram })) {
    if (value === undefined) throw new TypeError(`${name} is unresolved for Beam and must be supplied explicitly`);
    key(value, name);
  }
  if (global !== undefined && !global.buybackFeeRecipients?.includes(buybackFeeRecipient)) {
    throw new RangeError('buybackFeeRecipient is not listed in Global.buyback_fee_recipients');
  }
  const base = tokenProgramOf(baseTokenProgram);
  const quote = (owner) => getAssociatedTokenAddress({ owner, mint: WSOL_MINT, tokenProgram: TOKEN_PROGRAM_ID });
  const bondingCurve = deriveBondingCurve(mint).address;
  const creatorVault = deriveCreatorVault(creator).address;
  const userVolumeAccumulator = deriveUserVolumeAccumulator(user).address;
  return {
    global: deriveGlobal().address,
    baseMint: mint,
    quoteMint: WSOL_MINT,
    baseTokenProgram: base,
    quoteTokenProgram: TOKEN_PROGRAM_ID,
    associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
    feeRecipient,
    associatedQuoteFeeRecipient: quote(feeRecipient),
    buybackFeeRecipient,
    associatedQuoteBuybackFeeRecipient: quote(buybackFeeRecipient),
    bondingCurve,
    associatedBaseBondingCurve: getAssociatedTokenAddress({ owner: bondingCurve, mint, tokenProgram: base }),
    associatedQuoteBondingCurve: quote(bondingCurve),
    user,
    associatedBaseUser: getAssociatedTokenAddress({ owner: user, mint, tokenProgram: base }),
    associatedQuoteUser: quote(user),
    creatorVault,
    associatedCreatorVault: quote(creatorVault),
    sharingConfig: deriveSharingConfig(mint).address,
    globalVolumeAccumulator: deriveGlobalVolumeAccumulator().address,
    userVolumeAccumulator,
    associatedUserVolumeAccumulator: quote(userVolumeAccumulator),
    feeConfig,
    feeProgram,
    systemProgram: SYSTEM_PROGRAM_ID,
    eventAuthority: deriveEventAuthority().address,
    program: PROGRAM_ID,
  };
}

export function resolveBuyV2Accounts(input) {
  return resolveV2(input);
}

// `SellV2` has no `global_volume_accumulator`.
export function resolveSellV2Accounts(input) {
  const { globalVolumeAccumulator, ...accounts } = resolveV2(input);
  return accounts;
}

// Returns a new instruction with `remaining` appended after the base metas.
export function withRemainingAccounts(instruction, remaining) {
  if (!instruction || !Array.isArray(instruction.keys)) throw new TypeError('instruction must come from a build* helper');
  if (!Array.isArray(remaining)) throw new TypeError('remaining must be an array of account metas');
  for (const meta of remaining) key(meta?.pubkey, 'remaining pubkey');
  return { ...instruction, keys: [...instruction.keys, ...remaining.map((meta) => ({ ...meta }))] };
}
