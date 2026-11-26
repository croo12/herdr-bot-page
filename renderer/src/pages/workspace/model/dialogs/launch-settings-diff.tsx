/** Builds the launch settings patch. Omitted fields are preserved and null resets a setting. */
import type { BotFacts, PermissionMode, ReasoningEffort } from "#shared/api/index.tsx";

export interface LaunchSettingsFormState {
  readonly cwd: string;
  readonly permissionMode: PermissionMode | null;
  readonly model: string | null;
  readonly reasoningEffort: ReasoningEffort | null;
}

export interface LaunchSettingsDiff {
  readonly cwd?: string;
  readonly permissionMode?: PermissionMode | null;
  readonly model?: string | null;
  readonly reasoningEffort?: ReasoningEffort | null;
}

export function launchSettingsFormFromFacts(bot: BotFacts): LaunchSettingsFormState {
  return { cwd: bot.cwd ?? "", permissionMode: bot.permissionMode, model: bot.model, reasoningEffort: bot.reasoningEffort };
}

/** `null` when nothing changed (the "Restart to apply" button should stay disabled); otherwise an
 * object containing only the touched keys, ready to spread into `bot.updateLaunchSettings`'s params
 * alongside `botId`/`allowHomeCwd`. */
export function diffLaunchSettings(original: BotFacts, edited: LaunchSettingsFormState): LaunchSettingsDiff | null {
  const diff: LaunchSettingsDiff = {
    ...(edited.cwd !== (original.cwd ?? "") ? { cwd: edited.cwd } : {}),
    ...(edited.permissionMode !== original.permissionMode ? { permissionMode: edited.permissionMode } : {}),
    ...(edited.model !== original.model ? { model: edited.model } : {}),
    ...(edited.reasoningEffort !== original.reasoningEffort ? { reasoningEffort: edited.reasoningEffort } : {}),
  };

  return Object.keys(diff).length > 0 ? diff : null;
}
