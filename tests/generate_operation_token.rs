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
      run_id: { oneOf: [{ type: string }, { type: "null" }] }
      topic: { type: string }
    required: [kind, workflow_id, topic]
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

/// Core's shared token vectors. See `tests/fixtures/README.md` for the source.
const VECTORS: &str = "tests/fixtures/operation_token_vectors.json";

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
import json
import pathlib
import sys

package = pathlib.Path(sys.argv[1])
vectors = json.loads(pathlib.Path(sys.argv[2]).read_text())
sys.path.insert(0, str(package.parent))
models = importlib.import_module(f"{package.name}.models")


def token_of(vector):
    ref = vector["ref"]
    return models.StreamOperationToken(
        ref=models.StreamRef(
            kind=ref["kind"],
            workflow_id=ref["workflow_id"],
            run_id=ref.get("run_id"),
            topic=ref["topic"],
        ),
        attach=vector["attach"],
        chain=vector["chain"],
    )


for vector in vectors["vectors"]:
    expected = token_of(vector)
    assert expected.to_token() == vector["token"], (expected.to_token(), vector["token"])
    assert models.StreamOperationToken.from_token(vector["token"]) == expected
for vector in vectors["decode_only"]:
    assert models.StreamOperationToken.from_token(vector["token"]) == token_of(vector)
for bad in vectors["refused"]:
    try:
        models.StreamOperationToken.from_token(bad)
    except ValueError:
        continue
    raise AssertionError(f"{bad!r} was read as a token")
print("ok", len(vectors["vectors"]), len(vectors["decode_only"]), len(vectors["refused"]))
"#;

#[test]
fn the_generated_token_reads_and_writes_cores_vectors() {
    let workspace = Workspace::new("token-python-toolchain");
    let package = workspace.package_path(Target::Python, "tokens");
    fs::create_dir_all(package.parent().expect("a package parent")).expect("create the parent");
    generate_into(&package, Language::Python, CONTRACT).expect("generate the models");
    let driver = workspace.root().join("drive_token.py");
    fs::write(&driver, PYTHON_DRIVER).expect("write the driver");
    let vectors = Path::new(env!("CARGO_MANIFEST_DIR")).join(VECTORS);
    let mut python = python_runtime_command().expect("a Python with the SDK");
    let output = run(python.arg(&driver).arg(&package).arg(&vectors)).expect_ok("drive the token");
    assert!(output.contains("ok 4 1 6"), "{output}");
}

#[test]
fn an_integer_version_reaches_its_literal_member_through_a_cast() {
    // mypy, unlike pyright, doesn't narrow the parsed `int` to the member's
    // `Literal` through the membership test.
    let models = python_models(CONTRACT, "token-version-cast");
    assert!(
        models.contains("v_value = typing.cast(\"typing.Any\", v_value_parsed)"),
        "{models}"
    );
}
