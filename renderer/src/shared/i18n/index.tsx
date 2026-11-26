/** English and Korean message catalogs, organized by UI area. */
import * as common from "./common.tsx";
import * as shell from "./messages/workspace.tsx";
import * as sidebar from "./messages/sidebar.tsx";
import * as palette from "./messages/palette.tsx";
import * as conversation from "./messages/conversation.tsx";
import * as composer from "./messages/composer.tsx";
import * as dialogs from "./messages/dialogs.tsx";
import { mergeCatalog, translate } from "./catalog.tsx";
import type { Vars } from "./catalog.tsx";
import type { Locale } from "./locale.tsx";

export type { Locale } from "./locale.tsx";
export { DEFAULT_LOCALE, parseLocale } from "./locale.tsx";

const CATALOG = mergeCatalog([common, shell, sidebar, palette, conversation, composer, dialogs]);

export function t(locale: Locale, key: string, vars?: Vars): string {
  return translate(CATALOG, locale, key, vars);
}
