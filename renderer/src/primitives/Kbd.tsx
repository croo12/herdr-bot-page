import styles from "./Kbd.module.css";
import { useBridge } from "../data/BridgeContext.tsx";

/** The small keyboard-shortcut badge (⌘K, ⌘1, Esc, ...) used in the command palette and tooltips. */
export function Kbd({ children }: { readonly children: string }) {
  const { platform } = useBridge();
  const label = platform === "darwin" ? children : children.replace("⌘", children.length > 1 ? "Ctrl+" : "Ctrl");
  return <kbd className={styles["hb-kbd"]}>{label}</kbd>;
}
