/** Structured events decoded by getNoticeEvent for localized transcript notices. */
export type BotSystemEvent =
  | { readonly type: "bot-renamed"; readonly oldName: string; readonly newName: string }
  | { readonly type: "bot-message-sent"; readonly targetChatId: string; readonly targetName: string; readonly targetKind: "bot" | "room" };
