import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "../apps/flashwright-gui/ui/src");
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
  /(?<!fw-)\bpixel\b/i,
  /magisk/i,
  /\bgoogle\b/i,
  /flintlock/i,
];

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

const hits = [];
for (const file of await filesIn(root)) {
  const text = await readFile(file, "utf8");
  const lines = text.split("\n");
  lines.forEach((line, index) => {
    for (const rule of banned) {
      if (rule.test(line)) {
        hits.push(`${path.relative(process.cwd(), file)}:${index + 1} ${rule} ${line.trim()}`);
      }
    }
  });
}

if (hits.length > 0) {
  console.error(hits.join("\n"));
  process.exit(1);
}

console.log("theme boundary ok");
