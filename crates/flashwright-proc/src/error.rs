// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::path::Path;

use thiserror::Error;

/// Failure while starting or collecting a child process.
#[derive(Debug, Error)]
pub enum ProcError {
    /// The program path was relative. Callers pass an absolute path.
    #[error("program path must be absolute")]
    ProgramNotAbsolute,

    /// The program name is a shell or a batch file.
    #[error("refusing to run shell program {name}")]
    ShellForbidden { name: String },

    /// The operating system rejected the spawn.
    #[error("failed to start {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    /// A scripted runner had no matching response. Tests treat this as a bug.
    #[error("no scripted response for {program} args {args:?}")]
    NoScript { program: String, args: Vec<String> },

    /// A Windows job object could not be created or assigned.
    #[error("failed to assign the process to a job: {0}")]
    Job(String),

    /// Reading or waiting on the child failed.
    #[error("process io error: {0}")]
    Io(#[from] std::io::Error),
}

impl ProcError {
    pub(crate) fn spawn(program: &Path, source: std::io::Error) -> Self {
        Self::Spawn {
            program: program.display().to_string(),
            source,
        }
    }
}
