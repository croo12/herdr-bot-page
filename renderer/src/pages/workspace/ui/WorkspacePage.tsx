import { DialogHost } from "#shared/ui/DialogHost.tsx";
import { StoreProvider } from "../model/store/StoreProvider.tsx";
import { DialogsSlot } from "./dialogs/DialogsSlot.tsx";
import { PaletteSlot } from "./palette/PaletteSlot.tsx";
import { WorkspaceLayout } from "./WorkspaceLayout.tsx";

export function WorkspacePage() {
  return (
    <StoreProvider>
      <DialogHost>
        <WorkspaceLayout />
        <PaletteSlot />
        <DialogsSlot />
      </DialogHost>
    </StoreProvider>
  );
}
