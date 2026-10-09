import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";

const uiRoot = path.resolve(import.meta.dirname, "..");
const repoRoot = path.resolve(uiRoot, "../../..");
const outDir = process.env.FLASHWRIGHT_SCREENSHOT_DIR
  ? path.resolve(process.env.FLASHWRIGHT_SCREENSHOT_DIR)
  : path.join(repoRoot, "docs/wizard/screenshots");
const previewUrl = "http://127.0.0.1:1421/";

const chrome = [
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
].find((candidate) => existsSync(candidate));

if (!chrome) {
  throw new Error("No system Chrome binary found");
}

function wait(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function previewReady() {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const response = await fetch(previewUrl);
      if (response.ok) {
        return;
      }
    } catch {
      // The preview server is still starting.
    }
    await wait(200);
  }
  throw new Error("Vite dev server did not start");
}

async function save(page, name) {
  const file = path.join(outDir, name);
  await page.screenshot({ path: file });
}

async function assertClean(page) {
  const text = await page.locator("body").innerText();
  const allowed =
    text
      .replaceAll("Magisk app", "")
      .replaceAll("Magisk's patcher", "")
      .replaceAll("Google Pixel", "")
      .replaceAll("trademarks of Google LLC", "")
      .replaceAll("Magisk is a project by topjohnwu", "");
  const banned = [/pixelflasher/i, /\bpixel\b/i, /magisk/i, /\bgoogle\b/i, /flintlock/i];
  for (const rule of banned) {
    if (rule.test(allowed)) {
      throw new Error(`Banned word ${rule} on the wizard`);
    }
  }
  const legacy = await page.locator("[class*='fl-'], [class*='--fl-']").count();
  if (legacy > 0) {
    throw new Error("Legacy flintlock class is on the page");
  }
}

const viteBin = path.join(uiRoot, "node_modules", "vite", "bin", "vite.js");
const preview = spawn(process.execPath, [viteBin, "--host", "127.0.0.1", "--port", "1421", "--strictPort"], {
  cwd: uiRoot,
  stdio: "inherit",
  shell: false,
});

try {
  await mkdir(outDir, { recursive: true });
  await previewReady();
  const browser = await chromium.launch({ executablePath: chrome, headless: true });
  const page = await browser.newPage({ viewport: { width: 1024, height: 680 }, deviceScaleFactor: 1 });
  await page.goto(previewUrl);
  await page.locator("[data-phase='connect']").waitFor();
  await page.evaluate(() => document.fonts.ready);
  await assertClean(page);
  await save(page, "01-connect.png");

  await page.locator("[data-serial='FWMOCK000002']").click();
  await page.getByText("This phone is not authorised").waitFor();
  await page.locator(".fw-kv").getByText("FWMOCK000002").waitFor();
  await assertClean(page);
  await save(page, "02-connect-unauthorised.png");

  await page.locator("[data-serial='FWMOCK000001']").click();
  await page.locator(".fw-statusbar").getByText("FWMOCK000001").waitFor();
  await page.locator("[data-action='next']:not([disabled])").click();
  await page.locator("[data-phase='choose']").waitFor();
  await page.locator("[data-action='choice']").check();
  await page.locator("[data-action='next']:not([disabled])").waitFor();
  await assertClean(page);
  await save(page, "03-choose.png");

  await page.locator("[data-action='next']").click();
  await page.locator("[data-phase='firmware']").waitFor();
  await page.locator("[data-action='sample']").click();
  await page.getByText("harbor-ab12cd34-full.zip").waitFor();
  await assertClean(page);
  await save(page, "04-firmware.png");

  await page.locator("[data-action='next']").click();
  await page.locator("[data-phase='review']").waitFor();
  await page.getByText("PLAN 5d7c·6d3d").waitFor();
  await page.getByText("sideload").first().waitFor();
  await assertClean(page);
  await save(page, "05-review.png");

  await page.locator("[data-action='dry-run']").click();
  await page.getByText("WOULD RUN:").first().waitFor();
  await assertClean(page);
  await save(page, "06-dry-run.png");

  await page.locator("[data-action='flash']").click();
  await page.locator("[data-action='confirm-run']").waitFor();
  if (!(await page.locator("[data-action='confirm-run']").isDisabled())) {
    throw new Error("Flash now was enabled immediately");
  }
  await assertClean(page);
  await save(page, "07-confirm-disabled.png");

  await page.waitForFunction(() => {
    const button = document.querySelector("[data-action='confirm-run']");
    return button instanceof HTMLButtonElement && !button.disabled;
  });
  await assertClean(page);
  await save(page, "08-confirm-armed.png");

  await page.locator("[data-action='confirm-run']").click();
  await page.locator("[data-phase='flash']").waitFor();
  await wait(450);
  await assertClean(page);
  await save(page, "09-flash.png");

  await page.locator("[data-phase='done']").waitFor({ timeout: 5000 });
  await assertClean(page);
  await save(page, "10-done.png");

  await page.goto(`${previewUrl}?phase=recovery`);
  await page.locator("[data-phase='recovery']").waitFor();
  await page.getByText("Switch back to slot A").waitFor();
  await page.locator(".fw-statusbar").getByText("FWMOCK000001").waitFor();
  await assertClean(page);
  await save(page, "11-recovery.png");

  await page.locator("[data-action='backups']").click();
  await page.getByRole("heading", { name: "Backups" }).waitFor();
  await assertClean(page);
  await save(page, "12-backups.png");

  const wide = await browser.newPage({ viewport: { width: 1024, height: 680 }, deviceScaleFactor: 1.5 });
  await wide.goto(previewUrl);
  await wide.locator("[data-phase='connect']").waitFor();
  await wide.evaluate(() => document.fonts.ready);
  await save(wide, "13-connect-150.png");

  await browser.close();
  console.log("screenshots ok");
} finally {
  preview.kill("SIGTERM");
}
