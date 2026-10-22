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

fn client_config() -> NexgenConfig {
    NexgenConfig {
        mode: GenerationMode::NativeApi,
        client: true,
        ..Default::default()
    }
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
