import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { PROGRAM_ID, buildCreate, buildCreateV2, buildBuy, buildSell } from './beam-native.mjs';
import {
  SYSTEM_PROGRAM_ID, TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID,
  METADATA_PROGRAM_ID, MAYHEM_PROGRAM_ID,
  sha256, isOnCurve, createProgramAddress, findProgramAddress, getAssociatedTokenAddress,
  deriveGlobal, deriveMintAuthority, deriveEventAuthority, deriveGlobalVolumeAccumulator,
  deriveBondingCurve, deriveCreatorVault, deriveUserVolumeAccumulator, deriveMetadata,
  deriveMayhemGlobalParams, deriveMayhemSolVault, deriveMayhemState,
  resolveCreateAccounts, resolveCreateV2Accounts, resolveLegacyBuyAccounts, resolveLegacySellAccounts,
  deriveBondingCurveV2, legacyTradeRemainingAccounts, withRemainingAccounts,
  resolveBuyV2Accounts, resolveSellV2Accounts, deriveSharingConfig, WSOL_MINT, INHERITED_FEE_PROGRAM_ID,
} from './beam-pdas.mjs';
import { decodeGlobal } from './beam-global.mjs';

const fixtureBytes = readFileSync(new URL('./fixtures/pdas.json', import.meta.url));
const fixture = JSON.parse(fixtureBytes);
const receipt = JSON.parse(readFileSync(new URL('./fixtures/pdas.receipt.json', import.meta.url)));
const fromHex = (value) => Uint8Array.from(Buffer.from(value, 'hex'));
const { mint, user, creator } = fixture.inputs;
const expectedPda = Object.fromEntries(fixture.pdas.map((p) => [p.name, p]));
const expectedAta = Object.fromEntries(fixture.associatedTokenAccounts.map((a) => [a.name, a.address]));
// Synthetic placeholders for accounts this module deliberately leaves unresolved.
const SYNTHETIC = {
  feeRecipient: '4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi',
  feeConfig: '8qbHbw2BbbTHBW1sbeqakYXVKRQM8Ne7pLK7m6CVfeR',
  feeProgram: 'CktRuQ2mttgRGkXJtyksdKHjUdc2C4TgDzyB98oEzy8',
  mayhemTokenVault: 'GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq',
};

test('fixture bytes match the recorded Rust producer receipt', () => {
  assert.equal(createHash('sha256').update(fixtureBytes).digest('hex'), receipt.stdoutSha256);
  assert.equal(fixtureBytes.length, receipt.stdoutBytes);
  const source = readFileSync(new URL('./tools/generate-pda-fixtures.rs', import.meta.url));
  assert.equal(createHash('sha256').update(source).digest('hex'), receipt.sourceSha256);
});

test('program IDs match the constants compiled into beam-program', () => {
  assert.deepEqual(fixture.programIds, {
    beam: PROGRAM_ID, mayhem: MAYHEM_PROGRAM_ID, metadata: METADATA_PROGRAM_ID,
    token: TOKEN_PROGRAM_ID, token2022: TOKEN_2022_PROGRAM_ID,
    associatedToken: ASSOCIATED_TOKEN_PROGRAM_ID, system: SYSTEM_PROGRAM_ID,
  });
});

test('sha256 matches node:crypto across padding boundaries', () => {
  for (const length of [0, 1, 31, 32, 55, 56, 57, 63, 64, 65, 119, 120, 128, 1000]) {
    const input = Uint8Array.from({ length }, (_, i) => (i * 131 + length) & 255);
    assert.equal(Buffer.from(sha256(input)).toString('hex'), createHash('sha256').update(input).digest('hex'), `length ${length}`);
  }
  assert.throws(() => sha256('text'), TypeError);
});

test('curve membership matches curve25519-dalek decompression', () => {
  assert.ok(fixture.curvePoints.length >= 100);
  assert.ok(fixture.curvePoints.some((p) => p.onCurve) && fixture.curvePoints.some((p) => !p.onCurve));
  for (const point of fixture.curvePoints) {
    assert.equal(isOnCurve(fromHex(point.hex)), point.onCurve, point.hex);
  }
  assert.throws(() => isOnCurve(new Uint8Array(31)), TypeError);
});

test('raw seed derivation reproduces every Rust find_program_address result', () => {
  for (const pda of fixture.pdas) {
    const seeds = pda.seedsHex.map(fromHex);
    assert.deepEqual(findProgramAddress(seeds, pda.programId), { address: pda.address, bump: pda.bump }, pda.name);
    assert.equal(createProgramAddress([...seeds, Uint8Array.of(pda.bump)], pda.programId), pda.address);
  }
  assert.ok(fixture.pdas.some((p) => p.bump < 255), 'fixture exercises skipped on-curve bumps');
});

test('named Beam and Mayhem derivations match Anchor seeds', () => {
  const cases = {
    global: deriveGlobal(), mintAuthority: deriveMintAuthority(),
    eventAuthority: deriveEventAuthority(), globalVolumeAccumulator: deriveGlobalVolumeAccumulator(),
    bondingCurve: deriveBondingCurve(mint), creatorVault: deriveCreatorVault(creator),
    userVolumeAccumulator: deriveUserVolumeAccumulator(user), metadata: deriveMetadata(mint),
    mayhemGlobalParams: deriveMayhemGlobalParams(), mayhemSolVault: deriveMayhemSolVault(),
    mayhemState: deriveMayhemState(mint), bondingCurveV2: deriveBondingCurveV2(mint),
  };
  assert.deepEqual(Object.keys(cases).sort(), Object.keys(expectedPda).sort());
  for (const [name, actual] of Object.entries(cases)) {
    assert.deepEqual(actual, { address: expectedPda[name].address, bump: expectedPda[name].bump }, name);
  }
});

test('associated token addresses match spl-associated-token-account', () => {
  for (const ata of fixture.associatedTokenAccounts) {
    assert.equal(getAssociatedTokenAddress(ata), ata.address, ata.name);
  }
  assert.throws(() => getAssociatedTokenAddress({ owner: user, mint, tokenProgram: SYSTEM_PROGRAM_ID }), /SPL Token or Token-2022/);
});

test('seed limits mirror Solana and reject malformed input', () => {
  const seed = new Uint8Array(32);
  assert.throws(() => findProgramAddress([new Uint8Array(33)], PROGRAM_ID), /at most 32 bytes/);
  assert.throws(() => createProgramAddress(Array(17).fill(seed), PROGRAM_ID), /at most 16 seeds/);
  assert.doesNotThrow(() => createProgramAddress(Array(16).fill(new Uint8Array(1)), PROGRAM_ID));
  assert.throws(() => findProgramAddress(Array(16).fill(seed), PROGRAM_ID), /room for a bump/);
  assert.throws(() => findProgramAddress(['global'], PROGRAM_ID), TypeError);
  assert.throws(() => findProgramAddress([seed], 'not-a-key'), RangeError);
  assert.throws(() => deriveBondingCurve('0OIl'), /mint/);
  const onCurveBump = fixture.pdas.find((p) => p.bump < 255);
  assert.throws(
    () => createProgramAddress([...onCurveBump.seedsHex.map(fromHex), Uint8Array.of(255)], onCurveBump.programId),
    /on-curve/,
  );
});

test('resolved Create and CreateV2 accounts feed the existing builders', () => {
  const create = resolveCreateAccounts({ mint, user });
  assert.equal(create.bondingCurve, expectedPda.bondingCurve.address);
  assert.equal(create.associatedBondingCurve, expectedAta.bondingCurveToken);
  assert.equal(create.metadata, expectedPda.metadata.address);
  assert.equal(create.mintAuthority, expectedPda.mintAuthority.address);
  const args = { name: 'Beam', symbol: 'BEAM', uri: 'https://example.invalid/beam.json', creator };
  const ix = buildCreate(create, args);
  assert.equal(ix.keys.length, 14);
  assert.deepEqual(ix.keys.filter((k) => k.isSigner).map((k) => k.pubkey), [mint, user]);

  const createV2 = resolveCreateV2Accounts({ mint, user, mayhemTokenVault: SYNTHETIC.mayhemTokenVault });
  assert.equal(createV2.tokenProgram, TOKEN_2022_PROGRAM_ID);
  assert.equal(createV2.associatedBondingCurve, expectedAta.bondingCurveToken2022);
  assert.equal(createV2.mayhemState, expectedPda.mayhemState.address);
  assert.equal(createV2.globalParams, expectedPda.mayhemGlobalParams.address);
  assert.equal(createV2.solVault, expectedPda.mayhemSolVault.address);
  const v2 = buildCreateV2(createV2, {
    ...args, isMayhemMode: false, isCashbackEnabled: false, creatorFeeBps: 0, isHolderReward: false,
  });
  assert.equal(v2.keys.length, 16);
  assert.throws(() => resolveCreateV2Accounts({ mint, user }), /mayhemTokenVault/);
});

test('resolved legacy trade accounts keep unresolved fee accounts explicit', () => {
  for (const [resolve, build, args, length] of [
    [resolveLegacyBuyAccounts, buildBuy, { amount: 1, maxSolCost: 2, trackVolume: false, partialFill: false }, 16],
    [resolveLegacySellAccounts, buildSell, { amount: 1, minSolOutput: 0 }, 14],
  ]) {
    for (const [tokenProgram, ataName] of [[TOKEN_PROGRAM_ID, 'Token'], [TOKEN_2022_PROGRAM_ID, 'Token2022']]) {
      const accounts = resolve({ mint, user, creator, tokenProgram, ...SYNTHETIC });
      assert.equal(accounts.associatedBondingCurve, expectedAta[`bondingCurve${ataName}`]);
      assert.equal(accounts.associatedUser, expectedAta[`user${ataName}`]);
      assert.equal(accounts.creatorVault, expectedPda.creatorVault.address);
      assert.equal(accounts.feeConfig, SYNTHETIC.feeConfig);
      assert.equal(build(accounts, args).keys.length, length);
    }
    for (const missing of ['feeRecipient', 'feeConfig', 'feeProgram']) {
      const input = { mint, user, creator, tokenProgram: TOKEN_PROGRAM_ID, ...SYNTHETIC };
      delete input[missing];
      assert.throws(() => resolve(input), new RegExp(`${missing} is unresolved`));
    }
    assert.throws(() => resolve({ mint, user, tokenProgram: TOKEN_PROGRAM_ID, ...SYNTHETIC }), /creator/);
    assert.throws(() => resolve({ mint, user, creator, tokenProgram: PROGRAM_ID, ...SYNTHETIC }), /SPL Token or Token-2022/);
    const override = resolve({ mint, user, creator, tokenProgram: TOKEN_PROGRAM_ID, ...SYNTHETIC, associatedUser: SYNTHETIC.feeRecipient });
    assert.equal(override.associatedUser, SYNTHETIC.feeRecipient);
  }
  const buy = resolveLegacyBuyAccounts({ mint, user, creator, tokenProgram: TOKEN_PROGRAM_ID, ...SYNTHETIC });
  assert.equal(buy.userVolumeAccumulator, expectedPda.userVolumeAccumulator.address);
  assert.equal(buy.globalVolumeAccumulator, expectedPda.globalVolumeAccumulator.address);
});

test('legacy remaining accounts follow the handlers\' [bonding-curve-v2, buyback] layout', () => {
  const globalFixture = JSON.parse(readFileSync(new URL('./fixtures/global.json', import.meta.url)));
  const global = decodeGlobal(Uint8Array.from(Buffer.from(globalFixture.cases.find((c) => c.name === 'distinct').dataHex, 'hex')));
  const buyback = global.buybackFeeRecipients[3];
  const remaining = legacyTradeRemainingAccounts({ mint, buybackFeeRecipient: buyback, global });
  assert.deepEqual(remaining, [
    { pubkey: expectedPda.bondingCurveV2.address, isSigner: false, isWritable: false },
    { pubkey: buyback, isSigner: false, isWritable: true },
  ]);
  assert.throws(() => legacyTradeRemainingAccounts({ mint, buybackFeeRecipient: global.feeRecipient, global }), /not listed/);
  assert.throws(() => legacyTradeRemainingAccounts({ mint, buybackFeeRecipient: SYSTEM_PROGRAM_ID }), /nonzero/);
  assert.throws(() => legacyTradeRemainingAccounts({ mint }), /buybackFeeRecipient/);
  assert.equal(legacyTradeRemainingAccounts({ mint, buybackFeeRecipient: user })[1].pubkey, user, 'membership check is optional without a Global');

  const accounts = resolveLegacyBuyAccounts({ mint, user, creator, tokenProgram: TOKEN_2022_PROGRAM_ID, ...SYNTHETIC });
  const base = buildBuy(accounts, { amount: 1, maxSolCost: 2, trackVolume: false, partialFill: false });
  const full = withRemainingAccounts(base, remaining);
  assert.equal(full.keys.length, 18);
  assert.deepEqual(full.keys.slice(0, 16), base.keys);
  assert.deepEqual(full.keys.slice(16), remaining);
  assert.equal(base.keys.length, 16, 'the base instruction is not mutated');
  full.keys[16].isWritable = true;
  assert.equal(remaining[0].isWritable, false, 'metas are copied');
  assert.throws(() => withRemainingAccounts({}, remaining), TypeError);
  assert.throws(() => withRemainingAccounts(base, [{ pubkey: 'bad' }]), /remaining pubkey/);
});

// Anchor field order and privileges of `BuyV2` (programs/beam/src/buy_v2.rs);
// `SellV2` is the same without `globalVolumeAccumulator`. The names match the
// draft `beam-v2-trades.mjs` builder layout.
const BUY_V2_ORDER = [
  ['global', false, false], ['baseMint', false, false], ['quoteMint', false, false],
  ['baseTokenProgram', false, false], ['quoteTokenProgram', false, false],
  ['associatedTokenProgram', false, false], ['feeRecipient', false, true],
  ['associatedQuoteFeeRecipient', false, true], ['buybackFeeRecipient', false, true],
  ['associatedQuoteBuybackFeeRecipient', false, true], ['bondingCurve', false, true],
  ['associatedBaseBondingCurve', false, true], ['associatedQuoteBondingCurve', false, true],
  ['user', true, true], ['associatedBaseUser', false, true],
  ['associatedQuoteUser', false, true], ['creatorVault', false, true],
  ['associatedCreatorVault', false, true], ['sharingConfig', false, false],
  ['globalVolumeAccumulator', false, false], ['userVolumeAccumulator', false, true],
  ['associatedUserVolumeAccumulator', false, true], ['feeConfig', false, false],
  ['feeProgram', false, false], ['systemProgram', false, false],
  ['eventAuthority', false, false], ['program', false, false],
];
const SELL_V2_ORDER = BUY_V2_ORDER.filter(([name]) => name !== 'globalVolumeAccumulator');

test('V2 resolvers reproduce Anchor BuyV2/SellV2 metas for both base token programs', () => {
  const v2Bytes = readFileSync(new URL('./fixtures/v2-accounts.json', import.meta.url));
  const v2 = JSON.parse(v2Bytes);
  const v2Receipt = JSON.parse(readFileSync(new URL('./fixtures/v2-accounts.receipt.json', import.meta.url)));
  assert.equal(createHash('sha256').update(v2Bytes).digest('hex'), v2Receipt.stdoutSha256);
  const source = readFileSync(new URL('./tools/generate-v2-account-fixtures.rs', import.meta.url));
  assert.equal(createHash('sha256').update(source).digest('hex'), v2Receipt.sourceSha256);
  const i = v2.inputs;
  assert.equal(i.feeProgram, INHERITED_FEE_PROGRAM_ID);
  assert.equal(v2.cases.length, 4);
  for (const c of v2.cases) {
    const input = {
      mint: i.mint, user: i.user, creator: i.creator, baseTokenProgram: c.baseTokenProgram,
      feeRecipient: i.feeRecipient, buybackFeeRecipient: i.buybackFeeRecipient, feeConfig: i.feeConfig, feeProgram: i.feeProgram,
    };
    const buy = c.instruction === 'buyV2';
    const accounts = buy ? resolveBuyV2Accounts(input) : resolveSellV2Accounts(input);
    const order = buy ? BUY_V2_ORDER : SELL_V2_ORDER;
    assert.deepEqual(Object.keys(accounts).sort(), order.map(([n]) => n).sort(), `${c.instruction} field set`);
    const metas = order.map(([name, isSigner, isWritable]) => ({ pubkey: accounts[name], isSigner, isWritable }));
    assert.deepEqual(metas, c.metas, `${c.instruction} / ${c.label}`);
  }
  assert.equal(resolveBuyV2Accounts({ ...i, baseTokenProgram: TOKEN_2022_PROGRAM_ID }).quoteMint, WSOL_MINT);
});

test('V2 resolvers keep chain-state and inherited inputs explicit', () => {
  const input = { mint, user, creator, baseTokenProgram: TOKEN_2022_PROGRAM_ID, feeRecipient: SYNTHETIC.feeRecipient, buybackFeeRecipient: user, ...SYNTHETIC };
  for (const missing of ['feeConfig', 'feeProgram']) {
    const partial = { ...input };
    delete partial[missing];
    assert.throws(() => resolveBuyV2Accounts(partial), new RegExp(`${missing} is unresolved`));
  }
  assert.throws(() => resolveSellV2Accounts({ ...input, buybackFeeRecipient: undefined }), /buybackFeeRecipient/);
  assert.throws(() => resolveBuyV2Accounts({ ...input, baseTokenProgram: SYSTEM_PROGRAM_ID }), /SPL Token or Token-2022/);
  assert.throws(() => resolveBuyV2Accounts({ ...input, global: { buybackFeeRecipients: [creator] } }), /not listed/);
  assert.equal(resolveBuyV2Accounts({ ...input, global: { buybackFeeRecipients: [user] } }).buybackFeeRecipient, user);
  assert.equal(deriveSharingConfig(mint).address, resolveSellV2Accounts(input).sharingConfig);
  assert.equal(resolveBuyV2Accounts({ ...input, feeProgram: creator }).sharingConfig, deriveSharingConfig(mint).address,
    'sharing_config uses the hard-coded inherited fee program, not the fee_program input');
  assert.equal('globalVolumeAccumulator' in resolveSellV2Accounts(input), false);
});
