//! The forwarding handler `--forwarder` emits for Python.
//!
//! The emission tests assert the decisions as substrings. The toolchain test
//! registers a subclass with nexusrpc and drives each operation, since output
//! that reads right can still fail to register or convert.

mod toolchain;

use std::fs;
use std::path::{Path, PathBuf};

use nexgen::generator::GenerationMode;
use nexgen::language::Language;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};
use toolchain::{Target, Workspace, python_runtime_command, run};

/// A synchronous operation, a void one, and a wire name that is not a Python
/// identifier.
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
      waitMs: { type: integer, x-py-name: wait_ms }
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

fn generate_into(
    output_path: &Path,
    contract_path: &Path,
    language: Language,
    contract: &str,
    forwarder: bool,
) -> Result<(), String> {
    fs::write(contract_path, contract).expect("write the contract");
    generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            forwarder,
            ..Default::default()
        },
        language,
        input_paths: vec![contract_path.to_path_buf()],
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: output_path.to_path_buf(),
        format: false,
        java_package_name: (language == Language::Java).then(|| "streams".to_string()),
        ts_date_time_types: Default::default(),
    })
    .map_err(|error| error.to_string())
}

/// Generates into a fresh workspace and answers with the emitted files by name.
fn generate(
    language: Language,
    contract: &str,
    forwarder: bool,
    tag: &str,
) -> (Workspace, Vec<(String, String)>) {
    let workspace = Workspace::new(tag);
    let output_path = workspace.root().join("streams");
    let contract_path = workspace.root().join("streams.nexusrpc.yaml");
    generate_into(&output_path, &contract_path, language, contract, forwarder)
        .unwrap_or_else(|error| panic!("{language:?} should generate: {error}"));
    let files = read_files(&output_path);
    (workspace, files)
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
fn the_forwarder_forwards_each_operation_by_its_wire_name() {
    let (_workspace, files) = generate(Language::Python, CONTRACT, true, "forwarder-python");
    let forwarder = file(&files, "forwarder.py");
    assert!(
        forwarder.contains("\n\nclass StreamServiceForwarder(abc.ABC):\n"),
        "{forwarder}"
    );
    assert!(
        forwarder.contains(
            "    @nexusrpc.handler.sync_operation\n    async def append(\n        self,\n        \
             ctx: nexusrpc.handler.StartOperationContext,\n        input: AppendInput,\n    ) \
             -> AppendOutput:"
        ),
        "{forwarder}"
    );
    assert!(
        forwarder.contains(
            "transfer: typing.Any = _AppendInputTransferTypeConverter().to_transfer_type(input)"
        ),
        "{forwarder}"
    );
    assert!(
        forwarder.contains("answer: typing.Any = await self.forward(\"append\", ctx, transfer)"),
        "{forwarder}"
    );
    assert!(
        forwarder.contains(
            "return _AppendOutputTransferTypeConverter().from_transfer_type(answer, AppendOutput)"
        ),
        "{forwarder}"
    );
    // The wire name is a string literal, so no character in it can end it.
    assert!(
        forwarder.contains("await self.forward(\"streams/read {v1}\", ctx, transfer)"),
        "{forwarder}"
    );
    // The operation's own docstring carries over.
    assert!(
        forwarder.contains("\"\"\"Append one batch of records.\"\"\""),
        "{forwarder}"
    );
}

#[test]
fn a_void_operation_forwards_none_and_returns_nothing() {
    let (_workspace, files) = generate(Language::Python, CONTRACT, true, "forwarder-void");
    let forwarder = file(&files, "forwarder.py");
    assert!(
        forwarder.contains(
            "    async def ping(\n        self,\n        ctx: nexusrpc.handler.StartOperationContext,\n        \
             input: None,\n    ) -> None:\n        transfer: typing.Any = None\n        \
             await self.forward(\"ping\", ctx, transfer)\n"
        ),
        "{forwarder}"
    );
}

#[test]
fn no_forwarder_is_emitted_without_the_flag() {
    let (_workspace, files) = generate(Language::Python, CONTRACT, false, "forwarder-off");
    assert!(!has_file(&files, "forwarder.py"));
}

#[test]
fn the_other_targets_emit_no_forwarder() {
    // The flag is only on the `python` subcommand, but the library request
    // carries it for every target. The others ignore it.
    for language in [Language::Go, Language::TypeScript, Language::Java] {
        let (_workspace, files) = generate(language, CONTRACT, true, "forwarder-other-targets");
        assert!(
            !files.iter().any(|(name, _)| name.contains("forwarder")),
            "{language:?} emitted a forwarder"
        );
    }
}

#[test]
fn a_model_named_like_the_forwarder_is_refused() {
    let contract = CONTRACT.replace("ReadOutput", "StreamServiceForwarder");
    let workspace = Workspace::new("forwarder-clash");
    let error = generate_into(
        &workspace.root().join("streams"),
        &workspace.root().join("streams.nexusrpc.yaml"),
        Language::Python,
        &contract,
        false,
    )
    .expect_err("a model named like the forwarder class should be refused");
    assert!(error.contains("forwarding handler"), "{error}");
}

const PYTHON_DRIVER: &str = r#"
import asyncio
import importlib
import pathlib
import sys

import nexusrpc.handler

package = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(package.parent))
forwarder = importlib.import_module(f"{package.name}.forwarder")
models = importlib.import_module(f"{package.name}.models")

calls = []


services = importlib.import_module(f"{package.name}.services")


@nexusrpc.handler.service_handler(service=services.StreamService)
class Handler(forwarder.StreamServiceForwarder):
    async def forward(self, operation, ctx, transfer):
        calls.append((operation, transfer))
        if operation == "append":
            return {"cursor": "after-" + transfer["stream"]}
        if operation == "streams/read {v1}":
            return {"records": ["one", "two"]}
        return None


class Unfinished(forwarder.StreamServiceForwarder):
    pass


async def main():
    try:
        Unfinished()
    except TypeError:
        pass
    else:
        raise AssertionError("a forwarder without forward was built")
    handler = Handler()
    # Registering checks each operation against the service definition.
    nexusrpc.handler.Handler([handler])
    appended = await handler.append(None, models.AppendInput(stream="s", wait_ms=5))
    assert appended == models.AppendOutput(cursor="after-s"), appended
    read = await handler.read(None, models.ReadInput(stream="s"))
    assert read == models.ReadOutput(records=["one", "two"]), read
    assert await handler.ping(None, None) is None
    assert calls == [
        ("append", {"stream": "s", "waitMs": 5}),
        ("streams/read {v1}", {"stream": "s"}),
        ("ping", None),
    ], calls
    print("ok")


asyncio.run(main())
"#;

#[test]
fn the_generated_forwarder_registers_and_round_trips_each_operation() {
    let workspace = Workspace::new("forwarder-python-toolchain");
    let package: PathBuf = workspace.package_path(Target::Python, "forwarded");
    fs::create_dir_all(package.parent().expect("a package parent")).expect("create the parent");
    let contract_path = workspace.root().join("forwarded.nexusrpc.yaml");
    generate_into(&package, &contract_path, Language::Python, CONTRACT, true)
        .expect("generate the forwarder");
    let driver = workspace.root().join("drive_forwarder.py");
    fs::write(&driver, PYTHON_DRIVER).expect("write the driver");
    let mut python = python_runtime_command().expect("a Python with nexusrpc");
    let output = run(python.arg(&driver).arg(&package)).expect_ok("drive the forwarder");
    assert!(output.contains("ok"), "{output}");
}
