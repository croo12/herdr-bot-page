import { useChat } from "../../model/useChat.tsx";
import { useTranscript } from "../../model/useTranscript.tsx";
import { Transcript } from "./Transcript.tsx";

export interface TranscriptSlotProps {
  readonly chatId: string;
}

/** Mounted entry point for the transcript -- `pages/workspace/ui/WorkspaceLayout.tsx` only renders this once a
 * chat is active, so `chatId` is always non-null here. Thin by design: `Transcript.tsx` owns the actual
 * scrolling/rendering logic; this just wires the two data hooks it needs. */
export function TranscriptSlot({ chatId }: TranscriptSlotProps) {
  const chat = useChat(chatId);
  const transcript = useTranscript(chatId);

  if (chat == null) return null;

  return <Transcript chatId={chatId} chat={chat} transcript={transcript} />;
}
