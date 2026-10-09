import { Channel, invoke } from "@tauri-apps/api/core";

import type { EngineApi, FirmwareRef, Route, Snapshot } from "./types";

function hasTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const tauriApi: EngineApi = {
  snapshot: () => invoke<Snapshot>("engine_snapshot"),
  subscribe: () => {
    const channel = new Channel();
    return invoke<void>("subscribe", { channel });
  },
  toolsStatus: () => invoke<Snapshot>("tools_status"),
  toolsPickFolder: () => invoke<Snapshot>("tools_pick_folder"),
  toolsImportZip: () => invoke<Snapshot>("tools_import_zip"),
  scan: () => invoke<Snapshot>("scan"),
  selectDevice: (serial) => invoke<Snapshot>("select_device", { serial }),
  continueFromConnect: () => invoke<Snapshot>("continue_from_connect"),
  setChoice: (action, route: Route, preferDryRun) =>
    invoke<Snapshot>("set_choice", { action, route, dryRunFirst: preferDryRun }),
  continueFromChoose: () => invoke<Snapshot>("continue_from_choose"),
  back: () => invoke<Snapshot>("back"),
  pickFirmware: () => invoke<FirmwareRef>("pick_firmware"),
  openFirmware: (name, sha256) => invoke<Snapshot>("firmware_open", { firmwareId: name, publishedSha256: sha256 }),
  preparePatch: (firmwareId) => invoke<Snapshot>("prepare_patch", { firmwareId }),
  buildPlan: () => invoke<Snapshot>("build_plan"),
  ackGate: (planHash, gateId) => invoke<Snapshot>("ack_gate", { planHash, gateId }),
  dryRun: (planHash) => invoke<Snapshot>("dry_run", { planHash }),
  confirmAndRun: (planHash) => invoke<Snapshot>("confirm_and_run", { planHash }),
  cancel: () => invoke<Snapshot>("cancel"),
  recoveryPlan: (optionId) => invoke<Snapshot>("recovery_plan", { optionId }),
  backupsList: () => invoke<Snapshot>("backups_list"),
  restorePlan: (setId, item) => invoke<Snapshot>("restore_plan", { setId, item }),
  openExternal: (urlId) => invoke<Snapshot>("open_external", { urlId }),
};

export function runningInTauri(): boolean {
  return hasTauri();
}

export async function pickPackage(): Promise<string | null> {
  if (!hasTauri()) {
    return null;
  }
  try {
    const picked = await tauriApi.pickFirmware();
    return picked.display_name;
  } catch {
    return null;
  }
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
