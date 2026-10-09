# `x-nexus-long-poll`

Source: **not** a JSON Schema 2020-12 keyword. A Nexus extension on an
`operations:` entry, in the `x-` family the generator already admits
(`x-<lang>-name`). It names the two members a caller needs to long-poll a
synchronous operation: the input member that asks the endpoint to park, and the
output member whose emptiness means "nothing yet". Like [[services]], this note
adapts the `features/<keyword>.md` skeleton.

## Spec summary

```yaml
services:
  StreamService:
    operations:
      read:
        x-nexus-long-poll:
          wait-field: waitMs      # input member, type: integer, milliseconds
          result-field: records   # output member, type: array
        input: { $ref: "#/$defs/ReadInput" }
        output: { $ref: "#/$defs/ReadOutput" }
```

- The value is an object with exactly two members, `wait-field` and
  `result-field`, each a non-empty string.
- Both name **wire** members (the authored property names), not emitted
  identifiers, because the authored contract is the only thing both ends of the
  call agree on. Each emitter resolves them through its own `x-<lang>-name`
  mapping.
- The keyword changes no model, no binding and no wire byte. It tells the
  [[http-caller]] to emit one extra method per marked operation.

## Support decision

**Support: yes.** The keyword is accepted by every build and every target; the
loop it drives is emitted by the two targets with an HTTP caller.

Why the contract says this rather than each caller: a long-poll read is a
property of the endpoint (it parks up to a wait the caller sends and answers
early when it has something), and every hand-written caller re-derives the
same loop, usually with the same mistakes: asking the endpoint to park longer
than the transport will wait, spinning when the endpoint answers empty early,
and losing the resume position when the deadline passes.

The defining choices (citing [[PRINCIPLES.md]]):
- **Not gated (P7.1, P1).** A gated keyword would make one contract load under
  one build of the binary and fail under another. The keyword is admitted
  everywhere; a target with no caller accepts it and emits nothing for it.
- **Pinned member shapes (P7.1).** The wait member must be `type: integer` and
  the result member `type: array`, and both models must be
  `properties`-shaped. Pinning the shapes keeps every emitted loop one numeric
  assignment and one length test, and a name that does not resolve is a load
  reject rather than a compile error inside generated code, far from the
  contract that caused it.
- **The wire does not move (P1).** The loop calls the single-shot method; it
  sets the wait member on a copy of the request and reads the result member of
  the answer.

## The loop

Per marked operation the caller gains `<op>_until_<result>` (Python) /
`<Op>Until<Result>` (Go), beside the single-shot method, which stays. The
result member's emitted identifier is the suffix, so the contract is visible at
the call site: `read_until_records`.

| Aspect | Go | Python |
|---|---|---|
| Signature | `ReadUntilRecords(ctx, request ReadInput, deadline time.Duration) (ReadOutput, error)` | `async def read_until_records(self, request: ReadInput, *, deadline: float) -> ReadOutput` |
| Wait assignment | `attempt.WaitMs = milliseconds` when required, `= &milliseconds` when optional | `dataclasses.replace(request, wait_ms=int(wait * 1000))` |
| Done test | `len(answer.Records) > 0` | `if answer.records:` |

Each attempt:
1. **Budget.** The wait sent is the time left before the deadline, clamped to
   90% of the transport's own timeout (the Go `http.Client.Timeout` when set,
   the Python caller's `timeout`) and to at least one millisecond. Asking for
   longer than the transport will wait is a socket timeout dressed as a long
   poll.
2. **Answer.** The first answer whose result member is non-empty is returned.
   When the deadline passes first, the **last answer** is returned, because it
   still carries the caller's resume position.
3. **Pacing.** An empty answer that came back in less than half the wait it
   asked for is followed by a pause, doubling from 50 ms up to 2 s and never
   past the deadline. An endpoint that ignores the wait hint is otherwise asked
   again as fast as the network allows.
4. **Errors.** A status error that is retryable (`429`, `5xx`) is retried with
   the same backoff while the deadline allows; any other error ends the loop.
   In Go a cancelled context ends the loop with the context's error.

## Property-testing matrix

### Accepted (positive)

| Case | Asserted |
|---|---|
| Marked `read` beside an unmarked `append` | loop on `read` only; single shot kept |
| Optional wait member | Go assigns through a pointer |
| Required wait member | Go assigns by value |
| Contract without the keyword | no loop, no `time` import |

### Rejected at load time (negative)

| Case | Example |
|---|---|
| Not an object | `x-nexus-long-poll: true` |
| Unknown member | `deadline: 30` beside the two |
| Missing or empty member | no `result-field` |
| Named member not declared | `result-field: entries` on a model without it |
| Wrong member type | `waitMs: { type: string }` |
| Operation without the side | marked operation with no `output` |
| Model without `properties` | output is `additionalProperties: { type: string }` |

### Runtime fixtures

`tests/http_caller_toolchain.rs` drives both loops against a local endpoint:
two empty answers, a `503`, then records (four attempts, each wait inside the
budget); a deadline that passes on empty answers (the last answer, with its
resume token, comes back); a `400` (returned at once).

## Interactions

- [[http-caller]]: the loop is a method of the caller and posts through its
  single-shot method.
- [[properties]]: member names resolve through `x-<lang>-name`.

## See also

[[http-caller]], [[services]], [[PRINCIPLES.md]].
