use std::str::FromStr;

use herdr_harness_coordinator::contract::HarnessId;

#[test]
fn harness_id_accepts_a_valid_slug() {
    let id = HarnessId::from_str("codex-review");

    assert!(id.is_ok(), "valid Harness ID was rejected: {id:?}");
}

#[test]
fn harness_id_rejects_repeated_separators() {
    let error = HarnessId::from_str("codex--review").expect_err("invalid ID must fail");

    assert_eq!(error.to_string(), "invalid Harness ID `codex--review`");
}
