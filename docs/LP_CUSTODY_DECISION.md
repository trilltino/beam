# LP custody and fee rights: decision record

**Status: owner chose permanent LP burn on 3 November 2026; this implementation is a draft.**
`migrate_amm_v4` stays disabled in every production build.

## Where the decision stands

| Source | What it says | Status |
| --- | --- | --- |
| `AGENTS.md` on `main` | "Principal custody and fee rights remain unresolved." | Merged |
| Draft PR #41 (`shawkkkkk`, head `aec9660`, open, not merged) | "LP is locked permanently. A locked position is not a recurring fee claim." Creators, operators and keepers have no authority to withdraw liquidity principal. Any post-graduation Beam fee is unset. Its body lists "LP locked permanently" under "Decided policy stays as written". | **Draft only** |
| Owner direction, 3 November 2026 | Use what Pump.fun had before its move to PumpSwap. [Contemporaneous 2024 research](https://assets.ctfassets.net/i0qyt2j9snzb/45tD8g3D9YbHpJjIV8pFBs/da3fc7315d1f611d190eedcfb439705f/Pump.fun_-_Memecoin_Madness.pdf) describes Raydium graduation with the LP token burned. | **Selected: burn all LP issued to Beam** |

The owner's later direction resolves the custody choice. PR #41's permanent-lock wording
should be made precise before merge: the initial Beam-received LP is burned, so it gives
Beam no ongoing LP fee claim. This is a historical-pattern choice, not proof that Beam's
liquidity will have the same depth or market behavior.

## What the code does today

- The LP tokens go to the migration PDA's associated token account, are verified against
  the exact expected Raydium issuance, and are burned before settlement finishes.
  `BeamAmmV4Receipt::lp_disposition` is `1`, "burned".
- The handler checks that the LP mint supply and PDA LP balance are zero. The same
  transaction rolls back if the burn or any later step fails. No upgrade to Beam can
  recover those burned LP tokens.
- The separate IDL draft's immovability test must be updated before integration: it
  currently forbids the token burn this decision requires.

## Facts from the pinned Raydium source (raydium-amm 91739d8)

- A new pool charges a 0.25% swap fee. 12% of it (0.03% of volume) accrues to `need_take_pnl`, which
  only Raydium's `amm_owner` or the config's `pnl_owner` can withdraw (`process_withdrawpnl`). Beam
  and LP holders cannot.
- The remaining 0.22% stays in the pool and raises the value of every LP token. An LP holder realizes
  it only by redeeming LP for a proportional share of both assets (`process_withdraw`).
- `10^coin_decimals` LP is never minted to anyone; it stays locked whatever is chosen.
- `SwapOnly` pools allow removing liquidity (`state.rs`, `AmmStatus`). Redeeming from a pool this
  instruction created has **not** been exercised in a test.
- Raydium's program is upgradeable by a single key. Every option below depends on Raydium not
  changing the program or disabling a pool; no Beam choice removes that risk.
- Raydium's Burn & Earn does not support AMM v4 (`docs/ECONOMICS.md`), so there is no built-in way to
  burn principal while keeping a fee claim.

## The options

| | A. Permanent burn | B. Locked principal, claim fee growth | C. Beam-controlled, redeemable | D. Time-locked |
| --- | --- | --- | --- | --- |
| Who holds the LP | Nobody; destroyed | Migration PDA | Migration PDA | Migration PDA |
| Principal withdrawable | Never | Never (no instruction allows it) | By the named authority, any time | By the authority after the lock |
| Who receives LP value | No one | A recipient, **undecided** | The authority | The authority after the lock |
| Funds community distributions | No | Only if the recipient is a community vault | Yes, by discretion | After the lock |
| Creator claim | None | None unless defined | None unless defined | None unless defined |
| Admin can remove liquidity | No | No, except by upgrading the program | Yes | After the lock |
| Signatures needed | None | PDA seeds; a permissionless claim can pay a fixed recipient | Authority key plus PDA seeds | As C after the lock |
| If the authority is compromised | Nothing to compromise | Only unclaimed growth is exposed, unless the program is also upgraded | **All principal** | All principal after the lock |
| Other LP providers | Unaffected | Unaffected: the baseline below is per LP token | Unaffected | Unaffected |
| Cost | One burn CPI | Largest: a Raydium `Withdraw` CPI, a baseline, a recipient and a claim instruction | Medium: `Withdraw` CPI plus an authority check | Medium plus a lock length |

**How B would measure "growth".** Let `g = sqrt(coin_reserve * pc_reserve) / total_lp`, using reserves
net of pending PnL and a total that includes the locked minimum. Trading fees raise `g`; price moves
and other LPs depositing or withdrawing do not, so impermanent loss is not mistaken for income. The
claimable LP is `held * (1 - g0 / g_now)` rounded down, where `g0` is recorded at graduation.
Redeeming returns both assets in the pool's current ratio. Whether the recipient may receive that
mix, and how it relates to the 42/5/3 split and creator funding, is the open decision.

Compute and account costs for B, C and D are **estimates** until built. Before the burn,
the migration measured 208,623 compute units and 1,100 transaction bytes. The burn adds
a token-program CPI, so the new exact compute cost needs a validator measurement on this head.

## Decisions the maintainer must make

| Decision | Candidate in draft #41 | Approved? |
| --- | --- | --- |
| LP custody model | Locked permanently | **Burn all Beam-received LP** |
| Principal withdrawal policy | None | **No withdrawal from the burned position** |
| LP value / fee beneficiaries | None; post-graduation Beam fee unset | **No LP fee claim** |
| Fee claim or redemption authority | None | **None for the burned position** |
| Lock duration | Permanent | **Permanent burn** |
| Community distribution policy | Unset | No reviewed decision |
| Creator payout funding | From the 42 bps community share (pre-graduation fees) | **Owner confirmed; rate unset** |
| Migration cost allocation | Today the caller pays the setup budget | No reviewed decision |

## Consequence of the selected burn

The burn needs no withdrawal key, upgrade-authority promise or keeper, and it cannot be
reversed by a later Beam program upgrade. It also removes the initial position's fee claim.
Post-graduation creator payouts, community distributions and buybacks need a different
approved and actually collected source; neither Raydium's pool fee nor the burned LP is
spendable Beam revenue.

This is an engineering judgement about trust assumptions. It is not market evidence: `AGENTS.md`
asks for comparable depth, slippage and post-graduation data before any claim that a scheme preserves
liquidity or a chart, and none has been gathered.

## Consequences once a choice is approved

- **A, selected in this draft:** a burn step after verification, `lp_disposition = 1`, tests for
  the zero supply and balance, and rollback. Update the separate IDL draft's lock-immovability
  test before combining branches.
- **Permanent lock without burning:** no code change except naming the disposition and keeping the
  immovability test; the upgrade-authority caveat must be stated wherever "permanent" is claimed.
- **B, C, D:** new custody state, a Raydium `Withdraw` CPI tested against real deposits and
  withdrawals by other providers, authority and recipient checks, unauthorized-caller, repeated-claim,
  zero-value, overflow, rounding, lock-boundary and rollback tests, and an updated receipt. A change
  to the serialized receipt needs a compatibility note; none exists yet because nothing is deployed.
