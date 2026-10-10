# The HTTP caller (`--client`)

Source: **not** a JSON Schema keyword. A design note for an optional generated
output: a typed HTTP caller per service, for a process **outside** a Temporal
worker that calls an operation through the Nexus HTTP ingress. Like
[[services]], it adapts the `features/<keyword>.md` skeleton rather than
following it verbatim.

## Spec summary

- `go` and `python` accept a `--client` flag. With it, each service in a Nexus
  document gets one caller type, with one method per operation, typed with the
  generated models.
- A method serializes its input through the model's own encoder, posts it to
  `{base_url}/{operation}`, and decodes a `200` answer through the model's own
  decoder. A `204` is an answer with no body. Every other status is an error
  that carries the status.
- The caller is for **synchronous** operations. A handler that starts an
  operation asynchronously answers `201` with a token, which the caller reports
  as an error rather than reading it as the result.
- An operation marked [[long-poll]] also gets a loop method that re-issues the
  call until it answers with something or a deadline passes.
- The flag is gated behind the `advanced` Cargo feature. It adds files; it never
  changes a model, a binding, or the wire.

## Support decision

**Support: yes, for Go and Python, behind `advanced`.**

Why a caller at all: the service binding [[services]] emits is a typed
contract for a Nexus *handler* and for a *workflow* that calls through the
SDK's Nexus client. A process that is not a worker (a producer posting
records, a CLI, a test) has neither, and today hand-writes a JSON POST per
operation. The caller is that code, generated from the same contract.

The defining choices (citing [[PRINCIPLES.md]]):
- **The wire does not move (P1).** The caller serializes through the generated
  models' own transfer form, so the bytes it posts are the bytes the models
  already define, and validation runs on both sides exactly as it does for a
  handler (P10, P12).
- **Standard library only (P4).** Python uses `urllib` inside
  `asyncio.to_thread`; Go uses `net/http`. A caller is often not in a worker
  process, so it depends on nothing beyond the models module next to it.
- **Behind `advanced`.** `advanced` holds the CLI surface the root README does
  not document. The workflow-side `{Service}Client` is already reachable only
  through the `advanced`-gated `--native-api`, and a second caller kind should
  not widen the published binary's documented surface on its own.
- **Reject at load, not at compile (P15).** The caller's identifiers are fixed by
  the service identifier, so they are reserved whether or not the flag is on:
  one accept set for every invocation, as for the native client.

Not done, and why:
- **TypeScript, Java and .NET** emit no caller. Each needs its own HTTP stack
  and an async story, and no consumer asks for one yet. A library request that
  sets `client` for them emits nothing.
- **Asynchronous operations.** Following an operation token to its result is the
  workflow-side client's job; this caller refuses a `201` instead.

## Type mapping

| Aspect | Go | Python |
|---|---|---|
| File | `client.go`, in the models' flat package | `client.py`, beside `models.py` |
| Caller type | `<Service>HTTPClient` | `<Service>HttpClient` |
| Construction | `New<Service>HTTPClient(baseURL string, options *<Service>HTTPClientOptions)` | `<Service>HttpClient(base_url, *, headers=None, timeout=30.0)` |
| Options | `HTTPClient *http.Client` (nil is `http.DefaultClient`), `Header http.Header` | `headers` added to every request, `timeout` in seconds |
| Method | `<Op>(ctx context.Context, request In) (Out, error)` | `async def <op>(self, request: In) -> Out` |
| Void input | no `request` parameter, body `{}` | no `request` parameter, body `{}` |
| Void output | `error` only | `-> None` |
| Encode | `json.Marshal(request)` (the model's `MarshalJSON`) | `_<In>TransferTypeConverter().to_transfer_type(request)` |
| Decode | `json.Unmarshal(raw, &answer)` | `_<Out>TransferTypeConverter().from_transfer_type(answer, Out)` |
| Status error | `*HTTPStatusError{URL, StatusCode, Body, RetryableHeader}` with `Retryable()` | `HTTPStatusError(url, status, detail, retryable=None)` with `.retryable` |

`<Service>` and `<Op>` are the binding's own identifiers, so an `x-<lang>-name`
moves the caller with the binding. Go spells the fixed vocabulary
idiomatically (`HTTPClient`, `HTTPStatusError`) because it is not derived from
an authored name; the [[properties]] acronym folding applies to derived names
only.

When the handler sends `Nexus-Request-Retryable`, that header decides
`Retryable`. A Temporal handler error that isn't retryable comes back as a
`5xx` with the header set to `false`. Without the header, `Retryable` is true
for `429` and every `5xx`, the endpoint's own transient conditions. Every
other status is about the request, and repeating it changes nothing.

## Wire behavior

- **Path.** `base_url` is the service's address on the ingress, up to but not
  including the operation name; a trailing `/` is dropped. The operation's wire
  name (`fqn`) is percent-encoded as one RFC 3986 path segment at generation
  time, because a wire name may hold any character. Only the unreserved
  characters pass through, so the segment is also safe inside each target's
  string literal and format string.
- **Body.** `Content-Type: application/json`. A void input posts `{}`.
- **Answer.** `200` is decoded as the output. An empty `200` for an operation
  that declares an output is an error: reading it as the all-defaults value
  would hand the caller a value the endpoint never produced. `204` is accepted
  for a void output.
- **Not retried.** A single-shot method makes one attempt. A transport error is
  whatever the caller's own client settings make it. Only the [[long-poll]]
  loop repeats a call, and it retries only a retryable status.
- **Cancel.** The Python caller runs the request in a worker thread. A cancelled
  task doesn't stop that thread, so the request holds its connection until the
  endpoint answers or `timeout` passes. Keep `timeout` short for a caller that
  cancels long polls often.
- **Redirects.** Both targets use the platform's default redirect handling, and
  a `301`, `302` or `303` turns the `POST` into a `GET` with no body. A Nexus
  ingress doesn't redirect operation calls. A Go caller that wants a redirect
  to fail can pass an `http.Client` whose `CheckRedirect` returns
  `http.ErrUseLastResponse`.

## File layout & namespacing

Go flattens a generate closure into one package, so every input file's
services share one `client.go`, the way they share `definitions.go`. Python
emits one `client.py` per module that declares a service. The caller is not
re-exported from `__init__.py`; it is imported as
`from <package>.client import ...`.

Reserved identifiers, inserted into the [[generated-file-layout]] collision
pass whether or not `--client` is set:
- Go: `<Service>HTTPClient`, `<Service>HTTPClientOptions`,
  `New<Service>HTTPClient`, and `HTTPStatusError`.
- Python: `<Service>HttpClient` and `HTTPStatusError`.

## Property-testing matrix

### Accepted (positive)

| Case | Asserted |
|---|---|
| Operation with input and output | method signature, posted path, decode through the converter |
| Void input, void output | no parameter, body `{}`, `204` accepted |
| Wire name with `/`, space, `{` | one percent-encoded segment on the wire |
| `--client` off | no `client.py` / `client.go` |
| TypeScript, Java with `client` set | no caller emitted |

### Rejected at load time (negative)

| Case | Example |
|---|---|
| A model's identifier equals a reserved caller name | `x-go-name: ChatHTTPClient` beside service `Chat` |

### Runtime fixtures

`tests/http_caller_toolchain.rs` builds both callers with their real
toolchains and drives them against a local endpoint: the posted path and body,
a header, a void operation, and `201`, `400`, `429`, `503` reported as status
errors with the right retryability.

## Interactions

- [[services]]: the caller is a sibling of the binding, named from the same
  identifiers.
- `--native-api`: orthogonal. The native client calls Nexus from inside a
  workflow; this caller posts from outside one.
- [[long-poll]]: adds the loop method per marked operation.
- [[handles]]: projections that bind some input members once and post through
  this caller.

## See also

[[services]], [[long-poll]], [[handles]], [[generated-file-layout]], [[PRINCIPLES.md]].
