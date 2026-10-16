# `x-nexus-cursor`

Source: **not** a JSON Schema 2020-12 keyword. A Nexus extension on a model's
`type: string` member, in the `x-` family the generator already admits
(`x-<lang>-name`). It marks the member as an **opaque resume token** and names
the token type, so a caller can hold tokens in a type it cannot confuse with an
ordinary string. Like [[services]], this note adapts the
`features/<keyword>.md` skeleton.

## Spec summary

```yaml
$defs:
  ReadInput:
    type: object
    properties:
      afterToken: { type: string, x-nexus-cursor: StreamCursor }
  ReadOutput:
    type: object
    properties:
      nextToken: { type: string, x-nexus-cursor: StreamCursor }
```

- The value is the token type's name, `^[A-Z][a-zA-Z\d]+$`. Every member that
  names the same type shares it, across models and across input files, so a
  token read from one operation can be passed to another.
- The keyword sits on a model's **direct** property of `type: string`, without
  `contentEncoding`.
- It changes no model member and no wire byte: the member stays a string. The
  token type is for a caller's own variables.

## Support decision

**Support: yes.** Accepted by every target. Go and Python declare the token
type; TypeScript, Java and .NET accept the keyword and declare nothing.

Why a name and not `true`: a stream contract names one token concept in several
places (an append's result, a read's request and response, each record). A
boolean would declare one type per member, and a resume could not cross an
operation.

Why a distinct type and not an alias: an alias lets a caller pass any string
where a token belongs, which is the mistake the keyword exists to prevent. A
`NewType` / named string type keeps the runtime representation a plain string,
so it costs nothing at runtime and nothing on the wire (P1).

The defining choices (citing [[PRINCIPLES.md]]):
- **The wire models do not move (P1, P13.2).** A cursor member stays `str` /
  `*string`. Typing the member itself is the stronger guarantee, but it moves
  every target's committed output and adds a conversion on every validation
  path; that is an open question, not a default.
- **Generator-owned documentation.** The emitted type carries the token rules
  (never parsed, compared or constructed by a caller; pass one back to resume
  strictly after the record it names) rather than the authored description,
  because a contract that words them differently would still have to mean
  this.
- **Reject a keyword that would do nothing (P7.1).** Off a model's direct
  property (an array element, a union branch, a map value, the model root)
  the keyword would declare a type and mark nothing a caller reads its resume
  position from, so it is a load reject that names the position.
- **Collisions reject at load (P15).** Token type names enter the collision
  pass of the scope they are declared in, in Go and Python.

## Type mapping

| Aspect | Go | Python |
|---|---|---|
| Declaration | `type StreamCursor string` | `StreamCursor = typing.NewType("StreamCursor", str)` |
| File | `definitions.go`, once per package | `_definitions.py`, once per package, re-exported from the root `__init__.py` |
| Member type | unchanged (`string` / `*string`) | unchanged (`str` / `str \| None`) |
| Documentation | godoc paragraph with the token rules | docstring with the token rules |

Both targets declare the type in the one module every generated module of a
package shares, so two input files naming one type declare it once.

## Property-testing matrix

### Accepted (positive)

| Case | Asserted |
|---|---|
| Two input files naming one type | one declaration per package, in Go and Python |
| Two token types in one model | both declared, in name order |
| Optional and required members | members keep `string` / `*string`, `str` / `str \| None` |
| TypeScript, Java | nothing declared, nothing leaked |

### Rejected at load time (negative)

| Case | Example |
|---|---|
| Not a string value | `x-nexus-cursor: true` |
| Not a type name | `x-nexus-cursor: streamCursor` |
| Not on a string | `{ type: integer, x-nexus-cursor: StreamCursor }` |
| With `contentEncoding` | `{ type: string, contentEncoding: base64, x-nexus-cursor: ... }` |
| Off a direct property | `items: { type: string, x-nexus-cursor: PageCursor }` |
| Named like another type in scope | `x-nexus-cursor: ReadInput` beside model `ReadInput` |

## Interactions

- [[http-caller]] and [[long-poll]]: a long-poll loop returns the last answer
  at its deadline precisely so the caller keeps its resume token; the token
  type is what the caller holds it in.
- [[properties]]: the member's own name and `x-<lang>-name` are unaffected.

## Open question

Should the wire model's member carry the token type? The wire is unaffected
either way. Typing it costs a cast on each assignment in the generated Python
converters and a conversion on each Go validation path, and it changes every
target's committed output.

## See also

[[long-poll]], [[http-caller]], [[PRINCIPLES.md]].
