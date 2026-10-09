use flashwright_core::serde::Deserialize;

#[allow(dead_code)]
fn reject_deserialize<'de, T: Deserialize<'de>>() {}

#[allow(dead_code)]
fn check() {
    reject_deserialize::<flashwright_core::WriteToken>();
}

fn main() {}
