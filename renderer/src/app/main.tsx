import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { applyThemeAttribute } from "#shared/lib/theme/useTheme.tsx";
import "./styles/shadcn.css";
import "./styles/tokens.css";

async function resolveBridge() {
  const isDemo = new URLSearchParams(window.location.search).has("demo");
  if (!isDemo && "__TAURI_INTERNALS__" in window) {
    const { createTauriBridge } = await import("#shared/api/desktop/tauri-bridge.tsx");
    return createTauriBridge();
  }
  const { createFakeBridge } = await import("./dev/fake-bridge.tsx");
  return createFakeBridge();
}

async function main() {
  const bridge = await resolveBridge();
  // Apply the saved theme before React renders.
  applyThemeAttribute(bridge.theme.initial.resolved);

  const root = document.getElementById("root");
  if (root == null) throw new Error("#root missing from index.html");

  createRoot(root).render(
    <StrictMode>
      <App bridge={bridge} />
    </StrictMode>,
  );
}

void main().catch((error: unknown) => {
  const root = document.getElementById("root");
  if (root) root.textContent = "herdr-bot: " + String(error);
});
