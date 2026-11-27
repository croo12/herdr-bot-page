import { useToast } from "#shared/ui/index.tsx";
import { useStoreContext } from "./store/StoreProvider.tsx";
import { useActiveChat } from "./useActiveChat.tsx";

export function useOpenDirect() {
  const { client } = useStoreContext();
  const { setActiveChatId } = useActiveChat();
  const toast = useToast();
  return async (botId: string) => {
    try {
      const chat = await client.call("chat.openDirect", { botId });
      setActiveChatId(chat.id);
      return true;
    } catch (error) {
      toast.show(error instanceof Error ? error.message : String(error));
      return false;
    }
  };
}
