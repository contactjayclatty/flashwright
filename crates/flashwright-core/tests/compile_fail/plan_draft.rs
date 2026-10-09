fn main() {
    let _plan = flashwright_core::wizard::PlanDraft {
        serial: String::new(),
        dry_run: false,
        steps: Vec::new(),
        firmware: flashwright_core::wizard::FirmwareClaim::default(),
    };
}
