//! Strict decoder for the pinned Raydium AMM v4 `AmmInfo` account.
//!
//! Layout evidence: raydium-io/raydium-amm at 91739d8d9f5cf347e0ce14ebe4d2bb7069fb31b6,
//! `state.rs::AmmInfo` (`#[repr(C, packed)]`, 752 bytes). The account is raw packed data with
//! no Anchor discriminator, so it is read field by field at fixed offsets with bounds-checked
//! slices, never by pointer cast. `tests/amm-v4-validator/tests/pool_layout.rs` pins every offset
//! below to the struct, so a drifted offset fails a test rather than silently misreading a pool.
//!
//! This module only parses and compares. Whether the account is the right one (owner, address,
//! and that it is not an alias of another account) is the caller's job.
use anchor_lang::prelude::*;

use crate::beam_policy::BeamError;

/// Size of the pinned packed `AmmInfo` account.
pub const AMM_INFO_LEN: usize = 752;
/// `AmmStatus::SwapOnly`: the pool trades and nothing is scheduled.
pub const STATUS_SWAP_ONLY: u64 = 6;
/// Fees a fresh pool carries, in the order of the `Fees` struct:
/// min-separate 5/10000, trade 25/10000, PnL 12/100, swap 25/10000.
pub const PINNED_FEES: [u64; 8] = [5, 10_000, 25, 10_000, 12, 100, 25, 10_000];

const STATUS: usize = 0;
const NONCE: usize = 8;
const COIN_DECIMALS: usize = 32;
const PC_DECIMALS: usize = 40;
const FEES: usize = 128;
const NEED_TAKE_PNL_COIN: usize = 192;
const NEED_TAKE_PNL_PC: usize = 200;
const POOL_OPEN_TIME: usize = 224;
const COIN_VAULT: usize = 336;
const PC_VAULT: usize = 368;
const COIN_MINT: usize = 400;
const PC_MINT: usize = 432;
const LP_MINT: usize = 464;
const OPEN_ORDERS: usize = 496;
const MARKET: usize = 528;
const MARKET_PROGRAM: usize = 560;
const TARGET_ORDERS: usize = 592;
const LP_AMOUNT: usize = 720;

/// The `AmmInfo` fields Beam relies on, copied out of the packed account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolState {
    /// `AmmStatus`; `6` is `SwapOnly`.
    pub status: u64,
    /// Bump of Raydium's authority PDA.
    pub nonce: u64,
    /// Decimals of the coin (launch) mint.
    pub coin_decimals: u64,
    /// Decimals of the pc (wrapped SOL) mint.
    pub pc_decimals: u64,
    /// The eight `Fees` words.
    pub fees: [u64; 8],
    /// Coin owed to Raydium's PnL owner and not yet withdrawn.
    pub need_take_pnl_coin: u64,
    /// Pc owed to Raydium's PnL owner and not yet withdrawn.
    pub need_take_pnl_pc: u64,
    /// Unix time before which the pool does not trade.
    pub pool_open_time: u64,
    /// Coin vault.
    pub coin_vault: Pubkey,
    /// Pc vault.
    pub pc_vault: Pubkey,
    /// Coin mint.
    pub coin_mint: Pubkey,
    /// Pc mint.
    pub pc_mint: Pubkey,
    /// LP mint.
    pub lp_mint: Pubkey,
    /// OpenBook open-orders account; unset for these pools.
    pub open_orders: Pubkey,
    /// The key Raydium used as the pool's address seed.
    pub market: Pubkey,
    /// OpenBook program; unset for these pools.
    pub market_program: Pubkey,
    /// Target-orders account.
    pub target_orders: Pubkey,
    /// Total LP the pool accounts for, including the permanently locked minimum.
    pub lp_amount: u64,
}

/// What a freshly migrated pool must contain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpectedPool {
    /// Authority bump from Beam's own derivation.
    pub nonce: u8,
    /// Decimals of the launch mint.
    pub coin_decimals: u8,
    /// Decimals of wrapped SOL.
    pub pc_decimals: u8,
    /// The `open_time` passed to `Initialize2`.
    pub open_time: u64,
    /// Coin vault address.
    pub coin_vault: Pubkey,
    /// Pc vault address.
    pub pc_vault: Pubkey,
    /// Launch mint.
    pub coin_mint: Pubkey,
    /// Wrapped SOL mint.
    pub pc_mint: Pubkey,
    /// LP mint address.
    pub lp_mint: Pubkey,
    /// Seed Raydium must have used for the pool address.
    pub market_seed: Pubkey,
    /// Target-orders address.
    pub target_orders: Pubkey,
    /// Total LP the pool must account for: the user's LP plus the locked minimum.
    pub lp_amount: u64,
}

fn word(data: &[u8], at: usize) -> Option<u64> {
    let bytes: [u8; 8] = data.get(at..)?.get(..8)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn key(data: &[u8], at: usize) -> Option<Pubkey> {
    let bytes: [u8; 32] = data.get(at..)?.get(..32)?.try_into().ok()?;
    Some(Pubkey::new_from_array(bytes))
}

impl PoolState {
    /// Reads the relevant fields from a pool account's data.
    ///
    /// # Errors
    ///
    /// Fails with `InvalidMigrationResult` unless `data` is exactly [`AMM_INFO_LEN`] bytes.
    pub fn parse(data: &[u8]) -> Result<Self> {
        require!(data.len() == AMM_INFO_LEN, BeamError::InvalidMigrationResult);
        Self::read(data).ok_or_else(|| error!(BeamError::InvalidMigrationResult))
    }

    fn read(data: &[u8]) -> Option<Self> {
        let mut fees = [0u64; 8];
        for (index, slot) in fees.iter_mut().enumerate() {
            *slot = word(data, FEES.checked_add(index.checked_mul(8)?)?)?;
        }
        Some(Self {
            status: word(data, STATUS)?,
            nonce: word(data, NONCE)?,
            coin_decimals: word(data, COIN_DECIMALS)?,
            pc_decimals: word(data, PC_DECIMALS)?,
            fees,
            need_take_pnl_coin: word(data, NEED_TAKE_PNL_COIN)?,
            need_take_pnl_pc: word(data, NEED_TAKE_PNL_PC)?,
            pool_open_time: word(data, POOL_OPEN_TIME)?,
            coin_vault: key(data, COIN_VAULT)?,
            pc_vault: key(data, PC_VAULT)?,
            coin_mint: key(data, COIN_MINT)?,
            pc_mint: key(data, PC_MINT)?,
            lp_mint: key(data, LP_MINT)?,
            open_orders: key(data, OPEN_ORDERS)?,
            market: key(data, MARKET)?,
            market_program: key(data, MARKET_PROGRAM)?,
            target_orders: key(data, TARGET_ORDERS)?,
            lp_amount: word(data, LP_AMOUNT)?,
        })
    }

    /// Compares every field Beam depends on with what the migration planned.
    ///
    /// # Errors
    ///
    /// Fails with `InvalidMigrationResult` on the first field that differs: a pool that is not
    /// `SwapOnly`, has the wrong authority bump, decimals, fees, open time, vaults, mints, LP
    /// mint, seed, target orders or LP total, has any OpenBook link, or already owes PnL.
    pub fn verify(&self, expected: &ExpectedPool) -> Result<()> {
        let matches = self.status == STATUS_SWAP_ONLY
            && self.nonce == u64::from(expected.nonce)
            && self.coin_decimals == u64::from(expected.coin_decimals)
            && self.pc_decimals == u64::from(expected.pc_decimals)
            && self.fees == PINNED_FEES
            && self.need_take_pnl_coin == 0
            && self.need_take_pnl_pc == 0
            && self.pool_open_time == expected.open_time
            && self.coin_vault == expected.coin_vault
            && self.pc_vault == expected.pc_vault
            && self.coin_mint == expected.coin_mint
            && self.pc_mint == expected.pc_mint
            && self.lp_mint == expected.lp_mint
            && self.open_orders == Pubkey::default()
            && self.market == expected.market_seed
            && self.market_program == Pubkey::default()
            && self.target_orders == expected.target_orders
            && self.lp_amount == expected.lp_amount;
        require!(matches, BeamError::InvalidMigrationResult);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_word(data: &mut [u8], at: usize, value: u64) {
        data[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn put_key(data: &mut [u8], at: usize, value: &Pubkey) {
        data[at..at + 32].copy_from_slice(value.as_ref());
    }

    fn expected() -> ExpectedPool {
        ExpectedPool {
            nonce: 254,
            coin_decimals: 6,
            pc_decimals: 9,
            open_time: 0,
            coin_vault: Pubkey::new_unique(),
            pc_vault: Pubkey::new_unique(),
            coin_mint: Pubkey::new_unique(),
            pc_mint: Pubkey::new_unique(),
            lp_mint: Pubkey::new_unique(),
            market_seed: Pubkey::new_unique(),
            target_orders: Pubkey::new_unique(),
            lp_amount: 1_000_000_000,
        }
    }

    fn pool_bytes(e: &ExpectedPool) -> Vec<u8> {
        let mut d = vec![0u8; AMM_INFO_LEN];
        put_word(&mut d, STATUS, STATUS_SWAP_ONLY);
        put_word(&mut d, NONCE, u64::from(e.nonce));
        put_word(&mut d, COIN_DECIMALS, u64::from(e.coin_decimals));
        put_word(&mut d, PC_DECIMALS, u64::from(e.pc_decimals));
        for (i, fee) in PINNED_FEES.iter().enumerate() {
            put_word(&mut d, FEES + i * 8, *fee);
        }
        put_word(&mut d, POOL_OPEN_TIME, e.open_time);
        put_key(&mut d, COIN_VAULT, &e.coin_vault);
        put_key(&mut d, PC_VAULT, &e.pc_vault);
        put_key(&mut d, COIN_MINT, &e.coin_mint);
        put_key(&mut d, PC_MINT, &e.pc_mint);
        put_key(&mut d, LP_MINT, &e.lp_mint);
        put_key(&mut d, MARKET, &e.market_seed);
        put_key(&mut d, TARGET_ORDERS, &e.target_orders);
        put_word(&mut d, LP_AMOUNT, e.lp_amount);
        d
    }

    #[test]
    fn a_fresh_pool_matches_its_plan() -> Result<()> {
        let e = expected();
        PoolState::parse(&pool_bytes(&e))?.verify(&e)
    }

    #[test]
    fn every_field_is_compared() -> Result<()> {
        let e = expected();
        let mutations: [(&str, usize, usize); 17] = [
            ("status", STATUS, 8),
            ("nonce", NONCE, 8),
            ("coin decimals", COIN_DECIMALS, 8),
            ("pc decimals", PC_DECIMALS, 8),
            ("a fee word", FEES + 16, 8),
            ("pending coin pnl", NEED_TAKE_PNL_COIN, 8),
            ("pending pc pnl", NEED_TAKE_PNL_PC, 8),
            ("open time", POOL_OPEN_TIME, 8),
            ("coin vault", COIN_VAULT, 32),
            ("pc vault", PC_VAULT, 32),
            ("coin mint", COIN_MINT, 32),
            ("pc mint", PC_MINT, 32),
            ("lp mint", LP_MINT, 32),
            ("open orders", OPEN_ORDERS, 32),
            ("market seed", MARKET, 32),
            ("market program", MARKET_PROGRAM, 32),
            ("target orders", TARGET_ORDERS, 32),
        ];
        for (name, at, len) in mutations {
            let mut data = pool_bytes(&e);
            data[at] ^= 0xff;
            assert!(len > 0);
            assert!(PoolState::parse(&data)?.verify(&e).is_err(), "{name} must be compared");
        }
        let mut data = pool_bytes(&e);
        put_word(&mut data, LP_AMOUNT, e.lp_amount + 1);
        assert!(PoolState::parse(&data)?.verify(&e).is_err(), "lp amount must be compared");
        Ok(())
    }

    #[test]
    fn wrong_lengths_are_rejected_rather_than_read() {
        let e = expected();
        let good = pool_bytes(&e);
        assert!(PoolState::parse(&good[..AMM_INFO_LEN - 1]).is_err());
        assert!(PoolState::parse(&[]).is_err());
        let mut long = good;
        long.push(0);
        assert!(PoolState::parse(&long).is_err());
    }

    #[test]
    fn readers_never_panic_past_the_end() {
        assert_eq!(word(&[0; 7], 0), None);
        assert_eq!(word(&[0; 8], 1), None);
        assert_eq!(word(&[0; 8], usize::MAX), None);
        assert_eq!(key(&[0; 31], 0), None);
        assert_eq!(key(&[0; 32], 0), Some(Pubkey::default()));
    }
}
