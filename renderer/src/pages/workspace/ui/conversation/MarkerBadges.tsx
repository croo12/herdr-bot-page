import { useChats } from "../../model/useChats.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { Badge } from "#shared/ui/index.tsx";
import {
  getAnswersHeld,
  getHeldFor,
  getLateInfo,
  getStaleFor,
} from "#shared/api/transcript/entry-shapes.tsx";
import type { TranscriptEntry } from "#shared/api/index.tsx";
import { formatDurationShort } from "../../model/conversation/format-duration.tsx";
import styles from "./MarkerBadges.module.css";

export interface MarkerBadgesProps {
  readonly entry: TranscriptEntry;
}

/** Late/held/stale badges, rendered as their own line directly above a message bubble. */
export function MarkerBadges({ entry }: MarkerBadgesProps) {
  const chats = useChats();
  const { t, locale } = useT();

  const late = getLateInfo(entry);
  const staleFor = getStaleFor(entry);
  const answersHeld = getAnswersHeld(entry);
  const heldFor = getHeldFor(entry);

  const hasAny = late.late || staleFor != null || heldFor.length > 0;
  if (!hasAny) return null;

  return (
    <div className={styles["hb-marker-badges"]}>
      {late.late && (
        <Badge tone="late">
          {t("conversation.lateReply", {
            duration:
              late.lateAfterMs != null ? formatDurationShort(late.lateAfterMs, locale) : "0s",
          })}
        </Badge>
      )}
      {late.late && answersHeld != null && (
        <button type="button" className={styles["hb-marker-link"]}>
          {t("conversation.answersHeld")}
        </button>
      )}
      {staleFor != null && <Badge tone="neutral">{t("conversation.stale")}</Badge>}
      {heldFor.map((botId) => {
        const bot = chats.find((chat) => chat.id === botId);
        return (
          <Badge key={botId} tone="neutral">
            {t("conversation.held", { name: bot?.name ?? botId })}
          </Badge>
        );
      })}
    </div>
  );
}
