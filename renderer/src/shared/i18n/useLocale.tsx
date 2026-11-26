import { useCallback, useEffect, useState } from "react";
import { useBridge } from "../api/desktop/BridgeContext.tsx";
import { DEFAULT_LOCALE, parseLocale } from "./locale.tsx";
import type { Locale } from "./locale.tsx";

export interface LocaleHandle {
  readonly locale: Locale;
  setLocale(locale: Locale): void;
}

/** Follows the UI locale persisted by the desktop bridge. */
export function useLocale(): LocaleHandle {
  const bridge = useBridge();
  const [locale, setLocaleState] = useState<Locale>(() => parseLocale(bridge.locale.initial));

  useEffect(() => {
    let cancelled = false;
    void bridge.locale.get().then((value) => {
      if (!cancelled) setLocaleState(parseLocale(value));
    });
    return () => {
      cancelled = true;
    };
  }, [bridge]);

  useEffect(() => bridge.locale.onChanged((value) => setLocaleState(parseLocale(value as string | null))), [bridge]);

  const setLocale = useCallback(
    (next: Locale) => {
      setLocaleState(next);
      void bridge.locale.set(next);
    },
    [bridge],
  );

  return { locale, setLocale };
}

export { DEFAULT_LOCALE };
