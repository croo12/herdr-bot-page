/**
 * The reply-target preview bar shown above the composer once something sets a reply target (the
 * message hover "Reply" action; see `features/compose-message/model/reply-target.tsx` for the cross-feature contract). Purely
 * presentational -- `MessageComposer` owns reading `useReplyTarget`/clearing it on send.
 */
import { X } from "lucide-react";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import type { ReplyTarget } from "../model/reply-target.tsx";
import styles from "./ReplyPreviewBar.module.css";

export interface ReplyPreviewBarProps {
  readonly target: ReplyTarget;
  readonly onClear: () => void;
}

export function ReplyPreviewBar({ target, onClear }: ReplyPreviewBarProps) {
  const { t } = useT();
  return (
    <div className={styles["hb-reply-bar"]}>
      <div className={styles["hb-reply-bar-body"]}>
        <span className={styles["hb-reply-bar-label"]}>{t("composer.reply.label", { authorName: target.authorName })}</span>
        <span className={styles["hb-reply-bar-preview"]}>{target.preview}</span>
      </div>
      <button type="button" className={styles["hb-reply-bar-clear"]} aria-label={t("composer.reply.clear")} onClick={onClear}>
        <X size={14} />
      </button>
    </div>
  );
}
