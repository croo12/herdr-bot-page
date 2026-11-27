import { useActiveChat } from "../model/useActiveChat.tsx";
import { useChat } from "../model/useChat.tsx";
import { BotInfoPanel } from "./dialogs/BotInfoPanel.tsx";
import { WorkspaceComposer } from "./WorkspaceComposer.tsx";
import { HeaderSlot } from "./conversation/HeaderSlot.tsx";
import { TranscriptSlot } from "./conversation/TranscriptSlot.tsx";
import { BotOnboardingBanner } from "./dialogs/BotOnboardingBanner.tsx";
import { DialogsPanelSync } from "./dialogs/DialogsPanelSync.tsx";
import { useMainColumnOverride } from "./dialogs/useMainColumnOverride.tsx";
import { SidebarSlot } from "./sidebar/SidebarSlot.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { PanelHost } from "#shared/ui/PanelHost.tsx";
import { useSidebarLayout } from "../model/layout/useSidebarLayout.tsx";
import styles from "./WorkspaceLayout.module.css";

/** Resizable sidebar, active conversation and optional chat info panel. */
export function WorkspaceLayout() {
  const sidebar = useSidebarLayout();
  const { activeChatId } = useActiveChat();
  const activeChat = useChat(activeChatId);
  const { t } = useT();
  const mainOverride = useMainColumnOverride();

  return (
    <div className={styles["hb-shell"]}>
      <div className={styles["hb-shell-row"]}>
        <div className={styles["hb-shell-sidebar"]} style={{ width: sidebar.width }}>
          <SidebarSlot mode={sidebar.mode} />
          {/* Mounted in both modes -- the drag handle works in both directions: dragging it out
              from the collapsed rail past the snap threshold (sidebar-width.tsx's `resolveSidebarDrag`,
              stateless, so it doesn't matter which mode the drag started in) expands back to a labeled
              sidebar, not just ⌘B. */}
          <div
            className={styles["hb-shell-resize-handle"]}
            onPointerDown={(event) => {
              const row = event.currentTarget.parentElement?.parentElement;
              const startLeft = row?.getBoundingClientRect().left ?? 0;
              const target = event.currentTarget;
              target.setPointerCapture(event.pointerId);
              function onMove(moveEvent: PointerEvent) {
                sidebar.onDrag(moveEvent.clientX - startLeft);
              }
              function onUp() {
                sidebar.onDragEnd();
                window.removeEventListener("pointermove", onMove);
                window.removeEventListener("pointerup", onUp);
              }
              window.addEventListener("pointermove", onMove);
              window.addEventListener("pointerup", onUp);
            }}
          />
        </div>

        <PanelHost>
          <DialogsPanelSync />
          <div className={styles["hb-shell-main"]} aria-label={t("shell.main.ariaLabel")}>
            {mainOverride != null ? (
              mainOverride
            ) : activeChatId == null ? (
              <div className={styles["hb-shell-empty"]}>
                <div className={styles["hb-shell-empty-title"]}>{t("shell.emptyState.title")}</div>
                <div className={styles["hb-shell-empty-body"]}>{t("shell.emptyState.body")}</div>
              </div>
            ) : activeChat?.kind === "bot" ? (
              <BotInfoPanel key={activeChat.id} botId={activeChat.id} standalone />
            ) : (
              <>
                <BotOnboardingBanner chatId={activeChatId} />
                <HeaderSlot chatId={activeChatId} />
                <TranscriptSlot chatId={activeChatId} />
                <WorkspaceComposer chatId={activeChatId} />
              </>
            )}
          </div>
        </PanelHost>
      </div>
    </div>
  );
}
