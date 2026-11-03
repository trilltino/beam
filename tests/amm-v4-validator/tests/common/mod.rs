//! Wire constants shared by the migration tests; `idl_constants.rs` checks each against `sdk/idl/beam.json`.
#![allow(dead_code)]

/// `sha256("account:AmmV4Rights")[..8]`, the discriminator draft PR #49 reads.
pub const DISC_RIGHTS: [u8; 8] = [88, 155, 79, 159, 221, 232, 159, 98];
pub const DISC_RECEIPT: [u8; 8] = [179, 123, 65, 124, 38, 208, 232, 159];
pub const DISC_CONFIGURE: [u8; 8] = [144, 16, 143, 164, 78, 235, 65, 59];
pub const DISC_SET_ENABLED: [u8; 8] = [171, 118, 93, 180, 198, 99, 182, 159];
pub const DISC_MIGRATE: [u8; 8] = [10, 219, 107, 168, 193, 191, 50, 99];
pub const DISC_GLOBAL: [u8; 8] = [167, 232, 232, 177, 200, 108, 114, 127];
pub const DISC_CURVE: [u8; 8] = [23, 183, 248, 55, 96, 216, 172, 96];

/// `BeamError` starts at 7000; the curve error is the retained Pump one.
pub const ERR_NOT_ENABLED: u32 = 7002;
pub const ERR_INVALID_ACCOUNT: u32 = 7003;
pub const ERR_BALANCE: u32 = 7004;
pub const ERR_UNSUPPORTED_MINT: u32 = 7005;
pub const ERR_FEE_CAP: u32 = 7007;
pub const ERR_SETUP_BOUNDS: u32 = 7008;
pub const ERR_OPEN_TIME: u32 = 7009;
pub const ERR_SOURCE: u32 = 7010;
pub const ERR_CURVE_NOT_COMPLETE: u32 = 6006;
