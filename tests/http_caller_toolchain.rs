//! The generated HTTP callers, through the real toolchains.
//!
//! `generate_http_caller.rs` asserts the emission decisions as substrings. That
//! cannot catch output that reads correctly and does not compile, or that
//! compiles and posts somewhere else, so each caller is built and driven here
//! against a local endpoint.

mod toolchain;

use std::fs;
use std::path::PathBuf;

use nexgen::generator::GenerationMode;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};
use toolchain::{Target, Workspace, command, prepare_go_module, python_interpreter, run};

/// A synchronous operation, a void one, and a wire name that is not a URL-safe
/// path segment.
const CONTRACT: &str = r##"
nexusrpc: "1.0.0"
services:
  StreamService:
    fqn: example.streams.v1.StreamService
    operations:
      read:
        fqn: "streams/read {v1}"
        input: { $ref: "#/$defs/ReadInput" }
        output: { $ref: "#/$defs/ReadOutput" }
      ping:
        fqn: ping
$defs:
  ReadInput:
    type: object
    properties:
      stream: { type: string }
      afterToken: { type: string }
    required: [stream]
    additionalProperties: false
  ReadOutput:
    type: object
    properties:
      records: { type: array, items: { type: string } }
      nextToken: { type: string }
    additionalProperties: false
"##;

/// A long-pollable read with a required wait hint, which a target holds by
/// value rather than behind an optional.
const LONG_POLL_CONTRACT: &str = r##"
nexusrpc: "1.0.0"
services:
  StreamService:
    fqn: example.streams.v1.StreamService
    operations:
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
      waitMs: { type: integer }
    required: [waitMs]
    additionalProperties: false
  ReadOutput:
    type: object
    properties:
      records: { type: array, items: { type: string } }
      nextToken: { type: string }
    additionalProperties: false
"##;

fn generate_client(workspace: &Workspace, target: Target, dir: &str) -> PathBuf {
    generate_client_from(workspace, target, dir, CONTRACT)
}

fn generate_client_from(
    workspace: &Workspace,
    target: Target,
    dir: &str,
    contract_text: &str,
) -> PathBuf {
    let contract = workspace.root().join(format!("{dir}.nexusrpc.yaml"));
    fs::write(&contract, contract_text).expect("write the contract");
    let output_path = workspace.package_path(target, dir);
    fs::create_dir_all(output_path.parent().expect("a package parent"))
        .expect("create the package parent");
    generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            client: true,
            ..Default::default()
        },
        language: target.language(),
        input_paths: vec![contract],
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .expect("generate the caller");
    output_path
}

/// Drives the caller against an httptest endpoint: what reaches it, what comes
/// back, and how a status that is not a synchronous result is reported.
const GO_DRIVER: &str = r#"package streams

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestTheCallerPostsAndReads(t *testing.T) {
	var paths []string
	var bodies []map[string]any
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		var decoded map[string]any
		_ = json.Unmarshal(body, &decoded)
		paths = append(paths, r.URL.EscapedPath())
		bodies = append(bodies, decoded)
		if r.Header.Get("X-Test") != "yes" {
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		if r.URL.EscapedPath() == "/svc/ping" {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		_, _ = w.Write([]byte(`{"records": ["a", "b"], "nextToken": "t2"}`))
	}))
	defer server.Close()

	client := NewStreamServiceHTTPClient(server.URL+"/svc/", &StreamServiceHTTPClientOptions{
		Header: http.Header{"X-Test": []string{"yes"}},
	})
	after := "t1"
	answer, err := client.Read(context.Background(), ReadInput{Stream: "scores", AfterToken: &after})
	if err != nil {
		t.Fatal(err)
	}
	if len(answer.Records) != 2 || answer.NextToken == nil || *answer.NextToken != "t2" {
		t.Fatalf("read answered %+v", answer)
	}
	if err := client.Ping(context.Background()); err != nil {
		t.Fatal(err)
	}
	if len(paths) != 2 || paths[0] != "/svc/streams%2Fread%20%7Bv1%7D" || paths[1] != "/svc/ping" {
		t.Fatalf("posted to %v", paths)
	}
	if bodies[0]["stream"] != "scores" || bodies[0]["afterToken"] != "t1" {
		t.Fatalf("read carried %v", bodies[0])
	}
}

func TestAnAnswerThatIsNotASynchronousResultIsReported(t *testing.T) {
	for status, retryable := range map[int]bool{
		http.StatusCreated:            false,
		http.StatusBadRequest:         false,
		http.StatusTooManyRequests:    true,
		http.StatusServiceUnavailable: true,
	} {
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			w.WriteHeader(status)
			_, _ = w.Write([]byte(`{"token": "started"}`))
		}))
		_, err := NewStreamServiceHTTPClient(server.URL, nil).Read(context.Background(), ReadInput{Stream: "s"})
		server.Close()
		var statusErr *HTTPStatusError
		if !errors.As(err, &statusErr) {
			t.Fatalf("status %d: got %v, want an HTTPStatusError", status, err)
		}
		if statusErr.StatusCode != status || statusErr.Retryable() != retryable {
			t.Fatalf("status %d: got %+v retryable %v", status, statusErr, statusErr.Retryable())
		}
	}
}
"#;

#[test]
fn the_generated_go_caller_posts_to_a_local_endpoint() {
    let workspace = Workspace::new("http-caller-go-toolchain");
    let root = prepare_go_module(&workspace).expect("prepare the go module");
    let package = generate_client(&workspace, Target::Go, "streams");
    fs::write(package.join("caller_test.go"), GO_DRIVER).expect("write the driver");

    run(command("go")
        .current_dir(&root)
        .env("GOFLAGS", "-mod=mod")
        .args(["vet", "./..."]))
    .expect_ok("go vet over the generated caller");
    run(command("go")
        .current_dir(&root)
        .env("GOFLAGS", "-mod=mod")
        .args(["test", "./..."]))
    .expect_ok("go test over the generated caller");
}

/// Drives the caller against a local `http.server` endpoint.
const PYTHON_DRIVER: &str = r#"
import asyncio
import http.server
import importlib
import json
import pathlib
import sys
import threading

package = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(package.parent))
client_module = importlib.import_module(f"{package.name}.client")
models = importlib.import_module(f"{package.name}.models")

seen = []
status_for = {}


class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(length) or b"null")
        seen.append((self.path, body, self.headers.get("X-Test")))
        status = status_for.get(self.path, 200)
        if self.path.endswith("/ping") and status == 200:
            status = 204
        self.send_response(status)
        self.end_headers()
        if status != 204:
            self.wfile.write(b'{"records": ["a", "b"], "nextToken": "t2"}')

    def log_message(self, *args):
        pass


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = f"http://127.0.0.1:{server.server_port}/svc/"


async def main():
    client = client_module.StreamServiceHttpClient(base, headers={"X-Test": "yes"})
    answer = await client.read(models.ReadInput(stream="scores", after_token="t1"))
    assert answer.records == ["a", "b"] and answer.next_token == "t2", answer
    assert await client.ping() is None

    for status, retryable in [(201, False), (400, False), (429, True), (503, True)]:
        status_for["/svc/streams%2Fread%20%7Bv1%7D"] = status
        try:
            await client.read(models.ReadInput(stream="s"))
        except client_module.HTTPStatusError as error:
            assert error.status == status and error.retryable == retryable, error
        else:
            raise AssertionError(f"status {status} was read as a result")


asyncio.run(main())
server.shutdown()
read_path, read_body, header = seen[0]
assert read_path == "/svc/streams%2Fread%20%7Bv1%7D", read_path
assert read_body == {"stream": "scores", "afterToken": "t1"}, read_body
assert header == "yes", header
assert seen[1][0] == "/svc/ping" and seen[1][1] == {}, seen[1]
print("ok")
"#;

#[test]
fn the_generated_python_caller_posts_to_a_local_endpoint() {
    let workspace = Workspace::new("http-caller-python-toolchain");
    let package = generate_client(&workspace, Target::Python, "streams");
    let driver = workspace.root().join("drive.py");
    fs::write(&driver, PYTHON_DRIVER).expect("write the driver");

    let interpreter = python_interpreter();
    run(command(&interpreter.to_string_lossy())
        .arg("-m")
        .arg("compileall")
        .arg("-q")
        .arg(&package))
    .expect_ok("compile the generated caller");
    let output = run(command(&interpreter.to_string_lossy())
        .arg(&driver)
        .arg(&package))
    .expect_ok("drive the generated caller");
    assert!(output.contains("ok"), "{output}");
}

/// Drives the loop against an endpoint that answers empty twice, fails once
/// with a retryable status, then answers with records.
const GO_LONG_POLL_DRIVER: &str = r#"package polls

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"
)

func TestTheLoopReturnsTheFirstNonEmptyAnswer(t *testing.T) {
	var mu sync.Mutex
	var waits []float64
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		var decoded map[string]any
		_ = json.Unmarshal(body, &decoded)
		mu.Lock()
		calls++
		call := calls
		waits = append(waits, decoded["waitMs"].(float64))
		mu.Unlock()
		switch call {
		case 1, 2:
			_, _ = w.Write([]byte(`{"records": [], "nextToken": "t1"}`))
		case 3:
			w.WriteHeader(http.StatusServiceUnavailable)
		default:
			_, _ = w.Write([]byte(`{"records": ["a"], "nextToken": "t2"}`))
		}
	}))
	defer server.Close()
	client := NewStreamServiceHTTPClient(server.URL, nil)
	answer, err := client.ReadUntilRecords(context.Background(), ReadInput{}, 5*time.Second)
	if err != nil {
		t.Fatal(err)
	}
	if len(answer.Records) != 1 || *answer.NextToken != "t2" {
		t.Fatalf("loop answered %+v", answer)
	}
	if calls != 4 {
		t.Fatalf("want four attempts, got %d", calls)
	}
	for _, wait := range waits {
		if wait <= 0 || wait > 5000 {
			t.Fatalf("wait hint %v outside the budget", wait)
		}
	}
}

func TestTheLoopReturnsTheLastAnswerAtTheDeadline(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"records": [], "nextToken": "resume-here"}`))
	}))
	defer server.Close()
	client := NewStreamServiceHTTPClient(server.URL, nil)
	answer, err := client.ReadUntilRecords(context.Background(), ReadInput{}, 200*time.Millisecond)
	if err != nil {
		t.Fatal(err)
	}
	if len(answer.Records) != 0 || answer.NextToken == nil || *answer.NextToken != "resume-here" {
		t.Fatalf("loop answered %+v", answer)
	}
}

func TestTheLoopStopsOnAStatusThatIsNotRetryable(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
	}))
	defer server.Close()
	_, err := NewStreamServiceHTTPClient(server.URL, nil).ReadUntilRecords(context.Background(), ReadInput{}, time.Second)
	var status *HTTPStatusError
	if !errors.As(err, &status) || status.StatusCode != http.StatusBadRequest {
		t.Fatalf("got %v, want the 400", err)
	}
}
"#;

#[test]
fn the_generated_go_long_poll_loop_runs_against_a_local_endpoint() {
    let workspace = Workspace::new("http-caller-go-long-poll-toolchain");
    let root = prepare_go_module(&workspace).expect("prepare the go module");
    let package = generate_client_from(&workspace, Target::Go, "polls", LONG_POLL_CONTRACT);
    fs::write(package.join("poll_test.go"), GO_LONG_POLL_DRIVER).expect("write the driver");

    run(command("go")
        .current_dir(&root)
        .env("GOFLAGS", "-mod=mod")
        .args(["vet", "./..."]))
    .expect_ok("go vet over the generated loop");
    run(command("go")
        .current_dir(&root)
        .env("GOFLAGS", "-mod=mod")
        .args(["test", "./..."]))
    .expect_ok("go test over the generated loop");
}

/// The same three cases against a local `http.server` endpoint.
const PYTHON_LONG_POLL_DRIVER: &str = r#"
import asyncio
import http.server
import importlib
import json
import pathlib
import sys
import threading

package = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(package.parent))
client_module = importlib.import_module(f"{package.name}.client")
models = importlib.import_module(f"{package.name}.models")

script = []
waits = []


class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(length))
        waits.append(body["waitMs"])
        status, answer = script.pop(0) if script else (200, b'{"records": [], "nextToken": "resume-here"}')
        self.send_response(status)
        self.end_headers()
        self.wfile.write(answer)

    def log_message(self, *args):
        pass


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
client = client_module.StreamServiceHttpClient(f"http://127.0.0.1:{server.server_port}")


async def main():
    script.extend(
        [
            (200, b'{"records": [], "nextToken": "t1"}'),
            (200, b'{"records": [], "nextToken": "t1"}'),
            (503, b"busy"),
            (200, b'{"records": ["a"], "nextToken": "t2"}'),
        ]
    )
    answer = await client.read_until_records(models.ReadInput(wait_ms=0), deadline=5.0)
    assert answer.records == ["a"] and answer.next_token == "t2", answer
    assert len(waits) == 4 and all(0 < wait <= 5000 for wait in waits), waits

    answer = await client.read_until_records(models.ReadInput(wait_ms=0), deadline=0.2)
    assert answer.records == [] and answer.next_token == "resume-here", answer

    script.append((400, b"refused"))
    try:
        await client.read_until_records(models.ReadInput(wait_ms=0), deadline=1.0)
    except client_module.HTTPStatusError as error:
        assert error.status == 400, error
    else:
        raise AssertionError("a 400 was retried")


asyncio.run(main())
server.shutdown()
print("ok")
"#;

#[test]
fn the_generated_python_long_poll_loop_runs_against_a_local_endpoint() {
    let workspace = Workspace::new("http-caller-python-long-poll-toolchain");
    let package = generate_client_from(&workspace, Target::Python, "polls", LONG_POLL_CONTRACT);
    let driver = workspace.root().join("drive_poll.py");
    fs::write(&driver, PYTHON_LONG_POLL_DRIVER).expect("write the driver");

    let interpreter = python_interpreter();
    run(command(&interpreter.to_string_lossy())
        .arg("-m")
        .arg("compileall")
        .arg("-q")
        .arg(&package))
    .expect_ok("compile the generated loop");
    let output = run(command(&interpreter.to_string_lossy())
        .arg(&driver)
        .arg(&package))
    .expect_ok("drive the generated loop");
    assert!(output.contains("ok"), "{output}");
}

/// An operation that hands back a stream reference and a read that takes one,
/// so the emitted converter is run against the SDK type it builds.
const STREAM_REF_CONTRACT: &str = r##"
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
    x-nexus-stream-ref: true
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
      afterToken: { type: string }
    required: [stream]
    additionalProperties: false
  ReadOutput:
    type: object
    properties:
      records: { type: array, items: { type: string } }
    additionalProperties: false
"##;

/// Runs the emitted stream-reference converter against the type it builds.
///
/// The generated module imports `temporalio.contrib.streams.StreamRef` by name,
/// which the SDK release the sample environment installs does not carry yet,
/// so the driver stands one in with the members the contract declares. What is
/// checked is the emitted code's contract with that type: built by keyword
/// from the wire members, read back by attribute, the SDK's own nulls accepted,
/// and the value a caller gets back from an operation being that type.
const PYTHON_STREAM_REF_DRIVER: &str = r#"
import asyncio
import dataclasses
import importlib
import pathlib
import sys
import types

package = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(package.parent))


@dataclasses.dataclass(frozen=True)
class StreamRef:
    owner: str
    topic: str
    workflow_id: str | None = None
    run_id: str | None = None


import temporalio.contrib

streams = types.ModuleType("temporalio.contrib.streams")
streams.StreamRef = StreamRef
sys.modules["temporalio.contrib.streams"] = streams
temporalio.contrib.streams = streams

models = importlib.import_module(f"{package.name}.models")
client_module = importlib.import_module(f"{package.name}.client")
assert models.StreamRef is StreamRef, models.StreamRef

ref = StreamRef(owner="workflow", topic="scores", workflow_id="game-1")
request = models.ReadInput(stream=ref, after_token="t1")
converter = models._ReadInputTransferTypeConverter()
wire = converter.to_transfer_type(request)
assert wire == {
    "stream": {"owner": "workflow", "workflow_id": "game-1", "topic": "scores"},
    "afterToken": "t1",
}, wire
back = converter.from_transfer_type(wire, models.ReadInput)
assert type(back.stream) is StreamRef, type(back.stream)
assert back.stream == ref, back.stream

# A reference the SDK serialized itself writes null for what it leaves unset.
alone = models._StreamRefTransferTypeConverter()
sdk_wire = {"owner": "workflow", "topic": "scores", "workflow_id": "game-1", "run_id": None}
assert alone.from_transfer_type(sdk_wire, StreamRef) == ref


def fake_post(url, body, headers, timeout):
    return b'{"owner": "workflow", "topic": "scores", "workflow_id": "game-1", "run_id": null}'


client_module._post = fake_post
started = asyncio.run(
    client_module.ScoresServiceHttpClient("http://endpoint").start_game(
        models.GameRequest(game_id="g1")
    )
)
assert type(started) is StreamRef and started == ref, started
print("ok")
"#;

#[test]
fn the_generated_python_stream_reference_round_trips_through_the_sdk_type() {
    let workspace = Workspace::new("http-caller-python-stream-ref-toolchain");
    let package = generate_client_from(&workspace, Target::Python, "scores", STREAM_REF_CONTRACT);
    let driver = workspace.root().join("drive_stream_ref.py");
    fs::write(&driver, PYTHON_STREAM_REF_DRIVER).expect("write the driver");

    let interpreter = python_interpreter();
    run(command(&interpreter.to_string_lossy())
        .arg("-m")
        .arg("compileall")
        .arg("-q")
        .arg(&package))
    .expect_ok("compile the generated stream reference module");
    let output = run(command(&interpreter.to_string_lossy())
        .arg(&driver)
        .arg(&package))
    .expect_ok("round-trip the stream reference");
    assert!(output.contains("ok"), "{output}");
}
