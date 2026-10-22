//! WIT annotations for streaming services (`@nexus.offset` on record fields
//! and `@nexus.long-poll` on operations), the caller that `--client`
//! generates for code outside a Workflow, and a resource handle whose methods
//! take the rest of each request.
#![cfg(feature = "advanced")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::generator::GenerationMode;
use nexgen::language::Language;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};
use prost::Message;
use prost_types::field_descriptor_proto::{Label, Type};
use prost_types::{DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet};

static OUTPUT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A log service in the shape of a stream: an append that answers with
/// offsets, and a read that takes one and gives the next.
const LOG_WIT: &str = r#"package acme:logs@1.0.0;

world logs {
  export log-service;
}

/// @nexus.endpoint "logs"
interface log-service {
  use nexus:temporal-types/model@1.0.0.{duration};

  record entry {
    /// @nexus.offset "log-offset"
    offset: option<s64>,
    text: string,
  }

  record append-request {
    log-id: string,
    texts: list<string>,
  }

  record append-response {
    /// @nexus.offset "log-offset"
    first-offset: option<s64>,
    /// @nexus.offset "log-offset"
    last-offset: option<s64>,
  }

  record read-request {
    log-id: string,
    /// @nexus.offset "log-offset"
    from-offset: option<s64>,
    wait: option<duration>,
    idle-timeout: option<duration>,
  }

  record read-response {
    entries: list<entry>,
    /// @nexus.offset "log-offset"
    next-offset: option<s64>,
    done: option<bool>,
  }

  append: func(request: append-request) -> append-response;

  /// @nexus.long-poll
  ///   wait-field="wait"
  ///   max-wait-ms="60000"
  ///   result-field="entries"
  ///   done-field="done"
  ///   offset-field="from-offset"
  ///   next-offset-field="next-offset"
  ///   idle-timeout-field="idle-timeout"
  read: func(request: read-request) -> read-response;
}
"#;

const LONG_POLL_DIRECTIVE: &str = r#"  /// @nexus.long-poll
  ///   wait-field="wait"
  ///   max-wait-ms="60000"
  ///   result-field="entries"
  ///   done-field="done"
  ///   offset-field="from-offset"
  ///   next-offset-field="next-offset"
  ///   idle-timeout-field="idle-timeout"
"#;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn unique_output_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("nexgen-wit-streaming-{label}-{unique}-{counter}"))
}

fn read_files(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) {
    let mut entries = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            read_files(root, &path, files);
        } else {
            files.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                fs::read_to_string(&path).unwrap(),
            );
        }
    }
}

fn try_generate(
    language: Language,
    mode: GenerationMode,
    wit: &str,
) -> Result<BTreeMap<String, String>, String> {
    let config = NexgenConfig {
        mode,
        ..Default::default()
    };
    try_generate_with(config, language, wit, Vec::new())
}

fn try_generate_with(
    config: NexgenConfig,
    language: Language,
    wit: &str,
    extra_inputs: Vec<PathBuf>,
) -> Result<BTreeMap<String, String>, String> {
    try_generate_with_descriptors(config, language, wit, extra_inputs, None)
}

fn try_generate_with_descriptors(
    config: NexgenConfig,
    language: Language,
    wit: &str,
    extra_inputs: Vec<PathBuf>,
    extra_descriptors: Option<&FileDescriptorSet>,
) -> Result<BTreeMap<String, String>, String> {
    let temp_dir = unique_output_path(&format!("{language:?}"));
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("logs.wit");
    fs::write(&input_path, wit).unwrap();
    let output_path = temp_dir.join("generated");
    let mut input_paths = vec![input_path];
    input_paths.extend(extra_inputs);
    input_paths.push(project_root().join("advanced/samples/inputs/deps"));
    let result = generate_to_file(&GenerateRequest {
        config,
        language,
        input_paths,
        support_package: Some(
            match language {
                Language::Dotnet => "Acme.Support",
                _ => "acme_support",
            }
            .to_string(),
        ),
        descriptor_paths: std::iter::once(
            project_root().join("advanced/samples/descriptors/temporal_api.bin"),
        )
        .chain(extra_descriptors.map(|descriptors| {
            let path = temp_dir.join("extra.bin");
            fs::write(&path, descriptors.encode_to_vec()).unwrap();
            path
        }))
        .collect(),
        output_path: output_path.clone(),
        format: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    });
    let files = result.map(|()| {
        let mut files = BTreeMap::new();
        if output_path.is_file() {
            files.insert(String::new(), fs::read_to_string(&output_path).unwrap());
        } else {
            read_files(&output_path, &output_path, &mut files);
        }
        files
    });
    fs::remove_dir_all(temp_dir).unwrap();
    files.map_err(|error| error.to_string())
}

fn generate(language: Language, mode: GenerationMode, wit: &str) -> BTreeMap<String, String> {
    try_generate(language, mode, wit).unwrap()
}

fn generate_error(language: Language, wit: &str) -> String {
    try_generate(language, GenerationMode::NativeApi, wit).unwrap_err()
}

fn file<'a>(files: &'a BTreeMap<String, String>, name: &str) -> &'a str {
    files
        .get(name)
        .unwrap_or_else(|| panic!("missing `{name}` in {:?}", files.keys()))
}

#[test]
fn python_declares_one_offset_type_for_every_field_that_names_it() {
    let files = generate(Language::Python, GenerationMode::NativeApi, LOG_WIT);
    let models = file(&files, "models.py");

    assert_eq!(
        models
            .matches("LogOffset = typing.NewType(\"LogOffset\", int)")
            .count(),
        1,
        "{models}"
    );
    // The members keep their integer type, so the wire form does not move.
    assert!(
        models.contains("    from_offset: int | None = None"),
        "{models}"
    );
    assert!(
        models.contains("    next_offset: int | None = None"),
        "{models}"
    );
}

#[test]
fn offset_fields_generate_in_typescript_and_dotnet() {
    for language in [Language::TypeScript, Language::Dotnet] {
        let files = generate(language, GenerationMode::NativeApi, LOG_WIT);
        assert!(!files.is_empty(), "{language:?}");
    }
}

#[test]
fn offset_directive_rejects_a_field_that_is_not_an_integer() {
    let error = generate_error(
        Language::Python,
        &LOG_WIT.replace(
            "    /// @nexus.offset \"log-offset\"\n    offset: option<s64>,\n    text: string,",
            "    offset: option<s64>,\n    /// @nexus.offset \"log-offset\"\n    text: string,",
        ),
    );
    assert!(
        error.contains("@nexus.offset") && error.contains("only supported on integer fields"),
        "{error}"
    );
}

#[test]
fn offset_directive_requires_a_kebab_case_type_name() {
    for (directive, reason) in [
        ("@nexus.offset", "missing the offset type name"),
        (
            "@nexus.offset \"LogOffset\"",
            "not a kebab-case WIT identifier",
        ),
        (
            "@nexus.offset \"log-offset\" kind=\"cursor\"",
            "takes only the offset type name",
        ),
    ] {
        let error = generate_error(
            Language::Python,
            &LOG_WIT.replacen("@nexus.offset \"log-offset\"", directive, 1),
        );
        assert!(error.contains(reason), "{directive}: {error}");
    }
}

#[test]
fn offset_directive_cannot_be_combined_with_omit() {
    let error = generate_error(
        Language::Python,
        &LOG_WIT.replace(
            "    /// @nexus.offset \"log-offset\"\n    offset: option<s64>,",
            "    /// @nexus.omit\n    /// @nexus.offset \"log-offset\"\n    offset: option<s64>,",
        ),
    );
    assert!(
        error.contains("cannot be combined with `@nexus.offset`"),
        "{error}"
    );
}

#[test]
fn python_offset_type_cannot_share_a_name_with_a_model() {
    let error = generate_error(
        Language::Python,
        &LOG_WIT.replace("\"log-offset\"", "\"entry\""),
    );
    assert!(
        error.contains("`Entry`") && error.contains("offset type"),
        "{error}"
    );
}

/// Replaces the long-poll directive of `read` with `directive`.
fn with_long_poll(directive: &str) -> String {
    LOG_WIT.replace(LONG_POLL_DIRECTIVE, &format!("  /// {directive}\n"))
}

#[test]
fn long_poll_generates_in_python_typescript_and_dotnet() {
    for language in [Language::Python, Language::TypeScript, Language::Dotnet] {
        let files = generate(language, GenerationMode::NativeApi, LOG_WIT);
        assert!(!files.is_empty(), "{language:?}");
    }
}

#[test]
fn long_poll_rejects_members_that_do_not_resolve() {
    for (directive, reason) in [
        (
            "@nexus.long-poll result-field=\"entries\"",
            "missing required `wait-field`",
        ),
        (
            "@nexus.long-poll wait-field=\"wait\"",
            "missing required `result-field`",
        ),
        (
            "@nexus.long-poll wait-field=\"deadline\" result-field=\"entries\"",
            "`wait-field` names `deadline`, which the input record does not declare",
        ),
        (
            "@nexus.long-poll wait-field=\"wait\" result-field=\"records\"",
            "`result-field` names `records`, which the output record does not declare",
        ),
        (
            "@nexus.long-poll wait-field=\"wait\" result-field=\"entries\" until=\"done\"",
            "unknown member `until`",
        ),
        (
            "@nexus.long-poll \"read\" wait-field=\"wait\" result-field=\"entries\"",
            "takes only named members",
        ),
    ] {
        let error = generate_error(Language::Python, &with_long_poll(directive));
        assert!(
            error.contains("@nexus.long-poll") && error.contains(reason),
            "{directive}: {error}"
        );
    }
}

#[test]
fn long_poll_rejects_members_of_the_wrong_shape() {
    for (directive, reason) in [
        (
            "@nexus.long-poll wait-field=\"log-id\" result-field=\"entries\"",
            "`wait-field` must name an optional field",
        ),
        (
            "@nexus.long-poll wait-field=\"from-offset\" result-field=\"next-offset\"",
            "`result-field` must name a list field",
        ),
        (
            "@nexus.long-poll wait-field=\"wait\" result-field=\"entries\" \
             done-field=\"next-offset\"",
            "`done-field` must name a boolean field",
        ),
        (
            "@nexus.long-poll wait-field=\"wait\" result-field=\"entries\" max-wait-ms=\"0\"",
            "`max-wait-ms` must be a positive integer",
        ),
        (
            "@nexus.long-poll wait-field=\"wait\" result-field=\"entries\" \
             offset-field=\"from-offset\"",
            "`offset-field` and `next-offset-field` go together",
        ),
    ] {
        let error = generate_error(Language::Python, &with_long_poll(directive));
        assert!(
            error.contains("@nexus.long-poll") && error.contains(reason),
            "{directive}: {error}"
        );
    }
}

#[test]
fn long_poll_offsets_must_name_one_offset_type() {
    let wit = LOG_WIT.replace(
        "    /// @nexus.offset \"log-offset\"\n    next-offset: option<s64>,",
        "    next-offset: option<s64>,",
    );
    let error = generate_error(Language::Python, &wit);
    assert!(
        error.contains("must name fields marked with the same `@nexus.offset` type"),
        "{error}"
    );
}

#[test]
fn long_poll_rejects_an_operation_whose_output_is_not_a_record() {
    let wit = LOG_WIT.replace(
        "  read: func(request: read-request) -> read-response;",
        "  read: func(request: read-request);",
    );
    let error = generate_error(Language::Python, &wit);
    assert!(
        error.contains("the operation output must be a record"),
        "{error}"
    );
}

fn client_config() -> NexgenConfig {
    NexgenConfig {
        mode: GenerationMode::NativeApi,
        client: true,
        ..Default::default()
    }
}

fn generate_client(wit: &str) -> String {
    let files = try_generate_with(client_config(), Language::Python, wit, Vec::new()).unwrap();
    file(&files, "client.py").to_string()
}

#[test]
fn python_client_calls_each_operation_over_a_transport() {
    let client = generate_client(LOG_WIT);

    assert!(
        client.contains("class Transport(typing.Protocol):"),
        "{client}"
    );
    assert!(
        client.contains(
            "    async def call(\n        self,\n        *,\n        service: str,\n        \
             operation: str,\n        input: temporalio.api.common.v1.Payload,\n    ) -> \
             temporalio.api.common.v1.Payload:"
        ),
        "{client}"
    );
    assert!(
        client.contains("class TransportError(Exception):"),
        "{client}"
    );
    assert!(client.contains("class LogServiceClient:"), "{client}");
    assert!(
        client.contains("            service=\"LogService\",\n            operation=operation,"),
        "{client}"
    );
    // A model that is not proto-backed is one user value, so the data converter
    // converts and encodes it whole.
    assert!(
        client.contains(
            "    async def append(self, request: AppendRequest) -> AppendResponse:\n        \
             [payload] = await self._data_converter.encode([request])\n        \
             answer = await self._call(\"Append\", payload)\n        \
             [value] = await self._data_converter.decode([answer], [AppendResponse])\n"
        ),
        "{client}"
    );
    assert!(
        client.contains("    async def read(self, request: ReadRequest) -> ReadResponse:"),
        "{client}"
    );
    assert!(
        client.contains(
            "        data_converter: temporalio.converter.DataConverter | None = None,\n"
        ),
        "{client}"
    );
}

#[test]
fn python_client_converts_proto_backed_models_through_their_converters() {
    let root = project_root();
    let files = try_generate_with(
        client_config(),
        Language::Python,
        &fs::read_to_string(root.join("advanced/samples/inputs/start-workflow.wit")).unwrap(),
        Vec::new(),
    )
    .unwrap();
    let client = file(&files, "client.py");

    // A proto-backed model travels as is. The user payloads inside it go through the
    // data converter and its codec.
    assert!(
        client.contains(
            "        with self._converters():\n            \
             message = _CancelWorkflowRequestTransferTypeConverter().to_transfer_type(request)\n        \
             await _encode_payloads(message, self._data_converter.payload_codec)\n        \
             payload = _proto_payload(message)\n"
        ),
        "{client}"
    );
    assert!(
        client.contains(
            "        converter = _CancelWorkflowResponseTransferTypeConverter()\n        \
             message = _proto_message(answer, converter.transfer_type)\n        \
             await _decode_payloads(message, self._data_converter.payload_codec)\n        \
             with self._converters():\n            \
             return converter.from_transfer_type(message, CancelWorkflowResponse)\n"
        ),
        "{client}"
    );
    assert!(
        client.contains("    _CancelWorkflowRequestTransferTypeConverter,\n"),
        "{client}"
    );
}

#[test]
fn python_client_is_generated_only_with_the_client_flag() {
    let files = generate(Language::Python, GenerationMode::NativeApi, LOG_WIT);
    assert!(!files.contains_key("client.py"), "{:?}", files.keys());
}

#[test]
fn python_client_rejects_an_operation_that_returns_a_resource_without_a_wire_type() {
    let root = project_root();
    let error = try_generate_with(
        client_config(),
        Language::Python,
        &fs::read_to_string(root.join("advanced/samples/inputs/user-service.wit")).unwrap(),
        Vec::new(),
    )
    .unwrap_err();
    assert!(
        error.contains("cannot generate a caller for operation `UserService.GetUser`")
            && error.contains("it returns a resource"),
        "{error}"
    );
}

#[test]
fn client_flag_generates_nothing_more_in_typescript_and_dotnet() {
    for language in [Language::TypeScript, Language::Dotnet] {
        let with_client =
            try_generate_with(client_config(), language, LOG_WIT, Vec::new()).unwrap();
        let without_client = generate(language, GenerationMode::NativeApi, LOG_WIT);
        assert_eq!(
            with_client.keys().collect::<Vec<_>>(),
            without_client.keys().collect::<Vec<_>>(),
            "{language:?}"
        );
    }
}

#[test]
fn python_client_long_polls_an_operation_that_declares_it() {
    let client = generate_client(LOG_WIT);

    for expected in [
        "    async def read_until_entries(\n        self,\n        request: ReadRequest,\n        \
         *,\n        deadline: datetime.timedelta,\n    ) -> ReadResponse:\n",
        "            wait = max(end - clock.time(), 0.001)\n            wait = min(wait, 60.0)\n",
        "            attempt = dataclasses.replace(attempt, wait=datetime.timedelta(seconds=wait))\n",
        "                idle_left = max(idle_timeout - (clock.time() - started), 0.001)\n",
        "                    attempt, idle_timeout=datetime.timedelta(seconds=idle_left)\n",
        "                answer = await self.read(attempt)\n",
        "            except TransportError as error:\n                \
         if not error.retryable or clock.time() + pause >= end:\n",
        "            if answer.entries or answer.done:\n                return answer\n",
        "            if answer.next_offset is not None:\n                \
         attempt = dataclasses.replace(attempt, from_offset=answer.next_offset)\n",
        "            if left <= 0:\n                return answer\n",
        "_LONG_POLL_FIRST_PAUSE = 0.05\n_LONG_POLL_LAST_PAUSE = 2.0\n",
        "\nimport asyncio\nimport collections.abc\nimport contextlib\nimport dataclasses\nimport datetime\nimport typing\n",
    ] {
        assert!(
            client.contains(expected),
            "missing {expected:?} in\n{client}"
        );
    }
    assert!(!client.contains("append_until"), "{client}");
}

#[test]
fn python_client_long_poll_sends_milliseconds_for_an_integer_wait() {
    let wit = LOG_WIT
        .replace("    wait: option<duration>,", "    wait: option<u32>,")
        .replace(
            "    idle-timeout: option<duration>,",
            "    idle-timeout: option<u64>,",
        )
        .replace("  ///   max-wait-ms=\"60000\"\n", "");
    let client = generate_client(&wit);

    assert!(
        client.contains(
            "            attempt = dataclasses.replace(attempt, wait=max(int(wait * 1000), 1))\n"
        ),
        "{client}"
    );
    assert!(
        client.contains("            request.idle_timeout / 1000 if request.idle_timeout is not None else None\n"),
        "{client}"
    );
    assert!(!client.contains("wait = min(wait,"), "{client}");
}

#[test]
fn python_client_rejects_a_long_poll_method_that_another_method_names() {
    let wit = LOG_WIT.replace(
        "  append: func(request: append-request) -> append-response;",
        "  read-until-entries: func(request: append-request) -> append-response;",
    );
    let error = try_generate_with(client_config(), Language::Python, &wit, Vec::new()).unwrap_err();
    assert!(
        error.contains("its caller method `read_until_entries` is already declared"),
        "{error}"
    );
}

/// A handle that carries a proto-backed record, with a method whose parameters
/// mix defaults, a list, a proto enum and a required value.
const RUN_WIT: &str = r#"package acme:runs@1.0.0;

world runs {
  export run-service;
}

/// @nexus.endpoint "runs"
interface run-service {
  use nexus:temporal-types/model@1.0.0.{placeholder};

  /// @nexus.proto "temporal.api.common.v1.WorkflowExecution"
  record workflow-execution {
    workflow-id: string,
    run-id: option<string>,
  }

  /// @nexus.proto "temporal.api.enums.v1.ResetReapplyExcludeType"
  enum reset-reapply-exclude-type {
    reset-reapply-exclude-type-unspecified,
    reset-reapply-exclude-type-signal,
    reset-reapply-exclude-type-update,
    reset-reapply-exclude-type-nexus,
    reset-reapply-exclude-type-cancel-request,
  }

  /// @nexus.proto "temporal.api.enums.v1.ResetReapplyType"
  enum reset-reapply-type {
    reset-reapply-type-unspecified,
    reset-reapply-type-signal,
    reset-reapply-type-none,
    reset-reapply-type-all-eligible,
  }

  // The handle carries the execution, so its methods take the rest of each request.
  resource workflow-run {
    constructor(workflow-execution: workflow-execution);

    /// @nexus.operation "reset-workflow"
    reset: func(
      reason: option<string>,
      reset-reapply-type: option<reset-reapply-type>,
      reset-reapply-exclude-types: list<reset-reapply-exclude-type>,
      workflow-task-finish-event-id: s64,
    );
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.ResetWorkflowExecutionRequest"
  record reset-workflow-request {
    workflow-execution: option<workflow-execution>,
    reason: option<string>,
    workflow-task-finish-event-id: option<s64>,
    reset-reapply-type: option<reset-reapply-type>,
    reset-reapply-exclude-types: list<reset-reapply-exclude-type>,
    /// @nexus.omit
    namespace: placeholder,
    /// @nexus.omit
    request-id: placeholder,
    /// @nexus.omit
    post-reset-operations: placeholder,
    /// @nexus.omit
    identity: placeholder,
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.ResetWorkflowExecutionResponse"
  record reset-workflow-response {
    run-id: option<string>,
  }

  reset-workflow: func(request: reset-workflow-request) -> reset-workflow-response;
}
"#;

#[test]
fn python_resource_method_takes_keywords_when_a_required_parameter_follows_a_default() {
    let files = generate(Language::Python, GenerationMode::NativeApi, RUN_WIT);
    let run = file(&files, "_resources/workflow_run.py");

    assert!(
        run.contains("class WorkflowRun:\n    workflow_execution: WorkflowExecution\n"),
        "{run}"
    );
    assert!(
        run.contains(
            "    async def reset(\n        self,\n        *,\n        reason: str | None = None,\n"
        ),
        "{run}"
    );
    assert!(
        run.contains("        workflow_task_finish_event_id: int,\n    ) -> None:\n"),
        "{run}"
    );
    assert!(
        run.contains("            workflow_execution=self.workflow_execution,\n"),
        "{run}"
    );
    assert!(run.contains("        await handle\n"), "{run}");
    // The enum annotations name the proto module, so the module imports it.
    assert!(
        run.contains("\nimport temporalio.api.enums.v1.reset_pb2\n"),
        "{run}"
    );
}

#[test]
fn python_checks_a_required_resource_field_that_the_response_fills_in() {
    let root = project_root();
    let sample =
        fs::read_to_string(root.join("advanced/samples/inputs/start-workflow.wit")).unwrap();
    let wit = sample.replace(
        "constructor(namespace: string, workflow-id: string, run-id: option<string>);",
        "constructor(namespace: string, workflow-id: string, run-id: string);",
    );
    assert_ne!(wit, sample);
    let files = generate(Language::Python, GenerationMode::NativeApi, &wit);
    let start = file(&files, "operations/start_workflow.py");

    assert!(
        start.contains(
            "    result = await handle\n    if result.run_id is None:\n        \
             raise RuntimeError(\"StartWorkflow answered without `run_id`\")\n    \
             return StartedWorkflow(\n"
        ),
        "{start}"
    );
    assert!(start.contains("        run_id=result.run_id,\n"), "{start}");
}

fn proto_field(
    name: &str,
    number: i32,
    kind: Type,
    type_name: Option<&str>,
) -> FieldDescriptorProto {
    FieldDescriptorProto {
        name: Some(name.to_string()),
        number: Some(number),
        label: Some(Label::Optional as i32),
        r#type: Some(kind as i32),
        type_name: type_name.map(str::to_string),
        ..Default::default()
    }
}

fn proto_message(name: &str, fields: Vec<FieldDescriptorProto>) -> DescriptorProto {
    DescriptorProto {
        name: Some(name.to_string()),
        field: fields,
        ..Default::default()
    }
}

/// The protos of a log whose read is a long poll over offsets, as the Stream service's is.
fn log_descriptors() -> FileDescriptorSet {
    let mut entries = proto_field("entries", 1, Type::Message, Some(".acme.logs.v1.LogEntry"));
    entries.label = Some(Label::Repeated as i32);
    FileDescriptorSet {
        file: vec![FileDescriptorProto {
            name: Some("acme/logs/v1/logs.proto".to_string()),
            package: Some("acme.logs.v1".to_string()),
            syntax: Some("proto3".to_string()),
            message_type: vec![
                proto_message("LogRef", vec![proto_field("id", 1, Type::String, None)]),
                proto_message(
                    "LogEntry",
                    vec![
                        proto_field("offset", 1, Type::Int64, None),
                        proto_field("text", 2, Type::String, None),
                    ],
                ),
                proto_message(
                    "ReadLogRequest",
                    vec![
                        proto_field("log", 1, Type::Message, Some(".acme.logs.v1.LogRef")),
                        proto_field("from_offset", 2, Type::Int64, None),
                        proto_field("wait_ms", 3, Type::Int64, None),
                    ],
                ),
                proto_message(
                    "CreateLogRequest",
                    vec![proto_field("name", 1, Type::String, None)],
                ),
                proto_message(
                    "CreateLogResponse",
                    vec![
                        proto_field("log", 1, Type::Message, Some(".acme.logs.v1.LogRef")),
                        proto_field("created", 2, Type::Bool, None),
                    ],
                ),
                proto_message(
                    "ReadLogResponse",
                    vec![
                        entries,
                        proto_field("next_offset", 2, Type::Int64, None),
                        proto_field("done", 3, Type::Bool, None),
                    ],
                ),
            ],
            ..Default::default()
        }],
    }
}

/// A log handle whose read is a long poll that moves an offset.
const LOG_HANDLE_WIT: &str = r#"package acme:logs@1.0.0;

world logs {
  export log-service;
}

/// @nexus.endpoint "logs"
interface log-service {
  /// @nexus.proto "acme.logs.v1.LogRef"
  record log-ref {
    id: string,
  }

  /// @nexus.proto "acme.logs.v1.LogEntry"
  record log-entry {
    /// @nexus.offset "log-offset"
    offset: option<s64>,
    text: option<string>,
  }

  /// @nexus.proto "acme.logs.v1.ReadLogRequest"
  record read-log-request {
    log: option<log-ref>,
    /// @nexus.offset "log-offset"
    from-offset: option<s64>,
    wait-ms: option<s64>,
  }

  /// @nexus.proto "acme.logs.v1.ReadLogResponse"
  record read-log-response {
    entries: list<log-entry>,
    /// @nexus.offset "log-offset"
    next-offset: option<s64>,
    done: option<bool>,
  }

  /// @nexus.doc "A handle on one log."
  resource log {
    constructor(log: log-ref);

    /// @nexus.doc "Read one page of the log."
    /// @nexus.operation "read-log"
    read: func(from-offset: option<s64>) -> read-log-response;
  }

  /// @nexus.proto "acme.logs.v1.CreateLogRequest"
  record create-log-request {
    name: string,
  }

  /// @nexus.proto "acme.logs.v1.CreateLogResponse"
  record create-log-result {
    log: own<log>,
    created: option<bool>,
  }

  create-log: func(request: create-log-request) -> create-log-result;

  /// @nexus.long-poll
  ///   wait-field="wait-ms"
  ///   max-wait-ms="60000"
  ///   result-field="entries"
  ///   done-field="done"
  ///   offset-field="from-offset"
  ///   next-offset-field="next-offset"
  read-log: func(request: read-log-request) -> read-log-response;
}
"#;

fn generate_log_handle(config: NexgenConfig, wit: &str) -> BTreeMap<String, String> {
    try_generate_with_descriptors(
        config,
        Language::Python,
        wit,
        Vec::new(),
        Some(&log_descriptors()),
    )
    .unwrap()
}

#[test]
fn python_resource_reads_a_long_poll_that_moves_an_offset_as_one_sequence() {
    let files = generate_log_handle(client_config(), LOG_HANDLE_WIT);
    let log = file(&files, "_resources/log.py");

    for expected in [
        "    async def iter_read(\n        self,\n        from_offset: int | None = None,\n        \
         *,\n        deadline: datetime.timedelta | None = None,\n    ) -> \
         collections.abc.AsyncIterator[LogEntry]:\n",
        "        request = ReadLogRequest(\n            log=self.log,\n",
        "                request, wait_ms=max(int(page_wait * 1000), 1)\n",
        "            handle = await _read_log(request)\n            answer = await handle\n",
        "            for item in answer.entries or []:\n                yield item\n",
        "            if answer.done:\n                return\n",
        "                request = dataclasses.replace(request, from_offset=answer.next_offset)\n",
        "\nimport asyncio\n",
        "_LONG_POLL_FIRST_PAUSE = 0.05\n_LONG_POLL_LAST_PAUSE = 2.0\n",
    ] {
        assert!(log.contains(expected), "missing {expected:?} in\n{log}");
    }
}

#[test]
fn python_client_reads_a_long_poll_that_moves_an_offset_as_one_sequence() {
    let client = generate_client(LOG_WIT);

    for expected in [
        "    async def iter_read(\n        self,\n        request: ReadRequest,\n        *,\n        \
         deadline: datetime.timedelta | None = None,\n    ) -> \
         collections.abc.AsyncIterator[Entry]:\n",
        "            answer = await self.read_until_entries(\n                \
         attempt, deadline=datetime.timedelta(seconds=left)\n            )\n",
        "            for item in answer.entries or []:\n                yield item\n",
        "            if answer.done:\n                return\n",
        "                attempt = dataclasses.replace(attempt, from_offset=answer.next_offset)\n",
        "    Entry,\n",
    ] {
        assert!(
            client.contains(expected),
            "missing {expected:?} in\n{client}"
        );
    }
}

#[test]
fn python_operation_returns_a_record_that_holds_the_resource() {
    let files = generate_log_handle(client_config(), LOG_HANDLE_WIT);
    let create = file(&files, "operations/create_log.py");
    let models = file(&files, "models.py");

    assert!(
        create.contains(
            "    result = await handle\n    if result.log is None:\n        \
             raise RuntimeError(\"CreateLog answered without `log`\")\n    \
             return CreateLogResult(\n        log=Log(\n            log=result.log,\n        ),\n        \
             created=result.created,\n    )\n"
        ),
        "{create}"
    );
    assert!(
        create.contains("        output_type=CreateLogResponse,\n"),
        "{create}"
    );
    // The holder is the API model, and its message travels as a wire record of the
    // message's name.
    assert!(
        models.contains("class CreateLogResult:\n    log: Log\n    created: bool | None = None\n"),
        "{models}"
    );
    assert!(models.contains("class CreateLogResponse:\n"), "{models}");
    assert!(
        !models.contains("_CreateLogResultTransferTypeConverter"),
        "{models}"
    );
    assert!(
        models.contains("if typing.TYPE_CHECKING:\n    from ._resources import Log\n"),
        "{models}"
    );
}

#[test]
fn python_resource_and_its_methods_carry_their_docs() {
    let files = generate_log_handle(client_config(), LOG_HANDLE_WIT);
    let log = file(&files, "_resources/log.py");

    assert!(
        log.contains("class Log:\n    \"\"\"A handle on one log.\"\"\"\n    log: LogRef\n"),
        "{log}"
    );
    assert!(
        log.contains(") -> ReadLogResponse:\n        \"\"\"Read one page of the log.\"\"\"\n"),
        "{log}"
    );
}

#[test]
fn python_client_resource_calls_the_caller_with_the_resource_filled_in() {
    let files = generate_log_handle(client_config(), LOG_HANDLE_WIT);
    let client = file(&files, "client.py");

    for expected in [
        "class Log:\n    \"\"\"A handle on one log.\"\"\"\n\n    def __init__(\n        self,\n        \
         client: LogServiceClient,\n        log: LogRef,\n    ) -> None:\n",
        "        self._client = client\n        self.log = log\n",
        "        \"\"\"Read one page of the log.\"\"\"\n        request = ReadLogRequest(\n            \
         log=self.log,\n",
        "        return await self._client.read_log(request)\n",
        "        async for item in self._client.iter_read_log(request, deadline=deadline):\n            \
         yield item\n",
        "    LogRef,\n",
    ] {
        assert!(
            client.contains(expected),
            "missing {expected:?} in\n{client}"
        );
    }
}

#[test]
fn python_client_resource_and_caller_give_back_client_resources() {
    let root = project_root();
    let files = try_generate_with(
        client_config(),
        Language::Python,
        &fs::read_to_string(root.join("advanced/samples/inputs/start-workflow.wit")).unwrap(),
        Vec::new(),
    )
    .unwrap();
    let client = file(&files, "client.py");

    assert!(client.contains("class StartedWorkflow:\n"), "{client}");
    assert!(client.contains("    async def cancel(\n"), "{client}");
    assert!(
        client.contains("        await self._client.cancel_workflow(request)\n"),
        "{client}"
    );
    // An operation that returns a resource gives back the client class, bound to the
    // caller, so a method that returns one is on the client class too.
    assert!(
        client.contains(
            "    async def start_workflow(self, request: StartWorkflowRequest) -> StartedWorkflow:\n"
        ),
        "{client}"
    );
    assert!(
        client.contains(
            "        return StartedWorkflow(\n            self,\n            namespace=request.namespace,\n"
        ),
        "{client}"
    );
    assert!(
        client.contains("        return await self._client.restart_workflow(request)\n"),
        "{client}"
    );
}

#[test]
fn python_client_only_service_gets_a_caller_and_no_workflow_surface() {
    let wit = LOG_HANDLE_WIT.replace(
        "/// @nexus.endpoint \"logs\"\ninterface log-service {",
        "/// @nexus.client-only\ninterface log-service {",
    );
    assert_ne!(wit, LOG_HANDLE_WIT);
    let files = generate_log_handle(client_config(), &wit);

    let workflow_files = files
        .keys()
        .filter(|name| {
            name.starts_with("operations/")
                || name.starts_with("_resources/")
                || name.as_str() == "services.py"
        })
        .collect::<Vec<_>>();
    assert!(workflow_files.is_empty(), "{workflow_files:?}");
    let client = file(&files, "client.py");
    assert!(client.contains("class LogServiceClient:\n"), "{client}");
    assert!(client.contains("class Log:\n"), "{client}");
    assert!(
        client.contains(
            "        async for item in self._client.iter_read_log(request, deadline=deadline):\n"
        ),
        "{client}"
    );
}
