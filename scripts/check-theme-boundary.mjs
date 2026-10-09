import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

const uiRoot = path.resolve(import.meta.dirname, "../apps/flashwright-gui/ui");
const root = path.join(uiRoot, "src");
const themeRoot = path.join(uiRoot, "theme");
const banned = [
  /#[0-9a-fA-F]{3,8}\b/,
  /\brgb\s*\(/,
  /\bhsl\s*\(/,
  /font-family\s*:/,
  /IBM Plex/i,
  /Tahoma/i,
  /(?:^|[^a-z])fl-/,
  /--fl-/,
  /pixelflasher/i,
  /flintlock/i,
];

const allowedName = /Magisk app|Magisk's patcher|Google Pixel|trademarks of Google LLC|Magisk is a project by topjohnwu/i;

async function filesIn(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const found = [];
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      found.push(...(await filesIn(full)));
    } else if (/\.(ts|js|html)$/.test(entry.name)) {
      found.push(full);
    }
  }
  return found;
}

function classTokens(value) {
  return value.split(/\s+/).filter((token) => token.length > 0);
}

async function filesNamed(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const found = [];
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      found.push(...(await filesNamed(full)));
    } else {
      found.push(full);
    }
  }
  return found;
}

const hits = [];
for (const file of [...(await filesNamed(root)), ...(await filesNamed(themeRoot))]) {
  const name = path.basename(file).toLowerCase();
  if (name.includes("fl-") || name.includes("pixelflasher") || name.includes("flintlock")) {
    hits.push(`${path.relative(process.cwd(), file)} file name uses a banned mark`);
  }
}

for (const file of await filesIn(root)) {
  const text = await readFile(file, "utf8");
  const lines = text.split("\n");
  lines.forEach((line, index) => {
    const names = line.replace(/fw-pixel/g, "");
    if (/\b(magisk|google|pixel|android)\b/i.test(names) && !allowedName.test(line)) {
      hits.push(`${path.relative(process.cwd(), file)}:${index + 1} descriptive name outside the allowed phrases ${line.trim()}`);
    }
    for (const rule of banned) {
      if (rule.test(line)) {
        hits.push(`${path.relative(process.cwd(), file)}:${index + 1} ${rule} ${line.trim()}`);
      }
    }
    if (file.endsWith("views.ts") || file.endsWith("main.ts")) {
      for (const match of line.matchAll(/class:\s*"([^"]*)"/g)) {
        for (const token of classTokens(match[1])) {
          if (!/^fw-[a-z0-9_-]+$/.test(token)) {
            hits.push(`${path.relative(process.cwd(), file)}:${index + 1} view class ${token} is outside fw-`);
          }
        }
      }
    }
  });
}

function selectorLine(line) {
  return line.replace(/\/\*.*?\*\//g, "").replace(/url\((?:[^()"]|"[^"]*"|'[^']*')*\)/g, "");
}

const cssFiles = (await filesNamed(themeRoot)).filter((file) => file.endsWith(".css"));
for (const file of cssFiles) {
  const text = await readFile(file, "utf8");
  const lines = text.split("\n");
  lines.forEach((line, index) => {
    if (/(?:^|[^a-z])fl-|pixelflasher|flintlock/i.test(line)) {
      hits.push(`${path.relative(process.cwd(), file)}:${index + 1} banned mark ${line.trim()}`);
    }
    for (const match of selectorLine(line).matchAll(/\.([_a-zA-Z][\w-]*)/g)) {
      if (!match[1].startsWith("fw-")) {
        hits.push(`${path.relative(process.cwd(), file)}:${index + 1} css class ${match[1]} is outside fw-`);
      }
    }
  });
}

if (hits.length > 0) {
  console.error(hits.join("\n"));
  process.exit(1);
}

console.log("theme boundary ok");
