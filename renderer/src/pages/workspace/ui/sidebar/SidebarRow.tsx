import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { useChatMembers } from "../../model/useChat.tsx";
import { useStatus } from "../../model/useStatus.tsx";
import { useStoreContext } from "../../model/store/StoreProvider.tsx";
import { formatClockTime } from "#shared/lib/time/format-time.tsx";
import { resolvePersonaColor } from "#shared/ui/avatar-colors.tsx";
import { sidebarWaitingReason } from "../../model/status.tsx";
import type { ChatSummary } from "#shared/api/index.tsx";
import type { Locale } from "#shared/i18n/locale.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { Avatar, StatusDot, useContextMenu } from "#shared/ui/index.tsx";
import { SidebarRowMenu } from "./SidebarRowMenu.tsx";
import styles from "./SidebarSlot.module.css";

export interface SidebarRowProps {
  readonly chat: ChatSummary;
  readonly selected: boolean;
  readonly rail: boolean;
  readonly locale: Locale;
  readonly isPinned: boolean;
  readonly onSelect: () => void;
  readonly onTogglePin: () => void;
}

/** One sidebar row: avatar + status/unread dots, name (or an inline rename field),
 * last-message preview, timestamp, and the right-click context menu. A `<div role="option">`, not a
 * `<button>` -- a `<button>` can't legally contain a nested `<input>` for the inline-rename state
 * (invalid HTML; breaks the input's own typing/Space/focus), so this owns Enter/Space activation by
 * hand instead. */
export function SidebarRow({
  chat,
  selected,
  rail,
  locale,
  isPinned,
  onSelect,
  onTogglePin,
}: SidebarRowProps) {
  const members = useChatMembers(chat);
  const status = useStatus(chat);
  const waiting = sidebarWaitingReason(chat);
  const { t } = useT();
  const { client } = useStoreContext();
  const contextMenu = useContextMenu();
  const [renaming, setRenaming] = useState(false);
  const [draftName, setDraftName] = useState(chat.name);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!renaming) return;
    const input = inputRef.current;
    input?.focus();
    input?.select();
  }, [renaming]);

  function commitRename() {
    const name = draftName.trim();
    setRenaming(false);
    if (name.length > 0 && name !== chat.name)
      void client.call("chat.updateProfile", { chatId: chat.id, name });
  }

  function onRowKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (renaming) return;
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect();
    }
  }

  const preview =
    waiting != null ? t(`sidebar.waitingForYou.${waiting}`) : (chat.lastMessagePreview ?? "");

  return (
    // `SidebarRowMenu` renders a `Popover` that portals to `document.body` -- but React replays event
    // bubbling for a portaled subtree along the *React* tree, not the DOM tree (see the React docs on
    // portals and events), so a menu item nested *inside* this row's div would still fire the row's own
    // `onClick` (e.g. clicking "Pin" would also select the row). Rendering it as a sibling here, not a
    // child of the row div, keeps it out of that bubble path entirely.
    <>
      <div
        role="option"
        aria-selected={selected}
        tabIndex={renaming ? -1 : 0}
        className={`${styles["hb-sidebar-row"]} ${selected ? styles["hb-sidebar-row--selected"] : ""} hb-no-drag`}
        onClick={renaming ? undefined : onSelect}
        onKeyDown={onRowKeyDown}
        onContextMenu={contextMenu.openAt}
        title={rail ? chat.name : undefined}
      >
        {chat.kind === "bot" ? (
          <Avatar
            kind="bot"
            color={resolvePersonaColor(chat.avatarColor)}
            name={chat.name}
            statusDot={status.dot}
          />
        ) : (
          <Avatar
            kind="room"
            members={members.map((member) => ({
              color: resolvePersonaColor(member.avatarColor),
              name: member.name,
            }))}
            meLabel={t("common.me")}
          />
        )}
        {!rail && (
          <div className={styles["hb-sidebar-row-main"]}>
            <div className={styles["hb-sidebar-row-top"]}>
              {renaming ? (
                <input
                  ref={inputRef}
                  className={styles["hb-sidebar-row-rename"]}
                  aria-label={t("sidebar.renameInput.ariaLabel")}
                  value={draftName}
                  onClick={(event) => event.stopPropagation()}
                  onChange={(event) => setDraftName(event.target.value)}
                  onKeyDown={(event) => {
                    event.stopPropagation();
                    if (event.key === "Enter") commitRename();
                    else if (event.key === "Escape") setRenaming(false);
                  }}
                  onBlur={commitRename}
                />
              ) : (
                <span className={styles["hb-sidebar-row-name"]}>{chat.name}</span>
              )}
            </div>
            <div className={styles["hb-sidebar-row-preview"]}>{preview}</div>
          </div>
        )}
        {!rail && (
          <div className={styles["hb-sidebar-row-meta"]}>
            <span className={styles["hb-sidebar-row-time"]}>
              {formatClockTime(chat.lastActivityAt, locale)}
            </span>
            {chat.hasUnread && <StatusDot tone="unread" size={8} aria-label="unread" />}
          </div>
        )}
        {rail && chat.hasUnread && (
          <span className={styles["hb-sidebar-row-unread"]}>
            <StatusDot tone="unread" size={8} aria-label="unread" />
          </span>
        )}
      </div>
      <SidebarRowMenu
        chat={chat}
        state={contextMenu}
        isPinned={isPinned}
        onTogglePin={onTogglePin}
        onStartRename={() => {
          setDraftName(chat.name);
          setRenaming(true);
        }}
      />
    </>
  );
}
