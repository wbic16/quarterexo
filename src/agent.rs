//! The agent daemon (v1.9.0): `qexo agent` — the
//! resident executor Will's 2026-11-26 ruling named
//! when the stage-1 spool retired into the CLI. One
//! verb, three passes per cycle, all over the node's
//! own state, every act through the shipped verbs and
//! the §4.6 oracle — the daemon invents no second
//! machinery:
//!
//! - TELEMETRY pass: writes the temp/load/free/mem
//!   observations §6 placement reads (an unmeasured
//!   node is ineligible outright, so the series is
//!   kept fresh — a new observation whenever the
//!   latest is 10+ minutes old).
//! - DISPATCH pass: a queued item whose brief carries
//!   `run:` stages is machine work; the pass places
//!   one per cycle through `qexo dispatch` (the §6
//!   gates, the fair draw, the signed bundle). Briefs
//!   without `run:` are agent work — never
//!   auto-dispatched, never executed here.
//! - EXECUTOR pass: for each live execution
//!   assignment whose brief carries `run:` stages —
//!   heartbeat, run the stages in order as shell
//!   (checkpoint per stage, progress carried on the
//!   assignment record so a daemon restart resumes
//!   mid-brief), then a `run complete` receipt the
//!   final checkpoint cites as its output; or the
//!   fail verb with the stage and its exit code as
//!   evidence. A brief's `ram-gb:` estimate above the
//!   host's total memory HELDS the item (the spool's
//!   rule): it beats, is marked held once, and does
//!   not run.
//! - COORDINATOR pass: the shipped heartbeat
//!   reconcile for in-flight items, plus the F4
//!   closer the dispatch core was missing — an
//!   executor's completion closes its item
//!   running → done ONLY on evidence this pass reads
//!   back itself: the executor's assignment record,
//!   its final checkpoint, and the checkpoint's
//!   cited outputs re-read and hash-matched. Anything
//!   less is receipted `progress unverified` and left
//!   open; a completion status is never consumed
//!   unverified (the heartbeat reconcile retires an
//!   assignment on any status, so the closer runs
//!   first and the reconcile never sees a completed
//!   item).
//!
//! The daemon declares `capability: shell` on its
//! identity scroll at startup: machine work routes
//! only to nodes that declare it, through the
//! existing §6 gates — a node becomes an executor by
//! running the daemon, and by nothing else.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::core;
use crate::dispatch::{self, Actor, Item, ItemState, Transition};
use crate::lattice::{self, Lattice};
use crate::phext::{self, fmt_coord, Coord};
use crate::wire;

/// How old the newest telemetry observation may be
/// before the telemetry pass writes a fresh one.
const TELEMETRY_FRESH_MINUTES: i64 = 10;
/// The beat cadence while a stage runs: well inside
/// the §8 silence window (3 missed 2-minute beats).
const STAGE_BEAT_SECS: u64 = 45;

/// `qexo agent [--once] [--interval <secs>]` — the
/// daemon loop. State resolution is the caller's
/// (main.rs marks the verb write-intending); an
/// unborn node is an install error, named as such —
/// `qexo config` births, install.sh drives.
pub fn run_agent(once: bool, interval_arg: Option<u64>) -> i32 {
    let mut lat = match core::require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    if core::ensure_shell_capability(&mut lat) {
        if let Err(e) = lat.save() {
            eprintln!("qexo agent: cannot save state: {}", e);
            return 1;
        }
        println!("agent: capability: shell declared on the identity scroll");
    }
    let cfg = lat.px.get(&lattice::CONFIG_RECORD).cloned().unwrap_or_default();
    let interval = interval_arg
        .or_else(|| lattice::field(&cfg, "agent_interval_secs").and_then(|v| v.parse().ok()))
        .unwrap_or(60)
        .max(5);
    let (_, stamp) = core::now_stamp();
    println!(
        "qexo agent {} — {} mode, pass interval {}s, started {}",
        crate::plan::PLAN_VERSION,
        if once { "single-pass" } else { "daemon" },
        interval,
        stamp
    );
    loop {
        agent_pass();
        if once {
            return 0;
        }
        std::thread::sleep(Duration::from_secs(interval));
    }
}

/// One full cycle: telemetry, dispatch, executor,
/// coordinator — in that order, so a brief enqueued
/// before the cycle can be placed, started, and (for
/// fast stages) closed within it.
fn agent_pass() {
    let (_, stamp) = core::now_stamp();
    println!("agent pass — {}", stamp);
    telemetry_pass();
    dispatch_pass();
    executor_pass();
    coordinator_pass();
}

// ----------------------------------------------------------------
// Telemetry pass
// ----------------------------------------------------------------

fn telemetry_pass() {
    let mut lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    let index = lat.px.get(&lattice::index_addr(lattice::HL_TELEMETRY)).cloned().unwrap_or_default();
    if let Some(coord_s) = wire::latest_observation(&index) {
        if let Some(c) = phext::parse_coord(&coord_s) {
            if let Some(obs) = lat.px.get(&c) {
                if let Some(stamp) = lattice::field(obs, "stamp") {
                    if let Some(m) = core::stamp_minutes(&stamp) {
                        if core::now_minutes() - m < TELEMETRY_FRESH_MINUTES {
                            return;
                        }
                    }
                }
            }
        }
    }
    let obs = core::record_telemetry_observation(&mut lat);
    if let Err(e) = lat.save() {
        eprintln!("agent telemetry: cannot save state: {}", e);
        return;
    }
    println!("agent telemetry: observation {} recorded (gate level {})", fmt_coord(&obs.addr), obs.level);
}

// ----------------------------------------------------------------
// Dispatch pass — machine work only, one placement
// per cycle, through the shipped dispatch verb.
// ----------------------------------------------------------------

fn dispatch_pass() {
    let lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    for item in dispatch::load_items(&lat.px) {
        if item.state != Some(ItemState::Queued) {
            continue;
        }
        let brief = lat.px.get(&dispatch::brief_addr(item.book)).cloned().unwrap_or_default();
        if wire::brief_facts(&brief).run_stages.is_empty() {
            continue;
        }
        let item_s = fmt_coord(&item.addr);
        println!("agent dispatch: {} is machine work — placing it through the §6 gates", item_s);
        wire::cmd_dispatch(Some(&item_s), None, false);
        return;
    }
}

// ----------------------------------------------------------------
// Executor pass
// ----------------------------------------------------------------

fn executor_pass() {
    let lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    let self_name = wire::local_node(&lat.px);
    // Collect first; every item's work reloads state
    // (the verbs own their loads and saves).
    let mut mine: Vec<Coord> = Vec::new();
    for item in dispatch::load_items(&lat.px) {
        if wire::my_execution_assignment(&lat, &item.addr).is_some() {
            mine.push(item.addr);
        }
    }
    for addr in mine {
        execute_item(&addr, &self_name);
    }
}

fn execute_item(addr: &Coord, self_name: &str) {
    let item_s = fmt_coord(addr);
    let lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    let (asg, asg_text, attempt) = match wire::my_execution_assignment(&lat, addr) {
        Some(x) => x,
        None => return,
    };
    let item = match wire::held_item(&lat, addr) {
        Some(i) => i,
        None => return,
    };
    if !matches!(item.state, Some(ItemState::Dispatched) | Some(ItemState::Running)) {
        return;
    }
    // The brief as the executor holds it: the attempt
    // universe's copy, falling back to universe 1.
    let brief = lat
        .universe((attempt + 1) as usize)
        .and_then(|px| px.get(&dispatch::brief_addr(item.book)).cloned())
        .or_else(|| lat.px.get(&dispatch::brief_addr(item.book)).cloned())
        .unwrap_or_default();
    let facts = wire::brief_facts(&brief);
    if facts.run_stages.is_empty() {
        return; // agent work — an agent executes it, not the daemon
    }

    // The HELD rule (spool parity): an estimate above
    // the host's total memory — or a host whose total
    // cannot be measured — never runs. The item beats
    // (it is alive, §8 sees no silence) and the hold
    // is marked on the assignment once.
    let need = facts.ram_bytes();
    let total = core::total_mem_bytes();
    if need > 0 && (total == 0 || need > total) {
        if lattice::field(&asg_text, "held").is_none() {
            let why = if total == 0 {
                format!(
                    "ram-gb estimate {:.1} GB — the host's total memory cannot be measured; held (the spool's rule)",
                    facts.ram_gb.unwrap_or(0.0)
                )
            } else {
                format!(
                    "ram-gb estimate {:.1} GB exceeds the host's total {:.1} GB; held (the spool's rule)",
                    facts.ram_gb.unwrap_or(0.0),
                    total as f64 / 1.0e9
                )
            };
            set_assignment_field(&asg.addr, "held", &why);
            println!("agent executor: {} HELD — {}", item_s, why);
        }
        wire::cmd_heartbeat(&item_s);
        return;
    }

    // The first beat moves the item dispatched →
    // running through the oracle.
    if item.state == Some(ItemState::Dispatched) {
        wire::cmd_heartbeat(&item_s);
        let lat2 = match core::require_born() {
            Ok(l) => l,
            Err(_) => return,
        };
        match wire::held_item(&lat2, addr).map(|i| i.state) {
            Some(Some(ItemState::Running)) => {}
            _ => {
                eprintln!("agent executor: {} did not reach running — leaving it to the coordinator", item_s);
                return;
            }
        }
    }

    let total_stages = facts.run_stages.len();
    let mut next: usize = lattice::field(&asg_text, "run-stage").and_then(|v| v.parse().ok()).unwrap_or(0);
    let workdir = work_dir(item.book, attempt);
    if let Err(e) = std::fs::create_dir_all(&workdir) {
        eprintln!("agent executor: cannot create work dir {}: {}", workdir.display(), e);
        return;
    }
    for i in next..total_stages {
        let stage = &facts.run_stages[i];
        println!("agent executor: {} stage {}/{} — {}", item_s, i + 1, total_stages, one_line(stage));
        let code = run_stage(stage, &item_s, attempt, i, &workdir);
        if code != 0 {
            let note = format!(
                "agent executor: stage {}/{} exited {}: {}",
                i + 1,
                total_stages,
                code,
                one_line(stage)
            );
            wire::cmd_fail(&[item_s.clone(), "--note".to_string(), note]);
            return;
        }
        set_assignment_field(&asg.addr, "run-stage", &(i + 1).to_string());
        let note = format!("agent stage {}/{} complete — exit 0: {}", i + 1, total_stages, one_line(stage));
        wire::cmd_checkpoint(&[item_s.clone(), "--note".to_string(), note]);
        next = i + 1;
    }
    if next < total_stages {
        return;
    }

    // Completion: the run complete receipt first, then
    // the final checkpoint citing it as an output —
    // the pair the origin's closer verifies by
    // read-back (§8's custody rule, at close time).
    let mut lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    let exits = vec!["0"; total_stages].join(",");
    let receipt = core::append_receipt(
        &mut lat.px,
        "run complete",
        &[
            ("item", &item_s),
            ("attempt", &attempt.to_string()),
            ("stages", &total_stages.to_string()),
            ("exits", &exits),
            ("host", self_name),
            ("workdir", &workdir.display().to_string()),
        ],
    );
    if let Err(e) = lat.save() {
        eprintln!("agent executor: cannot save the run complete receipt: {}", e);
        return;
    }
    let note = format!("agent run complete — all {} stage(s) exited 0", total_stages);
    wire::cmd_checkpoint(&[
        item_s.clone(),
        "--note".to_string(),
        note,
        "--output".to_string(),
        fmt_coord(&receipt),
    ]);
    // Retire this node's own assignment with the
    // completion status the coordinator's closer
    // reads (its own, or the origin's over the wire).
    let lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    if let Some((asg2, text2, _)) = wire::my_execution_assignment(&lat, addr) {
        let cite = lattice::field(&text2, "checkpoint").unwrap_or_default();
        let mut lat = lat;
        wire::retire_assignment(
            &mut lat,
            &asg2.addr,
            &format!("complete — run complete receipt {}; final checkpoint {}", fmt_coord(&receipt), cite),
        );
        if let Err(e) = lat.save() {
            eprintln!("agent executor: cannot save the assignment retirement: {}", e);
            return;
        }
    }
    println!("agent executor: {} complete — run complete receipt {}", item_s, fmt_coord(&receipt));
}

/// One stage's working directory: under the state
/// dir, one per (item, attempt) — a re-dispatch's
/// fresh attempt never inherits a closed attempt's
/// outputs.
fn work_dir(book: u32, attempt: u32) -> PathBuf {
    lattice::state_dir().join("work").join(format!("item-{}-a{}", book, attempt))
}

/// Run one stage as shell, beating while it runs.
/// Returns the exit code (-1 when the stage could
/// not be spawned or died by signal). Stage output
/// lands in the work dir, named by stage — the
/// executor's working papers, beside the checkpoints
/// that vouch for them.
fn run_stage(stage: &str, item_s: &str, attempt: u32, idx: usize, workdir: &Path) -> i32 {
    let stdout = match std::fs::File::create(workdir.join(format!("stage-{}-stdout.log", idx + 1))) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("agent executor: cannot open the stage log in {}: {}", workdir.display(), e);
            return -1;
        }
    };
    let stderr = match std::fs::File::create(workdir.join(format!("stage-{}-stderr.log", idx + 1))) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("agent executor: cannot open the stage log in {}: {}", workdir.display(), e);
            return -1;
        }
    };
    let mut child = match Command::new("/bin/sh")
        .arg("-c")
        .arg(stage)
        .current_dir(workdir)
        .env("QEXO_ITEM", item_s)
        .env("QEXO_ATTEMPT", attempt.to_string())
        .env("QEXO_STAGE", (idx + 1).to_string())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("agent executor: cannot spawn stage {}: {}", idx + 1, e);
            return -1;
        }
    };
    let mut last_beat = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code().unwrap_or(-1),
            Ok(None) => {
                if last_beat.elapsed() >= Duration::from_secs(STAGE_BEAT_SECS) {
                    wire::cmd_heartbeat(item_s);
                    last_beat = Instant::now();
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            Err(e) => {
                eprintln!("agent executor: stage {} wait failed: {}", idx + 1, e);
                return -1;
            }
        }
    }
}

/// Set one field on an assignment record, in its own
/// load/save (the daemon's bookkeeping writes are as
/// deliberate as the verbs').
fn set_assignment_field(addr: &Coord, key: &str, value: &str) {
    if let Ok(Some(mut lat)) = Lattice::load() {
        if let Some(t) = lat.px.get(addr).cloned() {
            let t = lattice::set_field(&t, key, value);
            lat.px.insert(*addr, t);
            let _ = lat.save();
        }
    }
}

/// A stage command as one line for notes and logs
/// (receipt evidence stays single-line by shape).
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ----------------------------------------------------------------
// Coordinator pass — the heartbeat reconcile for
// in-flight items, plus the F4 closer.
// ----------------------------------------------------------------

fn coordinator_pass() {
    let lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    let self_name = wire::local_node(&lat.px);
    let mut open: Vec<Coord> = Vec::new();
    for item in dispatch::load_items(&lat.px) {
        if matches!(item.state, Some(ItemState::Dispatched) | Some(ItemState::Running)) {
            open.push(item.addr);
        }
    }
    for addr in open {
        coordinate_item(&addr, &self_name);
    }
}

fn coordinate_item(addr: &Coord, self_name: &str) {
    let item_s = fmt_coord(addr);
    let lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    let item = match wire::held_item(&lat, addr) {
        Some(i) => i,
        None => return,
    };
    if !matches!(item.state, Some(ItemState::Dispatched) | Some(ItemState::Running)) {
        return;
    }

    // Remote executor: read its assignment record
    // back BEFORE any reconcile — the heartbeat verb
    // retires the origin assignment on ANY status, so
    // a completion must be closed (or receipted
    // unverified) here, never handed to it.
    if let Some((_asg, _t, attempt, node)) = wire::my_origin_assignment(&lat, addr) {
        if node != self_name {
            match wire::endpoint_of(&lat, &node) {
                Some(ep) => {
                    let remote = fetch_remote_assignment(&ep, &item_s, self_name, attempt);
                    match &remote {
                        Some(rtext)
                            if lattice::field(rtext, "status")
                                .map(|s| s.starts_with("complete"))
                                .unwrap_or(false) =>
                        {
                            close_from_remote(&item, rtext, &ep, &node, attempt);
                        }
                        _ => {
                            wire::cmd_heartbeat(&item_s);
                        }
                    }
                }
                None => {
                    wire::cmd_heartbeat(&item_s);
                }
            }
            return;
        }
    }

    // Self-executed: the highest-attempt execution
    // assignment is this node's own; a completion
    // status on the CURRENT attempt closes the item.
    if let Some((_rec, text, attempt)) = latest_execution_assignment(&lat, &item) {
        if attempt == item.attempts
            && lattice::field(&text, "status")
                .map(|s| s.starts_with("complete"))
                .unwrap_or(false)
        {
            close_from_local(&item, &text, attempt);
        }
    }
}

/// The executor's assignment scroll for this attempt,
/// read over the scroll route — the heartbeat
/// reconcile's own scan (Collection 3 Books, dense
/// from 1, matched on item + home + attempt).
fn fetch_remote_assignment(endpoint: &str, item_s: &str, self_name: &str, attempt: u32) -> Option<String> {
    let attempt_s = attempt.to_string();
    for b in 1..=64u32 {
        let c = lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_ASSIGN_C, 1, b, 1, 1, 1]);
        match wire::fetch_scroll(endpoint, &fmt_coord(&c)) {
            Ok(t) => {
                if lattice::field(&t, "item").as_deref() == Some(item_s)
                    && lattice::field(&t, "home").as_deref() == Some(self_name)
                    && lattice::field(&t, "attempt").as_deref() == Some(attempt_s.as_str())
                {
                    return Some(t);
                }
            }
            Err(_) => break,
        }
    }
    None
}

/// This node's highest-attempt execution assignment
/// for an item, live or retired (retirement ends
/// liveness, never the record).
fn latest_execution_assignment(lat: &Lattice, item: &Item) -> Option<(dispatch::AssignmentRec, String, u32)> {
    let item_s = fmt_coord(&item.addr);
    let mut best: Option<(u32, dispatch::AssignmentRec, String)> = None;
    for rec in dispatch::load_assignments(&lat.px) {
        if rec.item.as_deref() != Some(item_s.as_str()) {
            continue;
        }
        if let Some(t) = wire::assignment_text(lat, &rec) {
            if lattice::field(&t, "home").is_some() {
                let a = rec.attempt.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
                if best.as_ref().map(|(ba, _, _)| a > *ba).unwrap_or(true) {
                    best = Some((a, rec, t));
                }
            }
        }
    }
    best.map(|(a, rec, t)| (rec, t, a))
}

/// Close an item whose executor is remote: the F4
/// evidence is built only from scrolls this pass
/// fetches from the executor itself.
fn close_from_remote(item: &Item, remote: &str, endpoint: &str, node: &str, attempt: u32) {
    let item_s = fmt_coord(&item.addr);
    let status = lattice::field(remote, "status").unwrap_or_default();
    let cp_cite = match lattice::field(remote, "checkpoint") {
        Some(c) => c,
        None => {
            receipt_unverified(&item_s, attempt, "the executor's assignment carries no checkpoint cite — nothing to verify against");
            return;
        }
    };
    // The cite as the executor stamped it: a `$N:` universe
    // address when the checkpoint verb recorded it so, else
    // a bare coordinate in the attempt universe.
    let cp_addr = if cp_cite.starts_with('$') {
        cp_cite.clone()
    } else {
        format!("${}:{}", attempt + 1, cp_cite)
    };
    let cp_text = match wire::fetch_scroll(endpoint, &cp_addr) {
        Ok(t) => t,
        Err(e) => {
            receipt_unverified(&item_s, attempt, &format!("the final checkpoint {} cannot be read back from {}: {}", cp_cite, node, e));
            return;
        }
    };
    let read = |cite: &str| wire::fetch_scroll(endpoint, cite).ok();
    let n = match wire::check_checkpoint_outputs(&cp_text, &read) {
        Ok(n) if n >= 1 => n,
        Ok(_) => {
            receipt_unverified(&item_s, attempt, "the final checkpoint cites no outputs — it verifies nothing");
            return;
        }
        Err(e) => {
            receipt_unverified(&item_s, attempt, &e);
            return;
        }
    };
    let receipt_coord = wire::field_all(&cp_text, "output").into_iter().next().unwrap_or_else(|| "?".to_string());
    let receipt_text = wire::fetch_scroll(endpoint, &receipt_coord).unwrap_or_default();
    let stages = lattice::field(&receipt_text, "stages").unwrap_or_else(|| "?".to_string());
    let exits = lattice::field(&receipt_text, "exits").unwrap_or_else(|| "?".to_string());
    let checked = format!(
        "executor {}'s assignment record, read back over the scroll route in this pass — status: {}",
        node, status
    );
    let showed = format!(
        "final checkpoint {} read back from {}; {} cited output(s) re-read and hash-matched, among them the run complete receipt {} (stages: {}; exits: {})",
        cp_cite, node, n, receipt_coord, stages, exits
    );
    let verdict = format!(
        "complete — {} stage(s) ran, every exit 0, and the completion's cited outputs verify by read-back; closing the item (F4)",
        stages
    );
    close_item(item, checked, showed, verdict);
}

/// Close an item this node executed itself: the same
/// F4 shape, with every read against the local chain.
fn close_from_local(item: &Item, asg_text: &str, attempt: u32) {
    let item_s = fmt_coord(&item.addr);
    let lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    let self_name = wire::local_node(&lat.px);
    let status = lattice::field(asg_text, "status").unwrap_or_default();
    let cp_cite = match lattice::field(asg_text, "checkpoint") {
        Some(c) => c,
        None => {
            receipt_unverified(&item_s, attempt, "the executor's assignment carries no checkpoint cite — nothing to verify against");
            return;
        }
    };
    // The cite carries its universe (`$N:` as the
    // checkpoint verb stamped it); local_read resolves
    // the chain address as written.
    let cp_text = match local_read(&lat, &cp_cite) {
        Some(t) => t,
        None => {
            receipt_unverified(&item_s, attempt, &format!("the final checkpoint {} is not in this node's chain", cp_cite));
            return;
        }
    };
    let read = |cite: &str| local_read(&lat, cite);
    let n = match wire::check_checkpoint_outputs(&cp_text, &read) {
        Ok(n) if n >= 1 => n,
        Ok(_) => {
            receipt_unverified(&item_s, attempt, "the final checkpoint cites no outputs — it verifies nothing");
            return;
        }
        Err(e) => {
            receipt_unverified(&item_s, attempt, &e);
            return;
        }
    };
    let receipt_coord = wire::field_all(&cp_text, "output").into_iter().next().unwrap_or_else(|| "?".to_string());
    let receipt_text = local_read(&lat, &receipt_coord).unwrap_or_default();
    let stages = lattice::field(&receipt_text, "stages").unwrap_or_else(|| "?".to_string());
    let exits = lattice::field(&receipt_text, "exits").unwrap_or_else(|| "?".to_string());
    let checked = format!(
        "this node's own executor assignment record, read back from state in this pass — status: {}",
        status
    );
    let showed = format!(
        "final checkpoint {} read back from the attempt universe; {} cited output(s) re-read and hash-matched, among them the run complete receipt {} (stages: {}; exits: {})",
        cp_cite, n, receipt_coord, stages, exits
    );
    let verdict = format!(
        "complete — {} stage(s) ran on {}, every exit 0, and the completion's cited outputs verify by read-back; closing the item (F4)",
        stages, self_name
    );
    close_item(item, checked, showed, verdict);
}

/// Read a cited scroll from this node's own chain —
/// a bare coordinate is universe 1, a `$N:` cite its
/// universe (the checkpoint resolver's shape).
fn local_read(lat: &Lattice, cite: &str) -> Option<String> {
    let (u, coord) = phext::parse_universe_addr(cite)?;
    if u == 1 {
        lat.px.get(&coord).cloned()
    } else {
        lat.universe(u).and_then(|px| px.get(&coord).cloned())
    }
}

/// The close itself, through the oracle: a
/// dispatched item first takes the executor's
/// dispatched → running step (its beats stand on the
/// assignment record the closer read), then the
/// coordinator's running → done with the F4 evidence.
/// The origin assignment retires in the same save.
fn close_item(item: &Item, checked: String, showed: String, verdict: String) {
    let item_s = fmt_coord(&item.addr);
    let mut lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    let cur = match wire::held_item(&lat, &item.addr) {
        Some(i) => i,
        None => return,
    };
    let actor_name = dispatch::local_actor(&lat.px);
    if cur.state == Some(ItemState::Dispatched) {
        let mut t = Transition::new(ItemState::Running, Actor::Executor, &actor_name);
        t.evidence = "the executor's first heartbeat stands on its assignment record, read back by the closing pass".to_string();
        if let Err(e) = dispatch::apply_transition(&mut lat, cur.book, &t) {
            eprintln!("agent coordinator: {} — the dispatched → running step was refused by the oracle: {}", item_s, e);
            return;
        }
    } else if cur.state != Some(ItemState::Running) {
        return;
    }
    let mut t = Transition::new(ItemState::Done, Actor::Coordinator, &actor_name);
    t.evidence = "agent coordinator close — the executor reported completion; the F4 evidence is this pass's own read-back verification".to_string();
    t.checked = Some(checked);
    t.showed = Some(showed);
    t.verdict = Some(verdict);
    let receipt = match dispatch::apply_transition(&mut lat, cur.book, &t) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("agent coordinator: {} — the completion close was refused by the oracle: {}", item_s, e);
            return;
        }
    };
    if let Some((asg, _t, _a, _n)) = wire::my_origin_assignment(&lat, &item.addr) {
        wire::retire_assignment(
            &mut lat,
            &asg.addr,
            &format!(
                "closed — completion verified by the agent coordinator (F4); transition receipt {}",
                fmt_coord(&receipt)
            ),
        );
    }
    if let Err(e) = lat.save() {
        eprintln!("agent coordinator: cannot save the close of {}: {}", item_s, e);
        return;
    }
    println!("agent coordinator: {} closed done — transition receipt {}", item_s, fmt_coord(&receipt));
}

/// A completion the pass could not verify: receipt
/// the finding and leave the item exactly as it
/// stands — an unverified completion is never
/// consumed, and the next pass tries again.
fn receipt_unverified(item_s: &str, attempt: u32, reason: &str) {
    let mut lat = match core::require_born() {
        Ok(l) => l,
        Err(_) => return,
    };
    let r = core::append_receipt(
        &mut lat.px,
        "progress unverified",
        &[("item", item_s), ("attempt", &attempt.to_string()), ("reason", reason)],
    );
    if let Err(e) = lat.save() {
        eprintln!("agent coordinator: cannot save the unverified receipt for {}: {}", item_s, e);
        return;
    }
    println!(
        "agent coordinator: {} completion UNVERIFIED — {} (receipt {})",
        item_s,
        reason,
        fmt_coord(&r)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_notes_stay_one_line() {
        assert_eq!(one_line("echo  hello\n  world"), "echo hello world");
        assert_eq!(one_line("single"), "single");
    }

    #[test]
    fn work_dirs_are_per_item_and_attempt() {
        let a = work_dir(4, 1);
        let b = work_dir(4, 2);
        let c = work_dir(5, 1);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert!(a.ends_with("item-4-a1"), "{}", a.display());
    }
}
