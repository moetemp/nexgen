//! The stream reference `x-nexus-stream-ref` marks, and the per-language type
//! a contract maps it to.
//!
//! The committed samples carry no marked model, because a mapped type has to
//! exist wherever the samples type-check; these pin the emission instead, and
//! `http_caller_toolchain.rs` runs it against a stand-in type.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::generator::GenerationMode;
use nexgen::language::Language;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};

/// An operation that hands back a stream reference and a read that takes one.
const CONTRACT: &str = r##"
nexusrpc: "1.0.0"
services:
  ScoresService:
    fqn: example.scores.v1.ScoresService
    operations:
      startGame:
        fqn: startGame
        input: { $ref: "#/$defs/GameRequest" }
        output: { $ref: "#/$defs/StreamRef" }
      read:
        fqn: read
        x-nexus-long-poll:
          wait-field: waitMs
          result-field: records
        input: { $ref: "#/$defs/ReadInput" }
        output: { $ref: "#/$defs/ReadOutput" }
$defs:
  GameRequest:
    type: object
    properties:
      gameId: { type: string }
    required: [gameId]
    additionalProperties: false
  StreamRef:
    type: object
    x-nexus-stream-ref: { py: example_sdk.streams.StreamRef }
    description: "A stream: its owner and the topic on it."
    properties:
      owner: { type: string, enum: [workflow, activity, standalone] }
      workflow_id: { oneOf: [{ type: string }, { type: "null" }] }
      run_id: { oneOf: [{ type: string }, { type: "null" }] }
      topic: { type: string }
    required: [owner, topic]
    additionalProperties: false
  ReadInput:
    type: object
    properties:
      stream: { $ref: "#/$defs/StreamRef" }
      afterToken: { type: string, x-nexus-cursor: StreamCursor }
      waitMs: { type: integer }
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

fn generate(language: Language, client: bool, label: &str) -> (PathBuf, Vec<(String, String)>) {
    generate_contract(language, CONTRACT, client, label)
}

fn generate_contract(
    language: Language,
    contract: &str,
    client: bool,
    label: &str,
) -> (PathBuf, Vec<(String, String)>) {
    let temp_dir = unique_output_path(label);
    let input_dir = temp_dir.join("input");
    fs::create_dir_all(&input_dir).unwrap();
    let input_path = input_dir.join("scores.nexusrpc.yaml");
    fs::write(&input_path, contract).unwrap();
    let output_path = temp_dir.join("scores");
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
        java_package_name: (language == Language::Java).then(|| "scores".to_string()),
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
        .unwrap_or_else(|| panic!("{name} should be generated"))
}

#[test]
fn python_emits_the_stream_reference_as_the_mapped_type() {
    let (temp_dir, files) = generate(Language::Python, false, "stream-ref-python");
    let models = file(&files, "models.py");
    // The mapped type stands where the wire dataclass would, so a value read
    // off an operation is the one the SDK opens.
    assert!(models.contains("import example_sdk.streams\n"), "{models}");
    assert!(
        models.contains("StreamRef: typing.TypeAlias = example_sdk.streams.StreamRef\n"),
        "{models}"
    );
    assert!(
        models.contains("\"\"\"A stream: its owner and the topic on it.\"\"\""),
        "{models}"
    );
    assert!(!models.contains("class StreamRef"), "{models}");
    // The converter stays and builds the SDK type from the wire members.
    assert!(
        models.contains("class _StreamRefTransferTypeConverter("),
        "{models}"
    );
    assert!(models.contains("return StreamRef(\n"), "{models}");
    assert!(
        models.contains("isinstance(runtime_value, StreamRef)"),
        "{models}"
    );
    // A member typed by the model names the alias like any other model.
    assert!(models.contains("stream: StreamRef\n"), "{models}");
    assert!(file(&files, "services.py").contains("StreamRef,"));
    // The package exports what it declares. The mapped type is the SDK's, and
    // re-exporting it gives it a second public home, which doc tools take as
    // its own and then fail to link the SDK's references to it.
    assert!(!file(&files, "__init__.py").contains("StreamRef"));
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn python_caller_hands_back_the_mapped_stream_reference() {
    let (temp_dir, files) = generate(Language::Python, true, "stream-ref-python-client");
    let client = file(&files, "client.py");
    assert!(client.contains(") -> StreamRef:"), "{client}");
    assert!(
        client.contains("_StreamRefTransferTypeConverter().from_transfer_type(answer, StreamRef)"),
        "{client}"
    );
    assert!(client.contains("async def read_until_records("), "{client}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_keeps_the_stream_reference_as_a_wire_struct() {
    // No Go stream SDK exists to swap in, so the wire model is what a Go caller
    // holds, and the keyword changes nothing there.
    let (temp_dir, files) = generate(Language::Go, false, "stream-ref-go");
    let declarations: usize = files
        .iter()
        .map(|(_, contents)| contents.matches("type StreamRef struct").count())
        .sum();
    assert_eq!(declarations, 1);
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn the_other_targets_keep_the_wire_model() {
    for language in [Language::TypeScript, Language::Java, Language::Go] {
        let (temp_dir, files) = generate(language, false, "stream-ref-other-targets");
        for (name, contents) in &files {
            assert!(
                !contents.contains("x-nexus-") && !contents.contains("example_sdk"),
                "{language:?} {name} acted on the marker"
            );
        }
        fs::remove_dir_all(temp_dir).unwrap();
    }
}

#[test]
fn a_marker_without_a_mapping_keeps_the_wire_model() {
    // No SDK type is assumed: without a mapping for the target, the marked
    // model is emitted like any other.
    let contract = CONTRACT.replace(
        "x-nexus-stream-ref: { py: example_sdk.streams.StreamRef }",
        "x-nexus-stream-ref: true",
    );
    let (temp_dir, files) = generate_contract(
        Language::Python,
        &contract,
        false,
        "stream-ref-python-unmapped",
    );
    let models = file(&files, "models.py");
    assert!(models.contains("class StreamRef"), "{models}");
    assert!(!models.contains("TypeAlias = "), "{models}");
    assert!(!models.contains("example_sdk"), "{models}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn a_mapped_package_named_like_a_model_is_refused() {
    // `import GameRequest.streams` would bind `GameRequest` over the model.
    let contract = CONTRACT.replace(
        "x-nexus-stream-ref: { py: example_sdk.streams.StreamRef }",
        "x-nexus-stream-ref: { py: GameRequest.streams.StreamRef }",
    );
    let temp_dir = unique_output_path("stream-ref-package-clash");
    let input_dir = temp_dir.join("input");
    fs::create_dir_all(&input_dir).unwrap();
    let input_path = input_dir.join("scores.nexusrpc.yaml");
    fs::write(&input_path, contract).unwrap();
    let outcome = generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            ..Default::default()
        },
        language: Language::Python,
        input_paths: vec![input_path],
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: temp_dir.join("scores"),
        format: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    });
    let message = match outcome {
        Ok(()) => panic!("the loader accepted a package that shadows a model"),
        Err(error) => error.to_string(),
    };
    assert!(
        message.contains("GameRequest") && message.contains("mapped stream reference"),
        "{message}"
    );
    fs::remove_dir_all(temp_dir).ok();
}
