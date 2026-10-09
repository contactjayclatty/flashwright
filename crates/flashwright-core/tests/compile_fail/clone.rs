#[allow(dead_code)]
fn reject_clone(token: flashwright_core::WriteToken) {
    let _copied = token.clone();
}

fn main() {}
