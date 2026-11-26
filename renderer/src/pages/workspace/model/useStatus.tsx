import { useMemo } from "react";
import {
  presenceMembers,
  sidebarWaitingReason,
  statusDotTone,
  statusPhraseKind,
} from "./status.tsx";
import type {
  PresenceMember,
  SidebarWaitingReason,
  StatusDotTone,
  StatusPhraseKind,
} from "./status.tsx";
import { useChatMembers } from "./useChat.tsx";
import type { ChatSummary } from "#shared/api/index.tsx";

export interface ChatStatus {
  readonly dot: StatusDotTone;
  readonly phrase: StatusPhraseKind;
  readonly sidebarWaitingReason: SidebarWaitingReason;
  /** Non-idle members, in order -- empty for a bot DM (a DM's own status is `phrase`/`dot` above). */
  readonly presence: readonly PresenceMember[];
}

/** Derives everything the sidebar row / header / presence strip need to render a chat's live status
 * from its `ChatSummary` (+, for a room, its resolved member summaries). Pure derivation wrapped as a
 * hook only so it can read `useChatMembers`; the underlying logic (src/pages/workspace/model/status.tsx) is plain and
 * unit-tested directly. */
export function useStatus(chat: ChatSummary | null): ChatStatus {
  const members = useChatMembers(chat);
  return useMemo(
    () => ({
      dot: chat == null ? null : statusDotTone(chat),
      phrase: chat == null ? null : statusPhraseKind(chat),
      sidebarWaitingReason: chat == null ? null : sidebarWaitingReason(chat),
      presence: chat == null || chat.kind !== "room" ? [] : presenceMembers(members),
    }),
    [chat, members],
  );
}
