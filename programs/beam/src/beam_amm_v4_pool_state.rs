//! Bounds-checked decoder for the pinned 752-byte Raydium AMM v4 `AmmInfo`; `pool_layout.rs` pins every offset to the struct.
use anchor_lang::prelude::*;

use crate::beam_policy::BeamError;

pub const AMM_INFO_LEN: usize = 752;
pub const TARGET_ORDERS_LEN: usize = 2208;
pub const STATUS_SWAP_ONLY: u64 = 6;
pub const PINNED_FEES: [u64; 8] = [5, 10_000, 25, 10_000, 12, 100, 25, 10_000];

mod offset {
    pub const STATUS: usize = 0;
    pub const NONCE: usize = 8;
    pub const COIN_DECIMALS: usize = 32;
    pub const PC_DECIMALS: usize = 40;
    pub const FEES: usize = 128;
    pub const NEED_TAKE_PNL_COIN: usize = 192;
    pub const NEED_TAKE_PNL_PC: usize = 200;
    pub const POOL_OPEN_TIME: usize = 224;
    pub const COIN_VAULT: usize = 336;
    pub const PC_VAULT: usize = 368;
    pub const COIN_MINT: usize = 400;
    pub const PC_MINT: usize = 432;
    pub const LP_MINT: usize = 464;
    pub const OPEN_ORDERS: usize = 496;
    pub const MARKET: usize = 528;
    pub const MARKET_PROGRAM: usize = 560;
    pub const TARGET_ORDERS: usize = 592;
    pub const LP_AMOUNT: usize = 720;
}

/// The fields of Raydium's `AmmInfo` that a fresh Beam pool must match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmmInfo {
    pub status: u64,
    pub nonce: u64,
    pub coin_decimals: u64,
    pub pc_decimals: u64,
    pub fees: [u64; 8],
    pub need_take_pnl_coin: u64,
    pub need_take_pnl_pc: u64,
    pub pool_open_time: u64,
    pub coin_vault: Pubkey,
    pub pc_vault: Pubkey,
    pub coin_mint: Pubkey,
    pub pc_mint: Pubkey,
    pub lp_mint: Pubkey,
    pub open_orders: Pubkey,
    pub market: Pubkey,
    pub market_program: Pubkey,
    pub target_orders: Pubkey,
    pub lp_amount: u64,
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    let bytes: [u8; 8] = data.get(at..)?.get(..8)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn key_at(data: &[u8], at: usize) -> Option<Pubkey> {
    let bytes: [u8; 32] = data.get(at..)?.get(..32)?.try_into().ok()?;
    Some(Pubkey::new_from_array(bytes))
}

/// Returns the name of the first listed field that differs, in the order given.
macro_rules! first_difference {
    ($left:expr, $right:expr, [$($field:ident),+ $(,)?]) => {
        $(if $left.$field != $right.$field {
            return Some(stringify!($field));
        })+
    };
}

impl AmmInfo {
    /// What every new pool must look like: swap-only, pinned fees, no pending `PnL`, no market.
    pub const FRESH: Self = Self {
        status: STATUS_SWAP_ONLY,
        nonce: 0,
        coin_decimals: 0,
        pc_decimals: 0,
        fees: PINNED_FEES,
        need_take_pnl_coin: 0,
        need_take_pnl_pc: 0,
        pool_open_time: 0,
        coin_vault: Pubkey::new_from_array([0; 32]),
        pc_vault: Pubkey::new_from_array([0; 32]),
        coin_mint: Pubkey::new_from_array([0; 32]),
        pc_mint: Pubkey::new_from_array([0; 32]),
        lp_mint: Pubkey::new_from_array([0; 32]),
        open_orders: Pubkey::new_from_array([0; 32]),
        market: Pubkey::new_from_array([0; 32]),
        market_program: Pubkey::new_from_array([0; 32]),
        target_orders: Pubkey::new_from_array([0; 32]),
        lp_amount: 0,
    };

    pub fn parse(data: &[u8]) -> Result<Self> {
        require!(data.len() == AMM_INFO_LEN, BeamError::InvalidMigrationResult);
        Self::read(data).ok_or_else(|| error!(BeamError::InvalidMigrationResult))
    }

    fn read(data: &[u8]) -> Option<Self> {
        let mut fees = [0u64; 8];
        for (index, slot) in fees.iter_mut().enumerate() {
            *slot = u64_at(data, offset::FEES.checked_add(index.checked_mul(8)?)?)?;
        }
        Some(Self {
            status: u64_at(data, offset::STATUS)?,
            nonce: u64_at(data, offset::NONCE)?,
            coin_decimals: u64_at(data, offset::COIN_DECIMALS)?,
            pc_decimals: u64_at(data, offset::PC_DECIMALS)?,
            fees,
            need_take_pnl_coin: u64_at(data, offset::NEED_TAKE_PNL_COIN)?,
            need_take_pnl_pc: u64_at(data, offset::NEED_TAKE_PNL_PC)?,
            pool_open_time: u64_at(data, offset::POOL_OPEN_TIME)?,
            coin_vault: key_at(data, offset::COIN_VAULT)?,
            pc_vault: key_at(data, offset::PC_VAULT)?,
            coin_mint: key_at(data, offset::COIN_MINT)?,
            pc_mint: key_at(data, offset::PC_MINT)?,
            lp_mint: key_at(data, offset::LP_MINT)?,
            open_orders: key_at(data, offset::OPEN_ORDERS)?,
            market: key_at(data, offset::MARKET)?,
            market_program: key_at(data, offset::MARKET_PROGRAM)?,
            target_orders: key_at(data, offset::TARGET_ORDERS)?,
            lp_amount: u64_at(data, offset::LP_AMOUNT)?,
        })
    }

    /// Name of the first field that differs from `expected`, or `None` when they are equal.
    #[must_use]
    pub fn first_difference(&self, expected: &Self) -> Option<&'static str> {
        first_difference!(
            self,
            expected,
            [
                status,
                nonce,
                coin_decimals,
                pc_decimals,
                fees,
                need_take_pnl_coin,
                need_take_pnl_pc,
                pool_open_time,
                coin_vault,
                pc_vault,
                coin_mint,
                pc_mint,
                lp_mint,
                open_orders,
                market,
                market_program,
                target_orders,
                lp_amount,
            ]
        );
        None
    }

    /// Fails, logging the field, unless every field equals `expected`.
    pub fn check(&self, expected: &Self) -> Result<()> {
        match self.first_difference(expected) {
            None => Ok(()),
            Some(field) => {
                msg!("amm v4: pool field `{}` differs from the plan", field);
                err!(BeamError::InvalidMigrationResult)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u64(data: &mut [u8], at: usize, value: u64) {
        data[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn put_key(data: &mut [u8], at: usize, value: &Pubkey) {
        data[at..at + 32].copy_from_slice(value.as_ref());
    }

    fn sample() -> AmmInfo {
        AmmInfo {
            nonce: 254,
            coin_decimals: 6,
            pc_decimals: 9,
            coin_vault: Pubkey::new_unique(),
            pc_vault: Pubkey::new_unique(),
            coin_mint: Pubkey::new_unique(),
            pc_mint: Pubkey::new_unique(),
            lp_mint: Pubkey::new_unique(),
            market: Pubkey::new_unique(),
            target_orders: Pubkey::new_unique(),
            lp_amount: 1_000_000_000,
            ..AmmInfo::FRESH
        }
    }

    fn to_bytes(info: &AmmInfo) -> Vec<u8> {
        let mut d = vec![0u8; AMM_INFO_LEN];
        put_u64(&mut d, offset::STATUS, info.status);
        put_u64(&mut d, offset::NONCE, info.nonce);
        put_u64(&mut d, offset::COIN_DECIMALS, info.coin_decimals);
        put_u64(&mut d, offset::PC_DECIMALS, info.pc_decimals);
        for (i, fee) in info.fees.iter().enumerate() {
            put_u64(&mut d, offset::FEES + i * 8, *fee);
        }
        put_u64(&mut d, offset::NEED_TAKE_PNL_COIN, info.need_take_pnl_coin);
        put_u64(&mut d, offset::NEED_TAKE_PNL_PC, info.need_take_pnl_pc);
        put_u64(&mut d, offset::POOL_OPEN_TIME, info.pool_open_time);
        put_key(&mut d, offset::COIN_VAULT, &info.coin_vault);
        put_key(&mut d, offset::PC_VAULT, &info.pc_vault);
        put_key(&mut d, offset::COIN_MINT, &info.coin_mint);
        put_key(&mut d, offset::PC_MINT, &info.pc_mint);
        put_key(&mut d, offset::LP_MINT, &info.lp_mint);
        put_key(&mut d, offset::OPEN_ORDERS, &info.open_orders);
        put_key(&mut d, offset::MARKET, &info.market);
        put_key(&mut d, offset::MARKET_PROGRAM, &info.market_program);
        put_key(&mut d, offset::TARGET_ORDERS, &info.target_orders);
        put_u64(&mut d, offset::LP_AMOUNT, info.lp_amount);
        d
    }

    #[test]
    fn a_fresh_pool_matches_its_plan() -> Result<()> {
        let want = sample();
        assert_eq!(AmmInfo::parse(&to_bytes(&want))?, want);
        AmmInfo::parse(&to_bytes(&want))?.check(&want)
    }

    #[test]
    fn every_field_is_compared_and_named() -> Result<()> {
        let want = sample();
        let fields: [(&str, usize); 18] = [
            ("status", offset::STATUS),
            ("nonce", offset::NONCE),
            ("coin_decimals", offset::COIN_DECIMALS),
            ("pc_decimals", offset::PC_DECIMALS),
            ("fees", offset::FEES + 16),
            ("need_take_pnl_coin", offset::NEED_TAKE_PNL_COIN),
            ("need_take_pnl_pc", offset::NEED_TAKE_PNL_PC),
            ("pool_open_time", offset::POOL_OPEN_TIME),
            ("coin_vault", offset::COIN_VAULT),
            ("pc_vault", offset::PC_VAULT),
            ("coin_mint", offset::COIN_MINT),
            ("pc_mint", offset::PC_MINT),
            ("lp_mint", offset::LP_MINT),
            ("open_orders", offset::OPEN_ORDERS),
            ("market", offset::MARKET),
            ("market_program", offset::MARKET_PROGRAM),
            ("target_orders", offset::TARGET_ORDERS),
            ("lp_amount", offset::LP_AMOUNT),
        ];
        for (name, at) in fields {
            let mut data = to_bytes(&want);
            data[at] ^= 0xff;
            let got = AmmInfo::parse(&data)?;
            assert_eq!(got.first_difference(&want), Some(name), "{name} must be compared");
            assert!(got.check(&want).is_err());
        }
        Ok(())
    }

    #[test]
    fn wrong_lengths_are_rejected_rather_than_read() {
        let good = to_bytes(&sample());
        assert!(AmmInfo::parse(&good[..AMM_INFO_LEN - 1]).is_err());
        assert!(AmmInfo::parse(&[]).is_err());
        let mut long = good;
        long.push(0);
        assert!(AmmInfo::parse(&long).is_err());
    }

    #[test]
    fn readers_never_panic_past_the_end() {
        assert_eq!(u64_at(&[0; 7], 0), None);
        assert_eq!(u64_at(&[0; 8], 1), None);
        assert_eq!(u64_at(&[0; 8], usize::MAX), None);
        assert_eq!(key_at(&[0; 31], 0), None);
        assert_eq!(key_at(&[0; 32], 0), Some(Pubkey::default()));
    }
}
