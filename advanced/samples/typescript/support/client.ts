import { asyncLocalStorage as activityContext } from "@temporalio/activity";

import { setActivityInfoProvider } from "./streams";

// Workflow code never imports this module, so it can use `@temporalio/activity`.
setActivityInfoProvider(() => activityContext.getStore()?.info);
