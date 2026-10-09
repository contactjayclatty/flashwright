import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

const dist = path.resolve(import.meta.dirname, "../dist");
const banned = ["FWMOCK", "ab12cd34", "harbor-ab12cd34"];

async function filesIn(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const found = [];
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      found.push(...(await filesIn(full)));
    } else if (/\.(js|css|html)$/.test(entry.name)) {
      found.push(full);
    }
  }
  return found;
}

const files = await filesIn(dist);
if (files.length === 0) {
  throw new Error("The release bundle is empty");
}

const hits = [];
for (const file of files) {
  const text = await readFile(file, "utf8");
  for (const needle of banned) {
    if (text.includes(needle)) {
      hits.push(`${path.relative(dist, file)} contains ${needle}`);
    }
  }
}

if (hits.length > 0) {
  console.error(hits.join("\n"));
  process.exit(1);
}

console.log("release bundle has no sample phone");
