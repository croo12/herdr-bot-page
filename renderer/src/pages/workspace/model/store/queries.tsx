import {
  infiniteQueryOptions,
  queryOptions,
  QueryClient,
  type InfiniteData,
} from "@tanstack/react-query";
import type { AppApiClient, TranscriptPage } from "#shared/api/index.tsx";

export const engineKey = ["engine"] as const;
export const chatsKey = [...engineKey, "chats"] as const;
export const settingsKey = [...engineKey, "settings"] as const;
export const transcriptKey = (chatId: string) => [...engineKey, "transcript", chatId] as const;
export type TranscriptPages = InfiniteData<TranscriptPage, number | null>;

export function createWorkspaceQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { networkMode: "always", staleTime: Infinity, retry: 1 },
      mutations: { networkMode: "always", retry: false },
    },
  });
}

export function chatsQuery(client: AppApiClient) {
  return queryOptions({ queryKey: chatsKey, queryFn: () => client.call("chat.list", undefined) });
}

export function settingsQuery(client: AppApiClient) {
  // Unlike chats, settings have no engine push event when another client writes them.
  return queryOptions({
    queryKey: settingsKey,
    staleTime: 0,
    queryFn: () => client.call("settings.get", undefined),
  });
}

export function transcriptQuery(client: AppApiClient, chatId: string) {
  return infiniteQueryOptions({
    queryKey: transcriptKey(chatId),
    initialPageParam: null as number | null,
    queryFn: ({ pageParam }) =>
      client.call("transcript.page", {
        chatId,
        limit: 200,
        ...(pageParam === null ? {} : { beforeSeq: pageParam }),
      }),
    getNextPageParam: (olderPage) => olderPage.nextBeforeSeq,
  });
}
