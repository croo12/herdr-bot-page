/** In-memory API transport for the browser demo dataset. */
import { AppApiError } from "#shared/api/index.tsx";
import type {
  AppApiEvent,
  AppApiEventType,
  AppApiTransport,
  AppSettings,
  ChatSummary,
  ReasoningEffort,
  TranscriptEntry,
} from "#shared/api/index.tsx";
import { DEMO_CHATS, DEMO_TRANSCRIPTS } from "./demo-data.tsx";

function clone<T>(value: T): T {
  return structuredClone(value);
}

export function createFakeAppApi(): AppApiTransport {
  const chats = new Map<string, ChatSummary>(DEMO_CHATS.map((chat) => [chat.id, clone(chat)]));
  const transcripts = new Map<string, TranscriptEntry[]>(
    Object.entries(DEMO_TRANSCRIPTS).map(([id, entries]) => [id, clone([...entries])]),
  );
  let settings: AppSettings = {
    defaultWorkingFolder: null,
    defaultProvider: null,
    defaultPermissionMode: "ask",
    terminal: null,
  };

  let eventSeq = 0;
  const listeners = new Set<(event: AppApiEvent) => void>();

  function emit(type: AppApiEventType, payload: unknown): void {
    eventSeq += 1;
    for (const listener of listeners) listener({ seq: eventSeq, type, payload } as AppApiEvent);
  }

  function requireChat(chatId: string): ChatSummary {
    const chat = chats.get(chatId);
    if (chat == null) throw new AppApiError("unknown_chat", `no such chat: ${chatId}`);
    return chat;
  }

  function putChat(next: ChatSummary): ChatSummary {
    chats.set(next.id, next);
    emit("chat.changed", next);
    return next;
  }

  function nextEntrySeq(chatId: string): number {
    const entries = transcripts.get(chatId) ?? [];
    return (entries[entries.length - 1]?.seq ?? 0) + 1;
  }

  function setRoomMembers(chat: ChatSummary, memberIds: readonly string[]) {
    for (const entry of roomEntries(chat.id)) {
      const task = entry.task;
      if (task?.assigneeId && task.status !== "completed" && !memberIds.includes(task.assigneeId)) {
        updateTask(chat.id, entry.id, (current) => ({
          ...current,
          task: { ...task, assigneeId: null, updatedAt: Date.now() },
        }));
      }
    }
    return putChat({ ...chat, memberIds });
  }

  function roomEntries(chatId: string) {
    if (requireChat(chatId).kind !== "room")
      throw new AppApiError("invalid_args", "Tasks belong to a room");
    return transcripts.get(chatId) ?? [];
  }

  function updateTask(
    chatId: string,
    entryId: string,
    update: (entry: TranscriptEntry) => TranscriptEntry,
  ) {
    const entries = roomEntries(chatId);
    const current = entries.find((entry) => entry.id === entryId);
    if (!current) throw new AppApiError("invalid_args", "Unknown message");
    const entry = update(current);
    if (entry !== current) {
      transcripts.set(
        chatId,
        entries.map((item) => (item.id === entryId ? entry : item)),
      );
      emit("transcript.entryUpdated", { chatId, entry });
    }
    return entry;
  }

  function blankChat(
    overrides: Partial<ChatSummary> & Pick<ChatSummary, "id" | "kind" | "name">,
  ): ChatSummary {
    const now = Date.now();
    return {
      description: "",
      title: "",
      avatarShape: null,
      avatarColor: null,
      createdAt: now,
      updatedAt: now,
      isRunning: false,
      currentActivity: null,
      lastMessagePreview: null,
      lastMessageId: null,
      hasUnread: false,
      unreadCount: 0,
      lastViewedAt: now,
      lastActivityAt: now,
      lastReadSeq: 0,
      lastIncomingSeq: 0,
      lastMessageAt: now,
      awaitingUserResponse: null,
      notifyOnUpdatesEnabled: true,
      isHiddenFromSidebar: false,
      isGroup: overrides.kind === "room",
      memberIds: [],
      bot: null,
      ...overrides,
    };
  }

  const methods: Record<string, (params: any) => unknown> = {
    "chat.openDirect": ({ botId }: { botId: string }) => {
      const bot = requireChat(botId);
      const existing = [...chats.values()].find(
        (chat) => chat.kind === "direct" && !chat.archived && chat.memberIds.includes(botId),
      );
      if (existing) return existing;
      const dm = blankChat({
        id: crypto.randomUUID(),
        kind: "direct",
        name: bot.name,
        memberIds: [botId],
        includesUser: true,
      });
      chats.set(dm.id, dm);
      emit("chat.listChanged", [...chats.values()]);
      return dm;
    },
    "task.list": ({ chatId }) => roomEntries(chatId).filter((entry) => entry.task),
    "task.run": () => {
      throw new AppApiError("bot_not_ready", "Task execution requires the desktop app.");
    },
    "task.track": ({ chatId, entryId, assigneeId, criteria }) =>
      updateTask(chatId, entryId, (entry) => {
        const trimmed = typeof criteria === "string" ? criteria.trim() : "";
        if (!trimmed || entry.kind !== "message" || !String(entry.content ?? "").trim())
          throw new AppApiError("invalid_args", "A goal and completion criteria are required");
        if (entry.task) {
          if (entry.task.assigneeId === assigneeId && entry.task.criteria === trimmed) return entry;
          throw new AppApiError("invalid_args", "This message already tracks a task");
        }
        if (assigneeId != null && !requireChat(chatId).memberIds.includes(assigneeId))
          throw new AppApiError("invalid_args", "Choose a room member");
        return {
          ...entry,
          task: { assigneeId, criteria: trimmed, status: "open", updatedAt: Date.now() },
        };
      }),
    "task.assign": ({ chatId, entryId, assigneeId, previousAssigneeId }) =>
      updateTask(chatId, entryId, (entry) => {
        const task = entry.task;
        if (!task) throw new AppApiError("invalid_args", "Message has no task");
        if (task.assigneeId === assigneeId) return entry;
        if (task.assigneeId !== previousAssigneeId || task.status === "completed")
          throw new AppApiError("invalid_args", "Task has changed");
        if (assigneeId !== null && !requireChat(chatId).memberIds.includes(assigneeId))
          throw new AppApiError("invalid_args", "Choose a room member");
        return { ...entry, task: { ...task, assigneeId, updatedAt: Date.now() } };
      }),
    "task.setStatus": ({ chatId, entryId, status, previousStatus }) =>
      updateTask(chatId, entryId, (entry) => {
        if (!entry.task || !["open", "in_progress", "completed"].includes(status))
          throw new AppApiError("invalid_args", "Invalid task status");
        if (status === entry.task.status) return entry;
        if (previousStatus !== entry.task.status)
          throw new AppApiError("invalid_args", "Task status has changed");
        return { ...entry, task: { ...entry.task, status, updatedAt: Date.now() } };
      }),
    "chat.list": () => [...chats.values()],
    "chat.search": (params: { query?: string }) => {
      const needle = (params.query ?? "").toLowerCase();
      return [...chats.values()].filter(
        (chat) =>
          needle.length === 0 ||
          chat.name.toLowerCase().includes(needle) ||
          chat.description.toLowerCase().includes(needle),
      );
    },
    "chat.updateProfile": (params) => {
      const chat = requireChat(params.chatId);
      return putChat({
        ...chat,
        name: params.name ?? chat.name,
        description: params.description ?? chat.description,
        title: params.title ?? chat.title,
        avatarShape: params.avatarShape === undefined ? chat.avatarShape : params.avatarShape,
        avatarColor: params.avatarColor === undefined ? chat.avatarColor : params.avatarColor,
      });
    },
    "chat.setUnread": (params) => {
      const chat = requireChat(params.chatId);
      putChat({ ...chat, hasUnread: params.isUnread, unreadCount: params.isUnread ? 1 : 0 });
    },
    "chat.markRead": (params) => {
      const chat = requireChat(params.chatId);
      putChat({ ...chat, hasUnread: false, unreadCount: 0, lastReadSeq: params.throughSeq });
      return { lastReadSeq: params.throughSeq };
    },
    "chat.setHidden": (params) => {
      const chat = requireChat(params.chatId);
      putChat({ ...chat, isHiddenFromSidebar: params.isHidden });
    },
    "chat.setNotifyOnUpdates": (params) => {
      const chat = requireChat(params.chatId);
      return putChat({ ...chat, notifyOnUpdatesEnabled: params.isEnabled });
    },
    "chat.delete": (params: { chatIds: readonly string[] }) => {
      for (const id of params.chatIds) chats.delete(id);
      for (const room of chats.values()) {
        if (room.kind === "room" && room.memberIds.some((id) => params.chatIds.includes(id))) {
          setRoomMembers(
            room,
            room.memberIds.filter((id) => !params.chatIds.includes(id)),
          );
        }
      }
      emit("chat.listChanged", [...chats.values()]);
      return { deletedIds: [...params.chatIds] };
    },

    "transcript.page": (params: { chatId: string; limit?: number; beforeSeq?: number }) => {
      const entries = transcripts.get(params.chatId) ?? [];
      const windowed =
        params.beforeSeq == null
          ? entries
          : entries.filter((entry) => entry.seq < params.beforeSeq!);
      const limit = params.limit ?? 200;
      const page = windowed.slice(Math.max(0, windowed.length - limit));
      const hasMore = windowed.length > page.length;
      return { entries: page, nextBeforeSeq: hasMore ? page[0]?.seq : undefined };
    },
    "transcript.thread": (params: { chatId: string; rootId: string }) => {
      const entries = transcripts.get(params.chatId) ?? [];
      const ids = new Set([params.rootId]);
      const thread = entries.filter((entry) => {
        if (typeof entry.replyTo === "string" && ids.has(entry.replyTo)) ids.add(entry.id);
        return ids.has(entry.id);
      });
      return { entries: thread };
    },

    "message.send": (params: {
      chatId: string;
      text: string;
      richText?: string;
      replyTo?: string;
      attachmentPaths?: readonly string[];
      clientNonce?: string;
    }) => {
      const chat = requireChat(params.chatId);
      const entries = transcripts.get(params.chatId) ?? [];
      // Use the same attachment lines and optional metadata as a persisted transcript entry.
      const content =
        params.attachmentPaths != null && params.attachmentPaths.length > 0
          ? `${params.text}\n\n${params.attachmentPaths.map((path) => `(attached: ${path})`).join("\n")}`
          : params.text;
      const entry: TranscriptEntry = {
        id: `demo-sent-${Date.now()}`,
        kind: "message",
        seq: nextEntrySeq(params.chatId),
        timestampMs: Date.now(),
        content,
        author: { id: "user", name: "Me" },
        ...(params.richText == null ? {} : { richText: params.richText }),
        ...(params.replyTo == null ? {} : { replyTo: params.replyTo }),
      };
      transcripts.set(params.chatId, [...entries, entry]);
      emit("transcript.entryAppended", { chatId: params.chatId, entry });
      putChat({
        ...chat,
        lastMessagePreview: params.text,
        lastMessageAt: entry.timestampMs,
        lastActivityAt: entry.timestampMs,
      });
      return { entryId: entry.id };
    },
    "message.react": (params: { chatId: string; entryId: string; emoji: string }) => {
      const entries = transcripts.get(params.chatId) ?? [];
      const index = entries.findIndex((entry) => entry.id === params.entryId);
      if (index === -1) return { entry: null };
      const entry = entries[index]!;
      const reactions = (entry as { reactions?: { emoji: string; by: string }[] }).reactions ?? [];
      const already = reactions.some((r) => r.emoji === params.emoji && r.by === "me");
      const nextReactions = already
        ? reactions.filter((r) => !(r.emoji === params.emoji && r.by === "me"))
        : [...reactions, { emoji: params.emoji, by: "me" }];
      const nextEntry = { ...entry, reactions: nextReactions };
      const nextEntries = [...entries];
      nextEntries[index] = nextEntry;
      transcripts.set(params.chatId, nextEntries);
      emit("transcript.entryUpdated", { chatId: params.chatId, entry: nextEntry });
      return { entry: nextEntry };
    },
    "message.search": (params: { query?: string }) => {
      const needle = (params.query ?? "").toLowerCase();
      if (needle.length === 0) return [];
      const hits: unknown[] = [];
      for (const [chatId, entries] of transcripts) {
        const chat = chats.get(chatId);
        for (const entry of entries) {
          const record = entry as Record<string, unknown>;
          const message = record.message as { content?: string } | undefined;
          const text = String(record.content ?? message?.content ?? "");
          if (text.toLowerCase().includes(needle)) {
            const author = record.author as { name?: string } | undefined;
            hits.push({
              chatId,
              chatName: chat?.name ?? chatId,
              entryId: entry.id,
              role: entry.kind === "message" ? "user" : "assistant",
              author: author?.name ?? "",
              timestampMs: entry.timestampMs,
              snippet: text.slice(0, 120),
            });
          }
        }
      }
      return hits.slice(0, 50);
    },

    "bot.create": (params) => {
      const id = params.id ?? String(params.name).toLowerCase().replace(/\s+/g, "-");
      const chat = blankChat({
        id,
        kind: "bot",
        name: params.name,
        description: params.description ?? "",
        avatarShape: params.avatarShape ?? null,
        avatarColor: params.avatarColor ?? null,
        bot: {
          provider: params.provider ?? "claude",
          status: "idle",
          paneId: null,
          cwd: params.cwd ?? null,
          adopted: params.adoptPaneId != null,
          permissionMode: params.permissionMode ?? "ask",
          model: params.model ?? null,
          reasoningEffort: params.reasoningEffort ?? null,
          onboarding: null,
          pendingPrompt: null,
        },
      });
      chats.set(id, chat);
      emit("chat.listChanged", [...chats.values()]);
      return { chat };
    },
    "bot.quickCreate": (params: {
      requestId: string;
      provider: string;
      model?: string;
      reasoningEffort?: ReasoningEffort;
      name?: string;
      avatarColor?: string;
    }) =>
      methods["bot.create"]!({
        name: params.name ?? "New Bot",
        id: `bot-${params.requestId}`,
        provider: params.provider,
        model: params.model,
        reasoningEffort: params.reasoningEffort,
        avatarColor: params.avatarColor,
      }),
    "bot.retrySetup": (params: { botId: string }) => ({ chat: requireChat(params.botId) }),
    "bot.updateLaunchSettings": (params) => {
      const chat = requireChat(params.botId);
      const bot = chat.bot!;
      return {
        chat: putChat({
          ...chat,
          bot: {
            ...bot,
            cwd: params.cwd ?? bot.cwd,
            // Mirrors the real host: an absent `permissionMode` keeps the bot's own mode, an explicit
            // `null` resets to the (fake) effective default ("ask", same as `bot.defaults()` below).
            permissionMode:
              "permissionMode" in params ? (params.permissionMode ?? "ask") : bot.permissionMode,
            model: "model" in params ? (params.model ?? null) : bot.model,
            reasoningEffort:
              "reasoningEffort" in params ? (params.reasoningEffort ?? null) : bot.reasoningEffort,
          },
        }),
      };
    },
    "bot.focus": () => {},
    "bot.listAdoptable": () => [],
    "bot.listModels": () => ({
      models: [{ id: "sonnet", label: "Sonnet" }],
      source: "installed-cli",
    }),
    "bot.defaults": () => ({
      cwd: "/Users/ray/work/shop",
      provider: "claude",
      permissionMode: "ask",
      homeDir: "/Users/ray",
    }),

    "room.create": (params: {
      name: string;
      description?: string;
      memberIds: readonly string[];
      requestId?: string;
    }) => {
      const id = `room-${params.requestId ?? Date.now()}`;
      const chat = blankChat({
        id,
        kind: "room",
        name: params.name,
        description: params.description ?? "",
        memberIds: params.memberIds,
      });
      chats.set(id, chat);
      emit("chat.listChanged", [...chats.values()]);
      return { chat };
    },
    "room.setMembers": (params: { roomId: string; memberIds: readonly string[] }) => {
      const chat = requireChat(params.roomId);
      return setRoomMembers(chat, params.memberIds);
    },

    "prompt.answer": (params: { botId: string; signature: string }) => {
      const chat = requireChat(params.botId);
      if (chat.bot?.pendingPrompt?.signature !== params.signature)
        throw new AppApiError("prompt_changed", "the prompt has moved on");
      return {
        chat: putChat({
          ...chat,
          awaitingUserResponse: null,
          bot: { ...chat.bot, status: "idle", pendingPrompt: null },
        }),
      };
    },

    "directory.list": (params: { path?: string }) => ({
      exists: (params.path ?? "").length > 0,
      isHome: false,
      entries: [],
    }),

    "settings.get": () => ({ ...settings }),
    "settings.update": (params: Partial<AppSettings>) => {
      settings = {
        defaultWorkingFolder:
          params.defaultWorkingFolder !== undefined
            ? params.defaultWorkingFolder
            : settings.defaultWorkingFolder,
        defaultProvider:
          params.defaultProvider !== undefined ? params.defaultProvider : settings.defaultProvider,
        defaultPermissionMode:
          params.defaultPermissionMode !== undefined
            ? params.defaultPermissionMode
            : settings.defaultPermissionMode,
        terminal: params.terminal !== undefined ? params.terminal : settings.terminal,
      };
      return { ...settings };
    },

    "identity.get": () => ({ userName: "Ray" }),
    "host.status": () => ({
      home: "/demo/.herdr-bot",
      session: "herdr-bot-demo",
      bots: [...chats.values()].filter((c) => c.kind === "bot").length,
      rooms: [...chats.values()].filter((c) => c.kind === "room").length,
      runningChatIds: [...chats.values()].filter((c) => c.isRunning).map((c) => c.id),
    }),
  };

  return {
    call: async (method, params) => {
      const handler = methods[method];
      if (handler == null) throw new AppApiError("unknown_method", `unknown method: ${method}`);
      return handler(params);
    },
    subscribe: (onEvent) => {
      listeners.add(onEvent);
      return () => listeners.delete(onEvent);
    },
  };
}
