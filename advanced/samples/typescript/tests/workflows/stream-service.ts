import { createStream } from "../../wit/stream-service/index.ts";
import { Stream } from "../../wit/stream-service/resources.ts";

export async function streamServiceCaller(): Promise<{
  created: boolean;
  isStream: boolean;
  streamId: string;
}> {
  const result = await createStream({ ref: { id: "stream-1" } });
  return {
    created: result.created,
    isStream: result.stream instanceof Stream,
    streamId: result.stream.stream.id,
  };
}
