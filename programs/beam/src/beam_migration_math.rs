//! Pure migration arithmetic shared by the CPMM and AMM v4 paths.
use anchor_lang::prelude::*;

use crate::{beam_policy::BeamError, PumpError};

/// Post-completion buys can add quote to the curve after real reserves stop
/// changing. Include that quote in the pool, as the retained Pump migration
/// does, while charging the migration fee only once.
pub fn migration_quote(real_quote: u64, post_complete_quote_in: u64, migration_fee: u64) -> Result<(u64, u64)> {
    let total = real_quote.checked_add(post_complete_quote_in).ok_or(PumpError::Overflow)?;
    let pool_quote = total.checked_sub(migration_fee).ok_or(BeamError::InvalidMigrationBalance)?;
    require!(pool_quote > 0, BeamError::InvalidMigrationBalance);
    Ok((total, pool_quote))
}

/// Fee counters are separate claims on the curve's lamports, not deductions
/// from its recorded liquidity reserves. Extra lamports are left in place.
pub fn protected_curve_floor(curve_lamports: u64, rent: u64, creator_fee: u64, protocol_fees: u64, owned_quote: u64) -> Result<u64> {
    let floor = rent.checked_add(creator_fee).and_then(|v| v.checked_add(protocol_fees)).ok_or(PumpError::Overflow)?;
    let required = floor.checked_add(owned_quote).ok_or(PumpError::Overflow)?;
    require!(curve_lamports >= required, BeamError::InvalidMigrationBalance);
    Ok(floor)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_includes_post_completion_quote_and_charges_fee_once() -> Result<()> {
        assert_eq!(migration_quote(100, 25, 10)?, (125, 115));
        assert_eq!(migration_quote(100, 0, 10)?, (100, 90));
        assert_eq!(migration_quote(0, 25, 0)?, (25, 25));
        assert_eq!(migration_quote(u64::MAX, 0, 0)?, (u64::MAX, u64::MAX));
        assert!(migration_quote(0, 0, 0).is_err());
        assert!(migration_quote(100, 25, 125).is_err());
        assert!(migration_quote(u64::MAX, 1, 0).is_err());
        assert!(migration_quote(100, 25, 126).is_err());
        Ok(())
    }
    #[test]
    fn migration_preserves_fee_liabilities_and_donor_surplus() -> Result<()> {
        assert_eq!(protected_curve_floor(0, 0, 0, 0, 0)?, 0);
        assert_eq!(protected_curve_floor(u64::MAX, 0, 0, 0, u64::MAX)?, 0);
        let (owned, pool) = migration_quote(100, 25, 10)?;
        let floor = protected_curve_floor(150, 10, 5, 10, owned)?;
        assert_eq!((owned, pool, floor), (125, 115, 25));
        assert_eq!(150 - pool - 10, floor);
        assert!(protected_curve_floor(149, 10, 5, 10, owned).is_err());
        let donor_surplus = 42;
        let funded = 150 + donor_surplus;
        assert_eq!(protected_curve_floor(funded, 10, 5, 10, owned)?, floor);
        assert_eq!(funded - pool - 10, floor + donor_surplus);
        assert!(protected_curve_floor(u64::MAX, u64::MAX, 1, 0, 0).is_err());
        assert!(protected_curve_floor(u64::MAX, 0, u64::MAX, 1, 0).is_err());
        assert!(protected_curve_floor(u64::MAX, 1, 0, 0, u64::MAX).is_err());
        Ok(())
    }
}
