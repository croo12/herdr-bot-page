/** Typed calls and event subscriptions for the Tauri and browser demo transports. */
import { toAppApiError } from "./errors.tsx";
import type { AppApiEvent, AppApiEventType, AppApiMethod, AppApiParams, AppApiResult } from "./types.tsx";

export interface AppApiTransport {
  call(method: string, params: unknown): Promise<unknown>;
  /** Delivers every event, of every type, to `onEvent`; returns an unsubscribe function. */
  subscribe(onEvent: (event: AppApiEvent) => void): () => void;
}

export interface AppApiClient {
  call<M extends AppApiMethod>(method: M, params: AppApiParams<M>): Promise<AppApiResult<M>>;
  /** `types` is one event type, a list of them, or "*" for everything. Returns an unsubscribe function. */
  subscribe<K extends AppApiEventType>(types: K | readonly K[] | "*", listener: (event: AppApiEvent<K>) => void): () => void;
  /** The most recent event seq this client has observed, or null before the first event. Exposed mainly
   * for tests/diagnostics -- ordinary callers should rely on the `onResyncNeeded` gap callback instead. */
  lastSeq(): number | null;
}

export interface AppApiClientOptions {
  /** Called when an incoming event's seq is not exactly one more than the last one seen -- a missed
   * event (the transport dropped/reconnected). There is no replay: the caller should re-fetch whatever
   * it cares about (typically `chat.list`, and any open `transcript.page`) rather than trust local state. */
  readonly onResyncNeeded?: () => void;
}

function matches<K extends AppApiEventType>(types: K | readonly K[] | "*", type: AppApiEventType): type is K {
  if (types === "*") return true;
  return Array.isArray(types) ? (types as readonly string[]).includes(type) : types === type;
}

export function createAppClient(transport: AppApiTransport, options: AppApiClientOptions = {}): AppApiClient {
  let seen: number | null = null;

  const listeners = new Set<(event: AppApiEvent) => void>();
  let disconnect: (() => void) | undefined;

  return {
    async call(method, params) {
      try {
        return (await transport.call(method, params)) as never;
      } catch (error) {
        throw toAppApiError(error);
      }
    },
    subscribe(types, listener) {
      const onEvent = (event: AppApiEvent) => {
        if (matches(types, event.type)) listener(event as never);
      };
      listeners.add(onEvent);
      disconnect ??= transport.subscribe(event => {
        const missedEvents = seen !== null && event.seq !== seen + 1;
        seen = event.seq;
        for (const notify of listeners) notify(event);
        if (missedEvents) options.onResyncNeeded?.();
      });
      return () => {
        listeners.delete(onEvent);
        if (listeners.size === 0) {
          disconnect?.();
          disconnect = undefined;
        }
      };
    },
    lastSeq() {
      return seen;
    },
  };
}
