type Offset = Uint8Array | null | undefined;

function sameOffset(left: Offset, right: Offset): boolean {
  if (left == null || right == null) {
    return left == null && right == null;
  }
  return (
    left.length === right.length && left.every((byte, index) => byte === right[index])
  );
}

/** The next read of the stream-service poll, or `undefined` to stop. */
export function nextReadRequest<
  TRequest extends { readonly fromOffset?: Offset; readonly wait?: unknown },
>(
  request: TRequest,
  response: {
    readonly records?: readonly unknown[] | null;
    readonly lastOffset?: Offset;
    readonly done?: boolean | null;
  },
): TRequest | undefined {
  if (response.done) {
    return undefined;
  }
  const lastOffset = response.lastOffset;
  // A read that doesn't wait ends at the newest Record.
  if (
    !response.records?.length &&
    !request.wait &&
    (lastOffset == null || sameOffset(lastOffset, request.fromOffset))
  ) {
    return undefined;
  }
  if (lastOffset == null) {
    // The read covered no Record, so the next read starts at the same offset.
    return request;
  }
  // A from-offset is exclusive, so a read after an offset passes that offset.
  return { ...request, fromOffset: lastOffset };
}

/** The read that resumes after `record`, a Record of the page of `request`. */
export function resumeReadRequest<TRequest extends { readonly fromOffset?: Offset }>(
  request: TRequest,
  record: { readonly offset: Uint8Array },
): TRequest {
  // A from-offset is exclusive, so a read after a Record passes that Record's offset.
  return { ...request, fromOffset: record.offset };
}
