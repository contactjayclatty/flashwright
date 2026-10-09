fn main() {
    let runner = std::sync::Arc::new(flashwright_core::proc::ScriptedRunner::new());
    let transport = flashwright_core::device::PlatformToolsTransport::new(
        runner,
        std::path::PathBuf::from("/opt/flashwright-test/adb"),
        std::path::PathBuf::from("/opt/flashwright-test/fastboot"),
        flashwright_core::device::TransportConfig::for_tests(),
    );
    let mut session = flashwright_core::wizard::WizardSession::new(transport);
    let _ = session.set_safety();
}
