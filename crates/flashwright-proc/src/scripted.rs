// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! In-memory adb/fastboot stand-in. It checks the same path rules as the
//! real runner and records every invocation.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::lines::{push_capped, tail_of, LineAssembler};
use crate::{file_name_lower, CommandRunner, Invocation, ProcError, RunResult, StdStream};

/// Canned output for one matching invocation.
#[derive(Clone, Debug)]
pub struct ScriptedResponse {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub delay: Duration,
    /// Ignore `stdout` and block until the watchdog or the overall timeout.
    pub hang: bool,
}

impl ScriptedResponse {
    pub fn ok(stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            exit_code: 0,
            stdout: stdout.into(),
            stderr: Vec::new(),
            delay: Duration::ZERO,
            hang: false,
        }
    }

    pub fn fail(exit_code: i32, stderr: impl Into<Vec<u8>>) -> Self {
        Self {
            exit_code,
            stdout: Vec::new(),
            stderr: stderr.into(),
            delay: Duration::ZERO,
            hang: false,
        }
    }

    pub fn with_stdout(mut self, stdout: impl Into<Vec<u8>>) -> Self {
        self.stdout = stdout.into();
        self
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn hang() -> Self {
        Self {
            exit_code: -1,
            stdout: Vec::new(),
            stderr: Vec::new(),
            delay: Duration::ZERO,
            hang: true,
        }
    }
}

type Responder = Box<dyn Fn(&Invocation, usize) -> ScriptedResponse + Send>;

enum RouteBody {
    Fixed(ScriptedResponse),
    Dynamic(Responder),
}

struct Route {
    exe: String,
    args_prefix: Vec<String>,
    hits: usize,
    body: RouteBody,
}

struct Inner {
    routes: Vec<Route>,
    calls: Vec<Invocation>,
}

/// Records calls and returns scripted output. Safe to share across tasks.
pub struct ScriptedRunner {
    inner: Mutex<Inner>,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
}

impl ScriptedRunner {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                routes: Vec::new(),
                calls: Vec::new(),
            }),
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
        }
    }

    pub fn on(&self, exe: &str, args_prefix: &[&str], response: ScriptedResponse) {
        self.push_route(exe, args_prefix, RouteBody::Fixed(response));
    }

    pub fn on_fn<F>(&self, exe: &str, args_prefix: &[&str], respond: F)
    where
        F: Fn(&Invocation, usize) -> ScriptedResponse + Send + 'static,
    {
        self.push_route(exe, args_prefix, RouteBody::Dynamic(Box::new(respond)));
    }

    fn push_route(&self, exe: &str, args_prefix: &[&str], body: RouteBody) {
        let mut inner = self.inner.lock().expect("scripted runner lock");
        inner.routes.push(Route {
            exe: exe.to_ascii_lowercase(),
            args_prefix: args_prefix.iter().map(|part| (*part).to_string()).collect(),
            hits: 0,
            body,
        });
    }

    pub fn calls(&self) -> Vec<Invocation> {
        self.inner
            .lock()
            .expect("scripted runner lock")
            .calls
            .clone()
    }

    pub fn max_in_flight(&self) -> usize {
        self.max_in_flight.load(Ordering::SeqCst)
    }

    fn note_flight(&self) {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(now, Ordering::SeqCst);
    }

    fn end_flight(&self) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Default for ScriptedRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRunner for ScriptedRunner {
    async fn run(&self, invocation: Invocation) -> Result<RunResult, ProcError> {
        invocation.validate()?;
        self.note_flight();
        let result = self.dispatch(invocation).await;
        self.end_flight();
        result
    }
}

impl ScriptedRunner {
    async fn dispatch(&self, invocation: Invocation) -> Result<RunResult, ProcError> {
        let response = {
            let mut inner = self.inner.lock().expect("scripted runner lock");
            let exe = file_name_lower(&invocation.program);
            let index = best_route(&inner.routes, &exe, &invocation.args).ok_or_else(|| {
                ProcError::NoScript {
                    program: invocation.program.display().to_string(),
                    args: invocation.args.clone(),
                }
            })?;
            inner.calls.push(invocation.clone());
            let route = &mut inner.routes[index];
            let hit = route.hits;
            route.hits += 1;
            match &route.body {
                RouteBody::Fixed(response) => response.clone(),
                RouteBody::Dynamic(function) => function(&invocation, hit),
            }
        };

        let started = std::time::Instant::now();
        if response.hang {
            return hang(&invocation, started).await;
        }
        if response.delay >= invocation.timeout && !invocation.timeout.is_zero() {
            tokio::time::sleep(invocation.timeout).await;
            return Ok(finish(
                None,
                Vec::new(),
                Vec::new(),
                true,
                false,
                started.elapsed(),
                true,
                false,
            ));
        }
        if !response.delay.is_zero() {
            tokio::time::sleep(response.delay).await;
        }
        Ok(finish(
            Some(response.exit_code),
            response.stdout,
            response.stderr,
            false,
            false,
            started.elapsed(),
            false,
            false,
        ))
    }
}

fn best_route(routes: &[Route], exe: &str, args: &[String]) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (index, route) in routes.iter().enumerate() {
        if route.exe != exe {
            continue;
        }
        if args.starts_with(&route.args_prefix) {
            let len = route.args_prefix.len();
            match best {
                Some((_, best_len)) if best_len > len => {}
                _ => best = Some((index, len)),
            }
        }
    }
    best.map(|(index, _)| index)
}

async fn hang(
    invocation: &Invocation,
    started: std::time::Instant,
) -> Result<RunResult, ProcError> {
    let watchdog = invocation
        .watchdog
        .filter(|wait| *wait < invocation.timeout);
    if let Some(wait) = watchdog {
        tokio::time::sleep(wait).await;
        return Ok(finish(
            None,
            Vec::new(),
            Vec::new(),
            false,
            false,
            started.elapsed(),
            false,
            true,
        ));
    }
    tokio::time::sleep(invocation.timeout).await;
    Ok(finish(
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        started.elapsed(),
        true,
        false,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn finish(
    exit_code: Option<i32>,
    stdout_raw: Vec<u8>,
    stderr_raw: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    duration: Duration,
    timed_out: bool,
    killed_by_watchdog: bool,
) -> RunResult {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut out_flag = stdout_truncated;
    let mut err_flag = stderr_truncated;
    push_capped(&mut stdout, &stdout_raw, &mut out_flag);
    push_capped(&mut stderr, &stderr_raw, &mut err_flag);
    let mut lines = LineAssembler::default();
    lines.push(StdStream::Stdout, &stdout, Duration::ZERO);
    lines.push(StdStream::Stderr, &stderr, Duration::ZERO);
    RunResult {
        exit_code,
        duration,
        stdout_tail: tail_of(&stdout),
        stderr_tail: tail_of(&stderr),
        stdout,
        stderr,
        stdout_truncated: out_flag,
        stderr_truncated: err_flag,
        lines: lines.into_lines(duration),
        timed_out,
        killed_by_watchdog,
    }
}
