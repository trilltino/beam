# LP custody and fee rights: decision brief

**Status: open. This is an owner decision and nothing in the program chooses it.**
`TASKS.md` Scope 2 and `AGENTS.md` leave principal custody and fee rights unresolved and
say not to pick an LP scheme without a reviewed decision and comparable-liquidity evidence.
`migrate_amm_v4` stays disabled in every production build until this is settled. This page
lists the options and what each costs; it does not recommend one on market evidence, because
none has been gathered.

## What happens today

`migrate_amm_v4` leaves all LP tokens in the migration PDA's associated token account and
records `lp_disposition = 0` ("held, unresolved"). No Beam instruction can move, burn or
redeem them. Raydium also keeps `10^coin_decimals` LP units out of the user's share at
creation, so that minimum is locked permanently whatever is chosen.

## Facts from the pinned Raydium source (raydium-amm 91739d8)

- **Fees.** Pools start with a 25/10000 (0.25%) swap fee and a PnL share of 12/100
  (`state.rs`, `Fees::initialize`).
- **The PnL share is not the LP holder's.** That 12% (0.03% of volume) accrues to
  `need_take_pnl` and only Raydium's `amm_owner` or the config's `pnl_owner` can withdraw it
  (`processor.rs`, `process_withdrawpnl`). Beam and LP holders cannot.
- **The rest stays in the pool.** The remaining 0.22% is never paid out. It raises the value of
  each LP token, and an LP holder realizes it only by redeeming LP for a proportional share of
  both assets (`process_withdraw`).
- **`SwapOnly` pools allow removing liquidity** (`state.rs`, `AmmStatus` comment), so redeeming
  is possible. Redeeming from a pool created by this instruction has not been exercised in a
  test.
- **Raydium describes AMM v4 as legacy for new pools.** The venue is the owner's selection.

## Options

| | Who can withdraw principal | Who gets fee growth | Depth | Trust needed | What it costs to build |
| --- | --- | --- | --- | --- | --- |
| **A. Burn all LP** | Nobody, ever | Nobody (it stays in the pool) | Permanent | None | Small: one burn after the pool check. Gives up any LP fee income, so no recurring buyback funding from LP fees. |
| **B. Hold, claim fee growth only** | Nobody; principal is locked | A named recipient, via a claim instruction | Permanent principal | Claim math and the recipient | Largest. The claim redeems only `lp * (1 - g0/g_now)`, where `g = sqrt(coin*pc) / lp_supply` is measured at graduation and now. Needs a baseline in the receipt, a recipient, and tests for rounding, deposits by others and price moves. |
| **C. Hold, authority withdraws all** | The `Global` authority, any time | The authority | None promised | Full trust in one key | Medium. Maximum flexibility, but the authority can remove all liquidity, which cuts against the repo's goal of transparent risk for newcomers. |
| **D. Time-lock, then release** | Authority after a fixed date | The authority after the date | Guaranteed until the date only | A key, after the lock | Medium plus a lock length to choose. |

## Decisions needed from the owner

1. Which option (A to D), or a variant.
2. For B, C or D: who receives claimed value. A candidate is `Global.withdraw_authority`, the
   inherited fee recipient, but that interacts with the open 42/5/3 allocation and creator
   funding, so it is not assumed here.
3. For D: the lock length.
4. Whether the evidence the repo asks for must come first: initial and continuing depth,
   slippage and price impact at comparable trade sizes, and post-graduation behavior over
   matched periods. No claim that Raydium outperforms PumpSwap, or that any option protects
   a chart, is supported until that exists.

## What follows once an option is chosen

Each option needs the permissions written down in the program, tests for who can and cannot
move LP, rollback and unauthorized-caller cases, and a receipt that records the disposition.
Option A also changes what `lp_disposition` means. None of it changes the 12-test migration
suite until the choice is made.
