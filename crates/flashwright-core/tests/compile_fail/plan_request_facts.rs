fn main() {
    let _request = flashwright_core::wizard::PlanRequest {
        serial: String::new(),
        dry_run: true,
        steps: Vec::new(),
        firmware: flashwright_core::wizard::FirmwareClaim::default(),
        facts: (),
    };
}
