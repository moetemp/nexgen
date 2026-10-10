# `x-nexus-stream-result`

Source: **not** a JSON Schema 2020-12 keyword. A Nexus extension on an
`operations:` entry, in the `x-` family the generator already admits. It marks
an operation whose start hands its caller a **stream**: the operation keeps
running while a producer appends records, progress tells the caller that more
is there, and the operation completes when the stream closes. The keyword names
the type of one record's body and, per target, the SDK reader a Workflow caller
wraps the operation handle in. Like [[services]], this note adapts the
`features/<keyword>.md` skeleton.

## Spec summary

```yaml
services:
  ChatService:
    operations:
      chat:
        x-nexus-stream-result:
          item: { $ref: "#/$defs/Token" }                      # one record's body
          reader: { py: example_sdk.streams.OperationStreamReader }   # optional
        input: { $ref: "#/$defs/ChatInput" }
        output: { $ref: "#/$defs/ChatSummary" }   # the result at completion
```

- The value is an object with `item` and optionally `reader`.
- `item` is a `$ref` to an object model, resolved like an operation's input or
  output. It is the type a reader hands back for each record.
- `reader` maps a target's `x-<lang>-name` token to a dotted `module.Type`
  path: the SDK's reader type. Only `py` can be mapped today.
- **Without a reader for a target, the target emits nothing for the stream.**
  The operation stays an ordinary operation, and the generator assumes no SDK
  type.
- The keyword changes no model, no binding and no wire byte. The operation's
  `output` stays the result the operation completes with.

## Support decision

**Support: yes.** Accepted by every build and every target; the Python Workflow
client (`--native-api`) emits a reader method when the contract maps a Python
reader type.

Why the contract says it: whether an operation's start hands back a stream is a
property of the operation, and the type of the records it carries is part of
the contract the handler and the caller share. A caller should not learn either
from documentation.

Why the reader is the SDK's and the generated code only wraps the handle:
reading a stream from a Workflow is protocol work. The reader waits on the
operation's progress, reads the records after its cursor through the stream
service, and ends when the operation completes. Where the stream reference
comes from (today the operation token, later a typed start result) is the SDK's
business too. Generated code that did any of this would hand-roll the
protocol, and every change to it would mean regenerating every caller. The
generated method binds what only the contract knows, the item type, and what
only the caller knows, the endpoint.

Why the contract, not the generator, names the reader type: as for
[[stream-ref]], no published SDK carries one yet, and the module it lives in is
the SDK's decision.

The defining choices (citing [[PRINCIPLES.md]]):
- **Not gated (P7.1, P1).** A gated keyword would make one contract load under
  one build and fail under another. Every target accepts it; a target with no
  reader type emits nothing for it.
- **A pinned item shape (P7.1).** `item` is a `$ref` to an object model, so the
  reader's type argument is always one emitted model class, and a name that
  does not resolve is a load reject.
- **P15.** The reader method, `read_<op>_stream`, shares the Workflow client
  with the operation methods, and the reader's package is imported as
  `import pkg.mod`, which binds `pkg` in the service file. Both join the
  collision pass; a clash is refused with `x-py-name` as the remedy. The
  client's own members, `_nexus_client` and, with a reader, `_endpoint`, are
  in that pass too, since an operation method named like one would be hidden
  by the instance attribute. A refusal names the contract file and the
  operation as authored.

## Emission (Python)

Per marked operation with a `py` reader, the Workflow client gains a method
beside the operation's start method, and keeps its endpoint:

```python
class ChatServiceClient:
    def __init__(self, endpoint: str) -> None:
        ...
        self._endpoint = endpoint

    def read_chat_stream(
        self,
        handle: temporalio.workflow.NexusOperationHandle[ChatSummary],
        *,
        endpoint: str | None = None,
    ) -> example_sdk.streams.OperationStreamReader[Token]:
        return example_sdk.streams.OperationStreamReader(
            handle,
            item_type=Token,
            endpoint=self._endpoint if endpoint is None else endpoint,
        )
```

The reader type must accept `(handle, *, item_type, endpoint)` and be generic
in the item type. The method is the only place the item type and the endpoint
meet the handle. `endpoint` names the Nexus endpoint that serves the stream
service; by default it is the endpoint the operation was started on.

What the SDK reader does, and the generated code does not:

| Concern | Owner |
|---|---|
| The stream reference, from the operation token now and a start result later | SDK reader |
| Waiting for progress (`handle.progress(after_counter=...)`) | SDK reader |
| The cursor, one per reader | SDK reader |
| Reading through the stream service and decoding records into `item_type` | SDK reader |
| End of stream: `next()` answers `None` once the operation completes and the last records are read | SDK reader |
| Read errors | SDK reader; they surface from `next()` |

## Property-testing matrix

### Accepted (positive)

| Case | Asserted |
|---|---|
| Marked `chat` beside an unmarked `ping` | reader method for `chat` only; `ping` unchanged |
| No `py` reader | no method, no import, no stored endpoint |
| A service with a fixed `endpoint` | the keyword is accepted and no reader method is emitted |
| Go, TypeScript, Java | generate; nothing of the keyword appears |

### Rejected at load time (negative)

| Case | Example |
|---|---|
| Not an object | `x-nexus-stream-result: true` |
| Unknown member | `wait: 1` beside `item` |
| No `item` | `{ reader: { py: example.Reader } }` |
| `item` not a `$ref` | `item: { type: string }` |
| `item` does not resolve | `$ref: "#/$defs/Missing"` |
| `reader` not a map | `reader: example.Reader` |
| A target with no reader | `reader: { go: example.Reader }` |
| Not a dotted path, or a keyword segment | `{ py: Reader }`, `{ py: example.import.Reader }` |
| Reader method clashes with an operation | an operation with `x-py-name: read_chat_stream` |
| An operation named like a client member | `x-py-name: _endpoint` or `_nexus_client` |
| Reader package clashes in the service file | `{ py: Token.streams.Reader }` beside a `Token` model |

### Runtime fixtures

`tests/stream_result_toolchain.rs` generates the Workflow client, puts a
stand-in reader type at the mapped path, and drives the method against a stub
handle whose progress reaches counters 1 and 2 before the operation completes:
the reader is built with the handle, the `Token` model class and the client's
endpoint (or the one passed), its batches come back as `Token` values, and the
stream ends with `None`. basedpyright checks the emitted method.

## Interactions

- [[stream-ref]]: the reference the SDK reader reads from; the keyword does not
  name it, because it does not cross the generated code.
- [[long-poll]], [[cursor]]: the stream service's read, which the SDK reader
  calls.
- [[services]]: the Workflow client the method joins.

## See also

[[stream-ref]], [[long-poll]], [[cursor]], [[handles]], [[PRINCIPLES.md]].
