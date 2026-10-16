//! Runs the Workflow client's stream reader (`x-nexus-stream-result`) through
//! the Python toolchain against a stand-in SDK reader and operation handle.
//!
//! The stand-in reader is the contract the generated method relies on: it is
//! built from the operation handle, the item type and the stream service's
//! endpoint, and `next()` answers a typed batch or `None` once the stream has
//! ended. It drives a stub handle's `progress()` the way the SDK reader does, so
//! the batches come back typed and the stream ends with the operation.
#![cfg(feature = "advanced")]

mod toolchain;

use std::fs;
use std::path::PathBuf;

use nexgen::generator::GenerationMode;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};
use toolchain::{Target, Workspace, command, python_interpreter, run};

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

/// The stand-in SDK reader. The records a real reader reads through the stream
/// service come from the handle's stub here, so the test pins the generated
/// glue, not a store.
const STUB_SDK_READER: &str = r#"
from __future__ import annotations

import typing

T = typing.TypeVar("T")


class OperationStreamReader(typing.Generic[T]):
    def __init__(self, handle: typing.Any, *, item_type: type[T], endpoint: str) -> None:
        self.handle = handle
        self.item_type = item_type
        self.endpoint = endpoint
        self._counter = 0
        self._position = 0

    async def next(self) -> list[T] | None:
        progress = await self.handle.progress(after_counter=self._counter)
        if progress is None:
            return None
        self._counter = progress.counter
        records = self.handle.records[self._position : int(progress.position)]
        self._position = int(progress.position)
        return [self.item_type(**record) for record in records]
"#;

const DRIVER: &str = r#"
import asyncio
import dataclasses
import importlib
import pathlib
import sys

package = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(package.parent))

from example_sdk.streams import OperationStreamReader

services = importlib.import_module(f"{package.name}.services")
models = importlib.import_module(f"{package.name}.models")


@dataclasses.dataclass(frozen=True)
class Progress:
    position: str
    counter: int


class StubHandle:
    """Progress at counters 1 and 2, then the operation completes."""

    def __init__(self) -> None:
        self.records = [{"text": "a"}, {"text": "b"}, {"text": "c"}]
        self._progress = [Progress("1", 1), Progress("3", 2)]

    async def progress(self, *, after_counter: int = 0):
        for progress in self._progress:
            if progress.counter > after_counter:
                return progress
        return None


client = services.ChatServiceClient("chat-endpoint")
handle = StubHandle()
reader = client.read_chat_stream(handle)
assert isinstance(reader, OperationStreamReader), type(reader)
assert reader.handle is handle
assert reader.item_type is models.Token, reader.item_type
assert reader.endpoint == "chat-endpoint", reader.endpoint
assert client.read_chat_stream(handle, endpoint="streams").endpoint == "streams"


async def drain():
    batches = []
    while (batch := await reader.next()) is not None:
        batches.append(batch)
    return batches


batches = asyncio.run(drain())
assert batches == [
    [models.Token(text="a")],
    [models.Token(text="b"), models.Token(text="c")],
], batches
print("ok")
"#;

fn generate(workspace: &Workspace) -> PathBuf {
    let contract = workspace.root().join("chat.nexusrpc.yaml");
    fs::write(&contract, CONTRACT).expect("write the contract");
    let output_path = workspace.package_path(Target::Python, "chat");
    fs::create_dir_all(output_path.parent().expect("a package parent"))
        .expect("create the package parent");
    generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::NativeApi,
            ..Default::default()
        },
        language: Target::Python.language(),
        input_paths: vec![contract],
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .expect("generate the Workflow client");
    let sdk = output_path
        .parent()
        .expect("a package parent")
        .join("example_sdk");
    fs::create_dir_all(&sdk).expect("create the stand-in SDK");
    fs::write(sdk.join("__init__.py"), "").expect("write the stand-in SDK");
    fs::write(sdk.join("streams.py"), STUB_SDK_READER).expect("write the stand-in reader");
    output_path
}

#[test]
fn the_generated_python_stream_reader_reads_typed_batches_until_the_operation_ends() {
    let workspace = Workspace::new("stream-result-python-toolchain");
    let package = generate(&workspace);
    let driver = workspace.root().join("drive_stream_result.py");
    fs::write(&driver, DRIVER).expect("write the driver");
    let interpreter = python_interpreter();
    let interpreter = interpreter.to_string_lossy();

    let output = run(command(&interpreter).arg(&driver).arg(&package))
        .expect_ok("read the stream through the generated reader");
    assert!(output.contains("ok"), "{output}");
    run(command(&interpreter)
        .current_dir(package.parent().expect("a package parent"))
        .args([
            "-m",
            "basedpyright",
            "--pythonpath",
            &interpreter,
            "--level",
            "error",
        ])
        .arg(package.join("services.py")))
    .expect_ok("type-check the generated reader method");
}
