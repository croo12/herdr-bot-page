import { useChats } from "../../model/useChats.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { Button } from "#shared/ui/index.tsx";
import { useActiveChat } from "../../model/useActiveChat.tsx";
import { requestJumpToMessage } from "../../model/jump-target.tsx";
import type { TranscriptEntry } from "#shared/api/index.tsx";

export function DelegationStatus({
  entry,
  onNavigate,
}: {
  entry: TranscriptEntry;
  onNavigate?: () => void;
}) {
  const { t } = useT();
  const chats = useChats();
  const { setActiveChatId } = useActiveChat();
  const { delegation, delegatedFrom } = entry;
  function jump(chatId: string, entryId: string) {
    onNavigate?.();
    setActiveChatId(chatId);
    requestJumpToMessage(chatId, entryId);
  }
  return (
    <>
      {delegation && (
        <div className="flex flex-wrap items-center gap-2 text-sm" role="status">
          <span>
            {delegation.targetName} · {t(`conversation.delegation.${delegation.state}`)}
          </span>
          {delegation.detail && <span>{delegation.detail}</span>}
          {delegation.state === "waiting" && delegation.promptId && (
            <Button
              type="button"
              variant="secondary"
              onClick={() => {
                if (delegation.promptId)
                  jump(delegation.promptChatId ?? delegation.targetChatId, delegation.promptId);
              }}
            >
              {t("conversation.delegation.openRequest")}
            </Button>
          )}
        </div>
      )}
      {delegatedFrom && chats.some((candidate) => candidate.id === delegatedFrom.chatId) && (
        <Button
          type="button"
          variant="secondary"
          onClick={() => jump(delegatedFrom.chatId, delegatedFrom.entryId)}
        >
          {t("conversation.delegation.openOrigin")}
        </Button>
      )}
    </>
  );
}
