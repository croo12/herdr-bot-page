import assert from "node:assert/strict";
import { globSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

test("renderer source, configuration and tests use .tsx for code", () => {
  const files = globSync("**/*.{ts,mts,cts,js,mjs,cjs,jsx}", {
    cwd: fileURLToPath(new URL("../", import.meta.url)),
    exclude: ["**/node_modules", "**/dist", "**/coverage"],
  });
  assert.deepEqual(files, [], `Use .tsx for renderer code: ${files.join(", ")}`);
});
