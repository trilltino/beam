//! Explicit, hash-verified loading of locally compiled test artifacts only.
use solana_sdk::{account::Account, bpf_loader, hash::hash, rent::Rent};

pub fn account(prefix: &str) -> Account {
    let path = std::env::var(format!("{prefix}_ELF_PATH")).expect("explicit SBF ELF path required");
    let expected = std::env::var(format!("{prefix}_ELF_SHA256"))
        .expect("explicit expected SBF ELF SHA-256 required");
    assert_eq!(
        expected.len(),
        64,
        "expected SHA-256 must have 64 hex digits"
    );
    let data = std::fs::read(&path).expect("cannot read explicit SBF artifact");
    assert!(data.starts_with(b"\x7fELF"), "artifact is not ELF");
    let observed: String = hash(&data)
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        observed,
        expected.to_ascii_lowercase(),
        "artifact SHA-256 mismatch"
    );
    println!(
        "LOADED_SBF: {prefix} path={path} sha256={observed} bytes={}",
        data.len()
    );
    Account {
        lamports: Rent::default().minimum_balance(data.len()).max(1),
        data,
        owner: bpf_loader::id(),
        executable: true,
        rent_epoch: 0,
    }
}
