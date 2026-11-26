/** Localizes structured notices and displays the original content for plain notices. */
import { getNoticeEvent, getRawText } from "#shared/api/transcript/entry-shapes.tsx";
import type { TranscriptEntry } from "#shared/api/index.tsx";

export type NoticeView =
  | { readonly kind: "renamed"; readonly oldName: string; readonly newName: string }
  | {
      readonly kind: "delegated";
      readonly targetChatId: string;
      readonly targetName: string;
      readonly targetKind: "bot" | "room";
    }
  | { readonly kind: "plain"; readonly text: string };

export function deriveNoticeView(entry: TranscriptEntry): NoticeView {
  const event = getNoticeEvent(entry);
  if (event?.type === "bot-renamed")
    return { kind: "renamed", oldName: event.oldName, newName: event.newName };
  if (event?.type === "bot-message-sent")
    return {
      kind: "delegated",
      targetChatId: event.targetChatId,
      targetName: event.targetName,
      targetKind: event.targetKind,
    };
  return { kind: "plain", text: getRawText(entry) ?? "" };
}
