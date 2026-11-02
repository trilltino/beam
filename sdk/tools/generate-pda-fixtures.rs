//! Host-only PDA/ATA/curve-point fixture producer. No handler, wallet, RPC or
//! transaction. Run as an example of the existing, locked beam-program package
//! (copy to programs/beam/examples/beam_pda_fixture.rs, run, then delete):
//!
//!   cargo run --locked --manifest-path programs/beam/Cargo.toml --example beam_pda_fixture
//!
//! Seeds below restate the Anchor constraints on `Create`, `LegacyBuy`,
//! `LegacySell` (programs/beam/src/lib.rs) and `CreateV2`
//! (programs/beam/src/create_v2.rs), plus the `bonding-curve-v2` remaining
//! account checked by `check_bonding_curve_v2` (programs/beam/src/sell.rs);
//! program IDs come from the crate itself.
use anchor_lang::solana_program::{hash::hashv, pubkey::Pubkey};
use anchor_spl::associated_token::get_associated_token_address_with_program_id as ata;

fn synthetic(label: &str) -> Pubkey {
    Pubkey::new_from_array(hashv(&[b"beam-pda-fixture", label.as_bytes()]).to_bytes())
}
fn hex(data: &[u8]) -> String { data.iter().map(|b| format!("{b:02x}")).collect() }
fn pda(name: &str, seeds: &[&[u8]], program: &Pubkey, first: &mut bool) {
    let (address, bump) = Pubkey::find_program_address(seeds, program);
    let seeds_hex: Vec<String> = seeds.iter().map(|s| format!("\"{}\"", hex(s))).collect();
    println!(
        "{}{{\"name\":\"{name}\",\"programId\":\"{program}\",\"seedsHex\":[{}],\"address\":\"{address}\",\"bump\":{bump}}}",
        if *first { "" } else { "," }, seeds_hex.join(",")
    );
    *first = false;
}

fn main() {
    let beam = beam_program::ID;
    let mayhem = beam_program::mayhem::ID;
    let metadata = anchor_spl::metadata::ID;
    let token = anchor_spl::token::ID;
    let token_2022 = anchor_spl::token_2022::ID;
    let mint = synthetic("mint");
    let user = synthetic("user");
    let creator = synthetic("creator");

    println!("{{\"provenance\":\"Pubkey::find_program_address and spl-associated-token-account via the locked beam-program crate; synthetic inputs only\",");
    println!("\"programIds\":{{\"beam\":\"{beam}\",\"mayhem\":\"{mayhem}\",\"metadata\":\"{metadata}\",\"token\":\"{token}\",\"token2022\":\"{token_2022}\",\"associatedToken\":\"{}\",\"system\":\"{}\"}},",
        anchor_spl::associated_token::ID, anchor_lang::solana_program::system_program::ID);
    println!("\"inputs\":{{\"mint\":\"{mint}\",\"user\":\"{user}\",\"creator\":\"{creator}\"}},");

    println!("\"pdas\":[");
    let mut first = true;
    pda("global", &[b"global"], &beam, &mut first);
    pda("mintAuthority", &[b"mint-authority"], &beam, &mut first);
    pda("eventAuthority", &[b"__event_authority"], &beam, &mut first);
    pda("globalVolumeAccumulator", &[b"global_volume_accumulator"], &beam, &mut first);
    pda("bondingCurve", &[b"bonding-curve", mint.as_ref()], &beam, &mut first);
    pda("bondingCurveV2", &[b"bonding-curve-v2", mint.as_ref()], &beam, &mut first);
    pda("creatorVault", &[b"creator-vault", creator.as_ref()], &beam, &mut first);
    pda("userVolumeAccumulator", &[b"user_volume_accumulator", user.as_ref()], &beam, &mut first);
    pda("metadata", &[b"metadata", metadata.as_ref(), mint.as_ref()], &metadata, &mut first);
    pda("mayhemGlobalParams", &[b"global-params"], &mayhem, &mut first);
    pda("mayhemSolVault", &[b"sol-vault"], &mayhem, &mut first);
    pda("mayhemState", &[b"mayhem-state", mint.as_ref()], &mayhem, &mut first);
    println!("],");

    let curve = Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &beam).0;
    println!("\"associatedTokenAccounts\":[");
    let cases = [
        ("bondingCurveToken", curve, token), ("bondingCurveToken2022", curve, token_2022),
        ("userToken", user, token), ("userToken2022", user, token_2022),
    ];
    for (index, (name, owner, program)) in cases.iter().enumerate() {
        println!(
            "{}{{\"name\":\"{name}\",\"owner\":\"{owner}\",\"mint\":\"{mint}\",\"tokenProgram\":\"{program}\",\"address\":\"{}\"}}",
            if index == 0 { "" } else { "," }, ata(owner, &mint, program)
        );
    }
    println!("],");

    // Curve-membership vectors: hashes plus encodings that exercise the sign
    // bit, non-canonical y and the identity point.
    let mut points: Vec<[u8; 32]> = (0u8..96).map(|i| hashv(&[b"beam-curve-vector", &[i]]).to_bytes()).collect();
    let mut p = [0xffu8; 32]; p[0] = 0xed; p[31] = 0x7f; // p = 2^255 - 19 (non-canonical 0)
    let mut p_plus_one = p; p_plus_one[0] = 0xee;      // non-canonical 1
    let mut one = [0u8; 32]; one[0] = 1;
    let mut neg_zero = [0u8; 32]; neg_zero[31] = 0x80;
    let mut two = [0u8; 32]; two[0] = 2;
    points.extend([[0u8; 32], neg_zero, one, two, p, p_plus_one, [0xff; 32], beam.to_bytes(), mayhem.to_bytes(), token.to_bytes()]);
    println!("\"curvePoints\":[");
    for (index, bytes) in points.iter().enumerate() {
        println!(
            "{}{{\"hex\":\"{}\",\"onCurve\":{}}}",
            if index == 0 { "" } else { "," }, hex(bytes), Pubkey::new_from_array(*bytes).is_on_curve()
        );
    }
    println!("]}}");
}
