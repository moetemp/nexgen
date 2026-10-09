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

/// Operations that answer with no output, so the caller decodes nothing.
const VOID_CONTRACT: &str = r##"
nexusrpc: "1.0.0"
services:
  StreamService:
    fqn: example.streams.v1.StreamService
    operations:
      close:
        fqn: close
        input: { $ref: "#/$defs/CloseInput" }
      ping:
        fqn: ping
$defs:
  CloseInput:
    type: object
    properties:
      stream: { type: string }
    additionalProperties: false
"##;

#[test]
fn the_generated_go_caller_imports_only_what_it_uses() {
    let workspace = Workspace::new("http-caller-go-imports-toolchain");
    let root = prepare_go_module(&workspace).expect("prepare the go module");
    generate_client_from(&workspace, Target::Go, "void", VOID_CONTRACT);
    run(command("go")
        .current_dir(&root)
        .env("GOFLAGS", "-mod=mod")
        .args(["vet", "./..."]))
    .expect_ok("go vet over a caller with nothing to decode");
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
