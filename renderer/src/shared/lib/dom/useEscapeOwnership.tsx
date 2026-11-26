import { useEffect, useEffectEvent } from "react";

/** Let an open suggestion list consume Escape before its enclosing dialog dismisses. */
export function useEscapeOwnership(active: boolean, onEscape: () => void): void {
  const handleEscape = useEffectEvent((event: KeyboardEvent) => {
    if (event.key !== "Escape" || !active) return;
    event.stopImmediatePropagation();
    onEscape();
  });

  useEffect(() => {
    function onKeyDownCapture(event: KeyboardEvent) {
      handleEscape(event);
    }
    document.addEventListener("keydown", onKeyDownCapture, true);
    return () => document.removeEventListener("keydown", onKeyDownCapture, true);
  }, []);
}
