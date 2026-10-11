# `x-nexus-operation-token`

Source: **not** a JSON Schema keyword. A Nexus extension on a model, marking it
as the content of an operation token. Like [[cursor]], it adapts the
`features/<keyword>.md` skeleton.

## Spec summary

- The value is an object with one member, `version-field`, naming the model's
  version member.
- The model is a `type: object` with `properties` and
  `additionalProperties: false`. The version member is required, `type:
  integer`, and carries an integer `const`.
- Python gives the model `to_token()` and the class method `from_token(token)`.
  The token is the transfer form as JSON with sorted keys and no spaces
  (`separators=(",", ":")`), in base64url with no padding.
- `from_token` restores the padding, decodes, and reads the JSON through the
  model's own converter. That converter checks the version member's `const`,
  so a token of another version fails. Every failure is a `ValueError`.
- The other targets emit the model as they would without the keyword.

## Support decision

**Support: yes, for Python.**

An asynchronous Nexus operation's token is opaque to the caller, but the
handler and the caller's own SDK both read it. When the handler runs in one
place, such as Core, and the caller reads the token in a Workflow, two
implementations write and read the same text. A token schema in the contract,
and a token text that no language choice affects, keeps them in step. A Core
encoder is tested against the same vectors as the generated helpers.

- **Closed model.** A member the model doesn't declare would be dropped on
  read and change the token on write, so the model has to be closed.
- **Version.** A `const` the converter already checks makes the version a
  wire rule rather than one each SDK repeats.
- **Other targets.** A target gains the helpers when an SDK in that language
  needs them.

## Tests

`tests/generate_operation_token.rs` asserts the emitted helpers, the imports
they need and nothing else does, and each rule the parser holds the model to.
It also runs the generated helpers in Python against Core's shared vectors
(`tests/fixtures/operation_token_vectors.json`): each token is written byte for
byte and read back, the decode-only tokens read, and every refused token fails,
including another version and an extra member.
