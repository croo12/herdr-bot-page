import { useState } from "react";
import { Bubble, BubbleContent } from "#shared/ui/shadcn/bubble.tsx";
import { Message, MessageAvatar, MessageContent, MessageHeader } from "#shared/ui/shadcn/message.tsx";
import { useChats } from "../../model/useChats.tsx";
import { resolvePersonaColor, personaColorVar } from "#shared/ui/avatar-colors.tsx";
import { formatClockTime } from "#shared/lib/time/format-time.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { Avatar } from "#shared/ui/index.tsx";
import type { ChatSummary, TranscriptEntry } from "#shared/api/index.tsx";
import { splitAttachments } from "../../model/conversation/attachments.tsx";
import { Attachments } from "./Attachments.tsx";
import { getAuthor, getRawText, getReactions, getReplyTo, getRichText, isUserMessage } from "#shared/api/transcript/entry-shapes.tsx";
import { HoverActions } from "./HoverActions.tsx";
import { MarkdownView } from "./MarkdownView.tsx";
import { MarkerBadges } from "./MarkerBadges.tsx";
import type { MentionCandidate } from "../../model/conversation/mentions.tsx";
import { Reactions } from "./Reactions.tsx";
import { ReplyQuote } from "./ReplyQuote.tsx";
import styles from "./MessageBubble.module.css";

export interface MessageBubbleProps {
  readonly chat: ChatSummary;
  readonly entry: TranscriptEntry;
  /** Hide the repeated avatar/name label when grouped with the row above (group-messages.tsx). */
  readonly grouped: boolean;
  readonly showAuthorLabel: boolean;
  readonly members: readonly MentionCandidate[];
  readonly entriesById: ReadonlyMap<string, TranscriptEntry>;
}

export function MessageBubble({ chat, entry, grouped, showAuthorLabel, members, entriesById }: MessageBubbleProps) {
  const { locale, t } = useT();
  const chats = useChats();
  const [hovered, setHovered] = useState(false);

  const mine = isUserMessage(entry);
  const author = getAuthor(entry);
  const rawText = getRawText(entry) ?? "";
  const { text, attachmentPaths } = splitAttachments(rawText);
  const richText = getRichText(entry);
  const reactions = getReactions(entry);
  const replyToId = getReplyTo(entry);
  const quotedEntry = replyToId != null ? (entriesById.get(replyToId) ?? null) : null;

  const authorBot = !mine && author != null ? chats.find((c) => c.id === author.id) : undefined;
  const personaColor = resolvePersonaColor(authorBot?.avatarColor ?? chat.avatarColor);

  return (
    <Message
      align={mine ? "end" : "start"}
      className="px-5 py-px"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {!mine && (
        <MessageAvatar className="size-7 min-w-7 bg-transparent">{!grouped && <Avatar kind="bot" color={personaColor} name={author?.name ?? ""} size="sm" />}</MessageAvatar>
      )}

      <MessageContent className={mine ? "max-w-[60%] items-end gap-1" : "max-w-[78%] gap-1"}>
        {!mine && !grouped && showAuthorLabel && (
          <MessageHeader style={{ color: personaColorVar(personaColor) }}>
            {author?.name}
          </MessageHeader>
        )}

        <MarkerBadges entry={entry} />

        <Bubble align={mine ? "end" : "start"} variant={mine ? "default" : "secondary"} className="max-w-full">
          <BubbleContent>
            {quotedEntry != null && <ReplyQuote quotedEntry={quotedEntry} missingLabel="" mine={mine} />}
            {entry.permissionDecision && <strong>{t(`conversation.decision.${entry.permissionDecision}`)}</strong>}
            <MarkdownView text={text} richText={richText} members={members} />
            <Attachments paths={attachmentPaths} />
          </BubbleContent>
          <HoverActions
            chatId={chat.id}
            entryId={entry.id}
            authorName={author?.name ?? ""}
            previewText={text}
            isMine={mine}
            hovered={hovered}
          />
          <time className={styles["hb-message-timestamp"]} dateTime={new Date(entry.timestampMs).toISOString()}>
            {formatClockTime(entry.timestampMs, locale)}
          </time>
        </Bubble>

        <Reactions chatId={chat.id} entryId={entry.id} reactions={reactions} align={mine ? "end" : "start"} />
      </MessageContent>
    </Message>
  );
}
