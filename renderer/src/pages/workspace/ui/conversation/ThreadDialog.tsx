import { useQuery } from "@tanstack/react-query";
import { getAuthor, getRawText, getPrompt } from "#shared/api/transcript/entry-shapes.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { Dialog, Spinner } from "#shared/ui/index.tsx";
import { threadKey } from "../../model/store/queries.tsx";
import { useStoreContext } from "../../model/store/StoreProvider.tsx";
import { DelegationStatus } from "./DelegationStatus.tsx";

export function ThreadDialog({
  chatId,
  rootId,
  onClose,
}: {
  chatId: string;
  rootId: string;
  onClose(): void;
}) {
  const { client } = useStoreContext();
  const { t } = useT();
  const thread = useQuery({
    queryKey: threadKey(chatId, rootId),
    queryFn: () => client.call("transcript.thread", { chatId, rootId }),
  });
  return (
    <Dialog
      open
      title={t("conversation.openThread")}
      closeLabel={t("common.close")}
      onClose={onClose}
    >
      {thread.isPending && <Spinner />}
      {thread.error && <p role="alert">{thread.error.message}</p>}
      <ol className="grid gap-4">
        {thread.data?.entries.map((entry) => (
          <li key={entry.id}>
            <strong>{getAuthor(entry)?.name}</strong>
            <p className="whitespace-pre-wrap break-words">
              {getRawText(entry) ?? getPrompt(entry)?.question}
            </p>
            {(entry.delegation || entry.delegatedFrom) && (
              <DelegationStatus entry={entry} onNavigate={onClose} />
            )}
          </li>
        ))}
      </ol>
    </Dialog>
  );
}
