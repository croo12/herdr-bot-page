/** Coordinates a message search result with the transcript that loads and scrolls to it. */
import { useSyncExternalStore } from "react";

export interface JumpTarget {
  readonly chatId: string;
  readonly entryId: string;
  /** Distinguishes two requests for the *same* entry id back-to-back (e.g. selecting the same search hit
   * twice) so the consumer's `useEffect` re-fires instead of seeing an unchanged value. */
  readonly token: number;
}

let current: JumpTarget | null = null;
let tokenSeq = 0;
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) listener();
}

export function requestJumpToMessage(chatId: string, entryId: string): void {
  tokenSeq += 1;
  current = { chatId, entryId, token: tokenSeq };
  emit();
}

export function clearJumpTarget(): void {
  if (current == null) return;
  current = null;
  emit();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): JumpTarget | null {
  return current;
}

/** `null` unless a jump was requested for exactly this `chatId` and hasn't been cleared yet. */
export function useJumpTarget(chatId: string | null): JumpTarget | null {
  return useSyncExternalStore(subscribe, () => (chatId != null && current?.chatId === chatId ? current : null));
}

/** Test-only: reads the current request without a React subscription. */
export function peekJumpTarget(): JumpTarget | null {
  return current;
}

/** Test-only escape hatch so test files don't depend on import order across test files. */
export function resetJumpTargetForTests(): void {
  current = null;
  tokenSeq = 0;
}
