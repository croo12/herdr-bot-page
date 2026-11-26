import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { AppSettings } from "#shared/api/index.tsx";
import { useStoreContext } from "./store/StoreProvider.tsx";
import { settingsKey, settingsQuery } from "./store/queries.tsx";

export function useSettings() {
  const { client } = useStoreContext();
  const cache = useQueryClient();
  const { data } = useQuery(settingsQuery(client));
  const mutation = useMutation({
    scope: { id: "workspace-settings" },
    mutationFn: (patch: Partial<AppSettings>) => client.call("settings.update", patch),
    onSuccess: async settings => {
      await cache.cancelQueries({ queryKey: settingsKey });
      cache.setQueryData(settingsKey, settings);
    },
  });
  return { settings: data ?? null, update: mutation.mutateAsync };
}
