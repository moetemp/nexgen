# Test fixtures

- `operation_token_vectors.json` is a copy of
  `crates/streams/nexus/operation_token_vectors.json` in the Temporal Core
  repository (moetemp/sdk-rust `moe/bridge/cn5-stream-callers` at `d34278ab`).
  Core's token encoder and decoder read the same file, so the generated
  `to_token` and `from_token` are checked against the text Core writes. Copy
  it again when Core changes it.
