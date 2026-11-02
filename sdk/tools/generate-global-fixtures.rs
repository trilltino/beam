//! Host-only `Global` account fixture producer. No handler, wallet, RPC or
//! transaction. Run as an example of the existing, locked beam-program package
//! (copy to programs/beam/examples/beam_global_fixture.rs, run, then delete):
//!
//!   cargo run --locked --manifest-path programs/beam/Cargo.toml --example beam_global_fixture
//!
//! Bytes come from Anchor's `AccountSerialize` for `beam_program::Global`;
//! every value is synthetic.
use anchor_lang::{prelude::Pubkey, AccountDeserialize, AccountSerialize, Discriminator};
use beam_program::Global;

fn key(n: u8) -> Pubkey { Pubkey::new_from_array([n; 32]) }
fn hex(data: &[u8]) -> String { data.iter().map(|b| format!("{b:02x}")).collect() }
fn keys(list: &[Pubkey]) -> String {
    list.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(",")
}
fn fields(g: &Global) -> String {
    format!(
        concat!(
            "{{\"initialized\":{},\"authority\":\"{}\",\"feeRecipient\":\"{}\",",
            "\"initialVirtualTokenReserves\":\"{}\",\"initialVirtualSolReserves\":\"{}\",",
            "\"initialRealTokenReserves\":\"{}\",\"tokenTotalSupply\":\"{}\",\"feeBasisPoints\":\"{}\",",
            "\"withdrawAuthority\":\"{}\",\"enableMigrate\":{},\"poolMigrationFee\":\"{}\",",
            "\"creatorFeeBasisPoints\":\"{}\",\"feeRecipients\":[{}],\"setCreatorAuthority\":\"{}\",",
            "\"adminSetCreatorAuthority\":\"{}\",\"createV2Enabled\":{},\"whitelistPda\":\"{}\",",
            "\"reservedFeeRecipient\":\"{}\",\"mayhemModeEnabled\":{},\"reservedFeeRecipients\":[{}],",
            "\"isCashbackEnabled\":{},\"buybackFeeRecipients\":[{}],\"buybackBasisPoints\":\"{}\",",
            "\"initialVirtualQuoteReserves\":\"{}\",\"whitelistedQuoteMints\":[{}],",
            "\"creatorFeeConfigurable\":{},\"maxConfigurableCreatorFeeBps\":\"{}\",",
            "\"holderRewardClaimAuthority\":\"{}\",\"isHolderRewardEnabled\":{},\"maxCurveDepth\":{}}}"
        ),
        g.initialized, g.authority, g.fee_recipient, g.initial_virtual_token_reserves,
        g.initial_virtual_sol_reserves, g.initial_real_token_reserves, g.token_total_supply,
        g.fee_basis_points, g.withdraw_authority, g.enable_migrate, g.pool_migration_fee,
        g.creator_fee_basis_points, keys(&g.fee_recipients), g.set_creator_authority,
        g.admin_set_creator_authority, g.create_v2_enabled, g.whitelist_pda, g.reserved_fee_recipient,
        g.mayhem_mode_enabled, keys(&g.reserved_fee_recipients), g.is_cashback_enabled,
        keys(&g.buyback_fee_recipients), g.buyback_basis_points, g.initial_virtual_quote_reserves,
        keys(&g.whitelisted_quote_mints), g.creator_fee_configurable, g.max_configurable_creator_fee_bps,
        g.holder_reward_claim_authority, g.is_holder_reward_enabled, g.max_curve_depth,
    )
}

fn main() {
    let zeroed = vec![0u8; 4_096];
    let zero = Global::try_deserialize_unchecked(&mut &zeroed[..]).unwrap();

    let mut distinct = zero.clone_for_fixture();
    distinct.initialized = true;
    distinct.authority = key(1);
    distinct.fee_recipient = key(2);
    distinct.initial_virtual_token_reserves = 1_073_000_000_000_000;
    distinct.initial_virtual_sol_reserves = 30_000_000_000;
    distinct.initial_real_token_reserves = 793_100_000_000_000;
    distinct.token_total_supply = 1_000_000_000_000_000;
    distinct.fee_basis_points = 95;
    distinct.withdraw_authority = key(3);
    distinct.enable_migrate = true;
    distinct.pool_migration_fee = 15_000_001;
    distinct.creator_fee_basis_points = 30;
    distinct.fee_recipients = [key(10), key(11), key(12), key(13), key(14), key(15), key(16)];
    distinct.set_creator_authority = key(4);
    distinct.admin_set_creator_authority = key(5);
    distinct.create_v2_enabled = true;
    distinct.whitelist_pda = key(6);
    distinct.reserved_fee_recipient = key(7);
    distinct.mayhem_mode_enabled = false;
    distinct.reserved_fee_recipients = [key(20), key(21), key(22), key(23), key(24), key(25), key(26)];
    distinct.is_cashback_enabled = true;
    distinct.buyback_fee_recipients = [key(30), key(31), key(32), key(33), key(34), key(35), key(36), key(37)];
    distinct.buyback_basis_points = 2_000;
    distinct.initial_virtual_quote_reserves = u64::MAX;
    distinct.whitelisted_quote_mints = [key(40)];
    distinct.creator_fee_configurable = true;
    distinct.max_configurable_creator_fee_bps = 9_007_199_254_740_993;
    distinct.holder_reward_claim_authority = key(8);
    distinct.is_holder_reward_enabled = true;
    distinct.max_curve_depth = 255;

    // Same values with an unsorted fee-recipient list: the program's
    // `binary_search` membership is then not a plain `contains`.
    let mut unsorted = distinct.clone_for_fixture();
    unsorted.fee_recipients = [key(16), key(10), key(15), key(11), key(14), key(12), key(13)];

    let serialize = |g: &Global| { let mut out = Vec::new(); g.try_serialize(&mut out).unwrap(); out };
    let base_len = serialize(&zero).len();
    let mut padded = serialize(&distinct);
    padded.extend_from_slice(&[0xa5; 64]);

    println!("{{\"provenance\":\"beam_program::Global AccountSerialize via the locked crate; synthetic values only\",");
    println!("\"discriminatorHex\":\"{}\",\"serializedLength\":{base_len},", hex(Global::DISCRIMINATOR));
    println!("\"cases\":[");
    let cases: [(&str, Vec<u8>, &Global); 4] = [
        ("zero", serialize(&zero), &zero),
        ("distinct", serialize(&distinct), &distinct),
        ("unsortedFeeRecipients", serialize(&unsorted), &unsorted),
        ("trailingPadding", padded, &distinct),
    ];
    for (index, (name, data, global)) in cases.iter().enumerate() {
        let decoded = Global::try_deserialize(&mut &data[..]).unwrap();
        assert_eq!(fields(&decoded), fields(global));
        println!(
            "{}{{\"name\":\"{name}\",\"dataHex\":\"{}\",\"fields\":{}}}",
            if index == 0 { "" } else { "," }, hex(data), fields(global)
        );
    }
    println!("]}}");
}

trait CloneForFixture { fn clone_for_fixture(&self) -> Self; }
impl CloneForFixture for Global {
    fn clone_for_fixture(&self) -> Self {
        let mut out = Vec::new();
        self.try_serialize(&mut out).unwrap();
        Global::try_deserialize(&mut &out[..]).unwrap()
    }
}
