/** Copies request, response and event payloads across the test transport. */
import type { AppApiTransport } from "../../src/shared/api/protocol/client.tsx";
import type { AppApiEvent } from "../../src/shared/api/protocol/types.tsx";

export interface AppApiServerLike {
  call(method: string, params: unknown): Promise<unknown>;
  subscribe(onEvent: (event: AppApiEvent) => void): () => void;
}

export function createInMemoryTransport(server: AppApiServerLike): AppApiTransport {
  return {
    async call(method, params) {
      const result = await server.call(method, structuredClone(params));
      return structuredClone(result);
    },
    subscribe(onEvent) {
      return server.subscribe((event) => onEvent(structuredClone(event)));
    },
  };
}
