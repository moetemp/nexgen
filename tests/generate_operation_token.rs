//! `x-nexus-operation-token`: the token helpers Python emits on a marked model,
//! and the rules the parser holds the model to.
//!
//! The toolchain test checks the generated helpers against a token the Python
//! SDK writes today, so a Core encoder tested against the same vector agrees
//! with them.

mod toolchain;

use std::fs;
use std::path::Path;

use nexgen::generator::GenerationMode;
use nexgen::language::Language;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};
use toolchain::{Target, Workspace, python_runtime_command, run};

const CONTRACT: &str = r##"
nexusrpc: "1.0.0"
services:
  ChatService:
    fqn: example.chat.v1.ChatService
    operations:
      chat:
        fqn: chat
        input: { $ref: "#/$defs/ChatInput" }
        output: { $ref: "#/$defs/ChatInput" }
$defs:
  ChatInput:
    type: object
    properties:
      prompt: { type: string }
    required: [prompt]
    additionalProperties: false
  StreamRef:
    type: object
    properties:
      kind: { type: string }
      workflow_id: { type: string }
      run_id: { type: string }
      topic: { type: string }
    required: [kind, workflow_id, run_id, topic]
    additionalProperties: false
  StreamOperationToken:
    type: object
    x-nexus-operation-token: { version-field: v }
    properties:
      v: { type: integer, const: 2 }
      ref: { $ref: "#/$defs/StreamRef" }
      attach: { type: string }
      chain: { type: string }
    required: [v, ref, attach, chain]
    additionalProperties: false
"##;

/// The token the Python SDK's stream operation writes for the driver's value.
const SDK_TOKEN: &str = "eyJhdHRhY2giOiJhdHRhY2gtcmVxdWVzdCIsImNoYWluIjoicnVuLWlkIiwicmVmIjp7ImtpbmQiOiJ3b3JrZmxvdyIsInJ1bl9pZCI6IiIsInRvcGljIjoidG9rZW5zIiwid29ya2Zsb3dfaWQiOiJjaGF0LTEifSwidiI6Mn0";

fn generate_into(output_path: &Path, language: Language, contract: &str) -> Result<(), String> {
    let contract_path = output_path.with_extension("nexusrpc.yaml");
    fs::write(&contract_path, contract).expect("write the contract");
    generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            ..Default::default()
        },
        language,
        input_paths: vec![contract_path],
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: output_path.to_path_buf(),
        format: false,
        java_package_name: (language == Language::Java).then(|| "tokens".to_string()),
        ts_date_time_types: Default::default(),
    })
    .map_err(|error| error.to_string())
}

fn python_models(contract: &str, tag: &str) -> String {
    let workspace = Workspace::new(tag);
    let output_path = workspace.root().join("tokens");
    generate_into(&output_path, Language::Python, contract)
        .unwrap_or_else(|error| panic!("Python should generate: {error}"));
    fs::read_to_string(output_path.join("models.py")).expect("read models.py")
}

fn refusal(contract: &str, tag: &str) -> String {
    let workspace = Workspace::new(tag);
    generate_into(&workspace.root().join("tokens"), Language::Python, contract)
        .expect_err("the contract should be refused")
}

#[test]
fn a_marked_model_writes_and_reads_its_token() {
    let models = python_models(CONTRACT, "token-python");
    assert!(
        models.contains("    def to_token(self) -> str:\n"),
        "{models}"
    );
    assert!(
        models.contains(
            "    @classmethod\n    def from_token(cls, token: str) -> \"StreamOperationToken\":\n"
        ),
        "{models}"
    );
    assert!(
        models.contains("json.dumps(transfer, separators=(\",\", \":\"), sort_keys=True)"),
        "{models}"
    );
    assert!(models.contains("import base64\n"), "{models}");
    assert!(models.contains("import json\n"), "{models}");
    // Only the marked model gains the helpers.
    assert_eq!(models.matches("def to_token(").count(), 1, "{models}");
}

#[test]
fn a_contract_without_a_token_imports_neither_module() {
    let contract = CONTRACT.replace("    x-nexus-operation-token: { version-field: v }\n", "");
    let models = python_models(&contract, "token-none");
    assert!(!models.contains("def to_token("), "{models}");
    assert!(!models.contains("import base64\n"), "{models}");
    assert!(!models.contains("import json\n"), "{models}");
}

#[test]
fn the_other_targets_still_emit_the_model() {
    for language in [Language::Go, Language::TypeScript, Language::Java] {
        let workspace = Workspace::new("token-other-targets");
        generate_into(&workspace.root().join("tokens"), language, CONTRACT)
            .unwrap_or_else(|error| panic!("{language:?} should generate: {error}"));
    }
}

#[test]
fn the_keyword_must_name_the_version_member() {
    let cases = [
        (
            "x-nexus-operation-token: true",
            "must be an object naming the version member",
        ),
        ("x-nexus-operation-token: {}", "requires `version-field`"),
        (
            "x-nexus-operation-token: { version-field: v, prefix: tok }",
            "has unknown member `prefix`",
        ),
        (
            "x-nexus-operation-token: { version-field: version }",
            "names version member `version`, which the model does not declare",
        ),
    ];
    for (keyword, expected) in cases {
        let contract = CONTRACT.replace("x-nexus-operation-token: { version-field: v }", keyword);
        let error = refusal(&contract, "token-keyword");
        assert!(error.contains(expected), "{keyword}: {error}");
    }
}

#[test]
fn the_version_member_must_be_a_required_pinned_integer() {
    let pinned = "required `type: integer` with an integer `const`";
    let cases = [
        ("v: { type: integer, const: 2 }", "v: { type: integer }"),
        (
            "v: { type: integer, const: 2 }",
            "v: { type: string, const: \"2\" }",
        ),
        (
            "required: [v, ref, attach, chain]",
            "required: [ref, attach, chain]",
        ),
    ];
    for (from, to) in cases {
        let error = refusal(&CONTRACT.replace(from, to), "token-version");
        assert!(error.contains(pinned), "{to}: {error}");
    }
}

#[test]
fn the_token_model_must_be_closed() {
    let contract = CONTRACT.replace(
        "    required: [v, ref, attach, chain]\n    additionalProperties: false\n",
        "    required: [v, ref, attach, chain]\n",
    );
    let error = refusal(&contract, "token-open");
    assert!(
        error.contains("requires `additionalProperties: false`"),
        "{error}"
    );
}

const PYTHON_DRIVER: &str = r#"
import importlib
import pathlib
import sys

package = pathlib.Path(sys.argv[1])
expected = sys.argv[2]
sys.path.insert(0, str(package.parent))
models = importlib.import_module(f"{package.name}.models")

token = models.StreamOperationToken(
    ref=models.StreamRef(kind="workflow", workflow_id="chat-1", run_id="", topic="tokens"),
    attach="attach-request",
    chain="run-id",
)
assert token.to_token() == expected, token.to_token()
assert models.StreamOperationToken.from_token(expected) == token

# Another version, a token of something else, and text that isn't one.
import base64
import json


def encoded(value):
    text = json.dumps(value, separators=(",", ":"), sort_keys=True)
    return base64.urlsafe_b64encode(text.encode()).decode().rstrip("=")


other_version = json.loads(base64.urlsafe_b64decode(expected + "=="))
other_version["v"] = 3
for bad in (encoded(other_version), encoded({"prompt": "hi"}), "not a token!", ""):
    try:
        models.StreamOperationToken.from_token(bad)
    except ValueError:
        continue
    raise AssertionError(f"{bad!r} was read as a token")
print("ok")
"#;

#[test]
fn the_generated_token_matches_the_sdk_and_refuses_other_versions() {
    let workspace = Workspace::new("token-python-toolchain");
    let package = workspace.package_path(Target::Python, "tokens");
    fs::create_dir_all(package.parent().expect("a package parent")).expect("create the parent");
    generate_into(&package, Language::Python, CONTRACT).expect("generate the models");
    let driver = workspace.root().join("drive_token.py");
    fs::write(&driver, PYTHON_DRIVER).expect("write the driver");
    let mut python = python_runtime_command().expect("a Python with the SDK");
    let output = run(python.arg(&driver).arg(&package).arg(SDK_TOKEN)).expect_ok("drive the token");
    assert!(output.contains("ok"), "{output}");
}
