// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use proptest::prelude::*;

use super::device::{DeviceInfo, DeviceSummary, DeviceTransport};
use super::session::{FixedClock, Phase};
use super::Engine;
use super::PlanKind;
use super::Route;
use crate::mock::{
    MockTransport, FIXTURE_FACTORY_NAME, FIXTURE_OTA_NAME, FIXTURE_SHA256, PRIMARY_SERIAL,
};
use crate::CoreError;

fn engine() -> Engine<MockTransport> {
    Engine::mock_at(1_760_000_000_000)
}

fn ready_plan() -> Engine<MockTransport> {
    let mut engine = engine();
    engine.select_device(PRIMARY_SERIAL).unwrap();
    engine.continue_from_connect().unwrap();
    engine
        .set_choice("update_keep_root", Route::Ota, true)
        .unwrap();
    engine.continue_from_choose().unwrap();
    engine
        .firmware_open(FIXTURE_OTA_NAME, FIXTURE_SHA256)
        .unwrap();
    engine.build_plan().unwrap();
    engine
}

fn plan_hash(engine: &Engine<MockTransport>) -> String {
    engine.snapshot().unwrap().plan.unwrap().plan_hash
}

struct Flip {
    inner: MockTransport,
    flipped: AtomicBool,
    boot_only: bool,
}

impl DeviceTransport for Flip {
    fn list(&self) -> Result<Vec<DeviceSummary>, CoreError> {
        self.inner.list()
    }

    fn info(&self, serial: &str) -> Result<DeviceInfo, CoreError> {
        let mut info = self.inner.info(serial)?;
        if self.flipped.load(Ordering::SeqCst) {
            info.fingerprint.push_str("-changed");
        }
        if self.boot_only {
            info.uses_init_boot = false;
        }
        Ok(info)
    }
}

fn flip_engine(boot_only: bool) -> Engine<Flip> {
    let mut engine = Engine::new(
        Flip {
            inner: MockTransport::default(),
            flipped: AtomicBool::new(false),
            boot_only,
        },
        Box::new(FixedClock(1_760_000_000_000)),
    );
    engine.select_device(PRIMARY_SERIAL).unwrap();
    engine.continue_from_connect().unwrap();
    engine
        .set_choice("update_keep_root", Route::Ota, true)
        .unwrap();
    engine.continue_from_choose().unwrap();
    engine
        .firmware_open(FIXTURE_OTA_NAME, FIXTURE_SHA256)
        .unwrap();
    engine.build_plan().unwrap();
    engine
}

#[test]
fn unknown_slot_is_an_error() {
    let err = super::inactive_slot(None).unwrap_err();
    assert!(err.to_string().contains("unknown"));
}

#[test]
fn inactive_slot_is_the_other_one() {
    use super::Slot;
    assert_eq!(super::inactive_slot(Some(Slot::A)).unwrap(), Slot::B);
    assert_eq!(super::inactive_slot(Some(Slot::B)).unwrap(), Slot::A);
}

#[test]
fn unlock_prop_zero_means_unlocked() {
    assert!(super::unlocked_from_props("0", "green"));
    assert!(super::unlocked_from_props("1", "orange"));
    assert!(!super::unlocked_from_props("1", "green"));
}

#[test]
fn unauthorised_phone_cannot_continue() {
    let mut engine = engine();
    engine.select_device("FWMOCK000002").unwrap();
    let snap = engine.continue_from_connect().unwrap();
    assert_eq!(snap.phase, Phase::Connect);
    assert!(snap.notice.is_some());
}

#[test]
fn both_slots_stay_disabled() {
    let engine = ready_plan();
    let snap = engine.snapshot().unwrap();
    assert!(!snap.choice.both_slots_enabled);
    assert!(snap.choice.both_slots_reason.contains("fallback"));
}

#[test]
fn wrong_checksum_blocks() {
    let mut engine = engine();
    engine.select_device(PRIMARY_SERIAL).unwrap();
    let mut bad = FIXTURE_SHA256.to_string();
    bad.replace_range(0..1, "f");
    let snap = engine.firmware_open(FIXTURE_OTA_NAME, &bad).unwrap();
    let notice = snap.notice.expect("notice");
    assert!(notice
        .gates
        .iter()
        .any(|gate| gate.id == "G05" && gate.status == "fail"));
    assert!(snap.firmware.is_none());
}

#[test]
fn other_phone_package_blocks() {
    let mut engine = engine();
    engine.select_device(PRIMARY_SERIAL).unwrap();
    let name = format!("other-{}-full.zip", &FIXTURE_SHA256[..8]);
    let snap = engine.firmware_open(&name, FIXTURE_SHA256).unwrap();
    let notice = snap.notice.expect("notice");
    assert!(notice
        .gates
        .iter()
        .any(|gate| gate.id == "G04" && gate.status == "fail"));
}

#[test]
fn plan_hash_is_stable_and_labels_a_dry_run() {
    let first = ready_plan();
    let second = ready_plan();
    let snap = first.snapshot().unwrap();
    let plan = snap.plan.unwrap();
    let hash_b = plan_hash(&second);
    assert_eq!(plan.plan_hash, hash_b);
    assert!(plan.plan_hash.starts_with("flp1-"));
    assert!(plan.dry_run);
    assert!(plan.plan_code.starts_with("DRY "));
    assert_eq!(plan.kind, PlanKind::UpdateKeepRoot);
    assert_eq!(snap.review_kind, Some(PlanKind::UpdateKeepRoot));

    let mut other = engine();
    other.select_device(PRIMARY_SERIAL).unwrap();
    other.continue_from_connect().unwrap();
    other
        .set_choice("update_keep_root", Route::FactoryKeepData, true)
        .unwrap();
    other.continue_from_choose().unwrap();
    other
        .firmware_open(FIXTURE_FACTORY_NAME, FIXTURE_SHA256)
        .unwrap();
    other.build_plan().unwrap();
    let factory = other.snapshot().unwrap().plan.unwrap();
    assert_ne!(plan.plan_hash, factory.plan_hash);
    assert!(factory.plan_code.starts_with("DRY "));
    // Regenerated goldens: dry_run is inside the hash.
    assert_eq!(
        plan.plan_hash,
        "flp1-904290458f91d47cf0ee00933ffa2640c074b844a504d2695287b8c2bee5e91e"
    );
    assert_eq!(
        factory.plan_hash,
        "flp1-937912b265eeab0ded5f5b05d45a4a38b5a8fbfbdb2669f6c7262ab71d68958d"
    );
}

#[test]
fn confirm_rejects_a_foreign_hash_and_leaves_the_plan_issued() {
    let mut engine = ready_plan();
    let hash = plan_hash(&engine);
    for bad in [
        "",
        "flp1-",
        &hash.to_uppercase(),
        &format!("{hash} "),
        &hash[..hash.len() - 1],
    ] {
        let err = engine.confirm_and_run(bad).unwrap_err();
        assert!(matches!(err, CoreError::Rejected { .. }), "{bad:?}");
        assert_eq!(engine.plan_life_name(), Some("issued"));
        assert_eq!(engine.snapshot().unwrap().phase, Phase::Review);
    }
    assert!(engine.confirm_and_run(&hash).is_err() || engine.snapshot().unwrap().plan.is_some());
    // The dry plan itself is refused, and it stays issued.
    let err = engine.confirm_and_run(&hash).unwrap_err();
    assert!(matches!(err, CoreError::DryRunPlan));
    assert_eq!(engine.plan_life_name(), Some("issued"));
}

#[test]
fn confirm_rejects_an_expired_plan_and_a_retry_is_discarded() {
    let mut engine = ready_plan();
    let hash = plan_hash(&engine);
    engine.set_clock(Box::new(FixedClock(1_760_000_000_000 + 16 * 60 * 1000)));
    let err = engine.confirm_and_run(&hash).unwrap_err();
    assert!(err.to_string().contains("expired"));
    assert_eq!(engine.plan_life_name(), Some("discarded"));
    let again = engine.confirm_and_run(&hash).unwrap_err();
    assert!(matches!(again, CoreError::Discarded));
}

#[test]
fn dry_run_prints_writes_reissues_a_real_plan_and_touches_nothing() {
    let mut engine = ready_plan();
    let hash = plan_hash(&engine);
    let snap = engine.dry_run(&hash).unwrap();
    assert_eq!(snap.job.state, "dry_done");
    assert!(snap
        .job
        .lines
        .iter()
        .any(|line| line.text.starts_with("WOULD RUN:")));
    assert!(!snap
        .job
        .lines
        .iter()
        .any(|line| line.text.contains("--slot all")));
    assert!(!snap
        .job
        .lines
        .iter()
        .any(|line| line.text.contains("WOULD RUN") && line.text.contains("getprop")));
    assert_eq!(engine.transport().write_calls(), 0);
    let real = snap.plan.expect("real plan");
    assert_ne!(real.plan_hash, hash);
    assert!(!real.dry_run);
    assert!(real.plan_code.starts_with("PLAN "));
    assert_eq!(real.after_dry_run.as_deref(), Some(hash.as_str()));
    assert!(snap.notice.unwrap().message.contains("plan code changed"));
    let again = engine.dry_run(&hash).unwrap_err();
    assert!(matches!(again, CoreError::AlreadyUsed));
    assert!(engine.consumed_contains(&hash));
}

#[test]
fn dry_run_on_a_real_plan_is_refused() {
    let mut engine = engine();
    engine.select_device(PRIMARY_SERIAL).unwrap();
    engine.continue_from_connect().unwrap();
    engine
        .set_choice("update_keep_root", Route::Ota, false)
        .unwrap();
    engine.continue_from_choose().unwrap();
    engine
        .firmware_open(FIXTURE_OTA_NAME, FIXTURE_SHA256)
        .unwrap();
    engine.build_plan().unwrap();
    let plan = engine.snapshot().unwrap().plan.unwrap();
    assert!(plan.plan_code.starts_with("PLAN "));
    assert!(!plan.dry_run);
    let err = engine.dry_run(&plan.plan_hash).unwrap_err();
    assert!(matches!(err, CoreError::NotDryRun));
    assert_eq!(engine.plan_life_name(), Some("issued"));
}

#[test]
fn dry_run_rejects_an_expired_plan() {
    let mut engine = ready_plan();
    let hash = plan_hash(&engine);
    engine.set_clock(Box::new(FixedClock(1_760_000_000_000 + 16 * 60 * 1000)));
    let err = engine.dry_run(&hash).unwrap_err();
    assert!(err.to_string().contains("expired"));
    assert_eq!(engine.transport().write_calls(), 0);
}

#[test]
fn blocked_dry_run_does_not_issue_a_real_plan() {
    let mut engine = ready_plan();
    let hash = plan_hash(&engine);
    engine.set_write_gates(false, true, true);
    let snap = engine.dry_run(&hash).unwrap();
    assert!(snap
        .job
        .lines
        .iter()
        .any(|line| line.text.contains("WOULD BLOCK: G21")));
    assert!(snap.plan.is_none());
    assert_eq!(engine.transport().write_calls(), 0);

    let mut missing = ready_plan();
    missing.set_write_gates(true, true, false);
    let snap = missing.dry_run(&plan_hash(&missing)).unwrap();
    assert!(snap
        .job
        .lines
        .iter()
        .any(|line| line.text.contains("WOULD BLOCK: G15")));
    assert!(snap.plan.is_none());

    let mut server = ready_plan();
    server.set_write_gates(true, false, true);
    let snap = server.dry_run(&plan_hash(&server)).unwrap();
    assert!(snap
        .job
        .lines
        .iter()
        .any(|line| line.text.contains("WOULD BLOCK: G22")));

    let mut locked = ready_plan();
    locked.set_step_bootloader(false);
    let snap = locked.dry_run(&plan_hash(&locked)).unwrap();
    assert!(snap
        .job
        .lines
        .iter()
        .any(|line| line.text.contains("WOULD BLOCK: G03")));
    assert!(snap.plan.is_none());
}

#[test]
fn successful_confirm_reports_root_without_device_writes_and_is_single_use() {
    let mut engine = ready_plan();
    let dry = plan_hash(&engine);
    let snap = engine.dry_run(&dry).unwrap();
    let hash = snap.plan.unwrap().plan_hash;
    let snap = engine.confirm_and_run(&hash).unwrap();
    assert_eq!(snap.phase, Phase::Done);
    assert!(snap.job.result_body.contains("Magisk app"));
    assert_eq!(engine.transport().write_calls(), 0);
    assert!(engine.consumed_contains(&hash));
    let again = engine.confirm_and_run(&hash).unwrap_err();
    assert!(matches!(again, CoreError::AlreadyUsed));
}

#[test]
fn confirm_is_single_use_under_contention() {
    let mut engine = ready_plan();
    let dry = plan_hash(&engine);
    let hash = engine.dry_run(&dry).unwrap().plan.unwrap().plan_hash;
    let shared = Arc::new(Mutex::new(engine));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let shared = Arc::clone(&shared);
        let hash = hash.clone();
        handles.push(thread::spawn(move || {
            let mut accepted = 0;
            for _ in 0..1000 {
                let result = shared.lock().expect("engine").confirm_and_run(&hash);
                if result.is_ok() {
                    accepted += 1;
                }
            }
            accepted
        }));
    }
    let total: i32 = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .sum();
    assert_eq!(total, 1);
    assert!(shared.lock().unwrap().consumed_contains(&hash));
}

#[test]
fn confirm_and_dry_run_only_from_the_matching_review() {
    let states = [
        "connect",
        "choose",
        "pick_firmware",
        "patching",
        "dry_run_done",
        "flash",
        "done",
        "recovery",
        "review_other",
    ];
    for name in states {
        let mut engine = ready_plan();
        let hash = plan_hash(&engine);
        engine.testing_set_state(name);
        let confirm = engine.confirm_and_run(&hash).unwrap_err();
        assert!(
            matches!(confirm, CoreError::WrongState),
            "{name} confirm {confirm}"
        );
        assert_eq!(engine.plan_life_name(), Some("issued"), "{name}");
        let dry = engine.dry_run(&hash).unwrap_err();
        assert!(matches!(dry, CoreError::WrongState), "{name} dry {dry}");
        assert_eq!(engine.plan_life_name(), Some("issued"), "{name}");
        assert_eq!(engine.transport().write_calls(), 0);
    }
}

#[test]
fn back_clears_the_plan() {
    let mut engine = ready_plan();
    let hash = plan_hash(&engine);
    engine.back().unwrap();
    assert!(engine.snapshot().unwrap().plan.is_none());
    let err = engine.confirm_and_run(&hash).unwrap_err();
    assert!(matches!(err, CoreError::WrongState));
}

#[test]
fn device_change_discards_the_plan() {
    let mut engine = flip_engine(false);
    let hash = engine.snapshot().unwrap().plan.unwrap().plan_hash;
    engine.transport().flipped.store(true, Ordering::SeqCst);
    let err = engine.confirm_and_run(&hash).unwrap_err();
    assert!(err.to_string().contains("changed"));
    assert_eq!(engine.plan_life_name(), Some("discarded"));
    assert!(matches!(
        engine.confirm_and_run(&hash).unwrap_err(),
        CoreError::Discarded
    ));
}

#[test]
fn input_change_discards_the_plan() {
    let mut engine = ready_plan();
    let hash = plan_hash(&engine);
    engine.testing_spoil_inputs();
    let err = engine.dry_run(&hash).unwrap_err();
    assert!(err.to_string().contains("package"));
    assert_eq!(engine.plan_life_name(), Some("discarded"));
}

#[test]
fn boot_only_phone_flashes_boot() {
    let engine = flip_engine(true);
    let plan = engine.snapshot().unwrap().plan.unwrap();
    let flash = plan
        .steps
        .iter()
        .find(|step| step.id == "flash_patched")
        .unwrap();
    assert!(flash.argv.contains(&"boot".to_string()), "{:?}", flash.argv);
    assert!(
        !flash.argv.contains(&"init_boot".to_string()),
        "{:?}",
        flash.argv
    );
}

#[test]
fn root_check_reads_su() {
    let engine = ready_plan();
    let plan = engine.snapshot().unwrap().plan.unwrap();
    let verify = plan
        .steps
        .iter()
        .find(|step| step.id == "verify_root")
        .unwrap();
    assert!(verify.argv.contains(&"su".to_string()), "{:?}", verify.argv);
    assert!(verify
        .argv
        .windows(2)
        .any(|pair| pair[0] == "su" && pair[1] == "-c"));
}

#[test]
fn recovery_plan_is_a_new_review_and_the_old_hash_does_not_run() {
    let mut engine = ready_plan();
    let dry = plan_hash(&engine);
    let hash = engine.dry_run(&dry).unwrap().plan.unwrap().plan_hash;
    let snap = engine.confirm_with_fault(&hash, true).unwrap();
    assert_eq!(snap.phase, Phase::Recovery);
    assert_eq!(snap.job.recovery.len(), 5);
    let snap = engine.recovery_plan("stock").unwrap();
    let plan = snap.plan.unwrap();
    assert_ne!(plan.plan_hash, hash);
    assert_eq!(plan.kind, PlanKind::Recovery);
    assert_eq!(snap.review_kind, Some(PlanKind::Recovery));
    assert_eq!(snap.phase, Phase::Review);
    let err = engine.confirm_and_run(&hash).unwrap_err();
    assert!(matches!(err, CoreError::AlreadyUsed));
    assert_eq!(engine.transport().write_calls(), 0);
}

#[test]
fn stop_after_a_step_stops() {
    let mut engine = ready_plan();
    let hash = engine
        .dry_run(&plan_hash(&engine))
        .unwrap()
        .plan
        .unwrap()
        .plan_hash;
    engine.arm_stop_after(1);
    let snap = engine.confirm_and_run(&hash).unwrap();
    assert_eq!(snap.job.state, "cancelled");
    assert!(snap.job.status_line.contains("Stopped after this step"));
    assert!(snap.job.lines.len() < 8);
}

#[test]
fn patched_flash_failure_ends_in_recovery() {
    let mut engine = ready_plan();
    let hash = engine
        .dry_run(&plan_hash(&engine))
        .unwrap()
        .plan
        .unwrap()
        .plan_hash;
    let snap = engine.confirm_with_fault(&hash, true).unwrap();
    assert_eq!(snap.phase, Phase::Recovery);
    assert_eq!(snap.job.recovery.len(), 5);
    assert_eq!(engine.transport().write_calls(), 0);
}

#[test]
fn sample_writes_stay_at_zero() {
    let engine = ready_plan();
    assert_eq!(engine.transport().write_calls(), 0);
}

#[test]
fn burst_stops_when_cancelled() {
    let engine = engine();
    let stats = engine.burst_logs(50_000, Some(1_000));
    assert_eq!(stats.emitted, 1_000);
    assert!(stats.cancelled);
    let full = engine.burst_logs(50_000, None);
    assert_eq!(full.emitted, 50_000);
    assert!(!full.cancelled);
}

#[test]
fn external_links_are_allow_listed() {
    assert!(super::external_url("platform_tools").is_ok());
    assert!(super::external_url("https://example.invalid").is_err());
}

#[test]
fn link_notice_hides_the_address() {
    let mut engine = engine();
    let snap = engine.note_link("firmware_full").unwrap();
    let json = serde_json::to_string(&snap).unwrap().to_ascii_lowercase();
    assert!(!json.contains("http"));
    assert!(!json.contains("google"));
    assert!(engine.note_link("nope").is_err());
}

#[test]
fn happy_path_names_the_magisk_app() {
    let engine = ready_plan();
    let json = serde_json::to_string(&engine.snapshot().unwrap()).unwrap();
    let lower = json.to_ascii_lowercase();
    assert!(lower.contains("magisk app"));
    assert!(!lower.contains("pixelflasher"));
    assert!(!lower.contains("on-device root tool"));
}

#[test]
fn argv_never_names_every_slot() {
    let engine = ready_plan();
    let plan = engine.snapshot().unwrap().plan.unwrap();
    for step in plan.steps {
        assert!(!step
            .argv
            .windows(2)
            .any(|pair| pair[0] == "--slot" && pair[1] == "all"));
    }
}

#[test]
fn prepare_patch_reviews_that_kind() {
    let mut engine = ready_plan();
    let id = engine.snapshot().unwrap().firmware.unwrap().id;
    let snap = engine.prepare_patch(&id).unwrap();
    let plan = snap.plan.unwrap();
    assert_eq!(plan.kind, PlanKind::PreparePatch);
    assert!(!plan.dry_run);
    assert!(plan.plan_code.starts_with("PLAN "));
    assert_eq!(snap.review_kind, Some(PlanKind::PreparePatch));
    let err = engine.dry_run(&plan.plan_hash).unwrap_err();
    assert!(matches!(err, CoreError::NotDryRun));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn random_sequences_reach_a_run_only_from_review(seed in proptest::collection::vec(0u8..12, 1..12)) {
        let mut engine = ready_plan();
        let mut previous = engine.snapshot().unwrap().phase;
        for command in seed {
            let hash = engine.snapshot().unwrap().plan.as_ref().map(|plan| plan.plan_hash.clone());
            let review_kind = engine.snapshot().unwrap().review_kind;
            match command {
                0 => { let _ = engine.scan(); }
                1 => { let _ = engine.back(); }
                2 => { let _ = engine.tools_status(); }
                3 => {
                    if let Some(hash) = hash {
                        let _ = engine.dry_run(&hash);
                    }
                }
                4 => {
                    if let Some(hash) = hash {
                        let _ = engine.confirm_and_run(&hash);
                    }
                }
                5 => { let _ = engine.cancel(); }
                6 => { let _ = engine.backups_list(); }
                _ => { let _ = engine.snapshot(); }
            }
            let snap = engine.snapshot().unwrap();
            if matches!(snap.phase, Phase::Flash | Phase::Done) && previous != Phase::Flash && previous != Phase::Done {
                prop_assert_eq!(previous, Phase::Review);
                prop_assert!(review_kind.is_some());
            }
            previous = snap.phase;
        }
    }
}
