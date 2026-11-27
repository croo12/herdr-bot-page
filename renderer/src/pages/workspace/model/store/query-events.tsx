import type { QueryClient, QueryKey } from "@tanstack/react-query";
import type { AppApiClient, ChatSummary, TranscriptEntry } from "#shared/api/index.tsx";
import { applyTranscriptEntry } from "../transcript-merge.tsx";
import {
  chatsKey,
  engineKey,
  transcriptKey,
  chatThreadsKey,
  type TranscriptPages,
} from "./queries.tsx";

export function resyncEngineQueries(cache: QueryClient) {
  // Also restart initial reads, which otherwise reuse their in-flight snapshot.
  void cache.cancelQueries({ queryKey: engineKey });
  void cache.invalidateQueries({ queryKey: engineKey });
}

function patchTranscript(data: TranscriptPages, entry: TranscriptEntry): TranscriptPages {
  const pageIndex = data.pages.findIndex((page) =>
    page.entries.some((existing) => existing.id === entry.id),
  );
  const target = pageIndex < 0 ? 0 : pageIndex;
  return {
    ...data,
    pages: data.pages.map((page, index) =>
      index === target ? { ...page, entries: applyTranscriptEntry(page.entries, entry) } : page,
    ),
  };
}

export function subscribeToEngineQueries(cache: QueryClient, client: AppApiClient) {
  // A snapshot (including fetchNextPage) can finish after a newer engine event.
  // Replay only events received during that fetch once its snapshot is committed.
  const pending = new Map<string, (() => void)[]>();
  const unsubscribeCache = cache.getQueryCache().subscribe((event) => {
    const hash = event.query.queryHash;
    if (event.type === "removed") pending.delete(hash);
    if (event.type !== "updated") return;
    const { action } = event;
    if (action.type === "fetch" || action.type === "error") pending.delete(hash);
    if (action.type !== "success" || action.manual) return;
    const updates = pending.get(hash);
    pending.delete(hash);
    updates?.forEach((apply) => apply());
  });

  function update<Data>(key: QueryKey, change: (current: Data | undefined) => Data | undefined) {
    const query = cache.getQueryCache().find({ queryKey: key, exact: true });
    if (!query) return; // Unopened chats will read the committed data when first opened.
    const apply = () => {
      cache.setQueryData<Data>(key, change);
    };
    if (query.state.fetchStatus === "fetching") {
      const updates = pending.get(query.queryHash) ?? [];
      updates.push(apply);
      pending.set(query.queryHash, updates);
    }
    // An incomplete patch must not mark a missed-event snapshot fresh.
    if (!query.state.isInvalidated) apply();
  }

  const cleanups = [
    unsubscribeCache,
    client.subscribe("chat.listChanged", ({ payload }) =>
      update<ChatSummary[]>(chatsKey, () => [...payload]),
    ),
    client.subscribe("chat.changed", ({ payload }) =>
      update<ChatSummary[]>(chatsKey, (chats) => {
        if (!chats) return undefined;
        return chats.some((chat) => chat.id === payload.id)
          ? chats.map((chat) => (chat.id === payload.id ? payload : chat))
          : [...chats, payload];
      }),
    ),
    client.subscribe(["transcript.entryAppended", "transcript.entryUpdated"], ({ payload }) => {
      const { entry, chatId } = payload;
      for (const query of cache.getQueryCache().findAll({ queryKey: chatThreadsKey(chatId) })) {
        const root = query.queryKey.at(-1);
        update<{ entries: readonly TranscriptEntry[] }>(query.queryKey, (data) => {
          if (!data) return data;
          const inThread =
            entry.id === root ||
            entry.replyTo === root ||
            data.entries.some(
              (existing) => existing.id === entry.id || existing.id === entry.replyTo,
            );
          return inThread ? { entries: applyTranscriptEntry(data.entries, entry) } : data;
        });
      }
      update<TranscriptPages>(
        transcriptKey(payload.chatId),
        (data) => data && patchTranscript(data, payload.entry),
      );
    }),
  ];
  return () => {
    cleanups.forEach((cleanup) => cleanup());
    pending.clear();
  };
}
