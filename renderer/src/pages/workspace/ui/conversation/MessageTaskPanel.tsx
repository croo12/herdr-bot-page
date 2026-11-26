import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import type { TaskStatus, TranscriptEntry } from "#shared/api/index.tsx";
import { useT } from "#shared/i18n/TranslationProvider.tsx";
import { Button, Select, TextField } from "#shared/ui/index.tsx";
import type { MentionCandidate } from "../../model/conversation/mentions.tsx";
import { useStoreContext } from "../../model/store/StoreProvider.tsx";

interface Props {
  chatId: string;
  entry: TranscriptEntry;
  members: readonly MentionCandidate[];
  onClose(): void;
}

export function MessageTaskPanel({ chatId, entry, members, onClose }: Props) {
  const { client } = useStoreContext();
  const { t } = useT();
  const [criteria, setCriteria] = useState("");
  const [assigneeId, setAssignee] = useState("");
  const task = entry.task;
  const save = useMutation({
    mutationFn: () => client.call("task.track", { chatId, entryId: entry.id, assigneeId: assigneeId || null, criteria: criteria.trim() }),
    onSuccess: onClose,
  });
  const change = useMutation({
    mutationFn: (edit: { previousStatus: TaskStatus; status: TaskStatus }) => client.call("task.setStatus", { chatId, entryId: entry.id, ...edit }),
  });
  const execution = task?.execution;
  const assigneeOptions = [
    { value: "", label: t("conversation.taskUnassigned") },
    ...members.map(member => ({ value: member.id, label: member.name })),
  ];
  const assign = useMutation({
    mutationFn: (next: string) => client.call("task.assign", {
      chatId, entryId: entry.id, assigneeId: next || null,
      previousAssigneeId: task?.assigneeId ?? null, version: execution?.version ?? null,
    }),
  });
  const hasAssignee = members.some(member => member.id === task?.assigneeId);
  const active = execution?.state === "queued" || execution?.state === "running";
  const control = useMutation({
    mutationFn: (method: "task.run" | "task.pause") => {
      const params = { chatId, entryId: entry.id, version: execution?.version ?? null };
      return method === "task.pause"
        ? client.call("task.pause", params)
        : client.call("task.run", { ...params, assigneeId: task?.assigneeId ?? "" });
    },
  });
  const error = assign.error ?? control.error ?? save.error ?? change.error;
  const statuses = ["open", "in_progress", "completed"] as const;
  return (
    <section aria-label={t("conversation.task")} className="grid gap-2 max-w-full rounded-md border p-3 text-sm">
      {task ? <>
        <Select label={t("conversation.taskAssignee")} value={task.assigneeId ?? ""} options={assigneeOptions}
          disabled={task.status === "completed" || execution?.state === "running" || assign.isPending || control.isPending}
          onChange={value => assign.mutate(value)} />
        <p className="whitespace-pre-wrap break-words">{t("conversation.taskCriteria")} · {task.criteria}</p>
        {execution ? <>
          <strong role="status">{t(`conversation.execution.${execution.state}`)}</strong>
          {execution.checkpoint && <p className="whitespace-pre-wrap break-words">{execution.checkpoint}</p>}
          {execution.detail && <p>{execution.detail}</p>}
        </> : <Select label={t("conversation.taskStatus")} value={task.status} disabled={change.isPending || control.isPending}
          options={statuses.map(status => ({ value: status, label: t(`conversation.taskStatus.${status}`) }))}
          onChange={value => {
            const status = statuses.find(option => option === value);
            if (status) change.mutate({ previousStatus: task.status, status });
          }} />}
        {task.status !== "completed" && <Button
          type="button"
          disabled={assign.isPending || control.isPending || change.isPending || (!active && !hasAssignee) || (active && execution?.pauseAfterStep)}
          onClick={() => control.mutate(active ? "task.pause" : "task.run")}
        >{active
          ? t(execution?.state === "queued" ? "conversation.taskPause" : "conversation.taskPauseAfterStep")
          : t(execution ? "conversation.taskResume" : "conversation.taskRun")}</Button>}
      </> : <form className="grid gap-2" onSubmit={event => { event.preventDefault(); save.mutate(); }}>
        <Select label={t("conversation.taskAssignee")} value={assigneeId} onChange={setAssignee}
          options={assigneeOptions} />
        <TextField label={t("conversation.taskCriteria")} value={criteria} onChange={event => setCriteria(event.target.value)} required />
        <div className="flex gap-2">
          <Button type="submit" disabled={save.isPending || !criteria.trim()}>{t("conversation.trackTask")}</Button>
          <Button type="button" variant="secondary" onClick={onClose}>{t("common.cancel")}</Button>
        </div>
      </form>}
      {error && <div role="alert">{error.message}</div>}
    </section>
  );
}
