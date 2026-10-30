TypeScript resource constructor fields and method parameters now use `Long` when bound to protobuf-backed 64-bit fields, while JSON-only values remain `number`. Conflicting protobuf/JSON bindings are rejected rather than silently losing precision.
TypeScript protobuf transfer converters now return real protobufjs messages for use with the SDK's protobuf-aware payload converter, preserving 64-bit integers across payload serialization.
Resource-to-request bindings now reject mismatched integer widths, and conflicting TypeScript bindings identify both destination field paths.
Mixed JSON/protobuf record nesting is rejected for every target, including model-only modules. Independent JSON and protobuf operations may still share a service.
