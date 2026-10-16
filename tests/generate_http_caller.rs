//! The HTTP callers `--client` emits for Go and Python.
//!
//! The committed `samples/schemas/streams.nexusrpc.yaml` advanced output is the
//! golden for the whole shape; these assert the individual decisions,
//! including the shapes that sample does not carry (a void operation, a wire
//! name that is not a URL-safe path segment, a target with no caller).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::generator::GenerationMode;
use nexgen::language::Language;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};

const CONTRACT: &str = r##"
nexusrpc: "1.0.0"
services:
  StreamService:
    fqn: example.streams.v1.StreamService
    description: Append records and read them back.
    operations:
      append:
        fqn: append
        description: Append one batch of records.
        input: { $ref: "#/$defs/AppendInput" }
        output: { $ref: "#/$defs/AppendOutput" }
      read:
        fqn: "streams/read {v1}"
        input: { $ref: "#/$defs/ReadInput" }
        output: { $ref: "#/$defs/ReadOutput" }
      ping:
        fqn: ping
$defs:
  AppendInput:
    type: object
    properties:
      stream: { type: string }
    required: [stream]
    additionalProperties: false
  AppendOutput:
    type: object
    properties:
      cursor: { type: string }
    additionalProperties: false
  ReadInput:
    type: object
    properties:
      stream: { type: string }
    required: [stream]
    additionalProperties: false
  ReadOutput:
    type: object
    properties:
      records: { type: array, items: { type: string } }
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

/// Generates the contract and answers with the emitted files by name.
fn generate(language: Language, client: bool, label: &str) -> (PathBuf, Vec<(String, String)>) {
    let temp_dir = unique_output_path(label);
    let input_dir = temp_dir.join("input");
    fs::create_dir_all(&input_dir).unwrap();
    let input_path = input_dir.join("streams.nexusrpc.yaml");
    fs::write(&input_path, CONTRACT).unwrap();
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
        java_package_name: (language == Language::Java).then(|| "streams".to_string()),
        ts_date_time_types: Default::default(),
    })
    .unwrap_or_else(|error| panic!("{language:?} should generate: {error}"));
    (temp_dir, read_files(&output_path))
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

#[test]
fn python_caller_posts_to_each_operation_wire_name() {
    let (temp_dir, files) = generate(Language::Python, true, "http-caller-python-path");
    let client = file(&files, "client.py");
    assert!(
        client.contains("class StreamServiceHttpClient:"),
        "{client}"
    );
    assert!(client.contains("f\"{self._base_url}/append\""), "{client}");
    // The wire name is one percent-encoded path segment, which is also what
    // keeps it from closing or interpolating the f-string it lands in.
    assert!(
        client.contains("f\"{self._base_url}/streams%2Fread%20%7Bv1%7D\""),
        "{client}"
    );
    assert!(
        client.contains(
            "async def append(\n        self,\n        request: AppendInput,\n    ) -> AppendOutput:"
        ),
        "{client}"
    );
    // A void operation takes no request and returns nothing.
    assert!(
        client.contains("async def ping(\n        self,\n    ) -> None:"),
        "{client}"
    );
    assert!(client.contains("body = b\"{}\""), "{client}");
    // Only a synchronous result is read as the output.
    assert!(client.contains("if status not in (200, 204):"), "{client}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_caller_posts_to_each_operation_wire_name() {
    let (temp_dir, files) = generate(Language::Go, true, "http-caller-go-path");
    let client = file(&files, "client.go");
    assert!(
        client.contains("type StreamServiceHTTPClient struct"),
        "{client}"
    );
    assert!(
        client.contains(
            "func NewStreamServiceHTTPClient(baseURL string, options *StreamServiceHTTPClientOptions)"
        ),
        "{client}"
    );
    assert!(client.contains("c.post(ctx, \"append\", body)"), "{client}");
    assert!(
        client.contains("c.post(ctx, \"streams%2Fread%20%7Bv1%7D\", body)"),
        "{client}"
    );
    assert!(
        client.contains(
            "func (c *StreamServiceHTTPClient) Append(ctx context.Context, request AppendInput) (AppendOutput, error)"
        ),
        "{client}"
    );
    assert!(
        client.contains("func (c *StreamServiceHTTPClient) Ping(ctx context.Context) error"),
        "{client}"
    );
    assert!(
        client.contains(
            "if response.StatusCode != http.StatusOK && response.StatusCode != http.StatusNoContent"
        ),
        "{client}"
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn no_caller_is_emitted_without_the_flag() {
    for (language, name) in [(Language::Python, "client.py"), (Language::Go, "client.go")] {
        let (temp_dir, files) = generate(language, false, "http-caller-off");
        assert!(!has_file(&files, name), "{language:?} emitted {name}");
        fs::remove_dir_all(temp_dir).unwrap();
    }
}

#[test]
fn the_other_targets_emit_no_caller() {
    // The flag is only on the `go` and `python` subcommands, but the library
    // request carries it for every target; the others ignore it.
    for language in [Language::TypeScript, Language::Java] {
        let (temp_dir, files) = generate(language, true, "http-caller-other-targets");
        for (name, contents) in &files {
            assert!(
                !contents.contains("HttpClient") && !contents.contains("HTTPClient"),
                "{language:?} {name} carries a caller"
            );
        }
        fs::remove_dir_all(temp_dir).unwrap();
    }
}
