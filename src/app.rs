//! The dispatch app surface (dispatch core, slice 7,
//! spec §2.3): the §2.1 dispatch verbs as a line protocol
//! for front-ends — the Android launcher v3's road in.
//!
//! The CLI is Will's first dispatch surface and the phone
//! is his main one; §2.3 binds every dispatch verb to an
//! `--app` counterpart speaking the shipped `learn --app`
//! line protocol (src/learn.rs): one command per line on
//! stdin, one `key: value` block per command on stdout,
//! every block closed by `end: <command>`, a greeting
//! block at start. NOT JSON — qexo's own front-ends are
//! not a foreign boundary.
//!
//! Any of the eight spellings — `enqueue --app`,
//! `queue --app`, `dispatch --app`, `show --app`,
//! `pause --app`, `resume --app`, `audit --app`,
//! `drain --app` — opens the same session, whose command
//! vocabulary is the dispatch verb set itself. The
//! spelling names the surface, the way `learn --app`
//! names the learn surface; no spelling is privileged.
//! `cancel` and `bridge` have no `--app` form: §2.3's
//! enumeration omits them, and cancel — terminal,
//! Will/Emise only, note required — stays on the CLI.
//!
//!   enqueue <brief-file> [flags]   the CLI's flags, verbatim
//!   enqueue [flags]                paste form: the block
//!                                  `paste: …` answers, then
//!                                  brief lines are content
//!                                  (never commands) until a
//!                                  line containing only `.`;
//!                                  a line `..` unstuffs to `.`
//!   queue [--all] [--node <name>]  the calculated order —
//!                                  each `item:` line is the
//!                                  CLI queue's own rendering
//!   queue --explain <address>      position + the record
//!   dispatch [<address>] [--to <node>] [--check]
//!   show <address>                 one record's fields
//!   pause <address> [--note <text>]
//!   resume <address>
//!   audit [--item <a>] [--kind <k>] [--since <date>]
//!   audit show <address>
//!   drain [--check] [--url <url>]
//!   quit                           session summary, then exit
//!
//! Argument shape: tokens split on whitespace, except the
//! text flags `--title`, `--goal`, `--exit`, `--note`,
//! whose value is the tokens that follow, up to the next
//! `--` flag — a front-end's title and note fields carry
//! spaces, flags still parse after them, and the protocol
//! grows no quoting language.
//!
//! Every command is a second driver over the same state
//! machine, never a second implementation: enqueue ends
//! in dispatch::enqueue_core, pause/resume construct the
//! CLI verbs' own Transitions and pass through the §4.6
//! oracle (dispatch::apply_transition), queue renders
//! dispatch::order_items through the CLI's own line
//! renderer, dispatch and drain run the wire/register
//! engines as captured RunLines and render their facts.
//! The session holds one lattice; commands whose engines
//! load their own (dispatch, drain) are followed by a
//! reload, so the session's view is never staler than the
//! last command. Command refusals are `error:` lines in
//! the command's block — the session itself ends only on
//! `quit`, EOF, or a state reload failure.

use crate::core::require_born;
use crate::dispatch::{self, Actor, ItemState, Transition};
use crate::lattice::{self, Lattice};
use crate::phext;
use std::io::Write;

/// The text flags whose value is the rest of the line.
const REST_OF_LINE: [&str; 4] = ["--title", "--goal", "--exit", "--note"];

/// Split a command's tail into argv: whitespace tokens,
/// except that a text flag (REST_OF_LINE) consumes the
/// tokens that follow it, up to the next `--` flag, as
/// its single value — a front-end's title and note
/// fields carry spaces, flags still parse after them,
/// and the protocol grows no quoting language.
fn app_args(tail: &str) -> Vec<String> {
    let words: Vec<&str> = tail.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        out.push(w.to_string());
        i += 1;
        if REST_OF_LINE.contains(&w) {
            let start = i;
            while i < words.len() && !words[i].starts_with("--") {
                i += 1;
            }
            if i > start {
                out.push(words[start..i].join(" "));
            }
        }
    }
    out
}

/// A brief being pasted: the enqueue arguments that
/// opened it, and the content lines so far.
struct Paste {
    args: Vec<String>,
    lines: Vec<String>,
}

struct AppSession {
    served: usize,
    paste: Option<Paste>,
}

/// The greeting block: who this node is, and the queue
/// as it stands — counts calculated from the records at
/// session start, never a stored summary.
fn greeting_lines(lat: &Lattice) -> Vec<String> {
    let node = lat
        .px
        .get(&lattice::IDENTITY)
        .and_then(|t| lattice::field(t, "node"))
        .unwrap_or_else(|| "unknown-node".to_string());
    let items = dispatch::load_items(&lat.px);
    let mut lines = vec![
        format!("node: {}", node),
        format!("version: {}", crate::plan::PLAN_VERSION),
        format!("state: {}", lattice::state_path().display()),
        format!("items: {}", items.len()),
    ];
    for st in dispatch::ALL_STATES {
        let n = items.iter().filter(|i| i.state == Some(st)).count();
        if n > 0 {
            lines.push(format!("{}: {}", st.name(), n));
        }
    }
    lines
}

/// Render one scroll as a protocol block: the address,
/// the scroll's first line as its kind, then its field
/// lines verbatim (a record's lines already ARE
/// `key: value` lines).
fn scroll_lines(addr_s: &str, body: &str) -> Vec<String> {
    let mut lines = vec![format!("address: {}", addr_s)];
    let mut it = body.lines();
    if let Some(first) = it.next() {
        lines.push(format!("scroll: {}", first));
        for l in it {
            lines.push(l.to_string());
        }
    }
    lines
}

impl AppSession {
    /// File the paste's brief through enqueue_core — the
    /// same birth a CLI enqueue gives an item (§4.1: the
    /// address exists only once the record is durable).
    fn finish_paste(&mut self, lat: &mut Lattice, paste: Paste) -> Vec<String> {
        let brief = if paste.lines.is_empty() {
            String::new()
        } else {
            let mut b = paste.lines.join("\n");
            b.push('\n');
            b
        };
        let parsed = match dispatch::parse_enqueue_args(&paste.args) {
            Ok(p) => p,
            Err(e) => return vec![format!("error: {}", e)],
        };
        match dispatch::enqueue_core(lat, &parsed, &brief) {
            Ok(addr) => {
                let mut lines = vec![format!("address: {}", addr)];
                if let Ok(book) = dispatch::item_book(&addr) {
                    if let Some(item) = dispatch::load_items(&lat.px).into_iter().find(|i| i.book == book) {
                        lines.push(format!("title: {}", item.title));
                    }
                }
                lines.push("state: queued".to_string());
                lines
            }
            Err((msg, _code)) => vec![format!("error: {}", msg)],
        }
    }

    fn enqueue(&mut self, lat: &mut Lattice, args: &[String]) -> Vec<String> {
        let parsed = match dispatch::parse_enqueue_args(args) {
            Ok(p) => p,
            Err(e) => return vec![format!("error: {}", e)],
        };
        let source = match &parsed.source {
            None => {
                // Paste form: the prompt block answers now;
                // the brief itself is content until the `.`
                // line, and the result block follows it.
                self.paste = Some(Paste { args: args.to_vec(), lines: Vec::new() });
                return vec![
                    "paste: brief lines follow — a line containing only '.' ends the brief".to_string(),
                ];
            }
            Some(s) => s.clone(),
        };
        if source == "-" {
            return vec![
                "error: enqueue - reads stdin, and stdin is the protocol channel in an app session — paste the brief (bare enqueue) or name a file".to_string(),
            ];
        }
        let brief = match std::fs::read_to_string(&source) {
            Ok(t) => t,
            Err(e) => return vec![format!("error: {}: {}", source, e)],
        };
        match dispatch::enqueue_core(lat, &parsed, &brief) {
            Ok(addr) => {
                let mut lines = vec![format!("address: {}", addr)];
                if let Ok(book) = dispatch::item_book(&addr) {
                    if let Some(item) = dispatch::load_items(&lat.px).into_iter().find(|i| i.book == book) {
                        lines.push(format!("title: {}", item.title));
                    }
                }
                lines.push("state: queued".to_string());
                lines
            }
            Err((msg, _code)) => vec![format!("error: {}", msg)],
        }
    }

    fn queue(&mut self, lat: &Lattice, args: &[String]) -> Vec<String> {
        let mut all = false;
        let mut node: Option<String> = None;
        let mut explain: Option<String> = None;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--all" => {
                    all = true;
                    i += 1;
                }
                "--node" if i + 1 < args.len() => {
                    node = Some(args[i + 1].clone());
                    i += 2;
                }
                "--explain" if i + 1 < args.len() => {
                    explain = Some(args[i + 1].clone());
                    i += 2;
                }
                other => return vec![format!("error: queue: unexpected argument '{}'", other)],
            }
        }
        if let Some(addr_s) = explain {
            return self.explain(lat, &addr_s);
        }
        let daily = dispatch::daily_goal(&lat.px);
        let mut items = dispatch::load_items(&lat.px);
        if let Some(want) = &node {
            items.retain(|i| i.enqueued_by.split_whitespace().next().unwrap_or("") == want.as_str());
        }
        if !all {
            items.retain(|i| !matches!(i.state, Some(ItemState::Done) | Some(ItemState::Cancelled)));
        }
        let ordered = dispatch::order_items(items, daily.as_deref());
        let mut lines = vec![format!("count: {}", ordered.len())];
        for it in &ordered {
            lines.push(format!("item: {}", dispatch::render_queue_line(it, daily.as_deref())));
        }
        lines
    }

    fn explain(&mut self, lat: &Lattice, addr_s: &str) -> Vec<String> {
        let (universe, coord) = match phext::parse_universe_addr(addr_s) {
            Some(x) => x,
            None => return vec![format!("error: '{}' is not a full 9D address", addr_s)],
        };
        if universe != 1 {
            return vec![format!(
                "error: queue items live in universe 1 — ${} names an attempt universe",
                universe
            )];
        }
        let items = dispatch::load_items(&lat.px);
        let item = match items.iter().find(|i| i.addr == coord) {
            Some(i) => i.clone(),
            None => return vec![format!("error: no queue item at {}", addr_s)],
        };
        let daily = dispatch::daily_goal(&lat.px);
        let working: Vec<dispatch::Item> = items
            .into_iter()
            .filter(|i| !matches!(i.state, Some(ItemState::Done) | Some(ItemState::Cancelled)))
            .collect();
        let ordered = dispatch::order_items(working, daily.as_deref());
        let mut lines = Vec::new();
        match ordered.iter().position(|i| i.addr == coord) {
            Some(p) => lines.push(format!("position: {} of {}", p + 1, ordered.len())),
            None => lines.push("position: terminal — outside the working order".to_string()),
        }
        match lat.px.get(&coord) {
            Some(body) => lines.extend(scroll_lines(addr_s, body)),
            None => lines.push(format!("error: no scroll at {}", addr_s)),
        }
        lines
    }

    fn show(&mut self, lat: &Lattice, args: &[String]) -> Vec<String> {
        let addr_s = match args.first() {
            Some(a) => a.clone(),
            None => return vec!["error: show needs an address".to_string()],
        };
        let (universe, coord) = match phext::parse_universe_addr(&addr_s) {
            Some(x) => x,
            None => {
                return vec![format!(
                    "error: '{}' is not a full 9D address (a $N: prefix names the universe)",
                    addr_s
                )]
            }
        };
        let doc = match lat.universe(universe) {
            Some(d) => d,
            None => {
                return vec![format!(
                    "error: this node's state chain has {} universe(s) — no universe ${}",
                    lat.universe_count(),
                    universe
                )]
            }
        };
        match doc.get(&coord) {
            Some(body) => scroll_lines(&addr_s, body),
            None => vec![format!("error: no scroll at {}", addr_s)],
        }
    }

    /// pause and resume: the CLI verbs' own Transitions,
    /// through the one oracle (§4.6) — a session pause is
    /// a manual pause, a session resume the manual kind
    /// paused-auto yields to (§8), identically to the CLI.
    fn transition(&mut self, lat: &mut Lattice, verb: &str, args: &[String]) -> Vec<String> {
        let (addr_s, note) = match dispatch::parse_item_verb(verb, args, verb == "pause") {
            Ok(x) => x,
            Err(e) => return vec![format!("error: {}", e)],
        };
        let book = match dispatch::item_book(&addr_s) {
            Ok(b) => b,
            Err(e) => return vec![format!("error: {}", e)],
        };
        let t = match verb {
            "pause" => {
                let mut t = Transition::new(ItemState::Paused, Actor::Manual, &dispatch::local_actor(&lat.px));
                t.evidence = note.unwrap_or_default();
                t.manual = true;
                t
            }
            _ => {
                let mut t = Transition::new(ItemState::Queued, Actor::Manual, &dispatch::local_actor(&lat.px));
                t.manual = true;
                t
            }
        };
        match dispatch::apply_transition(lat, book, &t) {
            Ok(receipt) => {
                if let Err(e) = lat.save() {
                    return vec![format!("error: cannot save state: {}", e)];
                }
                let state = dispatch::load_items(&lat.px)
                    .into_iter()
                    .find(|i| i.book == book)
                    .map(|i| i.state_raw)
                    .unwrap_or_default();
                vec![
                    format!("item: {}", addr_s),
                    format!("state: {}", state),
                    format!("receipt: {}", phext::fmt_coord(&receipt)),
                ]
            }
            Err(e) => vec![format!("error: {}", e)],
        }
    }

    fn audit(&mut self, lat: &Lattice, args: &[String]) -> Vec<String> {
        if args.first().map(|s| s.as_str()) == Some("show") {
            let addr_s = match args.get(1) {
                Some(a) => a.clone(),
                None => return vec!["error: audit show needs an address".to_string()],
            };
            let coord = match phext::parse_coord(&addr_s) {
                Some(c) => c,
                None => return vec![format!("error: '{}' is not a full 9D address", addr_s)],
            };
            let audits = dispatch::load_audits(&lat.px);
            return match audits.iter().find(|a| a.addr == coord) {
                Some(_) => {
                    let body = lat.px.get(&coord).cloned().unwrap_or_default();
                    scroll_lines(&addr_s, &body)
                }
                None => vec![format!("error: no audit record at {}", addr_s)],
            };
        }
        let mut item_f: Option<String> = None;
        let mut kind_f: Option<String> = None;
        let mut since_f: Option<String> = None;
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            let slot = match a.as_str() {
                "--item" => &mut item_f,
                "--kind" => &mut kind_f,
                "--since" => &mut since_f,
                _ => return vec![format!("error: unknown flag {}", a)],
            };
            if i + 1 >= args.len() {
                return vec![format!("error: {} needs a value", a)];
            }
            *slot = Some(args[i + 1].clone());
            i += 2;
        }
        let audits = dispatch::load_audits(&lat.px);
        let shown: Vec<&dispatch::AuditRec> = audits
            .iter()
            .filter(|a| item_f.as_deref().map(|w| a.item.as_deref() == Some(w)).unwrap_or(true))
            .filter(|a| kind_f.as_deref().map(|w| a.kind == w).unwrap_or(true))
            .filter(|a| since_f.as_deref().map(|w| a.at.get(..10).unwrap_or("") >= w).unwrap_or(true))
            .collect();
        let mut lines = vec![format!("count: {}", shown.len())];
        for a in shown {
            lines.push(format!("audit: {}", dispatch::render_audit_line(a)));
        }
        lines
    }

    /// A captured run (dispatch, drain) rendered as a
    /// block: its facts in order, its refusals as `error:`
    /// lines. The engines loaded their own lattice and
    /// saved; the caller reloads the session's.
    fn run_block(run: &dispatch::RunLines) -> Vec<String> {
        let mut lines: Vec<String> = run
            .facts
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        for e in &run.err {
            lines.push(format!("error: {}", e));
        }
        lines
    }

    fn reload(lat: &mut Lattice) -> Result<(), Vec<String>> {
        match Lattice::load() {
            Ok(Some(l)) => {
                *lat = l;
                Ok(())
            }
            _ => Err(vec![
                "error: cannot reload state after the command — the session ends here".to_string(),
            ]),
        }
    }

    /// One input line → (block, session over?). A paste
    /// content line answers with no block (None).
    fn handle(&mut self, lat: &mut Lattice, line: &str) -> (Option<Vec<String>>, bool) {
        if let Some(mut paste) = self.paste.take() {
            if line == "." {
                let lines = self.finish_paste(lat, paste);
                return (Some(lines), false);
            }
            paste.lines.push(if line == ".." { ".".to_string() } else { line.to_string() });
            self.paste = Some(paste);
            return (None, false);
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("").to_string();
        let tail = parts.next().unwrap_or("").trim_start();
        let args = app_args(tail);
        self.served += 1;
        match cmd.as_str() {
            "enqueue" => (Some(self.enqueue(lat, &args)), false),
            "queue" => (Some(self.queue(lat, &args)), false),
            "show" => (Some(self.show(lat, &args)), false),
            "pause" => (Some(self.transition(lat, "pause", &args)), false),
            "resume" => (Some(self.transition(lat, "resume", &args)), false),
            "audit" => (Some(self.audit(lat, &args)), false),
            "dispatch" => {
                let mut item: Option<String> = None;
                let mut to: Option<String> = None;
                let mut check = false;
                let mut i = 0;
                let mut bad: Option<String> = None;
                while i < args.len() {
                    match args[i].as_str() {
                        "--to" if i + 1 < args.len() => {
                            to = Some(args[i + 1].clone());
                            i += 2;
                        }
                        "--check" => {
                            check = true;
                            i += 1;
                        }
                        other if !other.starts_with("--") && item.is_none() => {
                            item = Some(other.to_string());
                            i += 1;
                        }
                        other => {
                            bad = Some(format!("error: dispatch: unexpected argument '{}'", other));
                            break;
                        }
                    }
                }
                if let Some(b) = bad {
                    return (Some(vec![b]), false);
                }
                let (_code, run) = crate::wire::dispatch_app_run(item.as_deref(), to.as_deref(), check);
                let lines = Self::run_block(&run);
                match Self::reload(lat) {
                    Ok(()) => (Some(lines), false),
                    Err(e) => (Some(e), true),
                }
            }
            "drain" => {
                let mut check = false;
                let mut url: Option<String> = None;
                let mut i = 0;
                let mut bad: Option<String> = None;
                while i < args.len() {
                    match args[i].as_str() {
                        "--check" => {
                            check = true;
                            i += 1;
                        }
                        "--url" if i + 1 < args.len() => {
                            url = Some(args[i + 1].clone());
                            i += 2;
                        }
                        other => {
                            bad = Some(format!("error: drain: unexpected argument '{}'", other));
                            break;
                        }
                    }
                }
                if let Some(b) = bad {
                    return (Some(vec![b]), false);
                }
                let (_code, run) = crate::register::drain_run(check, url);
                let lines = Self::run_block(&run);
                match Self::reload(lat) {
                    Ok(()) => (Some(lines), false),
                    Err(e) => (Some(e), true),
                }
            }
            "quit" => (
                Some(vec![
                    format!("served: {}", self.served),
                    format!("items: {}", dispatch::load_items(&lat.px).len()),
                ]),
                true,
            ),
            "" => (None, false),
            other => (Some(vec![format!("error: unknown command '{}'", other)]), false),
        }
    }
}

fn read_line(stdin: &std::io::Stdin) -> Option<String> {
    let mut s = String::new();
    match stdin.read_line(&mut s) {
        Ok(0) => None, // EOF
        Ok(_) => Some(s.trim_end_matches(['\n', '\r']).to_string()),
        Err(_) => None,
    }
}

/// The dispatch app session (§2.3): greeting block, then
/// one command per line until `quit` or EOF. Entered from
/// any dispatch verb's `--app` form. Exit codes match
/// learn --app's shape: 0 clean, plus require_born's codes
/// when the node is unborn; command refusals are in-band.
pub fn app_session() -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut session = AppSession { served: 0, paste: None };
    let emit = |lines: &[String], end: &str| {
        let mut w = stdout.lock();
        for l in lines {
            let _ = writeln!(w, "{}", l);
        }
        let _ = writeln!(w, "end: {}", end);
        let _ = w.flush();
    };
    emit(&greeting_lines(&lat), "hello");
    loop {
        let line = match read_line(&stdin) {
            Some(l) => l,
            // EOF: the front-end hung up. Every act already
            // saved as it landed, exactly as the CLI's do.
            None => break,
        };
        if session.paste.is_none() && line.trim().is_empty() {
            continue;
        }
        let cmd = if session.paste.is_some() {
            "enqueue".to_string()
        } else {
            line.split_whitespace().next().unwrap_or("").to_string()
        };
        let (block, done) = session.handle(&mut lat, &line);
        if let Some(lines) = block {
            emit(&lines, &cmd);
        }
        if done {
            break;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn born_lat() -> (Lattice, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("qexo-app-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (Lattice::born(dir.join("state.phext")), dir)
    }

    #[test]
    fn text_flags_take_words_up_to_the_next_flag() {
        let a = app_args("brief.txt --title Fix the thing --value 3");
        assert_eq!(a, vec!["brief.txt", "--title", "Fix the thing", "--value", "3"]);
        let b = app_args("--value 3 --title Fix the thing");
        assert_eq!(b, vec!["--value", "3", "--title", "Fix the thing"]);
        let c = app_args("--check");
        assert_eq!(c, vec!["--check"]);
        let d = app_args("11.11.11/2.1.1/1.1.1 --note hold on");
        assert_eq!(d, vec!["11.11.11/2.1.1/1.1.1", "--note", "hold on"]);
    }

    #[test]
    fn unknown_command_is_an_in_band_error() {
        let (mut lat, _dir) = born_lat();
        let mut s = AppSession { served: 0, paste: None };
        let (block, done) = s.handle(&mut lat, "frobnicate now");
        assert!(!done);
        assert_eq!(block.unwrap(), vec!["error: unknown command 'frobnicate'".to_string()]);
    }

    #[test]
    fn paste_files_an_item_through_enqueue_core() {
        let (mut lat, _dir) = born_lat();
        let mut s = AppSession { served: 0, paste: None };
        let (block, _) = s.handle(&mut lat, "enqueue --title Pasted work");
        let block = block.unwrap();
        assert!(block[0].starts_with("paste:"), "{:?}", block);
        // Content lines answer with no block; '..' unstuffs.
        let (none, _) = s.handle(&mut lat, "Pasted work");
        assert!(none.is_none());
        let (none, _) = s.handle(&mut lat, "..");
        assert!(none.is_none());
        let (block, _) = s.handle(&mut lat, ".");
        let block = block.unwrap();
        assert!(block.iter().any(|l| l.starts_with("address: 11.11.11/2.1.1/1.1.1")), "{:?}", block);
        assert!(block.iter().any(|l| l == "state: queued"), "{:?}", block);
        let items = dispatch::load_items(&lat.px);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Pasted work");
        let brief = lat.px.get(&dispatch::brief_addr(1)).cloned().unwrap_or_default();
        assert_eq!(brief, "Pasted work\n.\n");
    }

    #[test]
    fn queue_pause_resume_share_the_cli_state_machine() {
        let (mut lat, _dir) = born_lat();
        let mut s = AppSession { served: 0, paste: None };
        let (block, _) = s.handle(&mut lat, "enqueue --title Session item");
        assert!(block.is_some());
        let (block, _) = s.handle(&mut lat, ".");
        let addr = block
            .unwrap()
            .iter()
            .find_map(|l| l.strip_prefix("address: ").map(|a| a.to_string()))
            .unwrap();
        let (block, _) = s.handle(&mut lat, "queue");
        let block = block.unwrap();
        assert_eq!(block[0], "count: 1");
        assert!(block[1].contains(&addr), "{:?}", block);
        let (block, _) = s.handle(&mut lat, &format!("pause {} --note hold on", addr));
        let block = block.unwrap();
        assert!(block.iter().any(|l| l == "state: paused"), "{:?}", block);
        assert!(block.iter().any(|l| l.starts_with("receipt: ")), "{:?}", block);
        // A second pause is the oracle's refusal, in band.
        let (block, _) = s.handle(&mut lat, &format!("pause {}", addr));
        let block = block.unwrap();
        assert!(block[0].starts_with("error: "), "{:?}", block);
        let (block, _) = s.handle(&mut lat, &format!("resume {}", addr));
        let block = block.unwrap();
        assert!(block.iter().any(|l| l == "state: queued"), "{:?}", block);
        let (block, _) = s.handle(&mut lat, &format!("show {}", addr));
        let block = block.unwrap();
        assert!(block.iter().any(|l| l == "scroll: queue item"), "{:?}", block);
        assert!(block.iter().any(|l| l == "state: queued"), "{:?}", block);
    }

    #[test]
    fn show_of_a_missing_scroll_is_an_in_band_error() {
        let (mut lat, _dir) = born_lat();
        let mut s = AppSession { served: 0, paste: None };
        let (block, _) = s.handle(&mut lat, "show 11.11.11/2.1.9/1.1.1");
        let block = block.unwrap();
        assert!(block[0].starts_with("error: no scroll at"), "{:?}", block);
    }
}
