use std::fs;

use herdr_harness_coordinator::{
    contract::{HarnessId, HarnessKind},
    profile::ProfileRegistry,
};

#[test]
fn registry_resolves_only_the_explicit_profile_and_filters_environment() {
    let directory = tempfile::tempdir().expect("profile directory");
    let executable = std::env::current_exe().expect("test executable path");
    let executable = executable.display();
    let source = format!(
        "schema_version = 1\nid = \"omp-main\"\nkind = \"omp\"\nexecutable = \"{executable}\"\nprovider_profile = \"coordinator-worker\"\ninherit_env = [\"PATH\", \"API_TOKEN\"]\n"
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
    let executable = executable.display();
    fs::write(
        directory.path().join("omp.toml"),
        format!(
            "schema_version = 1\nid = \"omp-main\"\nkind = \"omp\"\nexecutable = \"{executable}\"\nprovider_profile = \"coordinator-worker\"\n"
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

#[test]
fn registry_resolves_v2_bare_executable_on_each_new_session() {
    let directory = tempfile::tempdir().expect("profile directory");
    let bin = tempfile::tempdir().expect("binary directory");
    let executable = bin.path().join("omp-current");
    fs::write(&executable, "fixture").expect("fake executable");
    fs::write(
        directory.path().join("omp.toml"),
        "schema_version = 2\nid = \"omp-kimi\"\nkind = \"omp\"\nexecutable = \"omp-current\"\nmodel = \"kimi-code/k3:high\"\ninherit_env = [\"PATH\"]\n",
    )
    .expect("profile fixture");
    let registry = ProfileRegistry::load(directory.path()).expect("registry must load");

    let resolved = registry
        .resolve(
            &"omp-kimi".parse().expect("valid ID"),
            HarnessKind::Omp,
            [("PATH", bin.path().to_string_lossy().into_owned())],
        )
        .expect("bare executable must resolve");

    assert_eq!(resolved.executable, executable);
    assert_eq!(resolved.profile.provider_profile, None);
}
