import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";

test("renderer rules check resolved imports, including aliases, types and dynamic imports", t => {
  const fixture = mkdtempSync(join(tmpdir(), "herdr-architecture-"));
  t.after(() => rmSync(fixture, { recursive: true, force: true }));
  const sources = {
    "pages/workspace/index.tsx": 'export * from "./ui.tsx";',
    "pages/workspace/ui.tsx": 'import "#features/chat"; import "#shared/ok.tsx";',
    "features/chat/index.tsx": 'export * from "./model.tsx";',
    "features/chat/model.tsx": "export type Chat = string;",
    "features/other/index.tsx": 'import "#features/chat";',
    "shared/bad.tsx": 'export * from "#pages/workspace";',
    "shared/ok.tsx": "export {};",
    "app/alias.tsx": 'import type { Chat } from "@/features/chat/model.tsx";',
    "app/relative.tsx": 'void import("../features/chat/model.tsx");',
    "app/public.tsx": 'import "@/features/chat/index.tsx";',
    "app/sdk.tsx": 'import "@tauri-apps/api/core";',
    "shared/api/desktop/tauri-bridge.tsx": 'import "@tauri-apps/api/core";',
    "shared/cycle-a.tsx": 'import "./cycle-b.tsx";',
    "shared/cycle-b.tsx": 'import "./cycle-a.tsx";',
  };
  for (const [file, source] of Object.entries(sources)) {
    const target = join(fixture, "renderer/src", file);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, source);
  }
  for (const file of ["tsconfig.base.json", "renderer/tsconfig.json", "renderer/package.json"]) {
    copyFileSync(file, join(fixture, file));
  }
  const sdk = join(fixture, "node_modules/@tauri-apps/api");
  mkdirSync(sdk, { recursive: true });
  writeFileSync(join(sdk, "core.js"), "export {};");
  const result = spawnSync(process.execPath, [
    resolve("node_modules/dependency-cruiser/bin/dependency-cruiser.mjs"),
    "--config", resolve("scripts/architecture/renderer.mjs"), "--output-type", "json", "renderer/src",
  ], { cwd: fixture, encoding: "utf8" });
  assert.ifError(result.error);
  assert.ok(result.stdout, result.stderr);
  const { violations } = JSON.parse(result.stdout).summary;
  const findings = violations.map(issue => [issue.from.replace("renderer/src/", ""), issue.rule.name].join(": "));
  assert.deepEqual(findings.sort(), [
    "features/other/index.tsx: features/chat: no sibling slice imports",
    "shared/bad.tsx: shared: no upward imports",
    "app/alias.tsx: features/chat: use index.tsx",
    "app/relative.tsx: features/chat: use index.tsx",
    "app/sdk.tsx: use the desktop bridge for Tauri",
    "shared/cycle-a.tsx: no renderer import cycles",
  ].sort());
});
