//! The handle projections `x-nexus-handle` declares over the `--client`
//! HTTP callers.
//!
//! The committed `samples/schemas/streams.nexusrpc.yaml` advanced output is the
//! golden for the sample's two handles; these pin the projection rules and the
//! refusals, and `http_caller_toolchain.rs` drives the emitted handles.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::generator::GenerationMode;
use nexgen::language::Language;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_output_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let ordinal = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("nexgen-{label}-{nanos}-{ordinal}"))
}

/// Generates one contract and answers with the emitted files by name.
fn generate(
    language: Language,
    contract: &str,
    client: bool,
    label: &str,
) -> (PathBuf, Vec<(String, String)>) {
    let temp_dir = unique_output_path(label);
    let input_dir = temp_dir.join("input");
    fs::create_dir_all(&input_dir).unwrap();
    let input_path = input_dir.join("streams.nexusrpc.yaml");
    fs::write(&input_path, contract).unwrap();
    let output_path = temp_dir.join("streams");
    generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            client,
            ..Default::default()
        },
        language,
        input_paths: vec![input_path],
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .unwrap();
    (temp_dir, read_files(&output_path))
}

/// Generates one contract and answers with the loader's refusal, if it made one.
fn refusal(contract: &str, label: &str) -> String {
    let temp_dir = unique_output_path(label);
    let input_dir = temp_dir.join("input");
    fs::create_dir_all(&input_dir).unwrap();
    let input_path = input_dir.join("streams.nexusrpc.yaml");
    fs::write(&input_path, contract).unwrap();
    let outcome = generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            client: true,
            ..Default::default()
        },
        language: Language::Go,
        input_paths: vec![input_path],
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: temp_dir.join("streams"),
        format: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    });
    let message = match outcome {
        Ok(()) => panic!("the loader accepted a contract it cannot lower"),
        Err(error) => error.to_string(),
    };
    fs::remove_dir_all(&temp_dir).ok();
    message
}

fn read_files(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            files.push((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
    files.sort();
    files
}

fn file<'a>(files: &'a [(String, String)], name: &str) -> &'a str {
    files
        .iter()
        .find(|(file_name, _)| file_name == name)
        .map(|(_, contents)| contents.as_str())
        .unwrap_or_else(|| {
            panic!(
                "{name} should be generated; got {:?}",
                files.iter().map(|(name, _)| name).collect::<Vec<_>>()
            )
        })
}

fn has_file(files: &[(String, String)], name: &str) -> bool {
    files.iter().any(|(file_name, _)| file_name == name)
}

const HANDLE_CONTRACT: &str = r##"
nexusrpc: "1.0.0"
services:
  StreamService:
    fqn: example.streams.v1.StreamService
    x-nexus-handle:
      StreamHandle: [workflowId, stream]
      StreamProducer: [workflowId, stream, producerId, attempt]
    operations:
      append:
        fqn: append
        input: { $ref: "#/$defs/AppendInput" }
        output: { $ref: "#/$defs/AppendOutput" }
      read:
        fqn: read
        x-nexus-long-poll:
          wait-field: waitMs
          result-field: records
        input: { $ref: "#/$defs/ReadInput" }
        output: { $ref: "#/$defs/ReadOutput" }
      ping:
        fqn: ping
$defs:
  AppendInput:
    type: object
    properties:
      workflowId: { type: string }
      stream: { type: string }
      producerId: { type: string }
      attempt: { type: integer, minimum: 1 }
      batchIndex: { type: integer, minimum: 1 }
      payloads:
        type: array
        items: { type: string, contentEncoding: base64 }
      finish: { type: boolean }
    required: [workflowId, stream, producerId, attempt, batchIndex]
    additionalProperties: false
  AppendOutput:
    type: object
    properties:
      cursor: { type: string, x-nexus-cursor: StreamCursor }
    additionalProperties: false
  ReadInput:
    type: object
    properties:
      workflowId: { type: string }
      stream: { type: string }
      afterToken: { type: string, x-nexus-cursor: StreamCursor }
      waitMs: { type: integer, minimum: 0 }
    required: [workflowId, stream]
    additionalProperties: false
  ReadOutput:
    type: object
    properties:
      records: { type: array, items: { type: string } }
      nextToken: { type: string, x-nexus-cursor: StreamCursor }
    additionalProperties: false
"##;

#[test]
fn python_projects_handles_over_the_flat_caller() {
    let (temp_dir, files) = generate(
        Language::Python,
        HANDLE_CONTRACT,
        true,
        "handles-python-handles",
    );
    let client = file(&files, "client.py");
    // The flat caller stays, and builds every handle from all of its keys.
    assert!(
        client.contains("    async def read(\n        self,\n        request: ReadInput,"),
        "{client}"
    );
    assert!(client.contains("    def stream(\n        self,\n        *,\n        workflow_id: str,\n        stream: str,\n    ) -> \"StreamHandle\":"), "{client}");
    assert!(client.contains("    def stream_producer(\n"), "{client}");
    // A joined operation drops the bound members and injects them.
    assert!(client.contains("class StreamHandle:"), "{client}");
    let handle = client
        .split("class StreamHandle:")
        .nth(1)
        .and_then(|rest| rest.split("class StreamProducer:").next())
        .unwrap_or_default();
    assert!(handle.contains("    async def read(\n        self,\n        *,\n        after_token: str | None = None,\n        wait_ms: int | None = None,\n    ) -> ReadOutput:"), "{handle}");
    assert!(handle.contains("workflow_id=self._workflow_id,\n                stream=self._stream,\n                after_token=after_token,"), "{handle}");
    // The loop method rides along and leaves the wait member to the loop.
    assert!(handle.contains("    async def read_until_records(\n        self,\n        *,\n        after_token: str | None = None,\n        deadline: float,\n    ) -> ReadOutput:"), "{handle}");
    assert!(handle.contains("deadline=deadline,"), "{handle}");
    // The producer is built from the handle by the keys it adds, named after
    // the part of its name the parent does not already say.
    assert!(handle.contains("    def producer(\n        self,\n        *,\n        producer_id: str,\n        attempt: int,\n    ) -> \"StreamProducer\":"), "{handle}");
    assert!(handle.contains("            self._client,\n            workflow_id=self._workflow_id,\n            stream=self._stream,\n            producer_id=producer_id,\n            attempt=attempt,"), "{handle}");
    // An operation without the keys stays off the handle.
    assert!(!handle.contains("def ping("), "{handle}");
    // The producer carries append with its own keys bound, and nothing else.
    let producer = client
        .split("class StreamProducer:")
        .nth(1)
        .unwrap_or_default();
    assert!(producer.contains("    async def append(\n        self,\n        *,\n        batch_index: int,\n        payloads: list[bytes] | None = None,\n        finish: bool | None = None,\n    ) -> AppendOutput:"), "{producer}");
    assert!(!producer.contains("def read("), "{producer}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn python_models_carry_no_handle_without_a_client() {
    // The projection is client-side only: the models module and the service
    // definition do not change when a contract declares handles.
    let (temp_dir, files) = generate(
        Language::Python,
        HANDLE_CONTRACT,
        false,
        "handles-python-handles-models",
    );
    assert!(!has_file(&files, "client.py"));
    for (name, contents) in &files {
        assert!(
            !contents.contains("StreamHandle"),
            "{name} carries the handle"
        );
        assert!(
            !contents.contains("x-nexus-"),
            "{name} leaked an annotation keyword"
        );
    }
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_projects_handles_over_the_flat_caller() {
    let (temp_dir, files) = generate(Language::Go, HANDLE_CONTRACT, true, "handles-go-handles");
    let client = file(&files, "client.go");
    assert!(client.contains("type StreamHandle struct {\n\tclient *StreamServiceHTTPClient\n\tworkflowID string\n\tstream string\n}"), "{client}");
    assert!(client.contains("func (c *StreamServiceHTTPClient) Stream(workflowID string, stream string) *StreamHandle {"), "{client}");
    assert!(client.contains("func (c *StreamServiceHTTPClient) StreamProducer(workflowID string, stream string, producerID string, attempt int64) *StreamProducer {"), "{client}");
    // A per-handle request type drops the bound members, and the method
    // builds the operation's own input from both.
    assert!(
        client.contains(
            "type StreamHandleReadRequest struct {\n\tAfterToken *string\n\tWaitMs *int64\n}"
        ),
        "{client}"
    );
    assert!(client.contains("func (h *StreamHandle) Read(ctx context.Context, request StreamHandleReadRequest) (ReadOutput, error) {\n\tfull := ReadInput{\n\t\tWorkflowID: h.workflowID,\n\t\tStream: h.stream,\n\t\tAfterToken: request.AfterToken,\n\t\tWaitMs: request.WaitMs,\n\t}\n\treturn h.client.Read(ctx, full)\n}"), "{client}");
    assert!(client.contains("func (h *StreamHandle) ReadUntilRecords(ctx context.Context, request StreamHandleReadRequest, deadline time.Duration) (ReadOutput, error) {"), "{client}");
    assert!(
        client.contains(
            "func (h *StreamHandle) Producer(producerID string, attempt int64) *StreamProducer {"
        ),
        "{client}"
    );
    assert!(client.contains("type StreamProducerAppendRequest struct {\n\tBatchIndex int64\n\tPayloads [][]byte\n\tFinish *bool\n}"), "{client}");
    assert!(client.contains("func (h *StreamProducer) Append(ctx context.Context, request StreamProducerAppendRequest) (AppendOutput, error) {\n\tfull := AppendInput{\n\t\tWorkflowID: h.workflowID,\n\t\tStream: h.stream,\n\t\tProducerID: h.producerID,\n\t\tAttempt: h.attempt,\n\t\tBatchIndex: request.BatchIndex,\n"), "{client}");
    assert!(!client.contains("func (h *StreamHandle) Ping("), "{client}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn the_other_targets_ignore_the_handle_keyword() {
    for language in [Language::TypeScript, Language::Java] {
        let temp_dir = unique_output_path("handles-other-targets-handles");
        let input_dir = temp_dir.join("input");
        fs::create_dir_all(&input_dir).unwrap();
        let input_path = input_dir.join("streams.nexusrpc.yaml");
        fs::write(&input_path, HANDLE_CONTRACT).unwrap();
        let output_path = temp_dir.join("streams");
        generate_to_file(&GenerateRequest {
            config: NexgenConfig::default(),
            language,
            input_paths: vec![input_path],
            support_package: None,
            descriptor_paths: Vec::new(),
            output_path: output_path.clone(),
            format: false,
            java_package_name: (language == Language::Java)
                .then(|| "json_schema.streams".to_string()),
            ts_date_time_types: Default::default(),
        })
        .unwrap_or_else(|error| panic!("{language:?} should generate: {error}"));
        for (name, contents) in read_files(&output_path) {
            assert!(
                !contents.contains("x-nexus-"),
                "{language:?} {name} leaked an annotation keyword"
            );
            assert!(
                !contents.contains("StreamHandle"),
                "{language:?} {name} emitted a handle"
            );
        }
        fs::remove_dir_all(temp_dir).unwrap();
    }
}

fn handle_variant(handles: &str) -> String {
    HANDLE_CONTRACT.replace(
        "    x-nexus-handle:\n      StreamHandle: [workflowId, stream]\n      StreamProducer: [workflowId, stream, producerId, attempt]\n",
        handles,
    )
}

#[test]
fn a_handle_nothing_joins_is_refused() {
    let message = refusal(
        &handle_variant("    x-nexus-handle:\n      Orphan: [workflowId, finish, afterToken]\n"),
        "handles-handle-orphan",
    );
    assert!(message.contains("handle `Orphan` binds [workflowId, finish, afterToken], which no operation's input carries in full"), "{message}");
}

#[test]
fn two_handles_over_one_key_set_are_refused() {
    let message = refusal(
        &handle_variant(
            "    x-nexus-handle:\n      StreamHandle: [workflowId, stream]\n      Twin: [stream, workflowId]\n",
        ),
        "handles-handle-twins",
    );
    assert!(
        message.contains("handles `StreamHandle` and `Twin` bind the same members"),
        "{message}"
    );
}

#[test]
fn a_key_declared_two_ways_is_refused() {
    // `attempt` is required on append; the variant adds it optional on read.
    let contract = handle_variant("    x-nexus-handle:\n      Attempt: [attempt]\n").replace(
        "      afterToken: { type: string, x-nexus-cursor: StreamCursor }\n",
        "      afterToken: { type: string, x-nexus-cursor: StreamCursor }\n      attempt: { type: integer, minimum: 1 }\n",
    );
    let message = refusal(&contract, "handles-handle-two-ways");
    assert!(message.contains("handle `Attempt` binds `attempt`, which operations `append` and `read` declare differently"), "{message}");
}

#[test]
fn a_handle_named_like_a_model_or_an_operation_is_refused() {
    let message = refusal(
        &handle_variant("    x-nexus-handle:\n      ReadInput: [workflowId]\n"),
        "handles-handle-model-name",
    );
    // The handle type joins the caller's other names in the collision pass.
    assert!(
        message.contains("ReadInput") && message.contains("HTTP caller handle type"),
        "{message}"
    );
    let message = refusal(
        &handle_variant("    x-nexus-handle:\n      Read: [workflowId]\n"),
        "handles-handle-operation-name",
    );
    assert!(
        message.contains("handle `Read` would be built by `Read`, which is also an operation"),
        "{message}"
    );
}

#[test]
fn a_handle_list_has_to_name_members() {
    let message = refusal(
        &handle_variant("    x-nexus-handle:\n      StreamHandle: workflowId\n"),
        "handles-handle-grammar",
    );
    assert!(
        message.contains("`x-nexus-handle.StreamHandle` must be a list of wire member names"),
        "{message}"
    );
    let message = refusal(
        &handle_variant("    x-nexus-handle:\n      StreamHandle: [workflowId, workflowId]\n"),
        "handles-handle-twice",
    );
    assert!(message.contains("lists `workflowId` twice"), "{message}");
}

#[test]
fn a_key_with_one_admissible_value_is_refused() {
    let contract = handle_variant("    x-nexus-handle:\n      Fixed: [workflowId, finish]\n")
        .replace(
            "      finish: { type: boolean }\n",
            "      finish: { type: boolean, const: true }\n",
        );
    let message = refusal(&contract, "handles-const-key");
    assert!(
        message.contains("handle `Fixed` binds `finish`, which operation `append` declares with one admissible value"),
        "{message}"
    );
}

#[test]
fn a_go_handle_request_type_named_like_a_model_is_refused() {
    let contract = HANDLE_CONTRACT.replace(
        "  AppendOutput:\n",
        "  StreamHandleReadRequest:\n    type: object\n    properties:\n      note: { type: string }\n  AppendOutput:\n",
    );
    let message = refusal(&contract, "handles-request-type-name");
    assert!(
        message.contains("StreamHandleReadRequest")
            && message.contains("HTTP caller handle request type"),
        "{message}"
    );
}

#[test]
fn a_go_handle_converts_an_enum_key_into_each_operations_own_type() {
    // Go names an inline enum after its model, so `append` and `read` type the
    // one `mode` key as two types over the same string.
    let contract = handle_variant(
        "    x-nexus-handle:\n      ModeHandle: [workflowId, mode]\n      ModeReader: [workflowId, mode, afterToken]\n",
    )
    .replace(
        "      finish: { type: boolean }\n",
        "      finish: { type: boolean }\n      mode: { type: string, enum: [fast, safe] }\n",
    )
    .replace(
        "      afterToken: { type: string, x-nexus-cursor: StreamCursor }\n",
        "      afterToken: { type: string, x-nexus-cursor: StreamCursor }\n      mode: { type: string, enum: [fast, safe] }\n",
    );
    let (temp_dir, files) = generate(Language::Go, &contract, true, "handles-go-enum-key");
    let client = file(&files, "client.go");
    assert!(client.contains("\tmode *AppendInputMode\n"), "{client}");
    assert!(
        client.contains("\t\tMode: (*ReadInputMode)(h.mode),\n"),
        "{client}"
    );
    // The child is built from the parent, which holds the other model's type.
    assert!(
        client.contains("\t\tmode: (*ReadInputMode)(h.mode),\n"),
        "{client}"
    );
    assert!(client.contains("\t\tMode: h.mode,\n"), "{client}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn a_go_handle_method_for_a_void_output_returns_only_an_error() {
    let contract = HANDLE_CONTRACT.replace(
        "      ping:\n        fqn: ping\n",
        "      close:\n        fqn: close\n        input: { $ref: \"#/$defs/ReadInput\" }\n",
    );
    let (temp_dir, files) = generate(Language::Go, &contract, true, "handles-go-void");
    let client = file(&files, "client.go");
    assert!(
        client.contains(
            "func (h *StreamHandle) Close(ctx context.Context, request StreamHandleCloseRequest) error {"
        ),
        "{client}"
    );
    fs::remove_dir_all(temp_dir).unwrap();
}
