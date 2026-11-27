import assert from "node:assert/strict";
import { test } from "node:test";
import { InfiniteQueryObserver, QueryObserver } from "@tanstack/react-query";
import { createAppClient, type AppApiTransport } from "../src/shared/api/protocol/client.tsx";
import type {
  AppApiEvent,
  ChatSummary,
  TranscriptPage,
} from "../src/shared/api/protocol/types.tsx";
import {
  chatsQuery,
  chatsKey,
  createWorkspaceQueryClient,
  transcriptQuery,
  threadKey,
} from "../src/pages/workspace/model/store/queries.tsx";
import {
  resyncEngineQueries,
  subscribeToEngineQueries,
} from "../src/pages/workspace/model/store/query-events.tsx";

function chat(id: string, patch: Partial<ChatSummary> = {}): ChatSummary {
  return {
    id,
    kind: "bot",
    name: id,
    description: "",
    title: "",
    avatarShape: null,
    avatarColor: null,
    createdAt: 0,
    updatedAt: 0,
    isRunning: false,
    currentActivity: null,
    lastMessagePreview: null,
    lastMessageId: null,
    hasUnread: false,
    unreadCount: 0,
    lastViewedAt: 0,
    lastActivityAt: 0,
    lastReadSeq: 0,
    lastIncomingSeq: 0,
    lastMessageAt: 0,
    awaitingUserResponse: null,
    notifyOnUpdatesEnabled: true,
    isHiddenFromSidebar: false,
    isGroup: false,
    memberIds: [],
    bot: null,
    ...patch,
  };
}

function deferred<Data>() {
  let resolve!: (value: Data) => void;
  const promise = new Promise<Data>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function workspace(call: AppApiTransport["call"]) {
  const cache = createWorkspaceQueryClient();
  const listeners = new Set<(event: AppApiEvent) => void>();
  const client = createAppClient(
    {
      call,
      subscribe(listener) {
        listeners.add(listener);
        return () => listeners.delete(listener);
      },
    },
    { onResyncNeeded: () => resyncEngineQueries(cache) },
  );
  const dispose = subscribeToEngineQueries(cache, client);
  return {
    client,
    cache,
    dispose,
    emit(event: AppApiEvent) {
      for (const listener of listeners) listener(event);
    },
  };
}

test("a thread retains a delegation update and result received during its initial read", async () => {
  const snapshot = deferred<TranscriptPage>();
  const { cache, emit, dispose } = workspace(async () => snapshot.promise);
  const key = threadKey("room", "root");
  const loading = cache.fetchQuery({ queryKey: key, queryFn: () => snapshot.promise });
  const tracker = { id: "tracking", kind: "send-message", seq: 2, timestampMs: 0, replyTo: "root" };
  const update = {
    ...tracker,
    delegation: {
      targetChatId: "bot",
      targetName: "Bot",
      state: "waiting" as const,
      promptId: "permission",
    },
  };
  const reply = { id: "result", kind: "send-message", seq: 3, timestampMs: 0, replyTo: "tracking" };
  emit({ seq: 1, type: "transcript.entryUpdated", payload: { chatId: "room", entry: update } });
  emit({ seq: 2, type: "transcript.entryAppended", payload: { chatId: "room", entry: reply } });
  emit({ seq: 3, type: "transcript.entryAppended", payload: { chatId: "other", entry: reply } });
  emit({
    seq: 4,
    type: "transcript.entryAppended",
    payload: { chatId: "room", entry: { ...reply, id: "unrelated", replyTo: "another-root" } },
  });
  snapshot.resolve({ entries: [tracker] });
  await loading;
  assert.deepEqual(cache.getQueryData<TranscriptPage>(key)?.entries, [update, reply]);
  dispose();
  cache.clear();
});

test("clients share the chat snapshot and replay live updates delivered during the initial read", async () => {
  const snapshot = deferred<ChatSummary[]>();
  let calls = 0;
  const { client, cache, emit, dispose } = workspace(async () => {
    calls++;
    return snapshot.promise;
  });
  const options = chatsQuery(client);
  const first = cache.fetchQuery(options);
  const second = cache.fetchQuery(options);
  emit({ seq: 1, type: "chat.changed", payload: chat("bot", { name: "Updated" }) });
  snapshot.resolve([chat("bot"), chat("room")]);
  await Promise.all([first, second]);
  assert.equal(calls, 1);
  assert.deepEqual(
    cache.getQueryData(options.queryKey)?.map((item) => item.name),
    ["Updated", "room"],
  );
  emit({ seq: 2, type: "chat.listChanged", payload: [chat("new")] });
  assert.deepEqual(
    cache.getQueryData(options.queryKey)?.map((item) => item.id),
    ["new"],
  );
  dispose();
  cache.clear();
});

test("a sequence gap reloads active queries and leaves inactive transcripts stale until opened", async () => {
  const reads = deferred<ChatSummary[]>();
  const { client, cache, emit, dispose } = workspace(async () => reads.promise);
  const options = chatsQuery(client);
  cache.setQueryData(chatsKey, [chat("old")]);
  const inactive = transcriptQuery(client, "archived");
  cache.setQueryData(inactive.queryKey, { pages: [{ entries: [] }], pageParams: [null] });
  const observer = new QueryObserver(cache, options);
  const unsubscribe = observer.subscribe(() => {});
  emit({ seq: 1, type: "chat.changed", payload: chat("old") });
  emit({
    seq: 3,
    type: "transcript.entryUpdated",
    payload: {
      chatId: "archived",
      entry: { id: "message", seq: 1, timestampMs: 0, kind: "message" },
    },
  });
  assert.equal(cache.getQueryState(chatsKey)?.fetchStatus, "fetching");
  assert.equal(cache.getQueryState(inactive.queryKey)?.isInvalidated, true);
  reads.resolve([chat("recovered")]);
  await cache.fetchQuery(options);
  assert.equal(cache.getQueryData(options.queryKey)?.[0]?.id, "recovered");
  unsubscribe();
  dispose();
  cache.clear();
});

test("older pages can be retried, preserve concurrent live edits and terminate at the last cursor", async () => {
  const earlier = deferred<TranscriptPage>();
  const latest = { id: "new", kind: "message", seq: 2, timestampMs: 0, content: "before" };
  let attempts = 0;
  const { client, cache, emit, dispose } = workspace(async (_method, params) => {
    if ((params as { beforeSeq?: number }).beforeSeq === undefined)
      return { entries: [latest], nextBeforeSeq: 2 };
    if (++attempts === 1) throw new Error("unavailable");
    return earlier.promise;
  });
  const observer = new InfiniteQueryObserver(cache, {
    ...transcriptQuery(client, "room"),
    retry: false,
  });
  await observer.refetch();
  assert.equal((await observer.fetchNextPage()).isFetchNextPageError, true);
  const pending = observer.fetchNextPage();
  emit({
    seq: 1,
    type: "transcript.entryUpdated",
    payload: { chatId: "room", entry: { ...latest, content: "after" } },
  });
  earlier.resolve({ entries: [{ ...latest, id: "old", seq: 1 }] });
  await pending;
  const data = cache.getQueryData(transcriptQuery(client, "room").queryKey);
  assert.equal(data?.pages[0]?.entries[0]?.content, "after");
  assert.equal(data?.pages[1]?.entries[0]?.id, "old");
  assert.equal(observer.getCurrentResult().hasNextPage, false);
  dispose();
  cache.clear();
});
