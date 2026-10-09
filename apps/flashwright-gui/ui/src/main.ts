import { getCurrentWindow } from "@tauri-apps/api/window";

import theme from "../theme/theme.json";
import "../theme/flashwright-ui.css";
import "../theme/fonts.css";
import "../theme/extras.css";
import "../theme/dark.css";
import { createEngine, pickPackage, runningInTauri } from "./ipc";
import type { EngineApi, Phase, Snapshot } from "./types";
import { logHost, mountLog, renderShell, type ShellModel } from "./views";

const EXPECTED_CSS = ["flashwright-ui.css", "fonts.css", "extras.css", "dark.css"];

function themeReady(): boolean {
  return (
    theme.name === "Flashwright" &&
    theme.css.length === EXPECTED_CSS.length &&
    theme.css.every((file, index) => file === EXPECTED_CSS[index])
  );
}

class Wizard {
  private readonly engine: EngineApi;
  private snap: Snapshot | null = null;
  private shown = 0;
  private playing = false;
  private jobKey = "";
  private playTimer = 0;
  private confirmUntil = 0;
  private confirmOpen = false;
  private confirmFocus = false;
  private armTimer = 0;
  private overlay: ShellModel["overlay"] = "none";
  private stopAsk = false;
  private stopFocus = false;
  private localError: string | null = null;
  private readonly themeError: string | null = themeReady()
    ? null
    : "The theme manifest does not match this window.";
  private draftName = "";
  private draftSha = "";
  private dryPreference = true;
  private readonly tauri = runningInTauri();

  constructor(engine: EngineApi) {
    this.engine = engine;
  }

  async start(): Promise<void> {
    document.addEventListener("click", (event) => {
      void this.onClick(event);
    });
    document.addEventListener("change", (event) => {
      void this.onChange(event);
    });
    document.addEventListener("input", (event) => {
      this.onInput(event);
    });
    document.addEventListener("keydown", (event) => {
      this.onKey(event);
    });
    document.addEventListener("keyup", (event) => {
      if (event.key === "Alt") {
        document.querySelector(".fw-app")?.classList.remove("fw-alt");
      }
    });
    const stored = sessionStorage.getItem("fw-theme");
    if (stored === "dark") {
      this.applyTheme("dark");
    }
    await this.refresh(this.engine.snapshot());
  }

  private visiblePhase(): Phase {
    if (!this.snap) {
      return "connect";
    }
    if (this.playing && this.shown < this.snap.job.lines.length) {
      return "flash";
    }
    return this.snap.phase;
  }

  private model(): ShellModel | null {
    if (!this.snap) {
      return null;
    }
    const total = this.snap.job.lines.length;
    const writing = this.playing && this.shown < total;
    const progress = writing && total > 0 ? Math.round((this.shown / total) * 100) : this.snap.job.progress;
    const armed = this.confirmOpen && Date.now() >= this.confirmUntil;
    const secondsLeft = this.confirmOpen ? Math.max(0, Math.ceil((this.confirmUntil - Date.now()) / 1000)) : 0;
    return {
      snap: this.snap,
      visiblePhase: this.visiblePhase(),
      lines: this.snap.job.lines.slice(0, this.shown),
      progress,
      writing,
      draftName: this.draftName,
      draftSha: this.draftSha,
      dryPreference: this.dryPreference,
      confirm: this.confirmOpen ? { secondsLeft, armed } : null,
      overlay: this.overlay,
      stopAsk: this.stopAsk,
      localError: this.themeError ?? this.localError,
      tauri: this.tauri,
    };
  }

  private render(): void {
    const model = this.model();
    const root = document.getElementById("app");
    if (!model || !root) {
      return;
    }
    const dialogLive = root.querySelector("[role='alertdialog']");
    const countdownOnly = this.confirmOpen && dialogLive !== null && !this.confirmFocus;
    if (countdownOnly) {
      this.patchConfirm(root);
      this.markBackground(root);
      return;
    }
    root.replaceChildren(renderShell(model));
    const themeName = sessionStorage.getItem("fw-theme");
    if (themeName === "dark") {
      root.querySelector(".fw-app")?.setAttribute("data-theme", "dark");
    }
    const host = logHost(root);
    if (host) {
      mountLog(host, model.lines);
    }
    this.markBackground(root);
    if (this.confirmFocus) {
      this.confirmFocus = false;
      window.requestAnimationFrame(() => {
        document.querySelector<HTMLButtonElement>(".fw-dialog .fw-btn--default")?.focus();
      });
    } else if (this.stopFocus) {
      this.stopFocus = false;
      window.requestAnimationFrame(() => {
        document.querySelector<HTMLButtonElement>("[data-action='cancel-stop']")?.focus();
      });
    }
  }

  private patchConfirm(root: ParentNode): void {
    const button = root.querySelector<HTMLButtonElement>("[data-action='confirm-run'], [data-action='confirm-patch']");
    if (!button) {
      return;
    }
    const armed = Date.now() >= this.confirmUntil;
    const patch = button.dataset.action === "confirm-patch";
    button.disabled = !armed;
    const secondsLeft = Math.max(0, Math.ceil((this.confirmUntil - Date.now()) / 1000));
    const rest = patch ? "atch now" : armed ? "lash now" : `lash now (${secondsLeft})`;
    button.textContent = "";
    const mark = document.createElement("span");
    mark.className = "fw-key";
    mark.textContent = patch ? "P" : "F";
    button.append(mark, rest);
  }

  private markBackground(root: ParentNode): void {
    const windowEl = root.querySelector(".fw-app > .fw-window");
    const dialog = root.querySelector("[role='alertdialog'], [role='dialog']");
    if (windowEl instanceof HTMLElement) {
      if (dialog) {
        windowEl.setAttribute("inert", "");
      } else {
        windowEl.removeAttribute("inert");
      }
    }
  }

  private noteJob(snap: Snapshot): void {
    const key = `${snap.phase}:${snap.job.state}:${snap.job.lines.length}:${snap.job.result_title}`;
    if (key === this.jobKey) {
      return;
    }
    this.jobKey = key;
    window.clearTimeout(this.playTimer);
    if (snap.job.state === "cancelled") {
      this.playing = false;
      return;
    }
    if ((snap.phase === "done" || snap.phase === "flash") && snap.job.state === "succeeded" && snap.job.lines.length > 0) {
      this.shown = 0;
      this.playing = true;
      this.playToward(snap.job.lines.length);
      return;
    }
    this.playing = false;
    this.shown = snap.job.lines.length;
  }

  private playToward(total: number): void {
    const tick = (): void => {
      if (!this.playing) {
        return;
      }
      this.shown += 1;
      this.render();
      if (this.shown < total) {
        this.playTimer = window.setTimeout(tick, 120);
      } else {
        this.playing = false;
        this.render();
      }
    };
    this.playTimer = window.setTimeout(tick, 120);
  }

  private async refresh(pending: Promise<Snapshot>): Promise<void> {
    try {
      const snap = await pending;
      this.snap = snap;
      this.localError = null;
      if (snap.firmware) {
        this.draftName = snap.firmware.name;
        this.draftSha = snap.firmware.sha256;
      }
      this.noteJob(snap);
      this.render();
    } catch (error) {
      this.localError = error instanceof Error ? error.message : "The engine refused that step.";
      this.render();
    }
  }

  private openConfirm(patch = false): void {
    this.confirmOpen = true;
    this.confirmUntil = patch ? Date.now() : Date.now() + 2000;
    this.confirmFocus = true;
    window.clearTimeout(this.armTimer);
    const tick = (): void => {
      if (!this.confirmOpen) {
        return;
      }
      this.render();
      if (Date.now() < this.confirmUntil) {
        this.armTimer = window.setTimeout(tick, 200);
      }
    };
    this.armTimer = window.setTimeout(tick, 200);
    this.render();
  }

  private applyTheme(name: "dark" | "light"): void {
    sessionStorage.setItem("fw-theme", name);
    if (name === "dark") {
      document.querySelector(".fw-app")?.setAttribute("data-theme", "dark");
    } else {
      document.querySelector(".fw-app")?.removeAttribute("data-theme");
    }
    if (this.tauri) {
      void getCurrentWindow().setTheme(name === "dark" ? "dark" : "light");
    }
  }

  private async windowControl(action: string): Promise<void> {
    if (!this.tauri) {
      return;
    }
    const window = getCurrentWindow();
    switch (action) {
      case "minimize":
        await window.minimize();
        break;
      case "maximize":
        await window.toggleMaximize();
        break;
      case "close":
        await window.close();
        break;
      default:
        break;
    }
  }

  private async onClick(event: Event): Promise<void> {
    const target = event.target;
    if (!(target instanceof Element)) {
      return;
    }
    const button = target.closest<HTMLElement>("[data-action]");
    if (!button || !this.snap) {
      return;
    }
    const action = button.dataset.action ?? "";
    switch (action) {
      case "select":
        await this.refresh(this.engine.selectDevice(button.dataset.serial ?? ""));
        break;
      case "scan":
        await this.refresh(this.engine.scan());
        break;
      case "next":
        await this.onNext();
        break;
      case "back":
        await this.refresh(this.engine.back());
        break;
      case "sample": {
        if (!import.meta.env.DEV) {
          break;
        }
        const fixtures = await import("./mock");
        this.draftName = fixtures.FIXTURE_OTA_NAME;
        this.draftSha = fixtures.FIXTURE_SHA256;
        await this.refresh(this.engine.openFirmware(this.draftName, this.draftSha));
        break;
      }
      case "check-firmware":
        await this.refresh(this.engine.openFirmware(this.draftName, this.draftSha));
        break;
      case "browse":
        await this.browse();
        break;
      case "dry-run":
        if (this.snap.plan) {
          await this.refresh(this.engine.dryRun(this.snap.plan.plan_hash));
        }
        break;
      case "prepare-patch":
        if (this.snap.firmware) {
          await this.refresh(this.engine.preparePatch(this.snap.firmware.id));
          if (this.snap?.plan?.kind === "prepare_patch") {
            this.openConfirm(true);
          }
        }
        break;
      case "flash":
        this.openConfirm(this.snap.plan?.kind === "prepare_patch");
        break;
      case "confirm-patch":
        if (this.snap.plan && Date.now() >= this.confirmUntil) {
          const hash = this.snap.plan.plan_hash;
          this.confirmOpen = false;
          await this.refresh(this.engine.confirmAndRun(hash));
        }
        break;
      case "confirm-run":
        if (this.snap.plan && Date.now() >= this.confirmUntil) {
          const hash = this.snap.plan.plan_hash;
          this.confirmOpen = false;
          await this.refresh(this.engine.confirmAndRun(hash));
        }
        break;
      case "cancel-dialog":
      case "close-overlay":
        this.confirmOpen = false;
        this.overlay = "none";
        window.clearTimeout(this.armTimer);
        this.render();
        break;
      case "cancel-job":
        this.playing = false;
        window.clearTimeout(this.playTimer);
        this.stopAsk = true;
        this.stopFocus = true;
        this.render();
        break;
      case "stop-confirm":
        this.playing = false;
        window.clearTimeout(this.playTimer);
        this.stopAsk = false;
        await this.refresh(this.engine.cancel());
        break;
      case "cancel-stop":
        this.stopAsk = false;
        this.render();
        break;
      case "backups":
        this.overlay = "backups";
        this.render();
        break;
      case "help":
        this.overlay = "help";
        this.render();
        break;
      case "theme": {
        const dark = document.querySelector(".fw-app")?.getAttribute("data-theme") === "dark";
        this.applyTheme(dark ? "light" : "dark");
        break;
      }
      case "link":
        await this.refresh(this.engine.openExternal(button.dataset.id ?? ""));
        break;
      case "recovery":
        await this.refresh(this.engine.recoveryPlan(button.dataset.id ?? ""));
        break;
      case "minimize":
      case "maximize":
      case "close":
        await this.windowControl(action);
        break;
      default:
        break;
    }
  }

  private async onNext(): Promise<void> {
    if (!this.snap) {
      return;
    }
    switch (this.snap.phase) {
      case "connect":
        await this.refresh(this.engine.continueFromConnect());
        break;
      case "choose":
        await this.refresh(this.engine.continueFromChoose());
        break;
      case "firmware":
        await this.refresh(this.engine.buildPlan());
        break;
      case "review":
      case "flash":
      case "done":
      case "recovery":
        break;
      default: {
        const unexpected: never = this.snap.phase;
        this.localError = unexpected;
      }
    }
  }

  private async onChange(event: Event): Promise<void> {
    const target = event.target;
    if (!(target instanceof HTMLInputElement) || !this.snap) {
      return;
    }
    if (target.dataset.action === "choice" && target.checked) {
      await this.refresh(this.engine.setChoice("update_keep_root", this.snap.choice.route, this.dryPreference));
      return;
    }
    if (target.dataset.action === "dry-toggle") {
      this.dryPreference = target.checked;
      if (this.snap.choice.action === "update_keep_root") {
        await this.refresh(this.engine.setChoice("update_keep_root", this.snap.choice.route, this.dryPreference));
      }
    }
  }

  private onInput(event: Event): void {
    const target = event.target;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }
    if (target.id === "firmware-name") {
      this.draftName = target.value;
    }
    if (target.id === "firmware-sha") {
      this.draftSha = target.value;
    }
  }

  private async browse(): Promise<void> {
    const name = await pickPackage();
    if (name) {
      this.draftName = name;
      this.render();
    }
  }

  private onKey(event: KeyboardEvent): void {
    if (event.key === "Alt") {
      document.querySelector(".fw-app")?.classList.add("fw-alt");
    }
    if (event.altKey && !event.repeat) {
      const key = event.key.toLowerCase();
      const action = { b: "back", n: "next", h: "help", d: "dry-run", l: "log" }[key];
      if (key === "c") {
        event.preventDefault();
        const cancel =
          document.querySelector<HTMLButtonElement>("[data-action='cancel-stop']") ??
          document.querySelector<HTMLButtonElement>("[data-action='cancel-dialog']") ??
          document.querySelector<HTMLButtonElement>("[data-action='cancel-job']") ??
          document.querySelector<HTMLButtonElement>("[data-action='close-overlay']");
        cancel?.click();
        return;
      }
      if (key === "f") {
        event.preventDefault();
        const armed = document.querySelector<HTMLButtonElement>("[data-action='confirm-run']:not(:disabled)");
        const open = document.querySelector("[role='alertdialog']")
          ? null
          : document.querySelector<HTMLButtonElement>("[data-action='flash']");
        (armed ?? open)?.click();
        return;
      }
      if (key === "p") {
        event.preventDefault();
        document.querySelector<HTMLButtonElement>("[data-action='confirm-patch']:not(:disabled)")?.click();
        return;
      }
      if (action === "log") {
        event.preventDefault();
        document.getElementById("log")?.focus();
        return;
      }
      if (action) {
        event.preventDefault();
        document.querySelector<HTMLButtonElement>(`[data-action='${action}']`)?.click();
        return;
      }
    }
    if (event.key === "Tab") {
      const dialog = document.querySelector("[role='alertdialog']");
      if (dialog) {
        const buttons = [...dialog.querySelectorAll<HTMLButtonElement>("button")].filter((button) => !button.disabled);
        if (buttons.length > 0) {
          event.preventDefault();
          const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
          const next = event.shiftKey
            ? buttons[(index - 1 + buttons.length) % buttons.length]
            : buttons[(index + 1) % buttons.length];
          next?.focus();
        }
        return;
      }
    }
    if (event.key === "Escape") {
      const cancel =
        document.querySelector<HTMLButtonElement>("[data-action='cancel-stop']") ??
        document.querySelector<HTMLButtonElement>("[data-action='cancel-dialog']") ??
        document.querySelector<HTMLButtonElement>("[data-action='close-overlay']");
      if (cancel) {
        cancel.click();
        return;
      }
      if (this.playing || this.visiblePhase() === "flash") {
        event.preventDefault();
        this.playing = false;
        window.clearTimeout(this.playTimer);
        this.stopAsk = true;
        this.stopFocus = true;
        this.render();
      }
    }
    if (event.key === "Enter" && !event.altKey && !event.shiftKey && !event.ctrlKey && !event.metaKey) {
      const target = event.target;
      if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) {
        return;
      }
      event.preventDefault();
      const root = document.querySelector(".fw-dialog") ?? document.querySelector(".fw-window");
      root?.querySelector<HTMLButtonElement>(".fw-btn--default:not(:disabled)")?.click();
    }
  }
}

void createEngine().then((engine) => new Wizard(engine).start());
