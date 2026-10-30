import { submit } from "../../wit/target-sources/workflow/index.ts";
import { Job } from "../../wit/target-sources/workflow/resources.ts";

export async function targetSourcesCaller(): Promise<void> {
  const handle = await submit({ id: "id", client: "supplied" });
  await handle.result();
  await new Job("job").restart();
}

// The type checker covers the client-only source: the request has a field for it.
async function clientOnlySourceIsRequired(): Promise<void> {
  // @ts-expect-error The request requires `client`.
  await submit({ id: "id" });
}
