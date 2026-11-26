import { Plus, Settings, SunMoon, Users } from "lucide-react";
import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { useChatMembers } from "../../model/useChat.tsx";
import { resolvePersonaColor } from "#shared/ui/avatar-colors.tsx";
import type { ChatSummary, MessageSearchHit } from "#shared/api/index.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { Avatar, Kbd } from "#shared/ui/index.tsx";
import type { PaletteActionDef } from "../../model/palette/actions.tsx";
import { highlightSnippet } from "../../model/palette/highlight-snippet.tsx";
import { formatRelativeTime } from "../../model/palette/relative-time.tsx";
import { messageHitSubtitle } from "../../model/palette/subtitle.tsx";
import styles from "./PaletteSlot.module.css";

export type PaletteResult =
  | { readonly kind: "chat"; readonly chat: ChatSummary }
  | { readonly kind: "message"; readonly hit: MessageSearchHit; readonly chat: ChatSummary | null }
  | { readonly kind: "action"; readonly action: PaletteActionDef; readonly label: string };

const ACTION_ICONS: Record<PaletteActionDef["id"], typeof Plus> = {
  newBot: Plus,
  newRoom: Users,
  settings: Settings,
  toggleTheme: SunMoon,
};

export interface PaletteResultRowProps {
  readonly result: PaletteResult;
  readonly query: string;
  readonly selected: boolean;
  readonly shortcutDigit: number | null;
  readonly nowMs: number;
  readonly onActivate: () => void;
}

/** One command-palette result row: a chat (name + description/members subtitle), a
 * message hit (host snippet, highlighted around the query, + a role-aware subtitle), or a static
 * action -- each optionally tagged with its ⌘-digit quick-pick badge. */
export function PaletteResultRow({
  result,
  query,
  selected,
  shortcutDigit,
  nowMs,
  onActivate,
}: PaletteResultRowProps) {
  const { t, locale } = useT();

  if (result.kind === "action") {
    const Icon = ACTION_ICONS[result.action.id];
    return (
      <PaletteRowShell selected={selected} shortcutDigit={shortcutDigit} onActivate={onActivate}>
        <span className={styles["hb-palette-row-icon"]}>
          <Icon size={18} />
        </span>
        <span className={styles["hb-palette-row-name"]}>{result.label}</span>
      </PaletteRowShell>
    );
  }

  if (result.kind === "chat") {
    return (
      <PaletteRowShell selected={selected} shortcutDigit={shortcutDigit} onActivate={onActivate}>
        <ChatAvatar chat={result.chat} />
        <div className={styles["hb-palette-row-main"]}>
          <div className={styles["hb-palette-row-name"]}>{result.chat.name}</div>
          <div className={styles["hb-palette-row-subtitle"]}>
            <ChatSubtitle chat={result.chat} />
          </div>
        </div>
      </PaletteRowShell>
    );
  }

  const { hit, chat } = result;
  const isRoom = chat?.kind === "room";
  const subtitle = messageHitSubtitle(hit, isRoom);
  const time = formatRelativeTime(hit.timestampMs, nowMs, locale);
  const subtitleText =
    subtitle.kind === "room"
      ? t("palette.subtitle.inChat", { chat: subtitle.chatName, time })
      : subtitle.kind === "dmFromBot"
        ? t("palette.subtitle.fromBotToYou", { author: subtitle.author, time })
        : t("palette.subtitle.fromYouToChat", { chat: subtitle.chatName, time });

  return (
    <PaletteRowShell selected={selected} shortcutDigit={shortcutDigit} onActivate={onActivate}>
      {chat != null ? (
        <ChatAvatar chat={chat} />
      ) : (
        <Avatar kind="bot" color="gray" name={hit.chatName} size="sm" />
      )}
      <div className={styles["hb-palette-row-main"]}>
        <div className={styles["hb-palette-row-name"]}>
          {highlightSnippet(hit.snippet, query).map((segment, index) => (
            <span
              key={index}
              className={segment.match ? styles["hb-palette-row-highlight"] : undefined}
            >
              {segment.text}
            </span>
          ))}
        </div>
        <div className={styles["hb-palette-row-subtitle"]}>{subtitleText}</div>
      </div>
    </PaletteRowShell>
  );
}

function ChatAvatar({ chat }: { readonly chat: ChatSummary }) {
  const members = useChatMembers(chat);
  if (chat.kind === "room")
    return (
      <Avatar
        kind="room"
        members={members.map((m) => ({ color: resolvePersonaColor(m.avatarColor), name: m.name }))}
        size="sm"
      />
    );
  return (
    <Avatar kind="bot" color={resolvePersonaColor(chat.avatarColor)} name={chat.name} size="sm" />
  );
}

function ChatSubtitle({ chat }: { readonly chat: ChatSummary }) {
  const members = useChatMembers(chat);
  if (chat.kind === "room") return <>{members.map((m) => m.name).join(", ")}</>;
  return <>{chat.description}</>;
}

function PaletteRowShell({
  selected,
  shortcutDigit,
  onActivate,
  children,
}: {
  readonly selected: boolean;
  readonly shortcutDigit: number | null;
  readonly onActivate: () => void;
  readonly children: ReactNode;
}) {
  const rowRef = useRef<HTMLDivElement>(null);

  // Keeps the keyboard-selected row in view when ArrowUp/Down moves past the visible results --
  // the list is fully keyboard-driven, so without this, selection can silently move off-screen.
  useEffect(() => {
    if (selected) rowRef.current?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  return (
    <div
      ref={rowRef}
      role="option"
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          event.stopPropagation();
          onActivate();
        }
      }}
      aria-selected={selected}
      className={`${styles["hb-palette-row"]} ${selected ? styles["hb-palette-row--selected"] : ""}`}
      onClick={onActivate}
    >
      {children}
      {shortcutDigit != null && <Kbd>{`⌘${shortcutDigit}`}</Kbd>}
    </div>
  );
}
