import { execFileSync, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { chromium } from "playwright-core";

const uiRoot = path.resolve(import.meta.dirname, "..");
const base = "http://127.0.0.1:1420/";

function which(name) {
  try {
    const finder = process.platform === "win32" ? "where" : "which";
    const output = execFileSync(finder, [name], { encoding: "utf8" });
    return output
      .split(/\r?\n/)
      .map((line) => line.trim())
      .find((line) => line.length > 0);
  } catch {
    return undefined;
  }
}

function chromePath() {
  const fromEnv = process.env.FLASHWRIGHT_CHROME;
  if (fromEnv && existsSync(fromEnv)) {
    return fromEnv;
  }
  const candidates = [
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
  ];
  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }
  return which("google-chrome") ?? which("google-chrome-stable") ?? which("chromium") ?? which("msedge");
}

const chrome = chromePath();
if (!chrome) {
  throw new Error("No system Chrome or Edge binary found");
}

function wait(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function ready() {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const response = await fetch(base);
      if (response.ok) {
        return;
      }
    } catch {
      // The dev server is still starting.
    }
    await wait(200);
  }
  throw new Error("Vite dev server did not start");
}

async function phaseOf(page) {
  return page.locator(".fw-app").getAttribute("data-phase");
}

async function reachReview(page) {
  await page.goto(base);
  await page.locator("[data-phase='connect']").waitFor();
  const steps = await page.locator(".fw-steps li").count();
  if (steps !== 5) {
    throw new Error(`expected five wizard steps, found ${steps}`);
  }
  await page.locator("[data-serial='FWMOCK000001']").click();
  await page.locator("[data-action='next']:not([disabled])").click();
  await page.locator("[data-phase='choose']").waitFor();
  await page.locator("[data-action='choice']").check();
  await page.locator("[data-action='next']:not([disabled])").click();
  await page.locator("[data-phase='firmware']").waitFor();
  await page.locator("[data-action='sample']").click();
  await page.locator("[data-action='next']:not([disabled])").click();
  await page.locator("[data-phase='review']").waitFor();
}

async function openFlash(page) {
  await page.locator("[data-action='flash']").click();
  await page.locator("[role='alertdialog']").waitFor();
  await page.waitForFunction(() => document.activeElement?.getAttribute("data-action") === "cancel-dialog");
}

async function assertDialogCopy(page) {
  const title = await page.locator("#confirm-title").innerText();
  if (!/Flash harbor slot [AB]\?/.test(title)) {
    throw new Error(`unexpected confirm title: ${title}`);
  }
  const body = await page.locator("#confirm-body").innerText();
  if (!/backup/i.test(body)) {
    throw new Error(`confirm body does not mention the backup: ${body}`);
  }
}

async function assertInside(page) {
  const action = await page.evaluate(() => document.activeElement?.getAttribute("data-action"));
  if (action !== "cancel-dialog" && action !== "confirm-run") {
    throw new Error(`focus left the confirm dialog (${action ?? "none"})`);
  }
}

const viteBin = path.join(uiRoot, "node_modules", "vite", "bin", "vite.js");
const server = spawn(process.execPath, [viteBin, "--host", "127.0.0.1", "--port", "1420", "--strictPort"], {
  cwd: uiRoot,
  stdio: "inherit",
  shell: false,
});
let serverGone = false;
server.on("error", (error) => {
  console.error(error);
  process.exit(1);
});
server.on("exit", () => {
  serverGone = true;
});

try {
  await ready();
  if (serverGone) {
    throw new Error("The dev server exited before the checks started");
  }
  const browser = await chromium.launch({
    executablePath: chrome,
    headless: true,
    args: ["--no-sandbox", "--disable-dev-shm-usage"],
  });
  const page = await browser.newPage({ viewport: { width: 1024, height: 680 }, deviceScaleFactor: 1 });

  await reachReview(page);
  const dryClass = await page.locator("[data-action='dry-run']").getAttribute("class");
  if (!dryClass?.split(/\s+/).includes("fw-btn--default")) {
    throw new Error("Dry run is not the default button on a dry plan");
  }
  const flashClass = await page.locator("[data-action='flash']").getAttribute("class");
  if (flashClass?.split(/\s+/).includes("fw-btn--default")) {
    throw new Error("Flash is the default button on a dry plan");
  }
  await openFlash(page);
  await assertDialogCopy(page);
  if ((await page.locator(".fw-app > .fw-window").getAttribute("inert")) === null) {
    throw new Error("the wizard stayed active behind the confirm dialog");
  }
  if (!(await page.locator("[data-action='confirm-run']").isDisabled())) {
    throw new Error("Flash now was enabled immediately");
  }
  const focused = await page.evaluate(() => document.activeElement?.getAttribute("data-action"));
  if (focused !== "cancel-dialog") {
    throw new Error(`Cancel was not focused (${focused ?? "none"})`);
  }
  await page.waitForTimeout(2200);
  const still = await page.evaluate(() => document.activeElement?.getAttribute("data-action"));
  if (still !== "cancel-dialog") {
    throw new Error(`focus moved while Flash now armed (${still ?? "none"})`);
  }
  if (await page.locator("[data-action='confirm-run']").isDisabled()) {
    throw new Error("Flash now stayed disabled after the arming delay");
  }

  for (let step = 0; step < 40; step += 1) {
    await page.keyboard.press("Tab");
    await assertInside(page);
  }
  await page.keyboard.press("Escape");
  await page.locator("[role='alertdialog']").waitFor({ state: "detached" });
  if ((await phaseOf(page)) !== "review") {
    throw new Error("Escape left the review step");
  }

  await openFlash(page);
  await page.waitForFunction(() => {
    const button = document.querySelector("[data-action='confirm-run']");
    return button instanceof HTMLButtonElement && !button.disabled;
  });
  await page.locator("[data-action='confirm-run']").focus();
  await page.keyboard.press("Enter");
  await page.locator("[role='alertdialog']").waitFor({ state: "detached" });
  if ((await phaseOf(page)) !== "review") {
    throw new Error("Enter confirmed the flash instead of cancelling");
  }

  await openFlash(page);
  await page.keyboard.press("Space");
  await page.locator("[role='alertdialog']").waitFor({ state: "detached" });
  if ((await phaseOf(page)) !== "review") {
    throw new Error("Space while Flash now was disarmed left the review step");
  }

  await openFlash(page);
  await page.keyboard.down("Alt");
  await page.keyboard.press("f");
  await page.keyboard.press("f");
  await page.keyboard.up("Alt");
  await wait(250);
  if ((await page.locator("[role='alertdialog']").count()) !== 1) {
    throw new Error("Alt+F closed the confirm dialog before Flash now was armed");
  }
  if ((await phaseOf(page)) !== "review") {
    throw new Error("Alt+F started a flash before Flash now was armed");
  }

  await page.keyboard.press("Escape");
  await page.locator("[role='alertdialog']").waitFor({ state: "detached" });
  await page.locator("[data-action='back']").click();
  await page.locator("[data-phase='firmware']").waitFor();
  await page.locator("[data-action='prepare-patch']").click();
  await page.locator("[role='alertdialog']").waitFor();
  await page.waitForFunction(() => document.activeElement?.getAttribute("data-action") === "cancel-dialog");
  const patchTitle = await page.locator("#confirm-title").innerText();
  if (patchTitle !== "Patch on your phone?") {
    throw new Error(`unexpected patch title: ${patchTitle}`);
  }
  if (await page.locator("[data-action='confirm-patch']").isDisabled()) {
    throw new Error("Patch now was delayed");
  }
  const shortcut = await page.locator("[data-action='confirm-patch']").getAttribute("aria-keyshortcuts");
  if (shortcut !== "Alt+P") {
    throw new Error(`Patch now shortcut is ${shortcut ?? "missing"}`);
  }
  await page.locator("[data-action='confirm-patch']").click();
  await page.locator("[role='alertdialog']").waitFor({ state: "detached" });
  if ((await phaseOf(page)) !== "firmware") {
    throw new Error("A patch confirm left the firmware step");
  }
  const patchText = await page.locator(".fw-app").innerText();
  if (patchText.includes("Updated to")) {
    throw new Error("A patch confirm said the phone was updated");
  }
  if (!patchText.includes("Patch ready")) {
    throw new Error("A patch confirm did not report that the patch is ready");
  }

  await page.locator("[data-action='next']:not([disabled])").click();
  await page.locator("[data-phase='review']").waitFor();
  await page.locator("[data-action='dry-run']").click();
  await page.locator("[data-action='dry-run']").waitFor({ state: "detached" });
  const realFlash = await page.locator("[data-action='flash']").getAttribute("class");
  if (!realFlash?.split(/\s+/).includes("fw-btn--default")) {
    throw new Error("Flash is not the default button after the dry run");
  }
  await page.locator("[data-action='flash']").click();
  await page.locator("[role='alertdialog']").waitFor();
  await page.waitForFunction(() => {
    const button = document.querySelector("[data-action='confirm-run']");
    return button instanceof HTMLButtonElement && !button.disabled;
  });
  await page.locator("[data-action='confirm-run']").click();
  await page.locator("[data-phase='flash']").waitFor();
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => document.activeElement?.getAttribute("data-action") === "cancel-stop");
  for (let step = 0; step < 4; step += 1) {
    await page.keyboard.press("Tab");
    const action = await page.evaluate(() => document.activeElement?.getAttribute("data-action"));
    if (action !== "cancel-stop" && action !== "stop-confirm") {
      throw new Error(`focus left the stop dialog (${action ?? "none"})`);
    }
  }
  await page.locator("[data-action='stop-confirm']").click();
  await page.locator("[data-phase='done']").waitFor();
  const finished = await page.locator(".fw-wizard__title").innerText();
  if (!finished.includes("Updated to")) {
    throw new Error(`stopping a finished job cleared the result (${finished})`);
  }

  const scaled = await browser.newPage({ viewport: { width: 1024, height: 680 }, deviceScaleFactor: 1.5 });
  await reachReview(scaled);
  await openFlash(scaled);
  await assertDialogCopy(scaled);
  const box = await scaled.locator("#confirm-title").boundingBox();
  if (!box || box.width < 40 || box.height < 8 || box.y < 0 || box.y + box.height > 680) {
    throw new Error("the confirm title is outside the window at 150% scale");
  }

  await browser.close();
  console.log("keyboard checks ok");
} finally {
  server.kill("SIGTERM");
}
