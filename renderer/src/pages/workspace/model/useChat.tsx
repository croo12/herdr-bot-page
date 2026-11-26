import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import type { ChatSummary } from "#shared/api/index.tsx";
import { useStoreContext } from "./store/StoreProvider.tsx";
import { chatsQuery } from "./store/queries.tsx";

export function useChat(chatId: string | null): ChatSummary | null {
  const { client } = useStoreContext();
  return useQuery({ ...chatsQuery(client), select: chats => chats.find(chat => chat.id === chatId) }).data ?? null;
}

export function useChatMembers(chat: ChatSummary | null): readonly ChatSummary[] {
  const { client } = useStoreContext();
  const { data: chats } = useQuery(chatsQuery(client));
  return useMemo(() => {
    if (!chat || !chats) return [];
    return chat.memberIds.flatMap(id => chats.find(member => member.id === id) ?? []);
  }, [chat, chats]);
}
