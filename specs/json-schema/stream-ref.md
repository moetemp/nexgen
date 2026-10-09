# `x-nexus-stream-ref`

Source: **not** a JSON Schema 2020-12 keyword. A Nexus extension on a `$defs`
model, in the `x-` family the generator already admits. It marks the model as
the **stream reference**: the value an operation returns to hand a stream to its
caller, and that later operations take to name the stream again. A contract may
also map the reference, per target, to a type the target emits in the model's
place, such as an SDK's own reference type. Like [[services]], this note adapts
the `features/<keyword>.md` skeleton.

## Spec summary

```yaml
$defs:
  StreamRef:
    type: object
    x-nexus-stream-ref: { py: example_sdk.streams.StreamRef }   # or `true`
    properties:
      owner: { type: string, enum: [workflow, activity, standalone] }
      workflow_id: { oneOf: [{ type: string }, { type: "null" }] }
      run_id: { oneOf: [{ type: string }, { type: "null" }] }
      topic: { type: string }
    required: [owner, topic]
    additionalProperties: false
```

- The value is `true` (mark the model, map nothing), `false` (the same as
  absent), or a map from a target's `x-<lang>-name` token to a dotted
  `module.Type` path. Only `py` can be mapped today.
- A marked model is a closed object with members: `type: object`, non-empty
  `properties`, `additionalProperties: false`. Every optional member is
  nullable.
- **Without a mapping for a target, the target emits the model**, exactly as if
  it were unmarked. The generator assumes no SDK type.
- With a `py` mapping, Python replaces the model's class with an alias to the
  mapped type and imports its module; the converter, and therefore the wire,
  stay the model's.
- An inline object marked as the reference is hoisted into a model of its own,
  as every inline object is, and the hoisted model carries the marker.

## Support decision

**Support: yes.** Accepted by every target; Python emits a mapped type when the
contract supplies one.

Why the contract marks it: a stream reference is a type the contract shares
between operations, the way a [[cursor]] token type is. The operation that
starts a stream returns it and the read and the append take it, and a caller
wants the value it holds to be the one its SDK opens, not a wire copy it has to
convert by hand.

Why the contract, not the generator, names the type: no published SDK carries a
stream reference type yet, and the module it will live in is the SDK's
decision. Naming it in the contract keeps the generator from hardcoding a path
that may not exist, and lets a contract target whichever SDK it is written for.

The defining choices (citing [[PRINCIPLES.md]]):
- **The wire is the model's (P1).** The model's converter is emitted and owns
  the wire form: it builds the mapped type from the declared members by keyword
  and reads them back by attribute. A mapped and an unmapped target agree on the
  wire.
- **No dependency by default (P4).** An unmapped reference adds no import. A
  mapped one imports only the module the contract names; P4 holds when that
  module is in the Temporal SDK closure, which is the contract author's choice.
- **Closed, declared members (P7.1).** A mapped type is built from the members
  by name, so the model has to declare them, and a member the type does not know
  could only be dropped, so the object has to be closed.
- **Nullable optional members.** An SDK that serializes its own reference
  writes `null` for every member it leaves unset, and a generated decoder
  refuses an explicit `null` on a member the contract did not make nullable.
  Requiring `oneOf: [{type: ...}, {type: "null"}]` on each optional member keeps
  the SDK's encoding and the generated one interchangeable.

**Limitation.** The loader cannot see the mapped type, so it does not check that
each member's Python name (its snake-cased wire name, or `x-py-name`) is a field
of it. A mismatch is a type error in the generated module, caught by the type
checker rather than the loader.

## Type mapping

| Target | Emitted for a marked model |
|---|---|
| Python, with `py: pkg.mod.Type` | `StreamRef: typing.TypeAlias = pkg.mod.Type`, with `import pkg.mod`; the transfer type converter is unchanged |
| Python, unmapped | the wire model, unchanged |
| Go, TypeScript, Java, .NET | the wire model, unchanged |

The Python alias keeps the model's name, so a member typed by the model, an
operation whose input or output is the model, and the [[http-caller]] all name
it unchanged.

## Property-testing matrix

### Accepted (positive)

| Case | Asserted |
|---|---|
| Mapped for `py`, returned and taken by operations | Python alias and import, converter kept, member typed by the alias |
| `true`, no mapping | Python emits the model's class; nothing imported |
| `--client` with a mapping | the caller returns the mapped type |
| Go, TypeScript, Java | wire model unchanged, nothing leaked |
| Marked inline object | hoisted model carries the marker |

### Rejected at load time (negative)

| Case | Example |
|---|---|
| Neither `true` nor a map | `x-nexus-stream-ref: "yes"` |
| Empty map | `x-nexus-stream-ref: {}` |
| A target without a mapping | `{ go: example.StreamRef }` |
| Not a `module.Type` path | `{ py: StreamRef }` |
| Not an object with members | `type: string` |
| Open object | no `additionalProperties: false` |
| Plain optional member | `run_id: { type: string }` not in `required` |

### Runtime fixtures

`tests/http_caller_toolchain.rs` maps the reference to a type the driver
defines, and checks that the emitted converter builds and reads that type,
accepts an SDK's own `null`s, and that the caller hands the type back from an
operation.

## Interactions

- [[cursor]]: a read typically takes the reference beside a token.
- [[http-caller]]: returns the mapped type for an operation whose output is the
  reference.

## See also

[[cursor]], [[http-caller]], [[PRINCIPLES.md]].
