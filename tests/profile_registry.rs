use std::fs;

use herdr_harness_coordinator::{
    contract::{HarnessId, HarnessKind},
    profile::ProfileRegistry,
};

#[test]
fn registry_resolves_only_the_explicit_profile_and_filters_environment() {
    let directory = tempfile::tempdir().expect("profile directory");
    let executable = std::env::current_exe().expect("test executable path");
    let source = format!(
        "schema_version = 1\nid = \"omp-main\"\nkind = \"omp\"\nexecutable = {:?}\nprovider_profile = \"coordinator-worker\"\ninherit_env = [\"PATH\", \"API_TOKEN\"]\n",
        executable
    );
    fs::write(directory.path().join("omp.toml"), &source).expect("profile fixture");

    let registry = ProfileRegistry::load(directory.path()).expect("registry must load");
    assert_eq!(
        registry.ids(),
        vec!["omp-main".parse::<HarnessId>().expect("valid ID")]
    );
    let resolved = registry
        .resolve(
            &"omp-main".parse().expect("valid ID"),
            HarnessKind::Omp,
            [("PATH", "/bin"), ("SECRET", "excluded")],
        )
        .expect("explicit profile must resolve");
    assert_eq!(resolved.snapshot, source);
    assert_eq!(resolved.digest.len(), 64);
    assert_eq!(
        resolved.environment.get("PATH").map(String::as_str),
        Some("/bin")
    );
    assert!(!resolved.environment.contains_key("SECRET"));
}

#[test]
fn registry_rejects_kind_mismatch_instead_of_routing_automatically() {
    let directory = tempfile::tempdir().expect("profile directory");
    let executable = std::env::current_exe().expect("test executable path");
    fs::write(
        directory.path().join("omp.toml"),
        format!(
            "schema_version = 1\nid = \"omp-main\"\nkind = \"omp\"\nexecutable = {:?}\nprovider_profile = \"coordinator-worker\"\n",
            executable
        ),
    )
    .expect("profile fixture");
    let registry = ProfileRegistry::load(directory.path()).expect("registry must load");
    let error = registry
        .resolve(
            &"omp-main".parse().expect("valid ID"),
            HarnessKind::Codex,
            std::iter::empty::<(String, String)>(),
        )
        .expect_err("registry must not choose another profile or Kind");
    assert!(error.to_string().contains("not compatible"));
}
