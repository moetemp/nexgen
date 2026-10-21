//! WIT annotations for streaming services (`@nexus.offset` on record fields,
//! and `@nexus.long-poll` and `@nexus.one-way` on operations) and the caller
//! that `--client` generates for code outside a Workflow.
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

  /// @nexus.one-way
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
        descriptor_paths: vec![
            project_root().join("advanced/samples/descriptors/temporal_api.bin"),
        ],
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

#[test]
fn python_one_way_stub_is_synchronous_and_returns_nothing() {
    let files = generate(Language::Python, GenerationMode::NativeApi, LOG_WIT);
    let append = file(&files, "operations/append.py");

    assert!(
        append.contains("\ndef _append(\n    request: AppendRequest,\n) -> None:\n"),
        "{append}"
    );
    assert!(append.contains("\ndef append(\n    *,\n"), "{append}");
    assert!(!append.contains("async def"), "{append}");
    assert!(!append.contains("NexusOperationHandle"), "{append}");
    // The call goes through the Nexus client, so the outbound interceptors see
    // it, and the stub drives the start to its end without suspending.
    assert!(
        append.contains("    start = nexus_client.start_operation(\n"),
        "{append}"
    );
    assert!(
        append
            .contains("        _ = start.send(None)\n    except StopIteration:\n        return\n"),
        "{append}"
    );
    assert!(
        append.contains("    start.close()\n    raise RuntimeError("),
        "{append}"
    );
    assert!(append.contains("    _append(request)\n"), "{append}");

    let read = file(&files, "operations/read.py");
    assert!(read.contains("async def read("), "{read}");
}

#[test]
fn one_way_generates_in_typescript_and_dotnet() {
    for language in [Language::TypeScript, Language::Dotnet] {
        let files = generate(language, GenerationMode::NativeApi, LOG_WIT);
        assert!(!files.is_empty(), "{language:?}");
    }
}

#[test]
fn one_way_rejects_arguments_and_conflicting_directives() {
    for (from, to, reason) in [
        (
            "  /// @nexus.one-way\n",
            "  /// @nexus.one-way buffered=true\n",
            "does not take arguments",
        ),
        (
            "  /// @nexus.long-poll\n",
            "  /// @nexus.one-way\n  /// @nexus.long-poll\n",
            "cannot be combined with `@nexus.long-poll`",
        ),
        (
            "  /// @nexus.one-way\n",
            "  /// @nexus.one-way\n  /// @nexus.output-transform python-type=\"int\" python=\"1\"\n",
            "cannot be combined with `@nexus.output-transform`",
        ),
    ] {
        let error = generate_error(Language::Python, &LOG_WIT.replacen(from, to, 1));
        assert!(
            error.contains("@nexus.one-way") && error.contains(reason),
            "{to}: {error}"
        );
    }
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
    // A model that is not proto-backed goes through the payload converter.
    assert!(
        client.contains(
            "    async def append(self, request: AppendRequest) -> AppendResponse:\n        \
             payload = self._payload_converter.to_payload(request)\n        \
             answer = await self._call(\"Append\", payload)\n"
        ),
        "{client}"
    );
    // A one-way operation is synchronous in a Workflow only. Outside one, the
    // caller still answers with the result.
    assert!(
        client.contains("    async def read(self, request: ReadRequest) -> ReadResponse:"),
        "{client}"
    );
    assert!(
        client.contains("[payload] = await self._payload_codec.encode([payload])"),
        "{client}"
    );
    assert!(
        client.contains("[answer] = await self._payload_codec.decode([answer])"),
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

    assert!(
        client.contains(
            "        payload = _proto_payload(_CancelWorkflowRequestTransferTypeConverter().to_transfer_type(request))\n"
        ),
        "{client}"
    );
    assert!(
        client.contains(
            "        converter = _CancelWorkflowResponseTransferTypeConverter()\n        \
             return converter.from_transfer_type(\n            \
             _proto_message(answer, converter.transfer_type),\n            CancelWorkflowResponse,\n        )\n"
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
        "\nimport asyncio\nimport dataclasses\nimport datetime\nimport typing\n",
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
        "  /// @nexus.one-way\n  append: func(request: append-request) -> append-response;",
        "  read-until-entries: func(request: append-request) -> append-response;",
    );
    let error = try_generate_with(client_config(), Language::Python, &wit, Vec::new()).unwrap_err();
    assert!(
        error.contains("its caller method `read_until_entries` is already declared"),
        "{error}"
    );
}

#[test]
fn python_client_module_carries_an_http_transport() {
    let client = generate_client(LOG_WIT);

    for expected in [
        "class HttpTransport:\n",
        "        timeout: float = 90.0,\n        ssl_context: ssl.SSLContext | None = None,\n",
        "                urllib.parse.quote(service, safe=\"\"),\n                \
         urllib.parse.quote(operation, safe=\"\"),\n",
        "        return await asyncio.to_thread(self._send, request)\n",
        "            retryable_header = error.headers.get(\"Nexus-Request-Retryable\")\n",
        "                retryable = error.code == 429 or error.code >= 500\n",
        // The payload maps to HTTP content as the Temporal server maps it.
        "        return f'application/x-protobuf; message-type=\"{message_type}\"', payload.data\n",
        "    return \"application/x-temporal-payload\", payload.SerializeToString()\n",
        "    if media_type == \"application/json\" and header.get_param(\"format\") == \"protobuf\":\n",
    ] {
        assert!(
            client.contains(expected),
            "missing {expected:?} in\n{client}"
        );
    }
}
