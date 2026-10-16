//! `x-nexus-stream-result`: an operation whose start hands back a stream, and
//! the typed reader a Workflow caller gets for it.
//!
//! The reader type is the SDK's, named by the contract per target, so these pin
//! the emitted glue; `stream_result_toolchain.rs` runs it against a stand-in
//! reader and handle.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::generator::GenerationMode;
use nexgen::language::Language;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};

/// A stream-returning `chat` beside a plain `ping`.
const CONTRACT: &str = r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    fqn: example.chat.v1.ChatService
    operations:
      chat:
        fqn: chat
        x-nexus-stream-result:
          item: { $ref: "#/$defs/Token" }
          reader: { py: example_sdk.streams.OperationStreamReader }
        input: { $ref: "#/$defs/ChatInput" }
        output: { $ref: "#/$defs/ChatSummary" }
      ping:
        fqn: ping
        input: { $ref: "#/$defs/ChatInput" }
        output: { $ref: "#/$defs/ChatSummary" }
$defs:
  ChatInput:
    type: object
    properties:
      prompt: { type: string }
    required: [prompt]
    additionalProperties: false
  ChatSummary:
    type: object
    properties:
      tokens: { type: integer }
    required: [tokens]
    additionalProperties: false
  Token:
    type: object
    properties:
      text: { type: string }
    required: [text]
    additionalProperties: false
"##;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_output_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let ordinal = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("nexgen-{label}-{nanos}-{ordinal}"))
}

fn generate(language: Language, contract: &str, label: &str) -> (PathBuf, Vec<(String, String)>) {
    let temp_dir = unique_output_path(label);
    let input_dir = temp_dir.join("input");
    fs::create_dir_all(&input_dir).unwrap();
    let input_path = input_dir.join("chat.nexusrpc.yaml");
    fs::write(&input_path, contract).unwrap();
    let output_path = temp_dir.join("chat");
    generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::NativeApi,
            ..Default::default()
        },
        language,
        input_paths: vec![input_path],
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        java_package_name: (language == Language::Java).then(|| "chat".to_string()),
        ts_date_time_types: Default::default(),
    })
    .unwrap_or_else(|error| panic!("{language:?} should generate: {error}"));
    (temp_dir, read_files(&output_path))
}

fn read_files(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push((
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    fs::read_to_string(&path).unwrap(),
                ));
            }
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
        .unwrap_or_else(|| panic!("{name} should be generated"))
}

fn all_text(files: &[(String, String)]) -> String {
    files
        .iter()
        .map(|(_, contents)| contents.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn python_emits_a_typed_reader_for_a_stream_returning_operation() {
    let (temp_dir, files) = generate(Language::Python, CONTRACT, "stream-result-python");
    let services = file(&files, "services.py");

    assert!(
        services.contains("import example_sdk.streams\n"),
        "{services}"
    );
    assert!(
        services.contains("        self._endpoint = endpoint\n"),
        "{services}"
    );
    assert!(
        services.contains(
            "    def read_chat_stream(\n        self,\n        handle: temporalio.workflow.NexusOperationHandle[ChatSummary],\n        *,\n        endpoint: str | None = None,\n    ) -> example_sdk.streams.OperationStreamReader[Token]:\n"
        ),
        "{services}"
    );
    assert!(
        services.contains(
            "        return example_sdk.streams.OperationStreamReader(\n            handle,\n            item_type=Token,\n            endpoint=self._endpoint if endpoint is None else endpoint,\n        )\n"
        ),
        "{services}"
    );
    assert!(!services.contains("read_ping_stream"), "{services}");
    assert!(
        services.contains("\"\"\"Read the stream the ``chat`` operation returns."),
        "{services}"
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn python_emits_no_reader_without_a_python_reader_type() {
    let contract = CONTRACT.replace(
        "          reader: { py: example_sdk.streams.OperationStreamReader }\n",
        "",
    );
    let (temp_dir, files) = generate(Language::Python, &contract, "stream-result-python-unmapped");
    let services = file(&files, "services.py");

    assert!(!services.contains("read_chat_stream"), "{services}");
    assert!(!services.contains("example_sdk"), "{services}");
    assert!(!services.contains("self._endpoint"), "{services}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn other_targets_accept_the_keyword_and_emit_nothing_for_it() {
    for language in [Language::Go, Language::TypeScript, Language::Java] {
        let (temp_dir, files) = generate(language, CONTRACT, "stream-result-other");
        let text = all_text(&files);
        for leak in [
            "stream-result",
            "OperationStreamReader",
            "example_sdk",
            "ChatStream",
        ] {
            assert!(!text.contains(leak), "{language:?} leaked `{leak}`");
        }
        fs::remove_dir_all(temp_dir).unwrap();
    }
}

fn python_reject(contract: &str) -> String {
    let temp_dir = unique_output_path("stream-result-python-reject");
    let input_dir = temp_dir.join("input");
    fs::create_dir_all(&input_dir).unwrap();
    let input_path = input_dir.join("chat.nexusrpc.yaml");
    fs::write(&input_path, contract).unwrap();
    let error = generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::NativeApi,
            ..Default::default()
        },
        language: Language::Python,
        input_paths: vec![input_path],
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: temp_dir.join("chat"),
        format: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .expect_err("the contract should be refused")
    .to_string();
    fs::remove_dir_all(temp_dir).unwrap();
    error
}

#[test]
fn python_refuses_a_reader_method_named_like_an_operation() {
    let contract = CONTRACT.replace(
        "      ping:\n        fqn: ping\n",
        "      ping:\n        fqn: ping\n        x-py-name: read_chat_stream\n",
    );
    let error = python_reject(&contract);
    assert!(error.contains("`read_chat_stream`"), "{error}");
}

#[test]
fn python_refuses_a_reader_package_named_like_a_service_file_name() {
    let contract = CONTRACT.replace(
        "reader: { py: example_sdk.streams.OperationStreamReader }",
        "reader: { py: Token.streams.OperationStreamReader }",
    );
    let error = python_reject(&contract);
    assert!(error.contains("`Token`"), "{error}");
}

#[test]
fn python_refuses_an_operation_named_like_a_client_member() {
    for member in ["_endpoint", "_nexus_client"] {
        let contract = CONTRACT.replace(
            "      ping:\n        fqn: ping\n",
            &format!("      ping:\n        fqn: ping\n        x-py-name: {member}\n"),
        );
        let error = python_reject(&contract);
        assert!(error.contains(&format!("`{member}`")), "{error}");
        assert!(error.contains("x-py-name"), "{error}");
    }
}

#[test]
fn python_reader_refusals_name_the_contract_file_and_the_authored_operation() {
    let method_clash = CONTRACT.replace(
        "      ping:\n        fqn: ping\n",
        "      ping:\n        fqn: ping\n        x-py-name: read_chat_stream\n",
    );
    let package_clash = CONTRACT.replace(
        "reader: { py: example_sdk.streams.OperationStreamReader }",
        "reader: { py: Token.streams.OperationStreamReader }",
    );
    for contract in [method_clash, package_clash] {
        let error = python_reject(&contract);
        assert!(error.contains("chat.nexusrpc.yaml"), "{error}");
        assert!(!error.contains("<json-schema>"), "{error}");
        assert!(error.contains("operation `chat`"), "{error}");
    }
}
