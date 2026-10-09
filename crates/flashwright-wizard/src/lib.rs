// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Update wizard engine.
//!
//! Phones are a trait plus sample data held in memory. Nothing here talks to
//! a phone, and this crate does not depend on a UI toolkit.

#![forbid(unsafe_code)]

mod device;
mod engine;
mod error;
mod mock;
mod model;
mod plan;

pub use device::{
    inactive_slot, unlocked_from_props, DeviceInfo, DeviceSummary, DeviceTransport, DeviceWrite,
    Mode, Partition, Slot,
};
pub use engine::{external_url, Engine, FixedClock, SystemClock};
pub use error::CoreError;
pub use mock::{
    MockTransport, FIXTURE_FACTORY_NAME, FIXTURE_OTA_NAME, FIXTURE_SHA256, PRIMARY_SERIAL,
};
pub use model::{
    BackupSet, BurstStats, ChoiceView, DriverStatus, ExternalLink, FirmwareReport, JobView,
    LogLine, Notice, Phase, RecoveryOption, Snapshot, ToolsStatus,
};
pub use plan::{
    plan_code, quote_argv, GateView, PlanPreview, Route, Step, StepClass, Tool, SCHEMA,
};

#[cfg(test)]
mod tests {
    use super::*;

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
            .open_firmware(FIXTURE_OTA_NAME, FIXTURE_SHA256)
            .unwrap();
        engine.build_plan().unwrap();
        engine
    }

    #[test]
    fn unknown_slot_is_an_error() {
        let err = inactive_slot(None).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn inactive_slot_is_the_other_one() {
        assert_eq!(inactive_slot(Some(Slot::A)).unwrap(), Slot::B);
        assert_eq!(inactive_slot(Some(Slot::B)).unwrap(), Slot::A);
    }

    #[test]
    fn unlock_prop_zero_means_unlocked() {
        assert!(unlocked_from_props("0", "green"));
        assert!(unlocked_from_props("1", "orange"));
        assert!(!unlocked_from_props("1", "green"));
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
        let snap = engine.open_firmware(FIXTURE_OTA_NAME, &bad).unwrap();
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
        let snap = engine.open_firmware(&name, FIXTURE_SHA256).unwrap();
        let notice = snap.notice.expect("notice");
        assert!(notice
            .gates
            .iter()
            .any(|gate| gate.id == "G04" && gate.status == "fail"));
    }

    #[test]
    fn plan_hash_is_stable_and_sensitive() {
        let first = ready_plan();
        let second = ready_plan();
        let hash_a = first.snapshot().unwrap().plan.unwrap().plan_hash;
        let hash_b = second.snapshot().unwrap().plan.unwrap().plan_hash;
        assert_eq!(hash_a, hash_b);
        assert!(hash_a.starts_with("flp1-"));
        assert_eq!(
            hash_a,
            "flp1-5d7c6d3d4be43d94c6c9e09e774cc2cfbc70df5053d86b97f01719105a6015b6"
        );

        let mut other = engine();
        other.select_device(PRIMARY_SERIAL).unwrap();
        other.continue_from_connect().unwrap();
        other
            .set_choice("update_keep_root", Route::FactoryKeepData, true)
            .unwrap();
        other.continue_from_choose().unwrap();
        other
            .open_firmware(FIXTURE_FACTORY_NAME, FIXTURE_SHA256)
            .unwrap();
        other.build_plan().unwrap();
        let hash_c = other.snapshot().unwrap().plan.unwrap().plan_hash;
        assert_ne!(hash_a, hash_c);
        assert_eq!(
            hash_c,
            "flp1-f8c1f9e13a6c822320af19636431ecebebd4942bb0ecbcc700290d5a31d2f480"
        );
    }

    #[test]
    fn confirm_rejects_a_foreign_hash() {
        let mut engine = ready_plan();
        let err = engine.confirm_and_run("flp1-deadbeef").unwrap_err();
        assert!(matches!(err, CoreError::Rejected { .. }));
    }

    #[test]
    fn confirm_rejects_an_expired_plan() {
        let mut engine = ready_plan();
        let hash = engine.snapshot().unwrap().plan.unwrap().plan_hash;
        engine.set_clock(Box::new(FixedClock(1_760_000_000_000 + 16 * 60 * 1000)));
        let err = engine.confirm_and_run(&hash).unwrap_err();
        assert!(err.to_string().contains("expired"));
    }

    #[test]
    fn dry_run_prints_writes_and_does_not_touch_the_device() {
        let mut engine = ready_plan();
        let hash = engine.snapshot().unwrap().plan.unwrap().plan_hash;
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
        assert_eq!(engine.transport().write_calls(), 0);
        assert!(snap.plan.is_some());
    }

    #[test]
    fn successful_confirm_reports_root_without_device_writes() {
        let mut engine = ready_plan();
        let hash = engine.snapshot().unwrap().plan.unwrap().plan_hash;
        let snap = engine.confirm_and_run(&hash).unwrap();
        assert_eq!(snap.phase, Phase::Done);
        assert!(snap.job.result_body.contains("Root is working"));
        assert_eq!(engine.transport().write_calls(), 0);
    }

    #[test]
    fn patched_flash_failure_ends_in_recovery() {
        let mut engine = ready_plan();
        let hash = engine.snapshot().unwrap().plan.unwrap().plan_hash;
        let snap = engine.confirm_with_fault(&hash, true).unwrap();
        assert_eq!(snap.phase, Phase::Recovery);
        assert_eq!(snap.job.recovery.len(), 5);
        assert_eq!(engine.transport().write_calls(), 0);
    }

    #[test]
    fn mock_write_trait_refuses_real_flashes() {
        let engine = engine();
        let err = engine
            .transport()
            .flash(PRIMARY_SERIAL, Slot::B, Partition::InitBoot, "patched.img")
            .unwrap_err();
        assert!(matches!(err, CoreError::DeviceLayerStub));
        assert_eq!(engine.transport().write_calls(), 1);
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
        assert!(external_url("platform_tools").is_ok());
        assert!(external_url("https://example.invalid").is_err());
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
    fn happy_path_text_avoids_reserved_names() {
        let engine = ready_plan();
        let json = serde_json::to_string(&engine.snapshot().unwrap())
            .unwrap()
            .to_ascii_lowercase();
        for word in ["pixelflasher", "pixel", "magisk", "google"] {
            assert!(!json.contains(word), "{word} leaked into wizard state");
        }
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
}
