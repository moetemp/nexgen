import "@temporalio/proto";

// TEMPORARY: Remove once the TypeScript SDK exports these models.
// tests/setup-protos.ts supplies the real runtime constructors from the same
// checked-in descriptor.
interface ProtoConstructor<T> {
  create(properties?: T): T;
}

declare module "@temporalio/proto" {
  namespace temporal.api.notificationservice.v1 {
    const OnCompleteRequest: ProtoConstructor<IOnCompleteRequest>;
    const OnCompleteResponse: ProtoConstructor<IOnCompleteResponse>;

    interface IOnCompleteRequest {
      result?: "success" | "failure";
      success?: temporal.api.common.v1.IPayload | null;
      failure?: temporal.api.failure.v1.IFailure | null;
      sourceContext?: temporal.api.common.v1.IPayload | null;
    }

    interface IOnCompleteResponse {}
  }
}
