import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import type { AppApiEvent } from "../index.tsx";
import type { HerdrBridge, ThemePreference, ThemeState, UpdateStatus } from "./bridge.tsx";
import { isNewerVersion } from "./version-check.tsx";

function signal<T>() {
  const listeners = new Set<(value: T) => void>();
  return {
    subscribe(listener: (value: T) => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    emit(value: T) {
      for (const listener of listeners) listener(value);
    },
  };
}
function get<T>(key: string, fallback: T): T {
  try {
    return JSON.parse(localStorage.getItem("native." + key) ?? "null") ?? fallback;
  } catch {
    return fallback;
  }
}
function set<T>(key: string, value: T): T {
  localStorage.setItem("native." + key, JSON.stringify(value));
  return value;
}

export async function createTauriBridge(): Promise<HerdrBridge> {
  const win = getCurrentWindow();
  const media = matchMedia("(prefers-color-scheme: dark)");
  const theme = (): ThemeState => {
    const preference = get<ThemePreference>("theme", "system");
    return {
      preference,
      resolved: preference === "system" ? (media.matches ? "dark" : "light") : preference,
    };
  };
  const state = async () => ({
    isFullscreen: await win.isFullscreen(),
    isMaximized: await win.isMaximized(),
  });
  const themes = signal<ThemeState>();
  const locales = signal<string | null>();
  const events = signal<AppApiEvent>();
  const updates = signal<UpdateStatus>();
  const windowStates = signal<Awaited<ReturnType<typeof state>>>();
  const zoom = signal<number>();
  const zoomFactor = () => window.visualViewport?.scale ?? 1;
  window.visualViewport?.addEventListener("resize", () => zoom.emit(zoomFactor()));
  media.addEventListener("change", () => themes.emit(theme()));
  await listen<AppApiEvent>("herdr-event", ({ payload }) => events.emit(payload));
  await win.onResized(() => void state().then(windowStates.emit));
  const currentVersion = await getVersion();
  let status: UpdateStatus = { currentVersion, state: { type: "idle" } };
  const releases = "https://github.com/croo12/herdr-bot-page/releases";
  const openExternal = (url: string) => invoke<void>("open_external", { url });
  const attachment = <T,>(action: string, path: string, extra = {}) =>
    invoke<T>("attachment", { action, path, ...extra });
  return {
    runtime: "tauri",
    platform: navigator.userAgent.includes("Windows")
      ? "win32"
      : navigator.userAgent.includes("Macintosh")
        ? "darwin"
        : "linux",
    isDev: import.meta.env.DEV,
    api: {
      call: (method, params) => invoke("call", { method, params: params ?? null }),
      subscribe: events.subscribe,
    },
    theme: {
      get initial() {
        return theme();
      },
      get: async () => theme(),
      set: async (preference) => {
        set("theme", preference);
        const next = theme();
        themes.emit(next);
        return next;
      },
      onChanged: themes.subscribe,
    },
    locale: {
      get initial() {
        return get("locale", null);
      },
      get: async () => get("locale", null),
      set: async (value) => {
        set("locale", value);
        locales.emit(value);
        return value;
      },
      onChanged: locales.subscribe,
    },
    window: {
      minimize: () => win.minimize(),
      toggleMaximize: () => win.toggleMaximize(),
      close: () => win.close(),
      getState: state,
      onStateChanged: windowStates.subscribe,
      async resizeWidth(delta) {
        const size = (await win.innerSize()).toLogical(await win.scaleFactor());
        const width = Math.max(512, Math.round(size.width + delta));
        await win.setSize(new LogicalSize(width, size.height));
        return width;
      },
      getZoomFactor: zoomFactor,
      onZoomChanged: zoom.subscribe,
    },
    openExternal,
    pickDirectory: (defaultPath) => invoke("pick", { avatar: false, defaultPath }),
    pickAvatarFile: () => invoke("pick", { avatar: true }),
    attachments: {
      stage: (name, bytes) =>
        invoke("stage", bytes, {
          headers: { "x-filename": btoa(String.fromCharCode(...new TextEncoder().encode(name))) },
        }),
      commit: async (paths) => [...paths],
      discard: (path) => attachment("discard", path),
      resolveMedia: (path) => attachment("media", path),
      readText: (path) => attachment("text", path),
      async readBytes(path, maxBytes) {
        const result = await attachment<{ kind: string; base64?: string } | null>("bytes", path, {
          limit: maxBytes,
        });
        return result?.base64 != null
          ? { kind: "bytes", bytes: Uint8Array.from(atob(result.base64), (c) => c.charCodeAt(0)) }
          : result;
      },
      download: (path, name) => attachment("download", path, { name }),
    },
    version: {
      get: async () => currentVersion,
      update: {
        getStatus: async () => status,
        async check() {
          const at = Date.now();
          try {
            const response = await fetch(
              "https://api.github.com/repos/croo12/herdr-bot-page/releases/latest",
              { signal: AbortSignal.timeout(10000) },
            );
            if (!response.ok && response.status !== 404)
              throw new Error("GitHub HTTP " + response.status);
            const tag = response.ok ? String((await response.json()).tag_name ?? "") : "";
            status = {
              currentVersion,
              state: isNewerVersion(currentVersion, tag)
                ? { type: "available", version: tag, releaseUrl: releases }
                : { type: "idle", lastCheck: { at, result: "up-to-date" } },
            };
          } catch (e) {
            status = {
              currentVersion,
              state: { type: "idle", lastCheck: { at, result: "error", errorMessage: String(e) } },
            };
          }
          updates.emit(status);
          return status;
        },
        install: async () => {
          if (status.state.type === "available") await openExternal(releases);
          return status;
        },
        onStatusChanged: updates.subscribe,
      },
    },
    onboarding: {
      getSeen: async () => get("onboarding", false),
      setSeen: async (seen) => {
        set("onboarding", seen);
      },
    },
    pins: { get: async () => get("pins", []), set: async (ids) => set("pins", [...ids]) },
  };
}
