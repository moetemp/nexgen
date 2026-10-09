# `x-nexus-handle`

Source: **not** a JSON Schema 2020-12 keyword. A Nexus extension on a
`services:` entry, in the `x-` family the generator already admits. It declares
**handle projections** over the [[http-caller]]: objects that bind some input
members once (a stream's identity, a producer's identity) and expose the
operations whose inputs carry them as methods with those members filled in.
Like [[services]], this note adapts the `features/<keyword>.md` skeleton.

## Spec summary

```yaml
services:
  StreamService:
    x-nexus-handle:
      WorkflowStreams: [workflowId]
      StreamProducer: [workflowId, stream, producerId, attempt]
    operations:
      append: { input: { $ref: "#/$defs/AppendInput" } }
      read: { input: { $ref: "#/$defs/ReadInput" } }
```

- The value maps an emitted type name (`^[A-Z][a-zA-Z\d]+$`) to a non-empty,
  duplicate-free list of **wire** member names, the same rule as the
  [[long-poll]] members.
- An operation **joins** a handle when its input carries every key member. The
  handle method drops those members from its signature and injects the bound
  values; the rest of the signature stays. Membership is not exclusive: an
  operation joins every handle whose keys it carries.
- A handle whose key set extends another's is constructible from it, so a
  `StreamProducer` is built from a `WorkflowStreams` by the keys it adds.
- The flat caller stays emitted. Handles are a projection over it; nothing about
  the wire, the handler, or a caller that ignores the keyword changes.

## Support decision

**Support: yes.** Accepted by every target; emitted by the two targets with an
HTTP caller, and only with `--client`.

Why the contract declares it: the object that binds a stream's identity once and
exposes the operations as methods is the largest piece of every hand-written
stream wrapper, and each language rewrites it. Declaring the key sets in the
contract lets every target emit the same projection.

The defining choices (citing [[PRINCIPLES.md]]):
- **One value per key (P7.1).** A handle holds one value per key and sends it on
  every call, so every operation that joins a handle must declare each key
  member the same way (the same shape and the same presence; documentation may
  differ). A key with one admissible value (`const`) is refused, because no
  caller chooses it.
- **Nothing derived silently (P7.1).** A handle no operation joins, and two
  handles over one key set, are load rejects: the first emits nothing, the
  second emits one projection twice under two names.
- **Names reject at load (P15).** A handle's type name, and in Go each
  `<Handle><Op>Request` type, enter the collision pass with the caller's other
  names. Derived constructor names are checked against the operations and each
  other on the object they land on.
- **Stateless (P1).** A handle binds at construction and carries no other state.
  An optional key may be bound absent, and the handle sends what it holds.

## Constructors

A constructor is named after the handle: a trailing `Handle` is dropped
(`StreamHandle` is built by `stream`), and on a parent handle the parent's own
base is dropped as a prefix when something is left (`StreamProducer` from
`StreamHandle` is `producer`; from the flat caller it stays `stream_producer`).
Each target cases the result.

## Type mapping

| Aspect | Go | Python |
|---|---|---|
| Handle | `type <Handle> struct` holding the caller and the keys | `class <Handle>` whose `__init__` takes the caller and the keys |
| Constructor on the caller | `func (c *<S>HTTPClient) <Ctor>(keys...) *<Handle>` | `def <ctor>(self, *, keys...) -> "<Handle>"` |
| Constructor on a parent | `func (h *<Parent>) <Ctor>(extra keys...) *<Handle>` | `def <ctor>(self, *, extra keys...) -> "<Handle>"` |
| Operation method | `<Op>(ctx, request <Handle><Op>Request) (Out, error)`; the request type is the input without the bound members | `async def <op>(self, *, free members...) -> Out` |
| Long-poll loop | rides along, sharing the request type; the loop sets the wait member | rides along, without the wait member in its signature |

Key and member types are the models' own field types, so the handle hands the
model exactly what its field holds. A key's type is the first joining
operation's. Go names an inline enum after its model, so another operation can
type the same key as another enum over the same underlying type; the handle
converts the value into that operation's type, and a child handle converts what
its parent holds.

## Property-testing matrix

### Accepted (positive)

| Case | Asserted |
|---|---|
| Two handles, one extending the other | both emitted; the child built from the parent and from the flat caller |
| An operation missing a key | stays off that handle |
| A long-poll operation | the loop method rides the handle |
| An inline enum key on two inputs | Go converts between the two models' enum types; the package compiles |
| No `--client` | no handle emitted; models and services unchanged |
| TypeScript, Java | nothing emitted |

### Rejected at load time (negative)

| Case | Example |
|---|---|
| Not a map of lists | `StreamHandle: workflowId` |
| A key listed twice | `[workflowId, workflowId]` |
| Nothing joins | `Orphan: [workflowId, finish, afterToken]` |
| Two handles, one key set | `[workflowId, stream]` and `[stream, workflowId]` |
| A key declared two ways | `attempt` required on one input, optional on another |
| A key with one admissible value | `finish: { type: boolean, const: true }` |
| A constructor named like an operation | handle `Read` beside operation `read` |
| A name already in scope | handle `ReadInput` beside model `ReadInput`; Go request type `StreamHandleReadRequest` beside a model of that name |

### Runtime fixtures

`tests/http_caller_toolchain.rs` drives both targets' handles against a local
endpoint and checks what reached it: the bound members beside the free ones,
through the flat caller, a handle and a child handle.

## Interactions

- [[http-caller]]: every handle method posts through the flat caller's method.
- [[long-poll]]: the loop method appears on a handle like any other operation.
- [[stream-ref]]: a handle key is a top-level input member, so a reference
  carried as one member (`stream`) binds whole.

## See also

[[http-caller]], [[long-poll]], [[PRINCIPLES.md]].
