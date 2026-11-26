import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { formatDateSeparatorText } from "../../model/conversation/date-separator-label.tsx";
import styles from "./DateSeparator.module.css";

export interface DateSeparatorProps {
  readonly timestampMs: number;
}

export function DateSeparator({ timestampMs }: DateSeparatorProps) {
  const { t, locale } = useT();
  return <div className={styles["hb-date-separator"]}>{formatDateSeparatorText(timestampMs, Date.now(), locale, t)}</div>;
}
