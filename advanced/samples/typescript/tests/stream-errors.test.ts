import "./setup-protos.ts";
import { ApplicationFailure } from "@temporalio/common";
import type { temporal } from "@temporalio/proto";
import { expect, test } from "vitest";

import {
  StreamError,
  StreamNotFoundError,
  StreamStorageError,
  streamError,
} from "../wit/stream-service/both/errors.ts";
import {
  StreamErrorReason,
  streamErrorDetailsTransferTypeConverter,
} from "../wit/stream-service/both/models.ts";

test("gives each reason with a class its class and retryability", () => {
  const notFound = streamError(
    { reason: StreamErrorReason.StreamErrorReasonNotFound },
    "gone",
  );
  const storage = streamError(
    { reason: StreamErrorReason.StreamErrorReasonStorage },
    "down",
  );

  expect(notFound).toBeInstanceOf(StreamNotFoundError);
  expect(notFound).toBeInstanceOf(ApplicationFailure);
  expect([notFound.type, notFound.nonRetryable]).toStrictEqual([
    "StreamNotFoundError",
    true,
  ]);
  expect(notFound.details).toStrictEqual([
    { reason: StreamErrorReason.StreamErrorReasonNotFound },
  ]);
  // A storage failure can pass on a retry.
  expect(storage).toBeInstanceOf(StreamStorageError);
  expect(storage.nonRetryable).toBe(false);
});

test("gives a reason with no class, or an unknown one, the base class", () => {
  const sequence = streamError(
    { reason: StreamErrorReason.StreamErrorReasonSequence },
    "seq",
  );
  // A newer server can send a reason that this code doesn't know.
  const unknown = streamError(
    streamErrorDetailsTransferTypeConverter.fromTransferType({
      reason: 99 as temporal.api.enums.v1.StreamErrorReason,
    }),
    "new",
  );

  expect(sequence.constructor).toBe(StreamError);
  expect(unknown.constructor).toBe(StreamError);
  expect(unknown.reason).toBe(StreamErrorReason.StreamErrorReasonUnspecified);
});
