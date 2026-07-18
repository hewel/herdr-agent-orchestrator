use herdr_harness_coordinator::contract::{HarnessKind, HarnessLaunchProfileV1, Validate};

#[test]
fn launch_profile_resolves_a_pinned_omp_configuration() {
    let profile: HarnessLaunchProfileV1 = toml::from_str(
        r#"
        schema_version = 1
        id = "omp-worker"
        kind = "omp"
        executable = "/usr/bin/omp"
        provider_profile = "work"
        model = "anthropic/claude-sonnet-4"
        inherit_env = ["ANTHROPIC_API_KEY"]
        config_overlays = ["/home/user/.config/omp/coordinator.yml"]
        "#,
    )
    .expect("profile must deserialize");

    profile.validate().expect("profile must validate");
    assert_eq!(profile.kind, HarnessKind::Omp);
}
