import { useEffect, useRef } from "react";

/** Let an open suggestion list consume Escape before its enclosing dialog dismisses. */
export function useEscapeOwnership(active: boolean, onEscape: () => void): void {
  const stateRef = useRef({ active, onEscape });
  stateRef.current = { active, onEscape };

  useEffect(() => {
    function onKeyDownCapture(event: KeyboardEvent) {
      if (event.key !== "Escape" || !stateRef.current.active) return;
      event.stopImmediatePropagation();
      stateRef.current.onEscape();
    }
    document.addEventListener("keydown", onKeyDownCapture, true);
    return () => document.removeEventListener("keydown", onKeyDownCapture, true);
  }, []);
}
