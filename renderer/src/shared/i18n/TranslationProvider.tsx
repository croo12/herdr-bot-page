import { createContext, useContext, useMemo } from "react";
import type { ReactNode } from "react";
import { useLocale } from "./useLocale.tsx";
import type { LocaleHandle } from "./useLocale.tsx";
import { t } from "./index.tsx";
import type { Vars } from "./catalog.tsx";
import type { Locale } from "./locale.tsx";

export type TranslateFn = (key: string, vars?: Vars) => string;

interface TranslationContextValue extends LocaleHandle {
  readonly t: TranslateFn;
}

const TranslationContext = createContext<TranslationContextValue | null>(null);

export function TranslationProvider({ children }: { readonly children: ReactNode }) {
  const localeHandle = useLocale();
  const value = useMemo<TranslationContextValue>(
    () => ({ ...localeHandle, t: (key, vars) => t(localeHandle.locale, key, vars) }),
    [localeHandle],
  );
  return <TranslationContext.Provider value={value}>{children}</TranslationContext.Provider>;
}

/** `const { t } = useT()` -- the one hook feature components use for copy. Also exposes the current
 * locale and `setLocale` so the Settings dialog's Language control can live anywhere. */
export function useT(): {
  readonly t: TranslateFn;
  readonly locale: Locale;
  readonly setLocale: (locale: Locale) => void;
} {
  const value = useContext(TranslationContext);
  if (value == null) throw new Error("useT() called outside <TranslationProvider>");
  return value;
}
