// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Tauri window for the Windows update wizard.
//!
//! Device writes stay inside the sample transport. This crate does not open a phone.

use std::sync::Mutex;

use tauri::State;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use flashwright_wizard::{external_url, CoreError, Engine, MockTransport, Route, Snapshot};

struct EngineState(Mutex<Engine<MockTransport>>);

fn snap<F>(state: &EngineState, f: F) -> Result<Snapshot, String>
where
    F: FnOnce(&mut Engine<MockTransport>) -> Result<Snapshot, CoreError>,
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
    prefer_dry_run: bool,
) -> Result<Snapshot, String> {
    snap(&state, |engine| {
        engine.set_choice(&action, route, prefer_dry_run)
    })
}

#[tauri::command]
fn continue_from_choose(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.continue_from_choose())
}

#[tauri::command]
fn open_firmware(
    state: State<'_, EngineState>,
    name: String,
    sha256: String,
) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.open_firmware(&name, &sha256))
}

#[tauri::command]
fn build_plan(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.build_plan())
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
fn back(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.back())
}

#[tauri::command]
fn recovery_plan(state: State<'_, EngineState>, option_id: String) -> Result<Snapshot, String> {
    snap(&state, |engine| engine.recovery_plan(&option_id))
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

#[tauri::command]
fn pick_package(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("Package", &["zip"])
        .blocking_pick_file()
        .and_then(|path| path.into_path().ok())
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(EngineState(Mutex::new(Engine::mock())))
        .invoke_handler(tauri::generate_handler![
            engine_snapshot,
            scan,
            select_device,
            continue_from_connect,
            set_choice,
            continue_from_choose,
            open_firmware,
            build_plan,
            dry_run,
            confirm_and_run,
            back,
            open_external,
            recovery_plan,
            pick_package,
        ])
        .run(tauri::generate_context!())
        .expect("Flashwright window failed to start");
}
