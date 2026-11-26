/** The selected reply is scoped to its chat and cleared after sending or dismissal. */
import { useSyncExternalStore } from "react";

export interface ReplyTarget {
  readonly entryId: string;
  readonly authorName: string;
  /** A short, already-plain-text (not markdown) preview of the quoted message for the composer to show. */
  readonly preview: string;
}

type Listener = () => void;

const targets = new Map<string, ReplyTarget>();
const listeners = new Set<Listener>();

function emit(): void {
  for (const listener of listeners) listener();
}

export function getReplyTarget(chatId: string): ReplyTarget | null {
  return targets.get(chatId) ?? null;
}

export function setReplyTarget(chatId: string, target: ReplyTarget): void {
  targets.set(chatId, target);
  emit();
}

export function clearReplyTarget(chatId: string): void {
  if (!targets.has(chatId)) return;
  targets.delete(chatId);
  emit();
}

export function subscribeReplyTarget(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Test-only escape hatch so test files don't depend on import order across test files. */
export function resetReplyTargetsForTests(): void {
  targets.clear();
}

export interface ReplyTargetHandle {
  readonly target: ReplyTarget | null;
  clear(): void;
}

/** The composer's own view of `chatId`'s current reply target (`null` once cleared/sent/never set). */
export function useReplyTarget(chatId: string | null): ReplyTargetHandle {
  const target = useSyncExternalStore(subscribeReplyTarget, () =>
    chatId == null ? null : getReplyTarget(chatId),
  );
  return { target, clear: () => chatId != null && clearReplyTarget(chatId) };
}
