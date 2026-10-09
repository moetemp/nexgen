# `x-nexus-stream-ref`

Source: **not** a JSON Schema 2020-12 keyword. A Nexus extension on a `$defs`
model, in the `x-` family the generator already admits. It marks the model as
the **stream reference**: the value an operation returns to hand a stream to its
caller, and that later operations take to name the stream again. A target with
a stream SDK emits the SDK's own reference type in place of the wire model.
Like [[services]], this note adapts the `features/<keyword>.md` skeleton.

## Spec summary

```yaml
$defs:
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
```

- The value is a boolean; `false` is the same as absent.
- A marked model is a closed object with members: `type: object`, non-empty
  `properties`, `additionalProperties: false`. Every optional member is
  nullable.
- The members are the reference's **wire form**, and every target still has a
  model for them. Python replaces the model's class with the SDK type; the
  converter, and therefore the wire, stay the model's.
- An inline object marked as the reference is hoisted into a model of its own,
  as every inline object is, and the hoisted model carries the marker.

## Support decision

**Support: yes.** Accepted by every target; Python emits the SDK type.

Why the contract marks it: a stream reference is a type the contract shares
between operations, the way a [[cursor]] token type is. The operation that
starts a stream returns it and the read and the append take it, and a caller
wants the value it holds to be the one its SDK opens, not a wire copy it has to
convert by hand.

The defining choices (citing [[PRINCIPLES.md]]):
- **The wire is the model's (P1).** The model's own converter is emitted and
  owns the wire form: it builds the SDK type from the declared members by
  keyword and reads them back by attribute. Two targets, one wire.
- **Temporal SDK only (P4).** The emitted alias names a Temporal SDK type, which
  P4 admits. No other dependency is added.
- **Closed, declared members (P7.1).** The SDK type is built from the members by
  name, so the model has to declare them, and a member the SDK type does not
  know could only be dropped, so the object has to be closed.
- **Nullable optional members.** An SDK that serializes its own reference
  writes `null` for every member it leaves unset, and a generated decoder
  refuses an explicit `null` on a member the contract did not make nullable.
  Requiring `oneOf: [{type: ...}, {type: "null"}]` on each optional member keeps
  the SDK's encoding and the generated one interchangeable.

## Type mapping

| Target | Emitted for a marked model |
|---|---|
| Python | `StreamRef: typing.TypeAlias = temporalio.contrib.streams.StreamRef`, with `import temporalio.contrib.streams`; the transfer type converter is unchanged |
| Go, TypeScript, Java, .NET | the wire model, unchanged |

The Python alias keeps the model's name, so a member typed by the model, an
operation whose input or output is the model, and the [[http-caller]] all name
it unchanged.

The member names are a contract with the SDK type: each member's Python name
(its snake-cased wire name, or `x-py-name`) must be a field of
`temporalio.contrib.streams.StreamRef`. The loader does not check that; a
mismatch is a type error in the generated module.

## Property-testing matrix

### Accepted (positive)

| Case | Asserted |
|---|---|
| Marked model returned and taken by operations | Python alias, converter kept, member typed by the alias |
| `--client` | the caller returns the SDK type |
| Go, TypeScript, Java | wire model unchanged, nothing leaked |
| Marked inline object | hoisted model carries the marker |

### Rejected at load time (negative)

| Case | Example |
|---|---|
| Not a boolean | `x-nexus-stream-ref: "yes"` |
| Not an object with members | `type: string` |
| Open object | no `additionalProperties: false` |
| Plain optional member | `run_id: { type: string }` not in `required` |

### Runtime fixtures

`tests/http_caller_toolchain.rs` stands in a `temporalio.contrib.streams`
module and checks that the emitted converter builds and reads the SDK type,
accepts the SDK's own `null`s, and that the caller hands the SDK type back from
an operation.

## Interactions

- [[cursor]]: a read typically takes the reference beside a token.
- [[http-caller]]: returns the SDK type for an operation whose output is the
  reference.

## See also

[[cursor]], [[http-caller]], [[PRINCIPLES.md]].
