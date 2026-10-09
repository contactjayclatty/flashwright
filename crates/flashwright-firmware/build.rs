// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../third_party/aosp/update_engine/update_metadata.proto");
    println!("cargo:rerun-if-changed={}", proto.display());
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", &protoc);
    prost_build::Config::new().compile_protos(&[proto.as_path()], &[proto.parent().unwrap()])?;
    Ok(())
}
