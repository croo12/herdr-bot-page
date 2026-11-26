import { useCallback, useSyncExternalStore } from "react";
import { useStoreContext } from "./store/StoreProvider.tsx";

export function useActiveChat() {
  const { store } = useStoreContext();
  const activeChatId = useSyncExternalStore(store.subscribe, () => store.getState().activeChatId);
  const setActiveChatId = useCallback((chatId: string | null) => {
    const state = store.getState();
    if (state.activeChatId !== chatId) store.setState({ ...state, activeChatId: chatId });
  }, [store]);
  return { activeChatId, setActiveChatId };
}
