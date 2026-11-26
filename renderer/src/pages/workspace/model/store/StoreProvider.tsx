/** Loads the workspace snapshot and keeps it in sync with engine events. */
import { createContext, useContext, useEffect, useMemo, useRef } from "react";
import type { ReactNode } from "react";
import { useBridge } from "#shared/api/desktop/BridgeContext.tsx";
import { createRendererClient } from "#shared/api/client.tsx";
import { createDraftStore, type DraftStore } from "#features/compose-message";
import type { AppApiClient } from "#shared/api/index.tsx";
import { createStore } from "#shared/lib/store/create-store.tsx";
import type { Store } from "#shared/lib/store/create-store.tsx";
import { reducer } from "./reducer.tsx";
import type { Action } from "./reducer.tsx";
import { createInitialState } from "./types.tsx";
import type { AppState } from "./types.tsx";

export interface StoreContextValue {
  readonly store: Store<AppState>;
  readonly client: AppApiClient;
  readonly drafts: DraftStore;
}

const StoreContext = createContext<StoreContextValue | null>(null);

export function useStoreContext(): StoreContextValue {
  const value = useContext(StoreContext);
  if (value == null) throw new Error("useStoreContext() called outside <StoreProvider>");
  return value;
}

export function StoreProvider({ children }: { readonly children: ReactNode }) {
  const bridge = useBridge();
  const store = useMemo(() => createStore<AppState>(createInitialState()), []);
  const dispatch = useRef((action: Action) => store.setState(reducer(store.getState(), action))).current;

  const client = useMemo(
    () =>
      createRendererClient(bridge.api, {
        onResyncNeeded: () => dispatch({ type: "resync/requested" }),
      }),
    [bridge, dispatch],
  );

  const drafts = useMemo(() => createDraftStore(), []);

  // Reload the initial snapshot when an event sequence gap triggers a resync.
  const resyncToken = store.getState().resyncToken;
  useEffect(() => {
    let cancelled = false;
    void client.call("chat.list", undefined).then((chats) => {
      if (!cancelled) dispatch({ type: "chats/replaced", chats });
    });
    void client.call("identity.get", undefined).then((identity) => {
      if (!cancelled) dispatch({ type: "identity/loaded", identity });
    });
    void client.call("settings.get", undefined).then((settings) => {
      if (!cancelled) dispatch({ type: "settings/loaded", settings });
    });
    void bridge.pins.get().then((pinnedChatIds) => {
      if (!cancelled) dispatch({ type: "pins/loaded", pinnedChatIds });
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- resyncToken is a trigger, not a dependency value
  }, [client, bridge, dispatch, resyncToken]);

  // Four separate `subscribe` calls (one per event type) rather than one `"*"` listener with a
  // `switch`: `client.subscribe<K>` infers `K` from the literal passed in, so each callback's
  // `event.payload` is already narrowed to that event's real payload type -- a single `"*"` listener
  // would see `AppApiEvent`'s generic (non-discriminated) `payload` union instead and need casts.
  useEffect(() => {
    const unsubscribers = [
      client.subscribe("chat.listChanged", (event) => dispatch({ type: "chats/replaced", chats: event.payload })),
      client.subscribe("chat.changed", (event) => dispatch({ type: "chats/changed", chat: event.payload })),
      client.subscribe("transcript.entryAppended", (event) => dispatch({ type: "transcript/entryAppended", chatId: event.payload.chatId, entry: event.payload.entry })),
      client.subscribe("transcript.entryUpdated", (event) => dispatch({ type: "transcript/entryUpdated", chatId: event.payload.chatId, entry: event.payload.entry })),
    ];
    return () => unsubscribers.forEach((unsubscribe) => unsubscribe());
  }, [client, dispatch]);

  const value: StoreContextValue = useMemo(() => ({ store, client, drafts }), [store, client, drafts]);

  return <StoreContext.Provider value={value}>{children}</StoreContext.Provider>;
}
