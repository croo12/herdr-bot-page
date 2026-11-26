import type { ReactNode } from "react";
import { Dialog as DialogRoot, DialogContent, DialogTitle } from "./shadcn/dialog.tsx";

export interface DialogProps {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly title?: string;
  readonly closeLabel: string;
  readonly children: ReactNode;
  readonly width?: number;
}

export function Dialog({ open, onClose, title, closeLabel, children, width = 520 }: DialogProps) {
  return (
    <DialogRoot
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <DialogContent
        className="block max-h-[calc(100dvh-4rem)] overflow-y-auto p-6 sm:max-w-[calc(100%-2rem)]"
        style={{ width }}
        showCloseButton={title != null}
        closeLabel={closeLabel}
      >
        {title != null && <DialogTitle className="mb-5 pr-8">{title}</DialogTitle>}
        {children}
      </DialogContent>
    </DialogRoot>
  );
}
