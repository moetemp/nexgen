# `$<lang>Ref` and the upstream form

Source: **not** a JSON Schema keyword. The upstream nexus-rpc-gen schema
(`schemas/nexus-rpc-gen.json`) lets an operation `input` or `output` name an
existing type per language instead of declaring a shape. The Temporal api
files use it to name proto messages. Like [[operation-token]], this adapts the
`features/<keyword>.md` skeleton.

## Spec summary

- The keywords are `$csharpRef`, `$goRef`, `$javaRef`, `$pythonRef` and
  `$typescriptRef` from the upstream schema, and `$dotnetRef` and `$rubyRef`,
  which the api files also write. `$csharpRef` and `$dotnetRef` both name the
  .NET type, so a node takes one of them.
- A node that has one of them names an existing type. Its other members can be
  `description`, `title` and `$comment` only. A node can't name a type and
  also declare a shape.
- Each value is a non-empty string. `$pythonRef` is a qualified Python name,
  `package.module.Type`.
- Only an operation `input` or `output` can name a type. The keyword anywhere
  else is refused, with the remedy to declare the shape.
- Python output imports the module of the `$pythonRef` name and uses the name
  as the operation's type. It emits no model and no converter. The payload
  converter reads and writes the type.
- Python output refuses an operation that names types but has no `$pythonRef`.
  The other targets refuse any operation that names a type.
- An operation that names a type takes no `x-nexus-*` annotation, because the
  annotations read the members of a shape. The `--client` caller and the
  `--forwarder` class refuse it, because both work with JSON transfer forms.

The upstream form also keys a service by its qualified wire name and an
operation by its wire name. The parser lowers both before any other pass.

- A service key that doesn't match `^[A-Z][a-zA-Z\d]+$`, has no `fqn`, and is
  a dotted name whose last segment matches, becomes that segment. The key
  becomes its `fqn`.
- An operation key that starts uppercase, then has letters and digits, and has
  no `fqn`, becomes its lower camel form. The key becomes its `fqn`.
- Two keys that lower to one name are refused, and the message names both.

Keys that already match keep their meaning, so no file that loaded before
changes its output.

## Support decision

**Support: yes, for Python.**

The api files describe services whose messages are protos. The SDKs already
carry the proto classes, and the payload converter already encodes them, so a
generated model would be a second, different type for the same message. Naming
the class keeps one type per message.

- **One target.** Python is the SDK that needs the bindings today. A target
  resolves its own keyword when an SDK in that language needs it. Until then
  it refuses, so it never emits a model that the file doesn't declare.
- **No shape beside a name.** The upstream generator uses a name for a
  language that has one and the shape for the rest. Two descriptions of one
  type can disagree on the wire, so a node takes one.
- **Lowered names.** The api files are shared with other generators that read
  the upstream form. Lowering in the parser lets nexgen read them as they are,
  and keeps the later passes on one form.
