// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

#[test]
fn write_token_is_crate_private() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/compile_fail/*.rs");
    cases.pass("tests/compile_pass/*.rs");
}
