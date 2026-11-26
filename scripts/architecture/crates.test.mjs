import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { dirname, relative, sep } from "node:path";
import test from "node:test";

test("desktop depends on native and the engine has no local crate dependencies", () => {
  for (const [directory, allowed] of [["desktop", ["native"]], ["native", []]]) {
    const manifest = `${directory}/Cargo.toml`;
    const metadata = JSON.parse(execFileSync("cargo", [
      "metadata", "--format-version", "1", "--no-deps", "--locked", "--manifest-path", manifest,
    ], { encoding: "utf8" }));
    const pkg = metadata.packages.find(item => relative(".", dirname(item.manifest_path)) === directory);
    const dependencies = pkg.dependencies.filter(item => item.path)
      .map(item => relative(".", item.path).split(sep).join("/"));
    assert.deepEqual([...new Set(dependencies)].sort(), allowed, `${manifest} local crate dependencies: ${dependencies}`);
  }
});
