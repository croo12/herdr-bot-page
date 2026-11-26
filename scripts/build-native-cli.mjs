import { execFileSync } from "node:child_process";
import { mkdirSync, copyFileSync } from "node:fs";
import { join } from "node:path";

const release = process.argv[2] === "release";
const triple = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)[1].trim();
const target = process.env.TAURI_ENV_TARGET_TRIPLE || triple;
const ext = target.includes("windows") ? ".exe" : "";
// Use an explicit target directory so a user-wide CARGO_TARGET_DIR cannot change the copy source.
const dir = "native/target";
execFileSync("cargo", ["build", "--locked", "--manifest-path", "native/Cargo.toml", "--target-dir", dir,
  "--target", target, "--bin", "herdr-bot-cli", ...(release ? ["--release"] : [])], { stdio: "inherit" });
mkdirSync("desktop/binaries", { recursive: true });
copyFileSync(join(dir, target, release ? "release" : "debug", "herdr-bot-cli" + ext),
  join("desktop/binaries", "herdr-bot-cli-" + target + ext));
