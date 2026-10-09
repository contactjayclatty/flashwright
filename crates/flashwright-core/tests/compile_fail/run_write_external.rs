use std::sync::Arc;

fn main() {
    let runner = Arc::new(flashwright_core::proc::ScriptedRunner::new());
    let transport = flashwright_core::device::PlatformToolsTransport::new(
        runner,
        "/opt/flashwright/adb",
        "/opt/flashwright/fastboot",
        flashwright_core::device::TransportConfig::for_tests(),
    );
    let _result = transport.run_write(loop {}, loop {});
}
