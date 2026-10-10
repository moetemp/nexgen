//! The HTTP callers `--client` emits for Go and Python, and the loops they
//! carry for `x-nexus-long-poll`.
//!
//! The committed `samples/schemas/streams.nexusrpc.yaml` advanced output is the
//! golden for the whole shape; these assert the individual decisions,
//! including the shapes that sample does not carry (a void operation, a wire
//! name that is not a URL-safe path segment, a required wait hint, a target
//! with no caller).

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

/// A long-pollable read beside an operation with no contract, and an optional
/// wait hint, which a target holds behind an optional.
const LONG_POLL_CONTRACT: &str = r##"
nexusrpc: "1.0.0"
services:
  StreamService:
    fqn: example.streams.v1.StreamService
    operations:
      append:
        fqn: append
        input: { $ref: "#/$defs/ReadInput" }
        output: { $ref: "#/$defs/ReadOutput" }
      read:
        fqn: read
        x-nexus-long-poll:
          wait-field: waitMs
          result-field: records
        input: { $ref: "#/$defs/ReadInput" }
        output: { $ref: "#/$defs/ReadOutput" }
$defs:
  ReadInput:
    type: object
    properties:
      stream: { type: string }
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

/// Generates the contract and answers with the emitted files by name.
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

#[test]
fn python_caller_loops_the_long_poll_operation_and_keeps_the_single_shot() {
    let (temp_dir, files) = generate_contract(
        Language::Python,
        LONG_POLL_CONTRACT,
        true,
        "http-caller-python-poll",
    );
    let client = file(&files, "client.py");
    assert!(client.contains("async def read_until_records("), "{client}");
    assert!(client.contains("deadline: float,"), "{client}");
    assert!(client.contains("wait_ms = int(wait * 1000)"), "{client}");
    assert!(client.contains("request, wait_ms=wait_ms"), "{client}");
    assert!(client.contains("if answer.records:"), "{client}");
    // The single-shot call is what the loop drives, and stays callable.
    assert!(
        client.contains("answer = await self.read(attempt)"),
        "{client}"
    );
    // The ask is clamped to the transport, and an empty answer that came back
    // faster than the wait it asked for is paced.
    assert!(
        client.contains("wait = self._poll_budget(end - time.monotonic())"),
        "{client}"
    );
    assert!(
        client.contains("min(remaining, self._timeout * 0.9)"),
        "{client}"
    );
    assert!(
        client.contains("if time.monotonic() - started < wait / 2:"),
        "{client}"
    );
    assert!(client.contains("if not error.retryable"), "{client}");
    // `append` declares no long-poll contract, so it gets no loop.
    assert!(!client.contains("append_until"), "{client}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_caller_loops_the_long_poll_operation_through_a_pointer_wait_hint() {
    let (temp_dir, files) = generate_contract(
        Language::Go,
        LONG_POLL_CONTRACT,
        true,
        "http-caller-go-poll",
    );
    let client = file(&files, "client.go");
    assert!(
        client.contains(
            "func (c *StreamServiceHTTPClient) ReadUntilRecords(ctx context.Context, request ReadInput, deadline time.Duration) (ReadOutput, error)"
        ),
        "{client}"
    );
    assert!(
        client.contains("attempt.WaitMs = &milliseconds"),
        "{client}"
    );
    assert!(client.contains("if len(answer.Records) > 0 {"), "{client}");
    assert!(
        client.contains("next, err := c.Read(ctx, attempt)"),
        "{client}"
    );
    assert!(
        client.contains("wait := c.pollBudget(time.Until(end))"),
        "{client}"
    );
    assert!(
        client.contains("if time.Since(started) < wait/2 {"),
        "{client}"
    );
    // A cancelled context during the pacing pause is reported, not swallowed.
    assert!(client.contains("return answer, paused"), "{client}");
    assert!(!client.contains("AppendUntil"), "{client}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_caller_reports_a_cancelled_context_during_a_retry_pause() {
    let (temp_dir, files) = generate_contract(
        Language::Go,
        LONG_POLL_CONTRACT,
        true,
        "http-caller-go-retry-cancel",
    );
    let client = file(&files, "client.go");
    // Both pauses, the one after a retryable status and the one pacing an empty
    // answer, hand back the context's error when the context ends first.
    assert_eq!(
        client
            .matches("if paused := longPollPause(ctx, backoff, end); paused != nil {\n")
            .count(),
        2,
        "{client}"
    );
    assert!(
        !client.contains("paused != nil {\n\t\t\t\treturn answer, err\n"),
        "{client}"
    );
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn a_caller_without_a_long_poll_carries_no_loop() {
    let (temp_dir, files) = generate(Language::Python, true, "http-caller-python-no-poll");
    let client = file(&files, "client.py");
    assert!(!client.contains("import time"), "{client}");
    assert!(!client.contains("_until_"), "{client}");
    fs::remove_dir_all(temp_dir).unwrap();
    let (temp_dir, files) = generate(Language::Go, true, "http-caller-go-no-poll");
    let client = file(&files, "client.go");
    assert!(!client.contains("UntilRecords"), "{client}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn a_done_flag_ends_the_loop_in_both_callers() {
    let contract = LONG_POLL_CONTRACT
        .replace(
            "          result-field: records\n",
            "          result-field: records\n          done-field: done\n",
        )
        .replace(
            "      records: { type: array, items: { type: string } }\n    additionalProperties: false\n",
            "      records: { type: array, items: { type: string } }\n      done: { type: boolean }\n    required: [done]\n    additionalProperties: false\n",
        );
    assert!(contract.contains("required: [done]"), "{contract}");
    let (temp_dir, files) = generate_contract(Language::Go, &contract, true, "http-caller-go-done");
    // A required flag is a plain bool in Go.
    assert!(
        file(&files, "client.go").contains("if len(answer.Records) > 0 || answer.Done {"),
        "{}",
        file(&files, "client.go")
    );
    fs::remove_dir_all(temp_dir).unwrap();
    let (temp_dir, files) =
        generate_contract(Language::Python, &contract, true, "http-caller-python-done");
    assert!(
        file(&files, "client.py").contains("            if answer.records or answer.done:\n"),
        "{}",
        file(&files, "client.py")
    );
    fs::remove_dir_all(temp_dir).unwrap();
}
