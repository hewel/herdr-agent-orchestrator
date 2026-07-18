use herdr_harness_coordinator::{
    adapter::{
        CodexFrame, CorrelationId, HarnessAdapter, OmpFrame, classify_codex_frame,
        classify_omp_frame, validate_codex_version_output, validate_omp_version_output,
    },
    contract::HarnessKind,
};

#[test]
fn adapter_error_exposes_typed_delivery_ambiguity() {
    let ambiguous = herdr_harness_coordinator::adapter::AdapterError::DeliveryAmbiguous {
        kind: HarnessKind::Omp,
        message: "write timed out".to_owned(),
    };
    let pre_write = herdr_harness_coordinator::adapter::AdapterError::Operation {
        kind: HarnessKind::Omp,
        message: "provider is not running".to_owned(),
    };

    assert!(ambiguous.provider_bytes_may_have_been_written());
    assert!(!pre_write.provider_bytes_may_have_been_written());
}

#[test]
fn harness_adapter_is_object_safe_for_runtime_provider_selection() {
    fn accepts_adapter(_: Option<&mut dyn HarnessAdapter>) {}

    accepts_adapter(None);
}

#[test]
fn omp_classifier_recognizes_ready_without_a_correlation_id() {
    let frame = classify_omp_frame(r#"{"type":"ready"}"#).expect("valid ready frame");

    assert_eq!(frame, OmpFrame::Ready);
}

#[test]
fn omp_classifier_correlates_a_successful_response() {
    let frame = classify_omp_frame(
        r#"{"type":"response","id":"delivery-7","command":"prompt","success":true,"data":{"agentInvoked":true}}"#,
    )
    .expect("valid response frame");

    assert!(matches!(
        frame,
        OmpFrame::Response {
            id: CorrelationId::String(ref id),
            ref command,
            result: Ok(_),
        } if id == "delivery-7" && command == "prompt"
    ));
}

#[test]
fn omp_classifier_keeps_interleaved_host_tool_calls_separate_from_responses() {
    let frame = classify_omp_frame(
        r#"{"type":"host_tool_call","id":"host-4","toolCallId":"call-9","toolName":"harness_complete","arguments":{"schema_version":1}}"#,
    )
    .expect("valid host tool frame");

    assert!(matches!(
        frame,
        OmpFrame::HostToolCall {
            id: CorrelationId::String(ref id),
            ref tool_call_id,
            ref tool_name,
            ..
        } if id == "host-4" && tool_call_id == "call-9" && tool_name == "harness_complete"
    ));
}

#[test]
fn omp_classifier_preserves_agent_end_as_a_session_event() {
    let frame = classify_omp_frame(r#"{"type":"agent_end","messages":[]}"#)
        .expect("valid session event frame");

    assert!(matches!(
        frame,
        OmpFrame::SessionEvent { ref event_type, .. } if event_type == "agent_end"
    ));
}

#[test]
fn omp_classifier_rejects_a_response_without_correlation() {
    let error = classify_omp_frame(r#"{"type":"response","command":"prompt","success":true}"#)
        .expect_err("response correlation is mandatory at the adapter boundary");

    assert!(error.to_string().contains("correlation"));
}

#[test]
fn codex_classifier_correlates_numeric_responses() {
    let frame = classify_codex_frame(r#"{"id":12,"result":{"turn":{"id":"turn-1"}}}"#)
        .expect("valid response frame");

    assert!(matches!(
        frame,
        CodexFrame::Response {
            id: CorrelationId::Number(12),
            result: Ok(_),
        }
    ));
}

#[test]
fn codex_classifier_distinguishes_server_requests_from_notifications() {
    let frame = classify_codex_frame(
        r#"{"id":"approval-2","method":"item/commandExecution/requestApproval","params":{"turnId":"turn-1"}}"#,
    )
    .expect("valid server request frame");

    assert!(matches!(
        frame,
        CodexFrame::ServerRequest {
            id: CorrelationId::String(ref id),
            ref method,
            ..
        } if id == "approval-2" && method == "item/commandExecution/requestApproval"
    ));
}

#[test]
fn codex_classifier_preserves_turn_completion_as_a_notification() {
    let frame = classify_codex_frame(
        r#"{"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}}}"#,
    )
    .expect("valid notification frame");

    assert!(matches!(
        frame,
        CodexFrame::Notification { ref method, .. } if method == "turn/completed"
    ));
}

#[test]
fn codex_classifier_rejects_an_id_without_response_or_method() {
    let error = classify_codex_frame(r#"{"id":3,"params":{}}"#)
        .expect_err("ambiguous correlated frame must fail");

    assert!(error.to_string().contains("result, error, or method"));
}

#[test]
fn omp_version_validator_records_newer_nonempty_output_without_a_pin() {
    let observed = validate_omp_version_output("omp/17.0.4\n").expect("bounded version output");

    assert_eq!(observed, "omp/17.0.4");
}

#[test]
fn codex_version_validator_records_arbitrary_release_output() {
    let observed =
        validate_codex_version_output("codex-cli 0.200.0\r\n").expect("bounded version output");

    assert_eq!(observed, "codex-cli 0.200.0");
}

#[test]
fn version_validator_rejects_empty_or_multiline_output() {
    let error = validate_omp_version_output(" \n").expect_err("empty output must fail");
    assert!(matches!(
        error,
        herdr_harness_coordinator::adapter::AdapterError::UnsupportedVersion {
            kind: HarnessKind::Omp,
            ..
        }
    ));
    assert!(validate_codex_version_output("codex 1\nextra\n").is_err());
}
