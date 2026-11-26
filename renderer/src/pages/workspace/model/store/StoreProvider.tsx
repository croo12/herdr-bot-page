import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  type ReactNode,
} from "react";
import { QueryClientProvider, useQuery } from "@tanstack/react-query";
import { useBridge } from "#shared/api/desktop/BridgeContext.tsx";
import { createRendererClient } from "#shared/api/client.tsx";
import { createDraftStore, type DraftStore } from "#features/compose-message";
import type { AppApiClient } from "#shared/api/index.tsx";
import { createStore, type Store } from "#shared/lib/store/create-store.tsx";
import { createWorkspaceQueryClient, engineKey } from "./queries.tsx";
import { resyncEngineQueries, subscribeToEngineQueries } from "./query-events.tsx";

interface WorkspaceUiState {
  readonly activeChatId: string | null;
  readonly pinnedChatIds: readonly string[];
}

interface StoreContextValue {
  readonly store: Store<WorkspaceUiState>;
  readonly client: AppApiClient;
  readonly drafts: DraftStore;
}

const StoreContext = createContext<StoreContextValue | null>(null);

export function useStoreContext(): StoreContextValue {
  const value = useContext(StoreContext);
  if (!value) throw new Error("useStoreContext() called outside <StoreProvider>");
  return value;
}

export function StoreProvider({ children }: { readonly children: ReactNode }) {
  const bridge = useBridge();
  const { cache, value } = useMemo(() => {
    const cache = createWorkspaceQueryClient();
    const client = createRendererClient(bridge.api, {
      onResyncNeeded: () => resyncEngineQueries(cache),
    });
    const store = createStore<WorkspaceUiState>({ activeChatId: null, pinnedChatIds: [] });
    return { cache, value: { client, drafts: createDraftStore(), store } };
  }, [bridge]);
  const { client, store } = value;

  useLayoutEffect(() => subscribeToEngineQueries(cache, client), [cache, client]);
  useEffect(() => {
    let active = true;
    const original = store.getState().pinnedChatIds;
    void bridge.pins.get().then((pinnedChatIds) => {
      if (active && store.getState().pinnedChatIds === original)
        store.setState({ ...store.getState(), pinnedChatIds });
    });
    return () => {
      active = false;
    };
  }, [bridge, store]);

  useQuery(
    { queryKey: [...engineKey, "identity"], queryFn: () => client.call("identity.get", undefined) },
    cache,
  );
  return (
    <QueryClientProvider client={cache}>
      <StoreContext.Provider value={value}>{children}</StoreContext.Provider>
    </QueryClientProvider>
  );
}
