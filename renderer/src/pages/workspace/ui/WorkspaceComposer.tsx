import { MessageComposer } from "#features/compose-message";
import { useChat, useChatMembers } from "../model/useChat.tsx";
import { useChats } from "../model/useChats.tsx";
import { useStoreContext } from "../model/store/StoreProvider.tsx";

export function WorkspaceComposer({ chatId }: { readonly chatId: string }) {
  const { client, drafts } = useStoreContext();
  const chat = useChat(chatId);
  const members = useChatMembers(chat);
  const contacts = useChats();
  return <MessageComposer chatId={chatId} chat={chat} members={members} contacts={contacts} client={client} drafts={drafts} />;
}
