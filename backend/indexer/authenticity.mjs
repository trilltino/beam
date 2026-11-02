import { createHash } from 'node:crypto';
import { PROGRAM_ID, decodeBase58Pubkey, encodeBase58Pubkey } from '../../sdk/beam-native.mjs';
import { buildSyntheticWebFixture, IndexerError } from './index.mjs';

export const LOCAL_PROOF_VERSION = 'beam-local-proof-simulation/v1';
const HASH = /^[0-9a-f]{64}$/;
const U64 = /^(0|[1-9][0-9]*)$/;
const U64_MAX = (1n << 64n) - 1n;

function fail(code, message) { throw new IndexerError(code, message); }
function exact(value, fields, name) {
  if (value === null || typeof value !== 'object'
    || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) fail('INVALID_PROOF', `${name} must be a plain object`);
  if (Reflect.ownKeys(value).length !== fields.length) fail('INVALID_PROOF', `${name} has missing or extra fields`);
  for (const field of fields) {
    const descriptor = Object.getOwnPropertyDescriptor(value, field);
    if (!descriptor || !Object.hasOwn(descriptor, 'value') || !descriptor.enumerable) fail('INVALID_PROOF', `${name}.${field} must be an own data field`);
  }
}
function key(value, name) {
  try {
    if (encodeBase58Pubkey(decodeBase58Pubkey(value)) !== value) throw new Error('noncanonical');
  } catch { fail('INVALID_PROOF', `${name} must be a canonical public key`); }
  return value;
}
function slot(value, name) {
  if (typeof value !== 'string' || value.length > 20 || !U64.test(value) || BigInt(value) > U64_MAX) fail('INVALID_PROOF', `${name} must be a canonical u64 slot`);
  return value;
}
function digest(value, name) {
  if (typeof value !== 'string' || !HASH.test(value)) fail('INVALID_PROOF', `${name} must be a SHA-256 hex digest`);
  return value;
}
function shaHex(hex) { return createHash('sha256').update(Buffer.from(hex, 'hex')).digest('hex'); }

/** Exercise required proof hooks locally; never issue an authenticated-chain label. */
export async function simulateSourceProofBoundary(indexer, evidence, proof, verifier) {
  const wrapped = buildSyntheticWebFixture(indexer, evidence);
  exact(proof, ['version', 'root', 'transaction', 'account', 'association'], 'proof');
  if (proof.version !== LOCAL_PROOF_VERSION) fail('INVALID_PROOF', 'unsupported local proof version');
  exact(proof.root, ['genesisHash', 'slot', 'hash', 'commitment'], 'root');
  exact(proof.transaction, ['signature', 'eventIndex', 'slot', 'blockHash', 'programId', 'dataHash'], 'transaction');
  exact(proof.account, ['address', 'ownerProgram', 'slot', 'blockHash', 'dataHash'], 'account');
  exact(proof.association, ['mint', 'curveAccount', 'quoteMint'], 'association');
  exact(verifier, ['verifyRoot', 'verifyTransaction', 'verifyAccount', 'verifyAssociation'], 'verifier');
  for (const name of ['verifyRoot', 'verifyTransaction', 'verifyAccount', 'verifyAssociation']) {
    if (typeof verifier[name] !== 'function') fail('INVALID_PROOF', `missing ${name} hook`);
  }
  const checks = Object.freeze(Object.fromEntries(
    ['verifyRoot', 'verifyTransaction', 'verifyAccount', 'verifyAssociation']
      .map((name) => [name, verifier[name].bind(verifier)])));
  const description = indexer.describe();
  if (description.stateDigest !== wrapped.stateDigest || description.revision !== wrapped.revision
    || !description.tip || !description.finalized || description.tip.hash !== description.finalized.hash) fail('STALE_SOURCE', 'source is not a stable finalized fixture tip');
  const token = wrapped.page.tokens[0];
  const webEvent = wrapped.page.events[0];
  const eventPage = indexer.query({ kind: 'complete-event', mint: token.mint, limit: 2 });
  const curvePage = indexer.latestCurve(token.curveAccount);
  const event = eventPage.items[0];
  const curve = curvePage.item;
  if (!event || !curve || eventPage.stateDigest !== wrapped.stateDigest || curvePage.stateDigest !== wrapped.stateDigest) fail('STALE_SOURCE', 'records changed during proof preparation');
  // Detach and freeze before any asynchronous hook can see caller-owned values.
  const root = Object.freeze({ ...proof.root });
  const tx = Object.freeze({ ...proof.transaction });
  const account = Object.freeze({ ...proof.account });
  const association = Object.freeze({ ...proof.association });
  const source = Object.freeze({ stateDigest: description.stateDigest, revision: description.revision,
    tip: Object.freeze({ ...description.tip }), finalized: Object.freeze({ ...description.finalized }) });
  key(root.genesisHash, 'root.genesisHash'); key(root.hash, 'root.hash'); slot(root.slot, 'root.slot');
  key(tx.blockHash, 'transaction.blockHash'); key(tx.programId, 'transaction.programId'); slot(tx.slot, 'transaction.slot'); digest(tx.dataHash, 'transaction.dataHash');
  key(account.address, 'account.address'); key(account.ownerProgram, 'account.ownerProgram'); key(account.blockHash, 'account.blockHash'); slot(account.slot, 'account.slot'); digest(account.dataHash, 'account.dataHash');
  key(association.mint, 'association.mint'); key(association.curveAccount, 'association.curveAccount');
  if (association.quoteMint !== null) key(association.quoteMint, 'association.quoteMint');
  if (root.commitment !== 'finalized' || root.slot !== description.tip.slot || root.hash !== description.tip.hash
    || tx.signature !== webEvent.signature || tx.eventIndex !== webEvent.eventIndex
    || tx.slot !== event.block.slot || tx.blockHash !== event.block.hash || tx.programId !== PROGRAM_ID || tx.dataHash !== shaHex(event.dataHex)
    || account.address !== curve.address || account.ownerProgram !== PROGRAM_ID
    || account.slot !== curve.block.slot || account.blockHash !== curve.block.hash || account.dataHash !== shaHex(curve.dataHex)
    || association.mint !== token.mint || association.curveAccount !== token.curveAccount || association.quoteMint !== token.quoteMint
    || BigInt(tx.slot) > BigInt(root.slot) || BigInt(account.slot) > BigInt(root.slot)) fail('PROOF_MISMATCH', 'claimed proof is not bound to the retained fixture');
  try {
    if (await checks.verifyRoot(root, source) !== true
      || await checks.verifyTransaction(tx, root) !== true
      || await checks.verifyAccount(account, root) !== true
      || await checks.verifyAssociation(association, tx, account) !== true) fail('PROOF_REJECTED', 'a required local proof hook rejected');
  } catch { fail('PROOF_REJECTED', 'a required local proof hook failed'); }
  const after = indexer.describe();
  if (after.stateDigest !== wrapped.stateDigest || after.revision !== wrapped.revision) fail('STALE_SOURCE', 'fixture changed while proof hooks ran');
  return Object.freeze({ kind: 'local-proof-simulation', live: false, verifiedChain: false,
    sourceId: wrapped.sourceId, stateDigest: wrapped.stateDigest, revision: wrapped.revision });
}
