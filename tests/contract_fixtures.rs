use std::{fs, path::Path};

use herdr_harness_coordinator::contract::{
    DeliveryReceiptV1, HarnessDefinitionV1, MessageSubmissionV1, RepositoryObservationV1,
    ResultManifestV1, TaskSubmissionV1, Validate,
};
use serde::de::DeserializeOwned;

fn read<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, serde_json::Error> {
    let bytes = fs::read(path).expect("fixture must be readable");
    serde_json::from_slice(&bytes)
}

#[test]
fn checked_in_positive_fixtures_match_rust_contracts() {
    let harness: HarnessDefinitionV1 =
        read("schemas/fixtures/harness-definition/worker.valid.json")
            .expect("Harness fixture must deserialize");
    let task: TaskSubmissionV1 = read("schemas/fixtures/task-submission/mutating.valid.json")
        .expect("Task fixture must deserialize");
    let message: MessageSubmissionV1 = read("schemas/fixtures/message-submission/reply.valid.json")
        .expect("Message fixture must deserialize");
    let result: ResultManifestV1 = read("schemas/fixtures/result-manifest/completed.valid.json")
        .expect("Result fixture must deserialize");
    let receipt: DeliveryReceiptV1 = read("schemas/fixtures/delivery-receipt/accepted.valid.json")
        .expect("Receipt fixture must deserialize");
    let observation: RepositoryObservationV1 =
        read("schemas/fixtures/repository-observation/result.valid.json")
            .expect("Observation fixture must deserialize");

    let outcomes = [
        harness.validate(),
        task.validate(),
        message.validate(),
        result.validate(),
        receipt.validate(),
        observation.validate(),
    ];
    assert!(
        outcomes.iter().all(Result::is_ok),
        "typed validation failed: {outcomes:?}"
    );
}

#[test]
fn invalid_task_path_is_rejected_by_typed_validation() {
    let task: TaskSubmissionV1 =
        read("schemas/fixtures/invalid/task-submission/invalid-path.invalid.json")
            .expect("shape remains readable before semantic validation");

    assert!(task.validate().is_err());
}

#[test]
fn unknown_harness_field_is_rejected_during_deserialization() {
    let harness = read::<HarnessDefinitionV1>(
        "schemas/fixtures/invalid/harness-definition/unknown-field.invalid.json",
    );

    assert!(harness.is_err());
}
