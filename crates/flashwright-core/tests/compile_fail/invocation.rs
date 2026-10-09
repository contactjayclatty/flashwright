fn main() {
    let _call = flashwright_core::proc::Invocation::tied(
        "/bin/sh",
        vec!["-c".to_string()],
        std::time::Duration::from_secs(1),
    );
}
