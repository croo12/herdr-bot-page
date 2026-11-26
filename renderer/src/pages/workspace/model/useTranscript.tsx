import { useInfiniteQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { useStoreContext } from "./store/StoreProvider.tsx";
import { transcriptQuery } from "./store/queries.tsx";
import { mergeTranscriptPage } from "./transcript-merge.tsx";

export function useTranscript(chatId: string) {
  const { client } = useStoreContext();
  const query = useInfiniteQuery(transcriptQuery(client, chatId));
  const entries = useMemo(() => {
    const pages = query.data?.pages ?? [];
    // Older pages come last in the cache. Prefer the newest page at overlapping boundaries.
    return mergeTranscriptPage([], pages.slice().reverse().flatMap(page => page.entries));
  }, [query.data]);
  return {
    entries,
    loaded: query.data !== undefined,
    loadingOlder: query.isFetchingNextPage,
    nextBeforeSeq: query.data?.pages.at(-1)?.nextBeforeSeq,
    error: query.error,
    retry: () => query.isFetchNextPageError ? query.fetchNextPage() : query.refetch(),
    loadOlder: () => {
      if (query.hasNextPage && !query.isFetching) void query.fetchNextPage({ cancelRefetch: false });
    },
  };
}

export type TranscriptHandle = ReturnType<typeof useTranscript>;
