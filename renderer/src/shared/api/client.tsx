/** Creates the renderer client for the active bridge. */
import { createAppClient } from "./index.tsx";
import type { AppApiClient, AppApiTransport } from "./index.tsx";

export interface CreateRendererClientOptions {
  readonly onResyncNeeded?: () => void;
}

export function createRendererClient(transport: AppApiTransport, options: CreateRendererClientOptions = {}): AppApiClient {
  return createAppClient(transport, options);
}
