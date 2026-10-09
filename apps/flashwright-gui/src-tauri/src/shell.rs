// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Tauri shell for the Windows update wizard.
//!
//! Every command forwards to the core wizard. This crate does not mint a
//! write token and does not start a device write.

use std::sync::Mutex;

use tauri::ipc::Channel;
use tauri::{State, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use flashwright_core::wizard::{external_url, Engine, FirmwareRef, Route, Snapshot, WizardEvent};
use flashwright_core::CoreError;

use crate::commands::PHASE1_COMMANDS;

#[cfg(debug_assertions)]
type ActiveEngine = Engine<flashwright_core::mock::MockTransport>;
#[cfg(not(debug_assertions))]
type ActiveEngine = Engine<flashwright_core::wizard::EmptyTransport>;

struct EngineState(Mutex<ActiveEngine>);

fn fresh() -> ActiveEngine {
    #[cfg(debug_assertions)]
    {
        Engine::mock()
    }
    #[cfg(not(debug_assertions))]
    {
        Engine::empty()
    }
}

fn snap<F>(state: &EngineState, f: F) -> Result<Snapshot, String>
where
    F: FnOnce(&mut ActiveEngine) -> Result<Snapshot, CoreError>,
{
    let mut engine = state
        .0
        .lock()
        .map_err(|_| "The wizard engine is unavailable.".to_string())?;
    f(&mut engine).map_err(|err| err.to_string())
}

fn with_engine<T, F>(state: &EngineState, f: F) -> Result<T, String>
where
    F: FnOnce(&mut ActiveEngine) -> Result<T, CoreError>,
{
    let mut engine = state
        .0
        .lock()
        .map_err(|_| "The wizard engine is unavailable.".to_string())?;
    f(&mut engine).map_err(|err| err.to_string())
}

#[tauri::command]
fn engine_snapshot(state: State<EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.snapshot())
}

#[tauri::command]
fn subscribe(state: State<EngineState>, channel: Channel<WizardEvent>) -> Result<(), String> {
    let event = with_engine(&state, |engine| Ok(engine.subscribe_seed()))?;
    channel
        .send(event)
        .map_err(|_| "The log channel could not be opened.".to_string())
}

#[tauri::command]
fn tools_status(state: State<EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.tools_status())
}

fn display_name(path: tauri_plugin_dialog::FilePath) -> Option<String> {
    path.into_path().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    })
}

async fn pick_path(
    app: &tauri::AppHandle,
    folder: bool,
    filter_name: Option<&str>,
    extensions: &[&str],
) -> Result<Option<tauri_plugin_dialog::FilePath>, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let mut builder = app.dialog().file();
    if let Some(name) = filter_name {
        builder = builder.add_filter(name, extensions);
    }
    if folder {
        builder.pick_folder(move |picked| {
            let _ = tx.try_send(picked);
        });
    } else {
        builder.pick_file(move |picked| {
            let _ = tx.try_send(picked);
        });
    }
    rx.recv()
        .await
        .map_err(|_| "The file dialog closed.".to_string())
}

#[tauri::command]
async fn tools_pick_folder(
    app: tauri::AppHandle,
    state: State<'_, EngineState>,
) -> Result<Snapshot, String> {
    let display = pick_path(&app, true, None, &[])
        .await?
        .and_then(display_name);
    snap(&state, |engine| engine.note_tools_folder(display))
}

#[tauri::command]
async fn tools_import_zip(
    app: tauri::AppHandle,
    state: State<'_, EngineState>,
) -> Result<Snapshot, String> {
    let display = pick_path(&app, false, Some("Zip"), &["zip"])
        .await?
        .and_then(display_name);
    snap(&state, |engine| engine.note_tools_zip(display))
}

#[tauri::command]
fn scan(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.scan())
}

#[tauri::command]
fn select_device(state: State<'_, EngineState>, serial: String) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.select_device(&serial))
}

#[tauri::command]
fn continue_from_connect(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.continue_from_connect())
}

#[tauri::command]
fn set_choice(
    state: State<'_, EngineState>,
    action: String,
    route: Route,
    dry_run_first: bool,
) -> Result<Snapshot, String> {
    snap(&state, |engine| {
        engine.set_choice(&action, route, dry_run_first)
    })
}

#[tauri::command]
fn continue_from_choose(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.continue_from_choose())
}

#[tauri::command]
fn back(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.back())
}

#[tauri::command]
async fn pick_firmware(
    app: tauri::AppHandle,
    state: State<'_, EngineState>,
) -> Result<FirmwareRef, String> {
    let picked = pick_path(&app, false, Some("Package"), &["zip"])
        .await?
        .ok_or_else(|| "No package was chosen.".to_string())?;
    let path = picked
        .into_path()
        .map_err(|_| "That path could not be read.".to_string())?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| "That path has no file name.".to_string())?;
    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    with_engine(&state, |engine| Ok(engine.remember_firmware(&name, size)))
}

#[tauri::command]
fn firmware_open(
    state: State<'_, EngineState>,
    firmware_id: String,
    published_sha256: String,
) -> Result<Snapshot, String> {
    snap(&state, |engine| {
        engine.firmware_open(&firmware_id, &published_sha256)
    })
}

#[tauri::command]
fn prepare_patch(state: State<'_, EngineState>, firmware_id: String) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.prepare_patch(&firmware_id))
}

#[tauri::command]
fn build_plan(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.build_plan())
}

#[tauri::command]
fn ack_gate(
    state: State<'_, EngineState>,
    plan_hash: String,
    gate_id: String,
) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.ack_gate(&plan_hash, &gate_id))
}

#[tauri::command]
fn dry_run(state: State<'_, EngineState>, plan_hash: String) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.dry_run(&plan_hash))
}

#[tauri::command]
fn confirm_and_run(state: State<'_, EngineState>, plan_hash: String) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.confirm_and_run(&plan_hash))
}

#[tauri::command]
fn cancel(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.cancel())
}

#[tauri::command]
fn recovery_plan(state: State<'_, EngineState>, option_id: String) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.recovery_plan(&option_id))
}

#[tauri::command]
fn backups_list(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.backups_list())
}

#[tauri::command]
fn restore_plan(
    state: State<'_, EngineState>,
    set_id: String,
    item: String,
) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.restore_plan(&set_id, &item))
}

#[tauri::command]
fn open_external(
    app: tauri::AppHandle,
    state: State<'_, EngineState>,
    url_id: String,
) -> Result<Snapshot, String> {
    let url = external_url(&url_id).map_err(|err| err.to_string())?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "The browser could not be opened.".to_string())?;
    snap(&state, |engine| engine.note_link(&url_id))
}

fn navigation_allowed(url: &tauri::Url) -> bool {
    let text = url.as_str();
    if text.starts_with("tauri://localhost") || text.starts_with("http://tauri.localhost") {
        return true;
    }
    #[cfg(dev)]
    {
        if text.starts_with("http://127.0.0.1:") || text.starts_with("http://localhost:") {
            return true;
        }
    }
    false
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(EngineState(Mutex::new(fresh())))
        .invoke_handler(tauri::generate_handler![
            engine_snapshot,
            subscribe,
            tools_status,
            tools_pick_folder,
            tools_import_zip,
            scan,
            select_device,
            continue_from_connect,
            set_choice,
            continue_from_choose,
            back,
            pick_firmware,
            firmware_open,
            prepare_patch,
            build_plan,
            ack_gate,
            dry_run,
            confirm_and_run,
            cancel,
            recovery_plan,
            backups_list,
            restore_plan,
            open_external,
        ])
        .setup(|app| {
            let window = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .cloned()
                .ok_or_else(|| "The main window is missing from the configuration.".to_string())?;
            let _ = WebviewWindowBuilder::from_config(app.handle(), &window)?
                .on_navigation(navigation_allowed)
                .on_new_window(|_url, _features| tauri::webview::NewWindowResponse::Deny)
                .build()?;
            let _ = PHASE1_COMMANDS;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Flashwright window failed to start");
}
