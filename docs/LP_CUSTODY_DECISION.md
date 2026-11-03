# LP custody and fee rights: decision record

**Status: engineering complete for the gated state; the policy needs the maintainer's confirmation.**
Nothing here selects a scheme. `migrate_amm_v4` stays disabled in every production build.

## Where the decision stands

| Source | What it says | Status |
| --- | --- | --- |
| `AGENTS.md` on `main` | "Principal custody and fee rights remain unresolved." | Merged |
| Draft PR #41 (`shawkkkkk`, head `aec9660`, open, not merged) | "LP is locked permanently. A locked position is not a recurring fee claim." Creators, operators and keepers have no authority to withdraw liquidity principal. Any post-graduation Beam fee is unset. Its body lists "LP locked permanently" under "Decided policy stays as written". | **Draft only** |

The two disagree. A statement in an unmerged draft is not a reviewed decision, and `AGENTS.md` on
`main` says the question is open, so this record treats #41 as the **documented direction awaiting
confirmation**, not as approval. If #41 merges as written, the "permanent lock, no withdrawal" row
below is the chosen policy and the consequences listed for it apply.

## What the code does today

- The LP tokens go to the migration PDA's associated token account.
  `BeamAmmV4Receipt::lp_disposition` is `0`, "held, unresolved".
- **No instruction can move them.** `nothing_in_the_program_can_move_the_migration_lp`
  (`programs/beam/tests/idl_compatibility.rs`, added with the IDL pull request) fails if any instruction other than the migration
  and the fail-closed CPMM one names the migration PDA or its token accounts, if any module other
  than the migration module signs with the PDA's seeds, if the PDA signs in more than the two
  places it does today (Raydium's `Initialize2` and the lamport refund), or if the migration module
  calls a token burn, close, authority-change, approve or plain transfer.
- That makes the current state a lock, but **only as permanent as the program's upgrade authority**:
  an upgradeable Beam program could be upgraded to add a withdrawal route. A burn does not depend on
  that. "Permanent" should not be promised on the strength of the PDA alone.

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

Compute and account costs for B, C and D are **estimates** until built; only the migration itself
(208,623 compute units, 1,100 bytes) has been measured. A burn is a single token CPI, in line with
the few thousand units the token program used in the measured migration.

## Decisions the maintainer must make

| Decision | Candidate in draft #41 | Approved? |
| --- | --- | --- |
| LP custody model | Locked permanently | No reviewed decision |
| Principal withdrawal policy | None | No reviewed decision |
| LP value / fee beneficiaries | None; post-graduation Beam fee unset | No reviewed decision |
| Fee claim or redemption authority | None | No reviewed decision |
| Lock duration | Permanent | No reviewed decision |
| Community distribution policy | Unset | No reviewed decision |
| Creator payout funding | From the 42 bps community share (pre-graduation fees) | Not about LP |
| Migration cost allocation | Today the caller pays the setup budget | No reviewed decision |

## Recommendation, conditional

If the intended policy is the one in #41, **burning the LP (A) is the sturdier way to deliver it**:
it needs no key, no upgrade-authority promise and no keeper, and it cannot be reversed by a later
program upgrade. Choose B instead only if the maintainer wants a path to fund buybacks or
distributions from LP growth, accepting the extra code, the recipient decision and the upgrade-authority
exposure. C is a rug-pull risk and sits badly with the repo's goal of transparent risk. D needs a
lock length nobody has proposed.

This is an engineering judgement about trust assumptions. It is not market evidence: `AGENTS.md`
asks for comparable depth, slippage and post-graduation data before any claim that a scheme preserves
liquidity or a chart, and none has been gathered.

## Consequences once a choice is approved

- **A:** a burn step after verification, `lp_disposition = 1`, tests for the exact burned amount, the
  supply decrease, rollback, and that nothing remains movable; the lock-immovability test is replaced
  by a burn test. Small.
- **Permanent lock without burning:** no code change except naming the disposition and keeping the
  immovability test; the upgrade-authority caveat must be stated wherever "permanent" is claimed.
- **B, C, D:** new custody state, a Raydium `Withdraw` CPI tested against real deposits and
  withdrawals by other providers, authority and recipient checks, unauthorized-caller, repeated-claim,
  zero-value, overflow, rounding, lock-boundary and rollback tests, and an updated receipt. A change
  to the serialized receipt needs a compatibility note; none exists yet because nothing is deployed.
