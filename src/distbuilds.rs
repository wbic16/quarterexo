//! distributed-builds: the upgrade command that brings a node
//! from bare to a configured member of the fleet's distributed
//! build cluster — sccache in front (the fleet-shared compiler
//! cache on the orchid-flow NFS share), icecream behind it
//! (C/C++ compiles spread across the fleet's iceccd daemons).
//!
//! The recipes are NOT here. They live in the exocortical source
//! tree, the single source of truth for how a node is built
//! (wbic16/exocortical): sccache-setup.sh, icecream-node.sh, and
//! — on the one scheduler host — icecream-scheduler.sh, wired
//! into setup.sh as Phase 12 and into build shells by
//! enable-distributed-builds.sh. This command ensures that tree
//! is present and current, runs the scripts (they are
//! idempotent), and verifies the result the same way --status
//! reports it. It never touches the node's qexo state: the
//! cluster's state is the daemons and the cache, not the lattice.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command as Proc;
use std::time::Duration;

const TREE: &str = "/source/exocortical";
const REPO: &str = "git@github.com:wbic16/exocortical.git";
const SCHEDULER_HOST: &str = "aletheia-core";
const SHARED_CACHE: &str = "/mnt/mirrorborn/shared/sccache";

fn hostname() -> String {
    if let Ok(out) = Proc::new("hostname").output() {
        let h = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !h.is_empty() {
            return h;
        }
    }
    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
}

/// Run a command with inherited stdio; true on exit success.
fn run(program: &str, args: &[&str], dir: Option<&Path>) -> bool {
    let mut c = Proc::new(program);
    c.args(args);
    if let Some(d) = dir {
        c.current_dir(d);
    }
    match c.status() {
        Ok(s) => s.success(),
        Err(e) => {
            eprintln!("distributed-builds: cannot run {}: {}", program, e);
            false
        }
    }
}

/// Captured stdout of a command, trimmed; None on any failure.
fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Proc::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn port_open(host: &str, port: u16) -> bool {
    let addr = match (host, port).to_socket_addrs() {
        Ok(mut a) => match a.next() {
            Some(a) => a,
            None => return false,
        },
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok()
}

fn service_active(unit: &str) -> bool {
    output("systemctl", &["is-active", unit]).map(|s| s == "active").unwrap_or(false)
}

/// Ensure the exocortical tree exists and is current. With a
/// branch named, check it out (the rollout rides the feature
/// branch until it merges to exo); otherwise pull the current
/// branch. Errors are the caller's to report.
fn ensure_tree(branch: Option<&str>) -> Result<(), String> {
    let tree = Path::new(TREE);
    if !tree.join(".git").exists() {
        println!("distributed-builds: cloning the exocortical tree into {}", TREE);
        if !Path::new("/source").exists() {
            let user = std::env::var("USER").unwrap_or_else(|_| "wbic16".to_string());
            run("sudo", &["mkdir", "-p", "/source"], None);
            run("sudo", &["chown", &format!("{}:{}", user, user), "/source"], None);
        }
        if !run("git", &["clone", REPO, TREE], None) {
            return Err(format!("git clone of {} failed", REPO));
        }
    }
    let t = Some(tree);
    if !run("git", &["fetch", "origin"], t) {
        return Err("git fetch origin failed in the exocortical tree".to_string());
    }
    if let Some(b) = branch {
        if !run("git", &["checkout", b], t) {
            return Err(format!("git checkout {} failed in the exocortical tree", b));
        }
    }
    if !run("git", &["pull", "--ff-only"], t) {
        return Err("git pull --ff-only failed in the exocortical tree".to_string());
    }
    Ok(())
}

fn run_script(name: &str, args: &[&str]) -> Result<(), String> {
    let path = Path::new(TREE).join(name);
    if !path.exists() {
        return Err(format!(
            "{} is missing from {} — the tree predates distributed builds; update it (or pass --branch)",
            name, TREE
        ));
    }
    println!("distributed-builds: running {}", name);
    let mut full: Vec<&str> = vec![path.to_str().unwrap()];
    full.extend_from_slice(args);
    if !run("bash", &full, Some(Path::new(TREE))) {
        return Err(format!("{} failed", name));
    }
    Ok(())
}

/// The verification half, shared by the upgrade run and
/// --status: every line is a fact probed now, nothing remembered.
pub fn report() -> i32 {
    let host = hostname();
    let on_scheduler = host == SCHEDULER_HOST;
    println!("distributed builds on {}:", host);
    let mut ok = true;

    // sccache: binary, then the shared cache the fleet builds against.
    match output("sccache", &["--version"]) {
        Some(v) => {
            let mounted = Proc::new("mountpoint")
                .args(["-q", "/mnt/mirrorborn/shared"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            let shared = mounted && Path::new(SHARED_CACHE).exists();
            if shared {
                println!("  sccache: {} — fleet-shared cache at {}", v, SHARED_CACHE);
            } else {
                println!("  sccache: {} — shared cache not mounted; local cache only", v);
            }
        }
        None => {
            println!("  sccache: MISSING");
            ok = false;
        }
    }

    // iceccd: the member daemon every build node runs.
    if service_active("iceccd") && port_open("127.0.0.1", 10245) {
        println!("  iceccd: active — daemon port 10245 listening");
    } else {
        println!("  iceccd: NOT RUNNING");
        ok = false;
    }

    // The scheduler: a service on its one host, a reachable port elsewhere.
    if on_scheduler {
        if service_active("icecc-scheduler") && port_open("127.0.0.1", 8765) {
            println!("  scheduler: active on this host — port 8765 listening");
        } else {
            println!("  scheduler: NOT RUNNING on this host");
            ok = false;
        }
    } else if port_open(SCHEDULER_HOST, 8765) {
        println!("  scheduler: {}:8765 reachable", SCHEDULER_HOST);
    } else {
        println!("  scheduler: {}:8765 UNREACHABLE", SCHEDULER_HOST);
        ok = false;
    }

    if Path::new(TREE).join("enable-distributed-builds.sh").exists() {
        println!("  tree: {} present (source it to build distributed)", TREE);
    } else {
        println!("  tree: {} missing or predates distributed builds", TREE);
    }

    if ok {
        0
    } else {
        1
    }
}

pub fn distributed_builds(scheduler: bool, status: bool, branch: Option<String>) -> i32 {
    if status {
        return report();
    }
    let host = hostname();
    let run_scheduler = scheduler || host == SCHEDULER_HOST;
    if let Err(e) = ensure_tree(branch.as_deref()) {
        eprintln!("distributed-builds: {}", e);
        return 1;
    }
    let steps: Vec<(&str, Vec<&str>)> = if run_scheduler {
        vec![
            ("sccache-setup.sh", vec![]),
            ("icecream-scheduler.sh", vec![]),
            ("icecream-node.sh", vec![SCHEDULER_HOST]),
        ]
    } else {
        vec![
            ("sccache-setup.sh", vec![]),
            ("icecream-node.sh", vec![SCHEDULER_HOST]),
        ]
    };
    for (name, args) in &steps {
        if let Err(e) = run_script(name, args) {
            eprintln!("distributed-builds: {}", e);
            return 1;
        }
    }
    println!();
    report()
}
