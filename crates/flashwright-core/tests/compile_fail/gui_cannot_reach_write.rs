fn main() {
    let _mint = flashwright_core::token::mint_confirmed("", "", &[]);
    let runner = std::sync::Arc::new(flashwright_core::proc::ScriptedRunner::new());
    let transport = flashwright_core::device::PlatformToolsTransport::new(
        runner,
        "/opt/flashwright/adb",
        "/opt/flashwright/fastboot",
        flashwright_core::device::TransportConfig::for_tests(),
    );
    let _write = transport.run_write(loop {}, loop {});
}
