import Long from "long";

type Offset = Long | number | null | undefined;

function sameOffset(left: Offset, right: Offset): boolean {
  if (left == null || right == null) {
    return left == null && right == null;
  }
  return Long.fromValue(left).equals(right);
}

/** The next read of the stream-service poll, or `undefined` to stop. */
export function nextReadRequest<
  TRequest extends {
    readonly fromOffset?: Offset;
    readonly start?: unknown;
    readonly wait?: unknown;
    readonly last?: number | null;
  },
>(
  request: TRequest,
  response: {
    readonly records?: readonly unknown[] | null;
    readonly nextOffset?: Offset;
    readonly done?: boolean | null;
  },
): TRequest | undefined {
  if (response.done) {
    return undefined;
  }
  const nextOffset = response.nextOffset;
  // A read that doesn't wait ends at the newest Record.
  if (
    !response.records?.length &&
    !request.wait &&
    (nextOffset == null || sameOffset(nextOffset, request.fromOffset))
  ) {
    return undefined;
  }
  if (nextOffset == null) {
    // `last` reads the newest Records only once.
    return { ...request, last: undefined };
  }
  // `last` takes no `fromOffset`, and a `fromOffset` overrides `start`.
  return { ...request, fromOffset: nextOffset, start: undefined, last: undefined };
}
