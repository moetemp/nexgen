# Test fixtures

- `operation_token_vectors.json` is a copy of
  `crates/streams/nexus/operation_token_vectors.json` in the Temporal Core
  repository (moetemp/sdk-rust `moe/bridge/cn5-stream-callers` at `b0ac595e`).
  Core's token encoder and decoder read the same file, so the generated
  `to_token` and `from_token` are checked against the text Core writes. Copy
  it again when Core changes it.
- `api/notification-service.nexusrpc.yaml`, `api/temporal-proto-models-nexusrpc.yaml`
  and `api/nexus-rpc/temporal-proto-models-nexusrpc.yaml` are copies of
  `nexus/notification-service.nexusrpc.yaml`,
  `nexus/temporal-proto-models-nexusrpc.yaml` and
  `nexus-rpc/temporal-proto-models-nexusrpc.yaml` in the Temporal api
  repository (moetemp/api `moe/bridge/u5b-notification-service-idl` at
  `ff9fe9b9`). They name proto messages with `$<lang>Ref`, so they check that
  nexgen reads the upstream form as the api writes it.
