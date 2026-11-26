import { useTheme } from "#shared/lib/theme/useTheme.tsx";

/**
 * Keeps `<html data-theme>` in sync with `bridge.theme` for the whole app lifetime (OS theme
 * flips while `preference: "system"`, or a future Settings dialog calling `setPreference`). Renders
 * nothing -- `main.tsx` already sets the attribute once synchronously before first paint; this is what
 * keeps it correct afterward. Mounted once in `App.tsx`, inside `BridgeProvider` (needs `useBridge`).
 */
export function ThemeSync() {
  useTheme();
  return null;
}
