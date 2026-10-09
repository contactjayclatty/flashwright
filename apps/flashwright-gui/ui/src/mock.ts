import type {
  DeviceInfo,
  EngineApi,
  GateView,
  JobView,
  Notice,
  Phase,
  Route,
  Snapshot,
  Step,
} from "./types";

/** Browser stand-in for flashwright-core. The Tauri host calls the Rust engine instead. */
export const FIXTURE_SHA256 =
  "ab12cd34e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
export const FIXTURE_OTA_NAME = "harbor-ab12cd34-full.zip";
export const FIXTURE_FACTORY_NAME = "harbor-ab12cd34-factory.zip";
export const PRIMARY_SERIAL = "FWMOCK000001";

const OTA_HASH = "flp1-5d7c6d3d4be43d94c6c9e09e774cc2cfbc70df5053d86b97f01719105a6015b6";
const FACTORY_HASH = "flp1-f8c1f9e13a6c822320af19636431ecebebd4942bb0ecbcc700290d5a31d2f480";

const LINKS = [
  { id: "platform_tools", label: "Open the official platform-tools page" },
  { id: "usb_driver", label: "Open the official USB driver page" },
  { id: "firmware_full", label: "Open the official full-package page" },
  { id: "firmware_factory", label: "Open the official factory-package page" },
];

function gate(id: string, severity: string, status: string, title: string, evidence: string): GateView {
  return { id, severity, status, title, evidence };
}

function passGates(): GateView[] {
  return [
    gate("G01", "block", "pass", "Platform tools", "37.0.1 is on the allow list."),
    gate("G02", "block", "pass", "One phone", "One authorised phone in the device state."),
    gate("G03", "block", "pass", "Unlocked", "The bootloader is unlocked."),
    gate("G04", "block", "pass", "Phone match", "The package matches this phone."),
    gate("G05", "block", "pass", "Checksum", "The pasted SHA-256 matches the file."),
    gate("G06", "block", "pass", "Full package", "The package is a full A/B update."),
    gate("G07", "block", "pass", "No downgrade", "The package is newer than the phone."),
    gate("G08", "block", "pass", "Patch level", "The image patch level matches the package."),
    gate("G09", "block", "pass", "Patched image", "The patched image matches the stock image that was prepared."),
    gate("G10", "block", "pass", "Root tool", "The on-device root tool is new enough."),
    gate("G11", "block", "pass", "Battery", "Battery is 82%."),
    gate("G12", "block", "pass", "Disk space", "The working disk has enough free space."),
    gate("G13", "block", "pass", "Phone space", "The phone has enough free space."),
    gate("G14", "block", "pass", "Backup", "A verified backup set exists."),
    gate("G15", "block", "pass", "Partition", "The target partition exists and is large enough."),
    gate("G16", "block", "pass", "Keep data", "The plan does not wipe data or turn off verification."),
    gate("G17", "ack", "pass", "USB driver", "The driver probe is OK."),
    gate("G18", "block", "pass", "Bootloader", "The package bootloader is not older."),
    gate("G19", "ack", "pass", "Minimum bootloader", "The phone bootloader meets the fixture minimum."),
    gate("G20", "block", "pass", "No waiting update", "No system update is waiting on the phone."),
  ];
}

function primary(): DeviceInfo {
  return {
    serial: PRIMARY_SERIAL,
    mode: "adb",
    model: "Harbor",
    codename: "harbor",
    build_id: "HQ1A.MOCK.001",
    fingerprint: "harbor/harbor/harbor:16/HQ1A.MOCK.001/100:user/release-keys",
    spl: "2026-09-05",
    active_slot: "a",
    bootloader_unlocked: true,
    bootloader_version: "harbor-1.0-100",
    root_present: true,
    root_tool_version: "30.7",
    root_tool_code: 30700,
    uses_init_boot: true,
    battery_percent: 82,
    battery_charging: false,
  };
}

function idleJob(): JobView {
  return {
    state: "idle",
    progress: 0,
    status_line: "Ready",
    lines: [],
    result_title: "",
    result_body: "",
    recovery: [],
    cancel_mode: "immediate",
  };
}

function otaSteps(name: string): Step[] {
  const serial = PRIMARY_SERIAL;
  const patched = `%LOCALAPPDATA%\\Flashwright\\cache\\patched\\${FIXTURE_SHA256.slice(0, 12)}.img`;
  const rows: Array<[number, string, "read" | "write", string[]]> = [
    [1, "preflight", "read", ["adb", "-s", serial, "shell", "getprop"]],
    [2, "backup", "read", ["backup", "verify", serial]],
    [3, "reboot_sideload", "write", ["adb", "-s", serial, "reboot", "sideload"]],
    [4, "sideload", "write", ["adb", "-s", serial, "sideload", name]],
    [5, "reboot_bootloader", "write", ["adb", "-s", serial, "reboot", "bootloader"]],
    [6, "verify_slot", "read", ["fastboot", "-s", serial, "getvar", "current-slot"]],
    [7, "flash_patched", "write", ["fastboot", "-s", serial, "--slot", "b", "flash", "init_boot", patched]],
    [8, "reboot_system", "write", ["fastboot", "-s", serial, "reboot"]],
    [9, "verify_root", "read", ["adb", "-s", serial, "shell", "id"]],
  ];
  return rows.map(([idx, id, kind, argv]) => ({
    idx,
    id,
    class: kind,
    tool: argv[0] === "fastboot" ? "fastboot" : argv[0] === "adb" ? "adb" : "internal",
    argv,
    timeout_s: 30,
  }));
}

function quote(argv: string[]): string {
  return argv
    .map((arg) => (arg.includes(" ") ? `"${arg}"` : arg))
    .join(" ");
}

export class MockEngine implements EngineApi {
  private phase: Phase = "connect";
  private selected: string | null = null;
  private actionChosen = false;
  private route: Route = "ota";
  private preferDryRun = true;
  private firmwareName = "";
  private firmwareOk = false;
  private notice: Notice | null = null;
  private job: JobView = idleJob();
  private failedGates: GateView[] = [];

  constructor(preview: "recovery" | null = null) {
    if (preview === "recovery") {
      this.phase = "recovery";
      this.selected = PRIMARY_SERIAL;
      this.actionChosen = true;
      this.firmwareOk = true;
      this.firmwareName = FIXTURE_OTA_NAME;
      this.job = {
        state: "failed",
        progress: 70,
        status_line: "The update stopped.",
        lines: [
          {
            ts: "13:41:02",
            level: "error",
            text: "Flash of the patched image failed. The phone was not rebooted.",
          },
        ],
        result_title: "The update stopped",
        result_body:
          "The patched image was not written. Pick a recovery option. Each option is its own checked plan.",
        recovery: [
          { id: "retry", title: "Retry this step", detail: "Only for a step that can be repeated safely." },
          {
            id: "stock",
            title: "Flash the stock image to slot B",
            detail: "Leaves the phone on the new build without root.",
          },
          {
            id: "switch_back",
            title: "Switch back to slot A",
            detail:
              "The original slot was not written. Anti-rollback can still block an older slot after a newer bootloader.",
          },
          {
            id: "restore",
            title: "Restore from a backup",
            detail: "Builds a restore plan for one backup item. It still needs a review and a confirm.",
          },
          {
            id: "leave",
            title: "Leave the phone in the bootloader and get help",
            detail: "Writes a plain-text report on this computer. Nothing is uploaded.",
          },
        ],
        cancel_mode: "after_step",
      };
    }
  }

  snapshot(): Promise<Snapshot> {
    const selected = this.selected === PRIMARY_SERIAL ? primary() : this.selected ? this.unauthorised() : null;
    const plan =
      this.firmwareOk && (this.phase === "review" || this.phase === "flash" || this.phase === "done" || this.phase === "recovery")
        ? this.plan()
        : null;
    return Promise.resolve({
      phase: this.phase,
      tools: {
        version: "37.0.1",
        classification: "allow",
        message: "Platform tools 37.0.1 are on the allow list.",
      },
      driver: {
        state: "ok",
        message: "USB driver looks fine for the selected phone.",
      },
      devices: [
        { serial: PRIMARY_SERIAL, mode: "adb", model: "Harbor", codename: "harbor" },
        { serial: "FWMOCK000002", mode: "unauthorized", model: "Harbor", codename: "harbor" },
      ],
      selected,
      choice: {
        action: this.actionChosen ? "update_keep_root" : "",
        route: this.route,
        prefer_dry_run: this.preferDryRun,
        slot_label: "Inactive slot (recommended)",
        both_slots_enabled: false,
        both_slots_reason:
          "Writing both slots removes your fallback and is risky with anti-rollback. Coming later in Expert mode.",
        root_tool_label: "On-device root tool",
        root_tool_version: selected?.root_present ? `stable ${selected.root_tool_version}` : "—",
        backup_folder: "%LOCALAPPDATA%\\Flashwright\\backups",
      },
      firmware: this.firmwareOk
        ? {
            id: "fw-harbor",
            name: this.firmwareName,
            route: this.route,
            sha256: FIXTURE_SHA256,
            codename: "harbor",
            build_id: "HQ1A.MOCK.002",
            patched_ready: true,
            partition: "init_boot",
          }
        : null,
      plan,
      job: this.job,
      backups: [
        {
          set_id: "4f2a9c01-0000-7000-8000-000000000001",
          label: "harbor · slot A · HQ1A.MOCK.001",
          size_label: "34 MiB",
          verified: true,
        },
      ],
      notice: this.notice,
      links: LINKS,
    });
  }

  async scan(): Promise<Snapshot> {
    this.notice = null;
    return this.snapshot();
  }

  async selectDevice(serial: string): Promise<Snapshot> {
    this.selected = serial;
    this.firmwareOk = false;
    this.job = idleJob();
    if (serial === "FWMOCK000002") {
      this.notice = {
        level: "warn",
        message: "This phone is not authorised. Unlock it and allow this computer, then scan again.",
        gates: [],
      };
    } else if (serial !== PRIMARY_SERIAL) {
      this.notice = { level: "block", message: "That phone is not in the scan.", gates: [] };
      this.selected = null;
    } else {
      this.notice = null;
    }
    return this.snapshot();
  }

  async continueFromConnect(): Promise<Snapshot> {
    if (this.selected !== PRIMARY_SERIAL) {
      this.notice = {
        level: "block",
        message: "Select one authorised phone in the normal device state.",
        gates: [],
      };
      return this.snapshot();
    }
    this.phase = "choose";
    this.notice = null;
    return this.snapshot();
  }

  async setChoice(action: string, route: Route, preferDryRun: boolean): Promise<Snapshot> {
    if (action !== "update_keep_root") {
      this.notice = { level: "block", message: "Only update-and-keep-root is available in this version.", gates: [] };
      return this.snapshot();
    }
    this.actionChosen = true;
    this.route = route;
    this.preferDryRun = preferDryRun;
    this.notice = null;
    return this.snapshot();
  }

  async continueFromChoose(): Promise<Snapshot> {
    if (!this.actionChosen) {
      this.notice = { level: "block", message: "Choose update and keep root to continue.", gates: [] };
      return this.snapshot();
    }
    this.phase = "firmware";
    this.notice = null;
    return this.snapshot();
  }

  async openFirmware(name: string, sha256: string): Promise<Snapshot> {
    const sha = sha256.trim().toLowerCase();
    const gates = passGates();
    let failed = false;
    if (!name.toLowerCase().endsWith(".zip")) {
      this.fail("Choose a .zip package. Other archive types are not accepted.", []);
      return this.snapshot();
    }
    if (sha.length !== 64 || !/^[0-9a-f]+$/.test(sha)) {
      this.fail("Paste the published 64-character SHA-256 checksum.", []);
      return this.snapshot();
    }
    if (sha !== FIXTURE_SHA256) {
      const row = gates.find((item) => item.id === "G05");
      if (row) {
        row.status = "fail";
        row.evidence = "The file checksum does not match the published SHA-256.";
      }
      failed = true;
    } else if (!name.toLowerCase().includes(sha.slice(0, 8))) {
      const row = gates.find((item) => item.id === "G05");
      if (row) {
        row.status = "fail";
        row.evidence = "The checksum fragment in the file name does not match the published SHA-256.";
      }
      failed = true;
    }
    const codename = name.split("-")[0] ?? "";
    if (codename !== "harbor") {
      const row = gates.find((item) => item.id === "G04");
      if (row) {
        row.status = "fail";
        row.evidence = "This package is for a different phone.";
      }
      failed = true;
    }
    if (failed) {
      this.firmwareOk = false;
      this.failedGates = gates;
      this.notice = { level: "block", message: "The package did not pass the checks.", gates };
      return this.snapshot();
    }
    this.route = name.includes("factory") ? "factory_keep_data" : "ota";
    this.firmwareName = name;
    this.firmwareOk = true;
    this.failedGates = [];
    this.notice = null;
    return this.snapshot();
  }

  async buildPlan(): Promise<Snapshot> {
    if (!this.firmwareOk) {
      this.notice = { level: "block", message: "Check a package before building a plan.", gates: this.failedGates };
      return this.snapshot();
    }
    this.phase = "review";
    this.job = idleJob();
    this.notice = null;
    return this.snapshot();
  }

  async dryRun(planHash: string): Promise<Snapshot> {
    const plan = this.plan();
    if (!plan || plan.plan_hash !== planHash) {
      throw new Error("The dry run hash does not match the plan this program issued.");
    }
    this.job = {
      state: "dry_done",
      progress: 100,
      status_line: "Dry run finished. No write was started.",
      lines: [
        { ts: "13:41:02", level: "read", text: "Pre-flight checks passed." },
        { ts: "13:41:02", level: "read", text: "Backup set is present and verified." },
        ...plan.steps
          .filter((step) => step.class === "write")
          .map((step) => ({ ts: "13:41:02", level: "would", text: `WOULD RUN: ${quote(step.argv)}` })),
      ],
      result_title: "Dry run",
      result_body: "Write steps were printed only. The phone was not rebooted.",
      recovery: [],
      cancel_mode: "immediate",
    };
    this.notice = null;
    return this.snapshot();
  }

  async confirmAndRun(planHash: string): Promise<Snapshot> {
    const plan = this.plan();
    if (!plan || plan.plan_hash !== planHash) {
      throw new Error("That plan code was not issued by Flashwright.");
    }
    this.phase = "done";
    this.job = {
      state: "succeeded",
      progress: 100,
      status_line: "Finished",
      lines: [
        ...plan.steps.map((step) => ({ ts: "13:41:02", level: "run", text: quote(step.argv) })),
        {
          ts: "13:41:02",
          level: "ok",
          text: "Updated to HQ1A.MOCK.002. Root is working (root tool 30.7).",
        },
      ],
      result_title: "Updated to HQ1A.MOCK.002",
      result_body: "Root is working (root tool 30.7).",
      recovery: [],
      cancel_mode: "immediate",
    };
    return this.snapshot();
  }

  async back(): Promise<Snapshot> {
    if (this.phase === "choose") this.phase = "connect";
    else if (this.phase === "firmware") this.phase = "choose";
    else if (this.phase === "review") this.phase = "firmware";
    this.notice = null;
    return this.snapshot();
  }

  async openExternal(urlId: string): Promise<Snapshot> {
    const known = LINKS.some((link) => link.id === urlId);
    this.notice = known
      ? { level: "info", message: "That page is allow-listed. Flashwright opens it in your browser.", gates: [] }
      : { level: "block", message: "That link is not on the allow list.", gates: [] };
    return this.snapshot();
  }

  async recoveryPlan(optionId: string): Promise<Snapshot> {
    if (this.phase !== "recovery") {
      throw new Error("Recovery plans are only available from the recovery page.");
    }
    this.notice = {
      level: "info",
      message: `Recovery option “${optionId}” would build its own plan. This build does not run it.`,
      gates: [],
    };
    return this.snapshot();
  }

  private plan() {
    if (!this.firmwareOk) return null;
    const hash = this.route === "factory_keep_data" ? FACTORY_HASH : OTA_HASH;
    const eight = hash.slice(5, 13);
    return {
      plan_hash: hash,
      plan_code: `PLAN ${eight.slice(0, 4)}·${eight.slice(4)}`,
      expires_unix_ms: 1_760_000_000_000 + 15 * 60 * 1000,
      route: this.route,
      target_slot: "b" as const,
      codename: "harbor",
      build_id: "HQ1A.MOCK.002",
      backup_set_id: "4f2a9c01-0000-7000-8000-000000000001",
      gates: passGates(),
      steps: otaSteps(this.firmwareName || FIXTURE_OTA_NAME),
      prefer_dry_run: this.preferDryRun,
    };
  }

  private unauthorised(): DeviceInfo {
    return {
      ...primary(),
      serial: "FWMOCK000002",
      mode: "unauthorized",
      build_id: "",
      fingerprint: "",
      spl: "",
      active_slot: null,
      bootloader_unlocked: false,
      bootloader_version: "",
      root_present: false,
      root_tool_version: "",
      root_tool_code: 0,
      uses_init_boot: false,
      battery_percent: 0,
    };
  }

  private fail(message: string, gates: GateView[]): void {
    this.firmwareOk = false;
    this.notice = { level: "block", message, gates };
  }
}
