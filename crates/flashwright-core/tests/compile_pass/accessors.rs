fn show(token: &flashwright_core::WriteToken) -> (&str, &str, u128) {
    (token.plan_hash(), token.serial(), token.run_id())
}

fn main() {
    let _ = show;
}
