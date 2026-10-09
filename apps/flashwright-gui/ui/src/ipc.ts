import { invoke } from "@tauri-apps/api/core";

import type { EngineApi, Route, Snapshot } from "./types";

function hasTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const tauriApi: EngineApi = {
  snapshot: () => invoke<Snapshot>("engine_snapshot"),
  scan: () => invoke<Snapshot>("scan"),
  selectDevice: (serial) => invoke<Snapshot>("select_device", { serial }),
  continueFromConnect: () => invoke<Snapshot>("continue_from_connect"),
  setChoice: (action, route: Route, preferDryRun) =>
    invoke<Snapshot>("set_choice", { action, route, dryRunFirst: preferDryRun }),
  continueFromChoose: () => invoke<Snapshot>("continue_from_choose"),
  openFirmware: (name, sha256) => invoke<Snapshot>("firmware_open", { firmwareId: name, publishedSha256: sha256 }),
  buildPlan: () => invoke<Snapshot>("build_plan"),
  dryRun: (planHash) => invoke<Snapshot>("dry_run", { planHash }),
  confirmAndRun: (planHash) => invoke<Snapshot>("confirm_and_run", { planHash }),
  back: () => invoke<Snapshot>("back"),
  openExternal: (urlId) => invoke<Snapshot>("open_external", { urlId }),
  recoveryPlan: (optionId) => invoke<Snapshot>("recovery_plan", { optionId }),
  cancel: () => invoke<Snapshot>("cancel"),
};

export function runningInTauri(): boolean {
  return hasTauri();
}

export function pickPackage(): Promise<string | null> {
  if (!hasTauri()) {
    return Promise.resolve(null);
  }
  return invoke<string | null>("pick_package");
}

export async function createEngine(): Promise<EngineApi> {
  if (hasTauri()) {
    return tauriApi;
  }
  if (import.meta.env.DEV) {
    const { MockEngine } = await import("./mock");
    const preview = new URLSearchParams(window.location.search).get("phase");
    return new MockEngine(preview === "recovery" ? "recovery" : null);
  }
  return tauriApi;
}
