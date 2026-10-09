import { assets } from "./assets";
import { el, mnemonic } from "./dom";
import type { GateView, LogLine, Mode, Phase, Snapshot } from "./types";

export interface ShellModel {
  snap: Snapshot;
  visiblePhase: Phase;
  lines: LogLine[];
  progress: number;
  writing: boolean;
  draftName: string;
  draftSha: string;
  dryPreference: boolean;
  confirm: { secondsLeft: number; armed: boolean } | null;
  overlay: "none" | "backups" | "help";
  stopAsk: boolean;
  localError: string | null;
  tauri: boolean;
}

const STEP_ORDER = ["connect", "choose", "firmware", "review", "flash"] as const;

function modeLabel(mode: Mode): string {
  switch (mode) {
    case "adb":
      return "device";
    case "recovery":
      return "recovery";
    case "sideload":
      return "sideload";
    case "rescue":
      return "rescue";
    case "fastboot":
      return "fastboot";
    case "fastbootd":
      return "fastbootd";
    case "unauthorized":
      return "not authorised";
    case "no_permissions":
      return "no permission";
    case "offline":
      return "offline";
    default: {
      const unexpected: never = mode;
      return unexpected;
    }
  }
}

function phaseTitle(phase: Phase, snap: Snapshot): string {
  switch (phase) {
    case "connect":
      return "Connect a phone";
    case "choose":
      return "Choose what to do";
    case "firmware":
      return "Pick a firmware package";
    case "review":
      return "Review the plan";
    case "flash":
      return "Flash";
    case "done":
      return snap.job.result_title || "Finished";
    case "recovery":
      return "Recovery";
    default: {
      const unexpected: never = phase;
      return unexpected;
    }
  }
}

function phaseLede(phase: Phase): string {
  switch (phase) {
    case "connect":
      return "Scan, then pick one authorised phone in the normal device state.";
    case "choose":
      return "This version updates the phone and keeps the Magisk app.";
    case "firmware":
      return "Choose a full package and paste the published checksum.";
    case "review":
      return "The plan code below is the one this program issued. Check it before a write.";
    case "flash":
      return "The log shows each step as it is played back. No phone is attached in this copy.";
    case "done":
      return "The mock plan finished. Nothing was written to a phone.";
    case "recovery":
      return "The update stopped. Each option below is its own checked plan.";
    default: {
      const unexpected: never = phase;
      return unexpected;
    }
  }
}

function phaseMark(phase: Phase): string {
  switch (phase) {
    case "connect":
    case "recovery":
      return assets.device;
    case "choose":
    case "done":
      return assets.root;
    case "firmware":
      return assets.factory;
    case "review":
    case "flash":
      return assets.update;
    default: {
      const unexpected: never = phase;
      return unexpected;
    }
  }
}

function stepState(id: string, phase: Phase): "done" | "current" | "todo" {
  if (phase === "done") {
    return "done";
  }
  const cursor = phase === "recovery" ? "flash" : phase;
  const phaseIndex = STEP_ORDER.indexOf(cursor as (typeof STEP_ORDER)[number]);
  const stepIndex = STEP_ORDER.indexOf(id as (typeof STEP_ORDER)[number]);
  if (stepIndex < phaseIndex) {
    return "done";
  }
  if (stepIndex === phaseIndex) {
    return "current";
  }
  return "todo";
}

function canAdvance(model: ShellModel): boolean {
  const { snap, visiblePhase } = model;
  switch (visiblePhase) {
    case "connect":
      return snap.selected?.mode === "adb";
    case "choose":
      return snap.choice.action === "update_keep_root";
    case "firmware":
      return snap.firmware !== null;
    case "review":
    case "flash":
    case "done":
    case "recovery":
      return false;
    default: {
      const unexpected: never = visiblePhase;
      return unexpected;
    }
  }
}

function keyLabel(letter: string, rest: string): HTMLSpanElement {
  const label = el("span", {});
  label.append(mnemonic(letter, rest));
  return label;
}

function keyButton(
  action: string,
  className: string,
  letter: string,
  rest: string,
  disabled = false,
): HTMLButtonElement {
  const button = el("button", { class: className, type: "button", "data-action": action }, [keyLabel(letter, rest)]);
  button.disabled = disabled;
  return button;
}

function callout(level: string, message: string): HTMLElement {
  const tone = level === "block" || level === "warn" ? "fw-callout fw-callout--warn" : "fw-callout";
  return el("div", { class: tone, role: "status" }, [el("p", {}, [message])]);
}

function gateList(gates: GateView[]): HTMLElement {
  const list = el("ul", { class: "fw-tasks fw-scroll" });
  for (const gate of gates) {
    const state = gate.status === "pass" ? "done" : gate.status === "fail" ? "now" : "todo";
    list.append(
      el("li", { "data-state": state }, [
        `${gate.id} ${gate.title}. ${gate.evidence}`,
      ]),
    );
  }
  return list;
}

function logLine(line: LogLine): HTMLElement {
  const tone = line.level === "ok" ? "fw-log-ok" : line.level === "error" ? "fw-log-warn" : line.level === "would" ? "fw-log-prompt" : "";
  const text = el("span", tone ? { class: tone } : {}, [line.text]);
  return el("div", { class: "fw-log-line" }, [el("span", { class: "fw-log-time" }, [`${line.ts} `]), text]);
}

export function mountLog(host: HTMLElement, lines: LogLine[]): void {
  host.replaceChildren();
  host.id = "log";
  host.tabIndex = 0;
  host.setAttribute("role", "log");
  host.setAttribute("aria-label", "Plan log");
  if (lines.length <= 80) {
    for (const line of lines) {
      host.append(logLine(line));
    }
    host.scrollTop = host.scrollHeight;
    return;
  }
  const row = 20;
  const spacer = el("div", {});
  spacer.style.position = "relative";
  spacer.style.height = `${lines.length * row}px`;
  const windowEl = el("div", {});
  windowEl.style.position = "absolute";
  windowEl.style.left = "0";
  windowEl.style.right = "0";
  spacer.append(windowEl);
  host.append(spacer);
  const paint = (): void => {
    const start = Math.max(0, Math.floor(host.scrollTop / row) - 2);
    const count = 12;
    const end = Math.min(lines.length, start + count);
    windowEl.style.top = `${start * row}px`;
    windowEl.replaceChildren();
    for (let index = start; index < end; index += 1) {
      const line = lines[index];
      if (line) {
        windowEl.append(logLine(line));
      }
    }
  };
  host.addEventListener("scroll", paint);
  paint();
}

function deviceList(snap: Snapshot): HTMLElement {
  const list = el("div", { class: "fw-list", role: "listbox", "aria-label": "Phones" });
  for (const device of snap.devices) {
    const selected = snap.selected?.serial === device.serial;
    const row = el("button", {
      class: "fw-list__row",
      type: "button",
      role: "option",
      "data-action": "select",
      "data-serial": device.serial,
      "aria-selected": selected ? "true" : "false",
    });
    row.append(
      el("img", { src: assets.device, alt: "" }),
      el("span", {}, [`${device.model} · ${device.codename}`]),
      el("span", { class: "fw-hint" }, [`${device.serial} · ${modeLabel(device.mode)}`]),
    );
    list.append(row);
  }
  return list;
}

function deviceFacts(snap: Snapshot): HTMLElement {
  const info = snap.selected;
  if (!info) {
    return el("p", { class: "fw-hint" }, ["No phone selected."]);
  }
  const rows: Array<[string, string]> = [
    ["Serial", info.serial],
    ["State", modeLabel(info.mode)],
    ["Build", info.build_id || "—"],
    ["Slot", info.active_slot ? info.active_slot.toUpperCase() : "—"],
    ["Bootloader", info.bootloader_unlocked ? "unlocked" : "locked"],
    ["Battery", info.battery_percent > 0 ? `${info.battery_percent}%` : "—"],
    ["Root tool", info.root_present ? `stable ${info.root_tool_version}` : "not detected"],
  ];
  const list = el("dl", { class: "fw-kv" });
  for (const [term, value] of rows) {
    list.append(el("dt", {}, [term]), el("dd", {}, [value]));
  }
  return list;
}

function connectView(model: ShellModel): HTMLElement {
  const wrap = el("div", {});
  wrap.append(deviceList(model.snap), deviceFacts(model.snap));
  wrap.append(el("div", { class: "fw-toolbar" }));
  const links = el("fieldset", { class: "fw-group" }, [el("legend", {}, ["Official pages"])]);
  for (const link of model.snap.links) {
    const button = el("button", { class: "fw-btn", type: "button", "data-action": "link", "data-id": link.id });
    button.textContent = link.label;
    links.append(button);
  }
  wrap.append(links);
  return wrap;
}

function choiceCard(
  value: string,
  title: string,
  detail: string,
  mark: string,
  checked: boolean,
  disabled: boolean,
  badge: string,
): HTMLLabelElement {
  const input = el("input", { type: "radio", name: "action", value });
  input.checked = checked;
  input.disabled = disabled;
  if (!disabled) {
    input.dataset.action = "choice";
  }
  const strong = el("strong", {}, [title]);
  if (badge) {
    strong.append(el("span", { class: "fw-badge" }, [badge]));
  }
  const label = el("label", { class: "fw-choice" }, [
    input,
    el("img", { src: mark, alt: "" }),
    el("span", {}, [strong, el("small", {}, [detail])]),
  ]);
  return label;
}

function chooseView(model: ShellModel): HTMLElement {
  const { snap } = model;
  const chosen = snap.choice.action === "update_keep_root";
  const dry = chosen ? snap.choice.prefer_dry_run : model.dryPreference;
  const wrap = el("div", {});
  wrap.append(
    choiceCard(
      "update_keep_root",
      "Update and keep root",
      "Update this phone and keep the Magisk app.",
      assets.update,
      chosen,
      false,
      "",
    ),
    choiceCard(
      "update_remove_root",
      "Update and remove root",
      "Not available in this version.",
      assets.root,
      false,
      true,
      "Phase 2",
    ),
    choiceCard(
      "factory_wipe",
      "Factory image, wipe data",
      "Not available in this version.",
      assets.factory,
      false,
      true,
      "Phase 2",
    ),
  );
  const slots = el("fieldset", { class: "fw-group" }, [el("legend", {}, ["Slot"])]);
  const inactive = el("input", { type: "radio", name: "slot", value: "inactive" });
  inactive.checked = true;
  const both = el("input", { type: "radio", name: "slot", value: "both" });
  both.disabled = true;
  slots.append(
    el("label", { class: "fw-check" }, [inactive, el("span", {}, [snap.choice.slot_label])]),
    el("label", { class: "fw-check" }, [both, el("span", {}, ["Both slots"])]),
    el("p", { class: "fw-hint" }, [snap.choice.both_slots_reason]),
  );
  const dryInput = el("input", { type: "checkbox", "data-action": "dry-toggle" });
  dryInput.checked = dry;
  wrap.append(
    slots,
    el("label", { class: "fw-check" }, [dryInput, el("span", {}, ["Dry run before flashing"])]),
    el("p", { class: "fw-hint" }, [
      `${snap.choice.root_tool_label}: ${snap.choice.root_tool_version}. Backups: ${snap.choice.backup_folder}`,
    ]),
  );
  return wrap;
}

function firmwareView(model: ShellModel): HTMLElement {
  const wrap = el("div", {});
  const name = el("input", { class: "fw-input", id: "firmware-name", type: "text", spellcheck: "false" });
  name.value = model.draftName;
  const sha = el("input", {
    class: "fw-input fw-input--mono",
    id: "firmware-sha",
    type: "text",
    spellcheck: "false",
    autocomplete: "off",
  });
  sha.value = model.draftSha;
  wrap.append(
    el("label", { class: "fw-field" }, [el("span", { class: "fw-label" }, ["Package file name"]), name]),
    el("label", { class: "fw-field" }, [el("span", { class: "fw-label" }, ["Published SHA-256"]), sha]),
  );
  const row = el("div", { class: "fw-toolbar" });
  const check = el("button", { class: "fw-btn", type: "button", "data-action": "check-firmware" });
  check.textContent = "Check package";
  row.append(check);
  if (import.meta.env.DEV) {
    const sample = el("button", { class: "fw-btn", type: "button", "data-action": "sample" });
    sample.textContent = "Use sample package";
    row.append(sample);
  }
  if (model.tauri) {
    const browse = el("button", { class: "fw-btn", type: "button", "data-action": "browse" });
    browse.textContent = "Browse…";
    row.append(browse);
  }
  wrap.append(row);
  const firmware = model.snap.firmware;
  if (firmware) {
    const facts = el("dl", { class: "fw-kv" });
    const route = firmware.route === "factory_keep_data" ? "Factory package, keep data" : "Full package";
    const pairs: Array<[string, string]> = [
      ["File", firmware.name],
      ["Route", route],
      ["Phone", firmware.codename],
      ["Build", firmware.build_id],
      ["Partition", firmware.partition],
      ["Checksum", firmware.sha256],
    ];
    for (const [term, value] of pairs) {
      facts.append(el("dt", {}, [term]), el("dd", {}, [value]));
    }
    wrap.append(facts);
  }
  return wrap;
}

function reviewView(model: ShellModel): HTMLElement {
  const plan = model.snap.plan;
  const wrap = el("div", {});
  if (!plan) {
    wrap.append(el("p", {}, ["No plan has been issued yet."]));
    return wrap;
  }
  const facts = el("dl", { class: "fw-kv" });
  const when = new Date(plan.expires_unix_ms).toISOString();
  const pairs: Array<[string, string]> = [
    ["Plan code", plan.plan_code],
    ["Plan hash", plan.plan_hash],
    ["Expires", when],
    ["Target slot", plan.target_slot.toUpperCase()],
    ["Backup", plan.backup_set_id],
  ];
  for (const [term, value] of pairs) {
    facts.append(el("dt", {}, [term]), el("dd", {}, [value]));
  }
  const passed = plan.gates.filter((gate) => gate.status === "pass").length;
  const steps = el("ol", { class: "fw-tasks fw-scroll" });
  for (const step of plan.steps) {
    steps.append(el("li", { "data-state": step.class === "write" ? "now" : "done" }, [step.argv.join(" ")]));
  }
  wrap.append(
    facts,
    el("h3", {}, ["Commands"]),
    steps,
    el("h3", {}, [`Checks (${passed} passed)`]),
    gateList(plan.gates),
  );
  if (model.lines.length > 0) {
    const log = el("div", { class: "fw-console" });
    wrap.append(el("h3", {}, ["Dry run"]), log);
  }
  return wrap;
}

function flashView(model: ShellModel): HTMLElement {
  const wrap = el("div", {});
  const label = el("div", { class: "fw-meter__label" }, [
    el("span", {}, [model.writing ? "Playing the plan" : model.snap.job.status_line]),
    el("b", {}, [`${model.progress}%`]),
  ]);
  const meter = el("div", {
    class: "fw-meter",
    role: "meter",
    "aria-valuemin": "0",
    "aria-valuemax": "100",
    "aria-valuenow": String(model.progress),
    "aria-label": "Plan progress",
  });
  meter.style.setProperty("--fw-value", String(model.progress));
  if (model.writing) {
    meter.dataset.busy = "true";
  }
  const log = el("div", { class: "fw-console" });
  wrap.append(label, meter, log);
  return wrap;
}

function doneView(model: ShellModel): HTMLElement {
  const wrap = el("div", {});
  wrap.append(
    el("h3", {}, [model.snap.job.result_title || "Finished"]),
    el("p", {}, [model.snap.job.result_body]),
  );
  const log = el("div", { class: "fw-console" });
  wrap.append(log);
  return wrap;
}

function recoveryView(model: ShellModel): HTMLElement {
  const wrap = el("div", {});
  wrap.append(el("p", {}, [model.snap.job.result_body || "Pick a recovery option."]));
  wrap.append(el("div", { class: "fw-console" }));
  for (const option of model.snap.job.recovery) {
    const button = el("button", {
      class: "fw-choice",
      type: "button",
      "data-action": "recovery",
      "data-id": option.id,
    });
    button.append(
      el("span", {}),
      el("img", { src: assets.device, alt: "" }),
      el("span", {}, [el("strong", {}, [option.title]), el("small", {}, [option.detail])]),
    );
    wrap.append(button);
  }
  return wrap;
}

function contentFor(model: ShellModel): HTMLElement {
  switch (model.visiblePhase) {
    case "connect":
      return connectView(model);
    case "choose":
      return chooseView(model);
    case "firmware":
      return firmwareView(model);
    case "review":
      return reviewView(model);
    case "flash":
      return flashView(model);
    case "done":
      return doneView(model);
    case "recovery":
      return recoveryView(model);
    default: {
      const unexpected: never = model.visiblePhase;
      return unexpected;
    }
  }
}

function footerFor(model: ShellModel): HTMLElement {
  const footer = el("div", { class: "fw-wizard__footer" });
  const phase = model.visiblePhase;
  if ((phase === "choose" || phase === "firmware" || phase === "review") && !model.writing) {
    footer.append(keyButton("back", "fw-btn", "B", "ack"));
  }
  footer.append(el("span", { class: "fw-spacer" }));
  if (phase === "connect" || phase === "choose" || phase === "firmware") {
    footer.append(keyButton("next", "fw-btn fw-btn--default", "N", "ext", !canAdvance(model)));
  }
  if (phase === "review" && model.snap.plan) {
    const dryDefault = model.snap.choice.prefer_dry_run;
    footer.append(keyButton("dry-run", dryDefault ? "fw-btn fw-btn--default" : "fw-btn", "D", "ry run"));
    const flashClass = dryDefault ? "fw-btn fw-btn--primary" : "fw-btn fw-btn--primary fw-btn--default";
    footer.append(keyButton("flash", flashClass, "F", "lash…"));
  }
  if (phase === "flash") {
    const cancel = el("button", {
      class: "fw-btn fw-btn--default",
      type: "button",
      "data-action": "cancel-job",
      "aria-keyshortcuts": "Alt+C",
    });
    cancel.textContent = "Stop after this step";
    footer.append(cancel);
  }
  return footer;
}

function steps(phase: Phase): HTMLElement {
  const list = el("ol", { class: "fw-steps" });
  const names: Array<[string, string]> = [
    ["connect", "Connect"],
    ["choose", "Choose"],
    ["firmware", "Pick firmware"],
    ["review", "Review plan"],
    ["flash", "Flash"],
  ];
  for (const [id, label] of names) {
    const state = stepState(id, phase);
    const item = el("li", { "data-state": state === "current" ? "current" : state }, [label]);
    if (state === "current") {
      item.setAttribute("aria-current", "step");
    }
    list.append(item);
  }
  return list;
}

function stepper(phase: Phase): HTMLElement {
  const row = el("div", { class: "fw-stepper", "aria-hidden": "true" });
  for (const id of STEP_ORDER) {
    const state = stepState(id, phase);
    row.append(el("span", { "data-state": state === "current" ? "current" : state === "done" ? "done" : "todo" }));
  }
  return row;
}

function confirmDialog(model: ShellModel): HTMLElement | null {
  const confirm = model.confirm;
  const plan = model.snap.plan;
  if (!confirm || !plan) {
    return null;
  }
  const patch = plan.kind === "prepare_patch";
  const slot = plan.target_slot.toUpperCase();
  const title = patch ? "Patch on your phone?" : `Flash ${plan.codename} slot ${slot}?`;
  const body = patch
    ? "Flashwright will copy the stock image to a temporary folder on your phone, run Magisk's patcher, copy the result back and delete the folder. Nothing is flashed."
    : `This writes slot ${slot}. Plan code ${plan.plan_code}. Backup set ${plan.backup_set_id}.`;
  const action = el("button", {
    class: "fw-btn fw-btn--primary",
    type: "button",
    "data-action": patch ? "confirm-patch" : "confirm-run",
    "aria-keyshortcuts": patch ? "Alt+P" : "Alt+F",
  });
  action.disabled = !confirm.armed;
  const label = patch ? "atch now" : confirm.armed ? "lash now" : `lash now (${confirm.secondsLeft})`;
  action.append(keyLabel(patch ? "P" : "F", label));
  const cancel = keyButton("cancel-dialog", "fw-btn fw-btn--default", "C", "ancel");
  const dialog = el("div", {
    class: "fw-dialog fw-window fw-dialog--warn",
    role: "alertdialog",
    "aria-modal": "true",
    "aria-labelledby": "confirm-title",
    "aria-describedby": "confirm-body",
  });
  dialog.append(
    el("div", { class: "fw-titlebar" }, [el("span", { class: "fw-titlebar__title" }, ["Confirm the plan"])]),
    el("div", { class: "fw-dialog__body" }, [
      el("img", { src: assets.update, alt: "" }),
      el("div", {}, [
        el("h2", { id: "confirm-title" }, [title]),
        el("p", { id: "confirm-body" }, [body]),
        el("p", { class: "fw-mono" }, [plan.plan_hash]),
      ]),
    ]),
    el("div", { class: "fw-dialog__actions" }, [action, cancel]),
  );
  return el("div", { class: "fw-backdrop" }, [dialog]);
}

function overlay(model: ShellModel): HTMLElement | null {
  if (model.overlay === "none") {
    return null;
  }
  const body = el("div", { class: "fw-dialog__body" });
  body.append(el("img", { src: model.overlay === "backups" ? assets.device : assets.root, alt: "" }));
  const copy = el("div", {});
  if (model.overlay === "help") {
    copy.append(
      el("h2", {}, ["About this window"]),
      el("p", {}, [
        "Flashwright walks through an update one step at a time. The plan code is issued by the engine. This copy uses a mock phone, so it never writes to a device. Android, Google and Pixel are trademarks of Google LLC. Magisk is a project by topjohnwu. They are named descriptively; Flashwright isn't affiliated with or endorsed by them.",
      ]),
    );
  } else {
    copy.append(el("h2", {}, ["Backups"]));
    const list = el("ul", { class: "fw-tasks" });
    for (const backup of model.snap.backups) {
      const mark = backup.verified ? "verified" : "not verified";
      list.append(el("li", { "data-state": "done" }, [`${backup.label} · ${backup.size_label} · ${mark}`]));
    }
    copy.append(list);
  }
  body.append(copy);
  const close = keyButton("close-overlay", "fw-btn fw-btn--default", "C", "lose");
  const dialog = el("div", { class: "fw-dialog fw-window", role: "dialog", "aria-modal": "true" });
  dialog.append(
    el("div", { class: "fw-titlebar" }, [
      el("span", { class: "fw-titlebar__title" }, [model.overlay === "help" ? "Help" : "Backups"]),
    ]),
    body,
    el("div", { class: "fw-dialog__actions" }, [close]),
  );
  return el("div", { class: "fw-backdrop" }, [dialog]);
}

export function renderShell(model: ShellModel): HTMLElement {
  const phase = model.visiblePhase;
  const app = el("div", { class: "fw-app", "data-phase": phase });
  const windowEl = el("div", { class: "fw-window" });
  const title = el("div", { class: "fw-titlebar", "data-tauri-drag-region": "" }, [
    el("img", { class: "fw-titlebar__icon fw-pixel", src: assets.icon, alt: "" }),
    el("span", { class: "fw-titlebar__title", "data-tauri-drag-region": "" }, ["Flashwright"]),
  ]);
  const controls = el("div", { class: "fw-titlebar__controls" });
  controls.append(
    el("button", { class: "fw-ctl fw-ctl--hide", type: "button", "data-action": "minimize", "aria-label": "Minimise" }),
    el("button", { class: "fw-ctl fw-ctl--size", type: "button", "data-action": "maximize", "aria-label": "Maximise" }),
    el("button", { class: "fw-ctl fw-ctl--close", type: "button", "data-action": "close", "aria-label": "Close" }),
  );
  title.append(controls);

  const toolbar = el("div", { class: "fw-toolbar" });
  const scan = el("button", { class: "fw-tool", type: "button", "data-action": "scan" }, [
    el("img", { src: assets.device, alt: "" }),
    "Scan",
  ]);
  const backups = el("button", { class: "fw-tool", type: "button", "data-action": "backups" }, [
    el("img", { src: assets.factory, alt: "" }),
    "Backups",
  ]);
  const help = el("button", {
    class: "fw-tool",
    type: "button",
    "data-action": "help",
    "aria-keyshortcuts": "Alt+H",
  }, [
    el("img", { src: assets.root, alt: "" }),
    "Help",
  ]);
  const theme = el("button", { class: "fw-tool", type: "button", "data-action": "theme" }, [
    el("img", { src: assets.update, alt: "" }),
    "Theme",
  ]);
  toolbar.append(scan, backups, help, el("span", { class: "fw-toolbar__spacer" }), theme);

  const index = STEP_ORDER.indexOf((phase === "done" || phase === "recovery" ? "flash" : phase) as (typeof STEP_ORDER)[number]);
  const stepNo = phase === "done" ? STEP_ORDER.length : Math.max(index, 0) + 1;
  const header = el("header", { class: "fw-wizard__header" }, [
    el("p", { class: "fw-wizard__step" }, [`Step ${stepNo} of ${STEP_ORDER.length}`]),
    el("h1", { class: "fw-wizard__title" }, [phaseTitle(phase, model.snap)]),
    el("p", { class: "fw-wizard__lede" }, [phaseLede(phase)]),
    el("img", { src: phaseMark(phase), alt: "" }),
    stepper(phase),
  ]);

  const main = el("div", { class: "fw-wizard__content" });
  const message = model.localError ?? model.snap.notice?.message;
  const level = model.localError ? "block" : model.snap.notice?.level;
  if (message && level) {
    main.append(callout(level, message));
  }
  if (model.snap.notice && model.snap.notice.gates.length > 0 && phase !== "review") {
    main.append(gateList(model.snap.notice.gates));
  }
  main.append(contentFor(model));

  const wizard = el("div", { class: "fw-wizard" }, [
    el("aside", { class: "fw-wizard__banner" }, [
      el("img", { src: assets.banner, alt: "Flashwright" }),
      steps(phase),
    ]),
    el("div", { class: "fw-wizard__main" }, [header, main]),
    footerFor(model),
  ]);

  const status = model.writing ? "A write step is on screen" : model.snap.job.status_line;
  const phone = model.snap.selected ? model.snap.selected.serial : "No phone";
  const meter = el("div", {
    class: model.writing ? "fw-meter fw-meter--warn" : "fw-meter",
    role: "meter",
    "aria-valuemin": "0",
    "aria-valuemax": "100",
    "aria-valuenow": String(model.progress),
    "aria-label": "Status",
  });
  meter.style.setProperty("--fw-value", String(model.progress));
  const statusbar = el("footer", { class: "fw-statusbar" }, [
    el("span", { class: "fw-statusbar__cell" }, [`Tools ${model.snap.tools.version}`]),
    el("span", { class: "fw-statusbar__cell" }, [model.snap.driver.state]),
    el("span", { class: "fw-statusbar__cell" }, [phone]),
    el("span", { class: model.writing ? "fw-statusbar__cell fw-statusbar__cell--grow fw-statusbar__cell--warn" : "fw-statusbar__cell fw-statusbar__cell--grow" }, [
      status,
    ]),
    el("span", { class: "fw-statusbar__cell" }, [meter]),
  ]);

  windowEl.append(title, toolbar, wizard, statusbar);
  app.append(windowEl);
  const dialog = model.stopAsk ? stopDialog() : confirmDialog(model) ?? overlay(model);
  if (dialog) {
    app.append(dialog);
  }
  return app;
}

function stopDialog(): HTMLElement {
  const stop = el("button", {
    class: "fw-btn",
    type: "button",
    "data-action": "stop-confirm",
  });
  stop.textContent = "Stop after this step";
  const cancel = keyButton("cancel-stop", "fw-btn fw-btn--default", "C", "ancel");
  const dialog = el("div", {
    class: "fw-dialog fw-window fw-dialog--warn",
    role: "alertdialog",
    "aria-modal": "true",
    "aria-labelledby": "stop-title",
  });
  dialog.append(
    el("div", { class: "fw-titlebar" }, [el("span", { class: "fw-titlebar__title" }, ["Stop"])]),
    el("div", { class: "fw-dialog__body" }, [
      el("h2", { id: "stop-title" }, ["Are you sure?"]),
      el("p", {}, ["The current step will finish, then the job stops."]),
    ]),
    el("div", { class: "fw-dialog__actions" }, [stop, cancel]),
  );
  return el("div", { class: "fw-backdrop" }, [dialog]);
}

export function logHost(root: ParentNode): HTMLElement | null {
  return root.querySelector(".fw-console");
}
