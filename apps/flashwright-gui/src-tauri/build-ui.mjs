import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const mode = process.argv[2];
if (mode !== "build" && mode !== "dev") {
  console.error("Usage: node build-ui.mjs <build|dev>");
  process.exit(1);
}

// This file sits next to tauri.conf.json. The UI package is the sibling folder.
const here = path.dirname(fileURLToPath(import.meta.url));
const uiDir = path.resolve(here, "../ui");
const manifest = path.join(uiDir, "package.json");
if (!existsSync(manifest)) {
  console.error(`Wizard UI package not found at ${manifest}`);
  process.exit(1);
}

const npm = process.platform === "win32" ? "npm.cmd" : "npm";
const result = spawnSync(npm, ["run", mode], {
  cwd: uiDir,
  stdio: "inherit",
  shell: false,
});

if (result.error) {
  console.error(result.error);
  process.exit(1);
}

process.exit(result.status ?? 1);
