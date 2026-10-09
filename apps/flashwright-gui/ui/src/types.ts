export type Phase =
  | "connect"
  | "choose"
  | "firmware"
  | "review"
  | "flash"
  | "done"
  | "recovery";

export type Mode =
  | "adb"
  | "recovery"
  | "sideload"
  | "rescue"
  | "fastboot"
  | "fastbootd"
  | "unauthorized"
  | "no_permissions"
  | "offline";

export type Route = "ota" | "factory_keep_data";

export interface DeviceSummary {
  serial: string;
  mode: Mode;
  model: string;
  codename: string;
}

export interface DeviceInfo extends DeviceSummary {
  build_id: string;
  fingerprint: string;
  spl: string;
  active_slot: "a" | "b" | null;
  bootloader_unlocked: boolean;
  bootloader_version: string;
  root_present: boolean;
  root_tool_version: string;
  root_tool_code: number;
  uses_init_boot: boolean;
  battery_percent: number;
  battery_charging: boolean;
}

export interface GateView {
  id: string;
  severity: string;
  status: string;
  title: string;
  evidence: string;
}

export interface Step {
  idx: number;
  id: string;
  class: "read" | "write";
  tool: string;
  argv: string[];
  timeout_s: number;
}

export interface PlanPreview {
  plan_hash: string;
  plan_code: string;
  expires_unix_ms: number;
  route: Route;
  target_slot: "a" | "b";
  codename: string;
  build_id: string;
  backup_set_id: string;
  gates: GateView[];
  steps: Step[];
  prefer_dry_run: boolean;
}

export interface FirmwareReport {
  id: string;
  name: string;
  route: Route;
  sha256: string;
  codename: string;
  build_id: string;
  patched_ready: boolean;
  partition: string;
}

export interface LogLine {
  ts: string;
  level: string;
  text: string;
}

export interface RecoveryOption {
  id: string;
  title: string;
  detail: string;
}

export interface JobView {
  state: string;
  progress: number;
  status_line: string;
  lines: LogLine[];
  result_title: string;
  result_body: string;
  recovery: RecoveryOption[];
  cancel_mode: string;
}

export interface Notice {
  level: string;
  message: string;
  gates: GateView[];
}

export interface Snapshot {
  phase: Phase;
  tools: { version: string; classification: string; message: string };
  driver: { state: string; message: string };
  devices: DeviceSummary[];
  selected: DeviceInfo | null;
  choice: {
    action: string;
    route: Route;
    prefer_dry_run: boolean;
    slot_label: string;
    both_slots_enabled: boolean;
    both_slots_reason: string;
    root_tool_label: string;
    root_tool_version: string;
    backup_folder: string;
  };
  firmware: FirmwareReport | null;
  plan: PlanPreview | null;
  job: JobView;
  backups: { set_id: string; label: string; size_label: string; verified: boolean }[];
  notice: Notice | null;
  links: { id: string; label: string }[];
}

export interface EngineApi {
  snapshot(): Promise<Snapshot>;
  scan(): Promise<Snapshot>;
  selectDevice(serial: string): Promise<Snapshot>;
  continueFromConnect(): Promise<Snapshot>;
  setChoice(action: string, route: Route, preferDryRun: boolean): Promise<Snapshot>;
  continueFromChoose(): Promise<Snapshot>;
  openFirmware(name: string, sha256: string): Promise<Snapshot>;
  buildPlan(): Promise<Snapshot>;
  dryRun(planHash: string): Promise<Snapshot>;
  confirmAndRun(planHash: string): Promise<Snapshot>;
  back(): Promise<Snapshot>;
  openExternal(urlId: string): Promise<Snapshot>;
  recoveryPlan(optionId: string): Promise<Snapshot>;
}
