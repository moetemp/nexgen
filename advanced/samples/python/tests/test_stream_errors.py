from __future__ import annotations

import temporalio.exceptions
from streamprotos.temporal.api.enums.v1 import stream_pb2
from streamprotos.temporal.api.stream.v1 import message_pb2

from wit.stream_service.errors import (
    StreamError,
    StreamNotFoundError,
    StreamStorageError,
    stream_error,
)
from wit.stream_service.models import StreamErrorDetails, StreamErrorReason

import temporal_support


def test_each_reason_with_a_class_gives_it() -> None:
    not_found = stream_error(
        StreamErrorDetails(reason=StreamErrorReason.StreamErrorReasonNotFound), "gone"
    )
    storage = stream_error(
        StreamErrorDetails(reason=StreamErrorReason.StreamErrorReasonStorage), "down"
    )

    assert type(not_found) is StreamNotFoundError
    assert (not_found.type, not_found.non_retryable) == ("StreamNotFoundError", True)
    # A storage failure can pass on a retry.
    assert type(storage) is StreamStorageError
    assert storage.non_retryable is False


def test_a_reason_with_no_class_gives_the_base_class() -> None:
    sequence = stream_error(
        StreamErrorDetails(reason=StreamErrorReason.StreamErrorReasonSequence), "seq"
    )
    unknown = stream_error(StreamErrorDetails(reason=StreamErrorReason(99)), "new")

    assert type(sequence) is StreamError
    # A reason that this code doesn't know keeps its number.
    assert type(unknown) is StreamError
    assert unknown.reason == 99


def test_the_details_come_from_a_nexus_operation_failure() -> None:
    error = temporalio.exceptions.NexusOperationError(
        "failed",
        scheduled_event_id=1,
        endpoint="__temporal_system",
        service="temporal.api.streamservice.v1.StreamService",
        operation="DescribeStream",
        operation_token="",
    )
    details = StreamErrorDetails(reason=StreamErrorReason.StreamErrorReasonClosed)
    error.__cause__ = temporalio.exceptions.ApplicationError("closed", details)

    assert temporal_support.error_details(error, StreamErrorDetails) == details


def test_the_details_come_from_a_local_activity_error() -> None:
    error = temporalio.exceptions.ActivityError(
        "failed",
        scheduled_event_id=1,
        started_event_id=2,
        identity="worker",
        activity_type="append",
        activity_id="1",
        retry_state=None,
    )
    # The details of an Activity's application error arrive as the proto message.
    error.__cause__ = temporalio.exceptions.ApplicationError(
        "fenced",
        message_pb2.StreamErrorDetails(reason=stream_pb2.STREAM_ERROR_REASON_FENCED),
        type="StreamFencedError",
    )

    assert temporal_support.error_details(
        error, StreamErrorDetails
    ) == StreamErrorDetails(reason=StreamErrorReason.StreamErrorReasonFenced)
