import { WorkspacePage } from "#pages/workspace";
import { BridgeProvider } from "#shared/api/desktop/BridgeContext.tsx";
import type { HerdrBridge } from "#shared/api/desktop/bridge.tsx";
import { TranslationProvider } from "#shared/i18n/TranslationProvider.tsx";
import { ShortcutsProvider } from "#shared/lib/shortcuts/ShortcutsProvider.tsx";
import { ToastProvider } from "#shared/ui/index.tsx";
import { ThemeSync } from "./ThemeSync.tsx";

export function App({ bridge }: { readonly bridge: HerdrBridge }) {
  return (
    <BridgeProvider bridge={bridge}>
      <ThemeSync />
      <TranslationProvider>
        <ShortcutsProvider>
          <ToastProvider>
            <WorkspacePage />
          </ToastProvider>
        </ShortcutsProvider>
      </TranslationProvider>
    </BridgeProvider>
  );
}
