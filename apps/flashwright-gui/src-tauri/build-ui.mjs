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

// Spawn the npm JavaScript entry point. Windows rejects CreateProcess on
// npm.cmd when shell is left off, and the shell stays off here.
function runNpm(args) {
  if (process.platform !== "win32") {
    return spawnSync("npm", args, { cwd: uiDir, stdio: "inherit", shell: false });
  }
  const cli = path.join(path.dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js");
  if (!existsSync(cli)) {
    console.error(`npm-cli.js was not found next to node (${cli})`);
    process.exit(1);
  }
  return spawnSync(process.execPath, [cli, ...args], {
    cwd: uiDir,
    stdio: "inherit",
    shell: false,
  });
}

const result = runNpm(["run", mode]);

if (result.error) {
  console.error(result.error);
  process.exit(1);
}

process.exit(result.status ?? 1);
