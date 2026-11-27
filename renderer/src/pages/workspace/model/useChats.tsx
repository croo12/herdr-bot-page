import { useQuery } from "@tanstack/react-query";
import type { ChatSummary } from "#shared/api/index.tsx";
import { useStoreContext } from "./store/StoreProvider.tsx";
import { chatsQuery } from "./store/queries.tsx";

const emptyChats: readonly ChatSummary[] = [];
const byActivity = (chats: ChatSummary[]) =>
  chats
    .filter((chat) => chat.kind !== "direct" || chat.includesUser)
    .sort((left, right) => right.lastActivityAt - left.lastActivityAt);

export function useChatsQuery() {
  const { client } = useStoreContext();
  return useQuery({ ...chatsQuery(client), select: byActivity });
}

export function useChats(): readonly ChatSummary[] {
  return useChatsQuery().data ?? emptyChats;
}
