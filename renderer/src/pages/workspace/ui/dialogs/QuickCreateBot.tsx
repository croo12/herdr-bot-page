import { useState } from "react";
import { useBridge } from "#shared/api/desktop/BridgeContext.tsx";
import { useActiveChat } from "../../model/useActiveChat.tsx";
import { useStoreContext } from "../../model/store/StoreProvider.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { DEFAULT_PERSONA_COLOR } from "#shared/ui/avatar-colors.tsx";
import type { PersonaColor } from "#shared/ui/avatar-colors.tsx";
import { toAppApiError } from "#shared/api/index.tsx";
import { Avatar, Button, TextField } from "#shared/ui/index.tsx";
import { EMPTY_LAUNCH_CHOICE, isLaunchChoiceComplete } from "../../model/dialogs/bot-form.tsx";
import type { LaunchChoice } from "../../model/dialogs/bot-form.tsx";
import { ColorSwatches } from "./ColorSwatches.tsx";
import { LaunchChoiceFields } from "./LaunchChoiceFields.tsx";
import styles from "./QuickCreateBot.module.css";

export interface QuickCreateBotProps {
  /** Two lines of explainer copy shown only in the empty-roster onboarding screen, not in
   * the "+" menu's quick-create action. */
  readonly showExplainer?: boolean;
  readonly onAdvanced: () => void;
  /** Called once the bot is created and activated -- e.g. to close the "+" menu it was opened from. */
  readonly onDone?: () => void;
}

/** The quick-create flow: color + name + provider/model/reasoning + Start, backed by `bot.quickCreate`.
 * Start stays disabled until the launch choice is complete -- quick create never picks a provider for you. The exact same flow as the
 * sidebar's "+" button. Shared by the onboarding empty state and the "+"
 * menu's "Create a new Bot" action. */
export function QuickCreateBot({ showExplainer = false, onAdvanced, onDone }: QuickCreateBotProps) {
  const bridge = useBridge();
  const { client } = useStoreContext();
  const { setActiveChatId } = useActiveChat();
  const { t, locale } = useT();
  const [color, setColor] = useState<PersonaColor>(DEFAULT_PERSONA_COLOR);
  const [name, setName] = useState("");
  const [launch, setLaunch] = useState<LaunchChoice>(EMPTY_LAUNCH_CHOICE);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function start() {
    setBusy(true);
    setError(null);
    try {
      let cwd: string | undefined;
      if (bridge.runtime === "tauri" && !(await client.call("bot.defaults", undefined)).cwd) {
        const selected = await bridge.pickDirectory();
        if (!selected) return;
        cwd = selected;
      }
      const { chat } = await client.call("bot.quickCreate", {
        cwd,
        requestId: crypto.randomUUID(),
        locale,
        provider: launch.provider,
        model: launch.model ?? undefined,
        reasoningEffort: launch.reasoningEffort ?? undefined,
        name: name.trim().length > 0 ? name.trim() : undefined,
        avatarColor: color,
      });
      setActiveChatId(chat.id);
      onDone?.();
    } catch (err) {
      setError(toAppApiError(err).message);
    } finally {
      setBusy(false);
    }
  }

  const previewName = name.trim().length > 0 ? name : t("dialogs.onboarding.namePlaceholder");

  return (
    <>
      <div className={styles["hb-quick-create-header"]}>
        <Avatar kind="bot" color={color} name={previewName} size="sm" />
        <span className={styles["hb-quick-create-header-title"]}>{previewName}</span>
      </div>
      <div className={styles["hb-quick-create"]}>
        {showExplainer && (
          <div className={styles["hb-quick-create-explainer"]}>
            <p>{t("dialogs.onboarding.explainer1")}</p>
            <p>{t("dialogs.onboarding.explainer2")}</p>
          </div>
        )}
        <Avatar kind="bot" color={color} name={previewName} size="xl" />
        <ColorSwatches
          value={color}
          onChange={setColor}
          aria-label={t("dialogs.botInfo.changeColor")}
        />
        <div className={styles["hb-quick-create-field"]}>
          <TextField
            label={t("dialogs.new.name")}
            placeholder={t("dialogs.onboarding.namePlaceholder")}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <LaunchChoiceFields
            value={launch}
            onChange={(next) => {
              setLaunch(next);
              setError(null);
            }}
          />
        </div>
        {error != null && <div className={styles["hb-quick-create-error"]}>{error}</div>}
        <div className={styles["hb-quick-create-start"]}>
          <Button
            variant="primary"
            size="lg"
            disabled={busy || !isLaunchChoiceComplete(launch)}
            onClick={start}
          >
            {t("dialogs.onboarding.start")}
          </Button>
        </div>
        <button type="button" className={styles["hb-quick-create-advanced"]} onClick={onAdvanced}>
          {t("dialogs.onboarding.advancedSetup")}
        </button>
      </div>
    </>
  );
}
