#[allow(dead_code)]
fn reject_copy(token: flashwright_core::WriteToken) {
    let _moved = token;
    let _again = token;
}

fn main() {}
