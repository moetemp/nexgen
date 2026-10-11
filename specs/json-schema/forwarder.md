# The forwarding handler (`--forwarder`)

Source: **not** a JSON Schema keyword. A design note for an optional generated
output: a handler base class per service that forwards each operation's JSON
transfer form to one method, for a runtime that serves the contract itself.
Like [[http-caller]], it adapts the `features/<keyword>.md` skeleton.

## Spec summary

- `python` accepts a `--forwarder` flag. With it, each service in a Nexus
  document gets one abstract class, `{Service}Forwarder`, in `forwarder.py`.
- Each operation is a synchronous nexusrpc operation. It turns its input into
  the transfer form through the model's own converter, awaits
  `forward(operation, ctx, transfer)` with the operation's wire name, and turns
  the answer back into the output model through the output's converter. An
  operation with no input forwards `None`, and one with no output returns
  `None`.
- `forward` is abstract. A subclass implements it and declares itself the
  service's handler with `@nexusrpc.handler.service_handler(service=...)`.
  nexusrpc reads that declaration from the class itself, never from a base
  class, so the generated class carries none.
- The flag is gated behind the `advanced` Cargo feature. It adds a file. It
  never changes a model, a binding, or the wire. `{Service}Forwarder` is
  reserved whether or not the flag is set, as the caller's names are.
- With the flag, an operation whose Python name is `forward` is refused,
  since it would replace the class's own method. `x-py-name` renames it.

## Support decision

**Support: yes, for Python, behind `advanced`.**

A runtime that implements the contract in one place, such as Core for every
Core-based SDK, takes the input in the transfer form and answers in it. Each
SDK would otherwise hand-write the same conversion per operation. The class is
that code, generated from the contract, and it holds no protocol rule, so the
rules stay in the runtime. What only the SDK can do, such as an authorizer in
user code or a payload codec, stays in the subclass.

- **The wire does not move (P1).** The class converts through the generated
  models' own transfer form, so the runtime sees the same JSON a caller posts.
- **Other targets.** A target gains a forwarder when an SDK in that language
  needs one.

## Tests

`tests/generate_forwarder.rs` asserts the emitted shape, the void operation,
the flag's absence and the reserved name. It also registers a subclass with
nexusrpc and drives each operation through the generated converters.
