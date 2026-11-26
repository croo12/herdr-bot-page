import { useState } from "react";
import { useChat } from "../../model/useChat.tsx";
import { useStoreContext } from "../../model/store/StoreProvider.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { Spinner } from "#shared/ui/index.tsx";
import { toAppApiError } from "#shared/api/index.tsx";
import styles from "./BotOnboardingBanner.module.css";

export interface BotOnboardingBannerProps {
  readonly chatId: string;
}

/**
 * The bot setup/onboarding banner: "setup in progress / failed with retry", keyed off
 * `ChatSummary.bot.onboarding.stage` (`provisioning -> briefing -> greeting -> ready`,
 * or `failed` with a retryable `herdrBot.retryBotSetup`/`bot.retrySetup`). Renders nothing once a bot
 * reaches `ready` or has no onboarding block at all (a bot created by hand, not via quick-create).
 * Mounted unconditionally above the header for the active chat (see `pages/workspace/ui/WorkspaceLayout.tsx`) so it needs
 * no wiring from the conversation feature.
 */
export function BotOnboardingBanner({ chatId }: BotOnboardingBannerProps) {
  const chat = useChat(chatId);
  const { client } = useStoreContext();
  const { t } = useT();
  const [retrying, setRetrying] = useState(false);
  const [retryError, setRetryError] = useState<string | null>(null);

  const onboarding = chat?.kind === "bot" ? chat.bot?.onboarding : null;
  if (onboarding == null || onboarding.stage === "ready") return null;

  async function retry() {
    setRetrying(true);
    setRetryError(null);
    try {
      await client.call("bot.retrySetup", { botId: chatId });
    } catch (err) {
      setRetryError(toAppApiError(err).message);
    } finally {
      setRetrying(false);
    }
  }

  if (onboarding.stage === "failed") {
    return (
      <div className={`${styles["hb-onboarding-banner"]} ${styles["hb-onboarding-banner--failed"]}`}>
        <span className={styles["hb-onboarding-banner-message"]}>{retryError ?? onboarding.error ?? t("dialogs.onboarding.setupFailed")}</span>
        <button type="button" className={styles["hb-onboarding-banner-retry"]} disabled={retrying} onClick={retry}>
          {t("dialogs.onboarding.retry")}
        </button>
      </div>
    );
  }

  return (
    <div className={styles["hb-onboarding-banner"]}>
      <Spinner />
      <span className={styles["hb-onboarding-banner-message"]}>{t(`dialogs.onboarding.stage.${onboarding.stage}`)}</span>
    </div>
  );
}
