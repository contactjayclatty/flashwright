// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Compare our adb with a server that is already listening.
//!
//! The host protocol exposes a protocol number, not the SDK version. When a
//! server is already up, Flashwright asks the user before restarting it.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use flashwright_proc::{CommandRunner, Invocation, ProcessGroup};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::version::SdkVersion;
use crate::ToolsError;

pub const DEFAULT_ADB_PORT: u16 = 5037;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerObservation {
    pub listening: bool,
    pub protocol: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerStatus {
    pub client_version: SdkVersion,
    pub needs_restart_confirmation: bool,
    pub message: Option<String>,
}

pub fn assess_server(client: &SdkVersion, observation: &ServerObservation) -> ServerStatus {
    if !observation.listening {
        return ServerStatus {
            client_version: client.clone(),
            needs_restart_confirmation: false,
            message: None,
        };
    }
    let shown = match observation.protocol {
        Some(protocol) => format!("protocol {protocol}"),
        None => "unknown".to_string(),
    };
    ServerStatus {
        client_version: client.clone(),
        needs_restart_confirmation: true,
        message: Some(format!(
            "Another adb is running (version {shown}). Flashwright will restart it."
        )),
    }
}

pub async fn probe_adb_server(port: u16) -> ServerObservation {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let connect = tokio::time::timeout(Duration::from_millis(400), TcpStream::connect(addr)).await;
    let Ok(Ok(mut stream)) = connect else {
        return ServerObservation {
            listening: false,
            protocol: None,
        };
    };
    if stream.write_all(b"000chost:version").await.is_err() {
        return ServerObservation {
            listening: true,
            protocol: None,
        };
    }
    let mut buf = [0u8; 64];
    let read = tokio::time::timeout(Duration::from_millis(400), stream.read(&mut buf)).await;
    let Ok(Ok(count)) = read else {
        return ServerObservation {
            listening: true,
            protocol: None,
        };
    };
    let text = String::from_utf8_lossy(&buf[..count]);
    ServerObservation {
        listening: true,
        protocol: parse_host_version(&text),
    }
}

pub fn parse_host_version(text: &str) -> Option<u32> {
    let rest = text.trim().strip_prefix("OKAY")?;
    if rest.len() < 8 {
        return None;
    }
    let len = usize::from_str_radix(&rest[..4], 16).ok()?;
    let data = rest.get(4..4 + len)?;
    u32::from_str_radix(data, 16).ok()
}

/// `kill-server` then `start-server`. `start-server` is detached.
///
/// Returns [`ToolsError::RestartNotConfirmed`] unless the user agreed.
pub async fn restart_adb_server<R: CommandRunner>(
    runner: &R,
    adb: &Path,
    user_confirmed: bool,
) -> Result<(), ToolsError> {
    if !user_confirmed {
        return Err(ToolsError::RestartNotConfirmed);
    }
    let kill = runner
        .run(Invocation {
            program: adb.to_path_buf(),
            args: vec!["kill-server".into()],
            timeout: Duration::from_secs(15),
            watchdog: None,
            group: ProcessGroup::TiedToParent,
        })
        .await?;
    if !kill.success_exit() {
        return Err(ToolsError::Version {
            detail: format!("adb kill-server exited {:?}", kill.exit_code),
        });
    }
    let start = runner
        .run(Invocation {
            program: adb.to_path_buf(),
            args: vec!["start-server".into()],
            timeout: Duration::from_secs(15),
            watchdog: None,
            group: ProcessGroup::Detached,
        })
        .await?;
    if !start.success_exit() {
        return Err(ToolsError::Version {
            detail: format!("adb start-server exited {:?}", start.exit_code),
        });
    }
    Ok(())
}
