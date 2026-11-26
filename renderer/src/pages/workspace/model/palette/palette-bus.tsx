/** Opens the command palette from elsewhere in the workspace. */

export interface PaletteOpenRequest {
  readonly initialQuery: string;
}

type Listener = (request: PaletteOpenRequest) => void;

const listeners = new Set<Listener>();

/** Called by the sidebar's Search button / row. `initialQuery` lets a future "search within this
 * chat" entry point pre-fill the palette; the plain Search button passes "". */
export function requestPaletteOpen(initialQuery = ""): void {
  for (const listener of listeners) listener({ initialQuery });
}

export function onPaletteOpenRequested(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
