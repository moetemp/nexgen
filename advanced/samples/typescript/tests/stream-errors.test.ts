import { ApplicationFailure } from "@temporalio/common";
import { expect, test } from "vitest";

import {
  StreamError,
  StreamNotFoundError,
  StreamStorageError,
  streamError,
} from "../wit/stream-service/errors.ts";
import { StreamErrorReason } from "../wit/stream-service/models.ts";

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
  const unknown = streamError({ reason: 99 as StreamErrorReason }, "new");

  expect(sequence.constructor).toBe(StreamError);
  expect(unknown.constructor).toBe(StreamError);
  expect(unknown.reason).toBe(99);
});
