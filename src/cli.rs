//! CLI parsing: words → a Command. This module only PARSES (and
//! owns the help text); dispatch lives in main.rs.
//!
//! The command set is the scratch line's surface (DESIGN-SCRATCH
//! §3). Commands that need the state lattice's addressing parse
//! fully here and are dispatched by main to the core when the
//! Layer 2 map lands; in this build they report the hold honestly
//! (see main.rs). Commands that never needed the map — help,
//! wiring-check, the phext file ops, version — run today.

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Help,
    Version,
    WiringCheck(Option<String>),
    /// parse / explode / implode / seal / unseal / select / get,
    /// with their raw argument tails.
    FileOp(Vec<String>),
    Config { auto_update: bool, evolve: bool, release_signer: Option<String> },
    Doctor { fix: bool },
    List { json: bool },
    Learn { status: bool, app: bool },
    Inventory { name: Option<String>, install: Option<String> },
    DistributedBuilds { scheduler: bool, status: bool, branch: Option<String> },
    Register(Vec<String>),
    Release,
    Update { check: bool },
    Evolve { check: bool },
    Enqueue { args: Vec<String>, app: bool },
    Queue { all: bool, node: Option<String>, explain: Option<String>, app: bool },
    Pause { args: Vec<String>, app: bool },
    Resume { args: Vec<String>, app: bool },
    Cancel(Vec<String>),
    Audit { args: Vec<String>, app: bool },
    Bridge { args: Vec<String>, write: bool },
    Drain { check: bool, url: Option<String>, app: bool },
    Dispatch { item: Option<String>, to: Option<String>, check: bool, app: bool },
    Heartbeat(String),
    Checkpoint(Vec<String>),
    Fail(Vec<String>),
    Show { addr: String, app: bool },
    Host { persona: Option<String>, boot: Option<String> },
    Unhost(String),
    Message { persona: String, text: String },
    Status,
    Plan,
    Serve(Option<u16>),
    Bare,
    Unknown(String),
}

pub fn parse(args: &[String]) -> Command {
    let w = |i: usize| args.get(i).map(|s| s.as_str()).unwrap_or("");
    // `--app` (slice 7, spec §2.3): the verb's app-surface
    // form. It is stripped from the verb's own tail — the
    // session it opens speaks for the whole dispatch set.
    let strip_app = |tail: &[String]| -> (Vec<String>, bool) {
        let app = tail.iter().any(|a| a == "--app");
        (tail.iter().filter(|a| *a != "--app").cloned().collect(), app)
    };
    match w(0) {
        "" => Command::Bare,
        "--help" | "-h" | "help" => Command::Help,
        "--version" | "version" => Command::Version,
        "wiring-check" => Command::WiringCheck(args.get(1).cloned()),
        "parse" | "explode" | "implode" | "seal" | "unseal" | "select" | "get" => {
            Command::FileOp(args.to_vec())
        }
        "config" => {
            let mut release_signer: Option<String> = None;
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--release-signer" && i + 1 < args.len() {
                    release_signer = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Command::Config {
                auto_update: args.iter().any(|a| a == "--auto-update"),
                evolve: args.iter().any(|a| a == "--evolve"),
                release_signer,
            }
        }
        "doctor" => Command::Doctor { fix: args.iter().any(|a| a == "--fix") },
        "list" => Command::List { json: args.iter().any(|a| a == "--json") },
        "learn" => Command::Learn {
            status: args.iter().any(|a| a == "--status"),
            app: args.iter().any(|a| a == "--app"),
        },
        "inventory" | "catalog" => {
            let mut name: Option<String> = None;
            let mut install: Option<String> = None;
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--install" && i + 1 < args.len() {
                    install = Some(args[i + 1].clone());
                    i += 2;
                } else if !args[i].starts_with("--") && name.is_none() {
                    name = Some(args[i].clone());
                    i += 1;
                } else {
                    i += 1;
                }
            }
            Command::Inventory { name, install }
        }
        "distributed-builds" => {
            let mut branch: Option<String> = None;
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--branch" && i + 1 < args.len() {
                    branch = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Command::DistributedBuilds {
                scheduler: args.iter().any(|a| a == "--scheduler"),
                status: args.iter().any(|a| a == "--status"),
                branch,
            }
        }
        "register" => Command::Register(args[1..].to_vec()),
        "enqueue" => {
            let (tail, app) = strip_app(&args[1..]);
            Command::Enqueue { args: tail, app }
        }
        "queue" => {
            let mut node: Option<String> = None;
            let mut explain: Option<String> = None;
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--node" && i + 1 < args.len() {
                    node = Some(args[i + 1].clone());
                    i += 2;
                } else if args[i] == "--explain" && i + 1 < args.len() {
                    explain = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Command::Queue {
                all: args.iter().any(|a| a == "--all"),
                node,
                explain,
                app: args.iter().any(|a| a == "--app"),
            }
        }
        "pause" => {
            let (tail, app) = strip_app(&args[1..]);
            Command::Pause { args: tail, app }
        }
        "resume" => {
            let (tail, app) = strip_app(&args[1..]);
            Command::Resume { args: tail, app }
        }
        "cancel" => Command::Cancel(args[1..].to_vec()),
        "audit" => {
            let (tail, app) = strip_app(&args[1..]);
            Command::Audit { args: tail, app }
        }
        "bridge" => Command::Bridge {
            write: args.iter().any(|a| a == "--add" || a == "--check" || a == "--retire"),
            args: args[1..].to_vec(),
        },
        "drain" => {
            let mut url: Option<String> = None;
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--url" && i + 1 < args.len() {
                    url = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Command::Drain { check: args.iter().any(|a| a == "--check"), url, app: args.iter().any(|a| a == "--app") }
        }
        "dispatch" => {
            let mut item: Option<String> = None;
            let mut to: Option<String> = None;
            let mut check = false;
            let mut i = 1;
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
                    _ => {
                        i += 1;
                    }
                }
            }
            Command::Dispatch { item, to, check, app: args.iter().any(|a| a == "--app") }
        }
        "heartbeat" => Command::Heartbeat(args.get(1).cloned().unwrap_or_default()),
        "checkpoint" => Command::Checkpoint(args[1..].to_vec()),
        "fail" => Command::Fail(args[1..].to_vec()),
        "show" => {
            let addr = args[1..].iter().find(|a| *a != "--app").cloned().unwrap_or_default();
            Command::Show { addr, app: args.iter().any(|a| a == "--app") }
        }
        "release" => Command::Release,
        "update" => Command::Update { check: args.iter().any(|a| a == "--check") },
        "evolve" => Command::Evolve { check: args.iter().any(|a| a == "--check") },
        "host" => {
            let mut persona: Option<String> = None;
            let mut boot: Option<String> = None;
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--boot" && i + 1 < args.len() {
                    boot = Some(args[i + 1].clone());
                    i += 2;
                } else if persona.is_none() {
                    persona = Some(args[i].clone());
                    i += 1;
                } else {
                    i += 1;
                }
            }
            Command::Host { persona, boot }
        }
        "unhost" => Command::Unhost(w(1).to_string()),
        "message" => Command::Message {
            persona: w(1).to_string(),
            text: args[2..].join(" "),
        },
        "status" => Command::Status,
        "plan" => Command::Plan,
        "serve" => Command::Serve(args.get(1).and_then(|s| s.parse().ok())),
        other => Command::Unknown(other.to_string()),
    }
}

pub fn help_text() -> String {
    let mut s = String::new();
    s.push_str(&format!("qexo {} — the Exocortex coordinator (phext-native)
", crate::plan::PLAN_VERSION));
    s.push_str("All node state lives in one phext file, found by resolution:\n");
    s.push_str("QEXO_STATE when set; else the system state /var/lib/qexo/state.phext when it\n");
    s.push_str("exists and is readable; else the per-user state ~/.qexo/state.phext (Windows:\n");
    s.push_str("under the user profile). A fresh `qexo config` as a normal user births a\n");
    s.push_str("per-user node — no sudo anywhere in its life. `qexo status` names the live path.\n\n");

    s.push_str("Everyday:\n");
    s.push_str("  qexo                      the current plan, human rendering\n");
    s.push_str("  qexo plan                 the same, explicitly\n");
    s.push_str("  qexo status               node, version, state path, key, peers, hosted, doctor headline\n");
    s.push_str("  qexo doctor [--fix]       health report; --fix repairs what is safely repairable\n");
    s.push_str("  qexo learn                Learn You a qexo! — the wizard: chapter map, lessons, due cards\n");
    s.push_str("                            as practice. keys: space reveal, 1/2/3 grade (again/good/easy),\n");
    s.push_str("                            b chapter map, d due queue, q quit (progress saves per card)\n");
    s.push_str("  qexo learn --status       deck size, due count, mastered count — no session\n");
    s.push_str("  qexo learn --app          the wizard's engine as a line protocol for front-ends\n");
    s.push_str("                            (the Android app): commands status / chapters / lesson <n> /\n");
    s.push_str("                            next / practice <n> / reveal / grade again|good|easy / quit,\n");
    s.push_str("                            replies as key: value lines, each block closed by end: <command>\n");
    s.push_str("  qexo --help               this text\n\n");

    s.push_str("The ecosystem:\n");
    s.push_str("  qexo inventory            the phext ecosystem roster (alias: catalog): every shipped product,\n");
    s.push_str("                            what it is, where it lives, how to install and launch it\n");
    s.push_str("  qexo inventory <name>     one product's full record\n");
    s.push_str("  qexo inventory --install <name>\n");
    s.push_str("                            print the product's install command, ready to copy — print-only:\n");
    s.push_str("                            qexo never installs ecosystem products silently\n\n");

    s.push_str("Identity & peers:\n");
    s.push_str("  qexo config               birth the node: create the state phext (the system path when\n");
    s.push_str("                            privileged, the per-user path otherwise), set up the node key\n");
    s.push_str("  qexo config --auto-update turn automatic updates on (config record) and install the systemd timer where possible\n");
    s.push_str("  qexo config --evolve      set evolve: on — the automatic timer's trigger becomes a full evolve pass\n");
    s.push_str("  qexo config --release-signer <FP>\n                            set the trusted release signer fingerprint in the config record\n");
    s.push_str("                            instead of update alone (update is one stage of evolve; see below)\n");
    s.push_str("  qexo list                 the phextnet as a navigation tree: this node and its peers nested\n");
    s.push_str("                            under the peers anchor 3.3.3/3.3.3/3.3.3, named by hostname\n");
    s.push_str("  qexo list --json          the flat machine shape (JSON is the interop gasket, unchanged)\n");
    s.push_str("  qexo register <pubkey> [--endpoint <url>]   register a peer's public key (armored text, a file path, or - for stdin);\n");
    s.push_str("                            the endpoint (its qexo serve base URL) is recorded on the peer so update can poll it.\n");
    s.push_str("                            the peer's host name is read from its key's user id and recorded as host: —\n");
    s.push_str("                            displays name machines, never bare addresses, where a name exists.\n");
    s.push_str("                            fleet convention: a peer's name IS its tailnet hostname; register it by a\n");
    s.push_str("                            hostname endpoint (http://<hostname>:8484) so the net reads by name end to end\n\n");

    s.push_str("Updates — over the phextnet:\n");
    s.push_str("  qexo release              announce this node's binary as a signed release (manifest scroll in the ledgers;\n");
    s.push_str("                            the binary itself is served from the releases dir beside the state file)\n");
    s.push_str("  qexo update               poll peers for the trusted signer's release; verify signature + sha256, then swap\n");
    s.push_str("  qexo update --check       the same poll, report only — downloads and changes nothing\n");
    s.push_str("                            trust rule: only a release whose manifest signature verifies against the peer\n");
    s.push_str("                            whose fingerprint is the config record's release_signer is ever applied\n\n");

    s.push_str("Convergence — one pass over the phextnet:\n");
    s.push_str("  qexo evolve               everything this node can learn from its peers, applied in one receipted\n");
    s.push_str("                            cycle: (1) peer gossip — peers-of-peers are recorded as discovered, which\n");
    s.push_str("                            is knowledge, never trust (a discovered peer is never polled, never a\n");
    s.push_str("                            signer; only `qexo register` makes a peer trusted); (2) persona sync —\n");
    s.push_str("                            newer persona revisions are adopted from trusted peers, and where both\n");
    s.push_str("                            lines advanced from a common ancestor the divergence is kept whole,\n");
    s.push_str("                            both copies standing as attempt artifacts — never last-writer-wins;\n");
    s.push_str("                            (3) the binary self-update, exactly `qexo update`. One evolution receipt\n");
    s.push_str("                            in the ledgers names every stage's outcome. A stage's failure never\n");
    s.push_str("                            blocks the next; exit is 1 only when the binary stage refuses or rolls\n");
    s.push_str("                            back an offer that claimed the trusted signer\n");
    s.push_str("  qexo evolve --check       the same pass as reads only — print what would change, write nothing\n");
    s.push_str("                            (update is one stage of evolve: with evolve: on in the config record,\n");
    s.push_str("                            the automatic timer fires `qexo evolve` instead of `qexo update`)\n\n");

    s.push_str("Dispatch (the dispatch core):\n");
    s.push_str("  qexo enqueue <brief-file> [--title <t>] [--coordinate <task-coordinate>] [--register <n>]\n");
    s.push_str("                            [--value <1-9>] [--cost <1-9>] [--goal <text>] [--after <item-address>]\n");
    s.push_str("                            [--exit <text>] [--origin <origin>]\n");
    s.push_str("                            file a queue item in the dispatch homeland (11.11.11/11.11.11/11.11.11,\n");
    s.push_str("                            cluster 2): the item record at Scroll 1, the brief at Scroll 2.\n");
    s.push_str("                            Prints the item's full address only after the record is durable\n");
    s.push_str("                            in state (write-before-ack). `qexo enqueue -` reads the brief\n");
    s.push_str("                            from stdin\n");
    s.push_str("  qexo queue [--all] [--node <name>]\n");
    s.push_str("                            the queue in its calculated order (spec §5: daily-goal class,\n");
    s.push_str("                            then ROI ratio, then age — calculated at every read, never\n");
    s.push_str("                            stored): state, title, sort keys, age, failures. Paused-auto\n");
    s.push_str("                            items pin to the head, flagged. --all includes terminal items\n");
    s.push_str("                            (done, cancelled)\n");
    s.push_str("  qexo queue --explain <item-address>\n");
    s.push_str("                            one item's sort keys, its calculated position, and its\n");
    s.push_str("                            re-order audit trail\n");
    s.push_str("  qexo pause <item-address> [--note <text>]\n");
    s.push_str("                            manual pause: the item leaves the dispatchable queue until\n");
    s.push_str("                            resumed. The transition and its receipt are one write\n");
    s.push_str("  qexo resume <item-address>\n");
    s.push_str("                            return a paused item to queued; also the manual override of\n");
    s.push_str("                            an auto-pause (resets the consecutive-failure counter, and\n");
    s.push_str("                            writes the audit that says who overrode)\n");
    s.push_str("  qexo cancel <item-address> --note <text>\n");
    s.push_str("                            terminal, Will/Emise only, note required — the note is the\n");
    s.push_str("                            receipt's evidence\n");
    s.push_str("  qexo drain [--check] [--url <register-url>]\n");
    s.push_str("                            the register drain (spec §10): pull the pending entries\n");
    s.push_str("                            and resolutions of Will's scope from the register, file\n");
    s.push_str("                            entries as queue items, close the items resolutions name\n");
    s.push_str("                            (register-close audits), retry pending write-backs, and\n");
    s.push_str("                            advance the watermarks — items and audits land before\n");
    s.push_str("                            watermarks move (write-before-ack), so a re-drain is a\n");
    s.push_str("                            no-op. --check reports what a drain would do; writes\n");
    s.push_str("                            nothing. --url names the register once; it is kept in\n");
    s.push_str("                            the drain record (11.11.11/6.1.1/1.1.1). Credentials live\n");
    s.push_str("                            in /var/lib/qexo/qexo.env (QEXO_ENV overrides the path),\n");
    s.push_str("                            never in state\n");
    s.push_str("  qexo audit [--item <address>] [--kind <kind>] [--since <date>]\n");
    s.push_str("                            the audit view (ledgers, cluster 9): what was checked and why\n");
    s.push_str("                            an act was allowed to happen quietly — newest last.\n");
    s.push_str("                            `qexo audit show <address>` renders one audit in full\n");
    s.push_str("  qexo dispatch [<item-address>] [--to <node>] [--check]\n");
    s.push_str("                            place the queue's head (or the named item) on a node drawn\n");
    s.push_str("                            fairly from the eligible set (spec §6: reachable, load/thermal\n");
    s.push_str("                            gates, no resident displaced, inputs stageable, capability fit),\n");
    s.push_str("                            as a signed bundle (spec §3.4) the target accepts or refuses with\n");
    s.push_str("                            a receipt. The placement receipt is the assignment record\n");
    s.push_str("                            (11.11.11, cluster 3). An empty eligible set is a receipted\n");
    s.push_str("                            outcome — the item stays queued; nothing defaults to the\n");
    s.push_str("                            coordinator. --check prints the round and writes nothing\n");
    s.push_str("  qexo heartbeat <item-address>\n");
    s.push_str("                            on the executing node: beat — the heartbeat line lands on the\n");
    s.push_str("                            assignment and the attempt record; the first beat moves the\n");
    s.push_str("                            item dispatched → running. On the item's home node: read the\n");
    s.push_str("                            executor's assignment over the scroll route and reconcile —\n");
    s.push_str("                            new checkpoint cites are taken into custody and their cited\n");
    s.push_str("                            outputs verified by read-back (verified progress resets the\n");
    s.push_str("                            failure counter, §8); on 3 missed beats with the executor\n");
    s.push_str("                            unreachable or silent, the failure is detected and the item\n");
    s.push_str("                            teleports to a fresh fair draw, resuming from its last\n");
    s.push_str("                            checkpoint (the third consecutive failure auto-pauses instead)\n");
    s.push_str("  qexo checkpoint <item-address> --note <text> [--output <coordinate>]...\n");
    s.push_str("                            append a checkpoint scroll (Scroll 4+) in the attempt universe's\n");
    s.push_str("                            item Book and cite it on the assignment; each --output names a\n");
    s.push_str("                            scroll this attempt produced — its SHA-256 is recorded beside\n");
    s.push_str("                            the cite, the pair the origin verifies progress against (§8)\n");
    s.push_str("  qexo fail <item-address> --note <evidence>\n");
    s.push_str("                            the executor's failure receipt: the item returns to queued (the\n");
    s.push_str("                            third consecutive failure auto-pauses it) and the assignment\n");
    s.push_str("                            retires with the evidence as its status\n");
    s.push_str("  qexo bridge [--add <name> --kind <kind> --endpoint <where> [--capability <text>]...\n");
    s.push_str("                            [--burn metered|never] [--reset <text>] [--freshness <seconds>]]\n");
    s.push_str("                            [--check [<name>]] [--retire <name>]\n");
    s.push_str("                            the bridge registry (spec §9: dispatch homeland cluster 5,\n");
    s.push_str("                            records from 11.11.11/5.1.1/1.1.1):\n");
    s.push_str("                            durable, health-checked routes to inference sources. Bare\n");
    s.push_str("                            `qexo bridge` lists the registry with health calculated from\n");
    s.push_str("                            each Book's health log — a bridge is live iff its latest probe\n");
    s.push_str("                            succeeded within its freshness window; health is never a stored\n");
    s.push_str("                            field. --check probes now and appends the evidence. An item whose\n");
    s.push_str("                            brief names `bridge:` preferences routes only over a bridge\n");
    s.push_str("                            verified live for that dispatch; failovers are receipts, and a\n");
    s.push_str("                            `burn: never` bridge never routes work (enqueue refuses a brief\n");
    s.push_str("                            that names one). Kinds: claude | grok | chatgpt | ollama | tlm |\n");
    s.push_str("                            slm | llm | sq | custom — the model line is first-class from day one\n");
    s.push_str("  qexo show <address>       render one record from this node's state; the address may carry\n");
    s.push_str("                            a universe prefix — $2:11.11.11/2.1.1/1.1.1 reads universe 2 of\n");
    s.push_str("                            the state chain (a bare address is universe 1)\n");
    s.push_str("  qexo <verb> --app         the dispatch verbs as a line protocol for front-ends (the Android\n");
    s.push_str("                            launcher): enqueue --app, queue --app, dispatch --app, show --app,\n");
    s.push_str("                            pause --app, resume --app, audit --app, drain --app — every spelling\n");
    s.push_str("                            opens the same session over the same queue state machine, the learn\n");
    s.push_str("                            --app pattern: one command per line (enqueue / queue / dispatch /\n");
    s.push_str("                            show / pause / resume / audit / drain / quit), replies as key: value\n");
    s.push_str("                            lines, each block closed by end: <command>, a greeting block at\n");
    s.push_str("                            start. Bare `enqueue` in the session pastes the brief: lines until\n");
    s.push_str("                            a line containing only '.' (a line '..' unstuffs to '.'). cancel\n");
    s.push_str("                            has no --app form — it stays a CLI act\n\n");

    s.push_str("Personas & inference:\n");
    s.push_str("  qexo host <persona>       load a persona into local inference (--boot <path> for a boot text file;\n");
    s.push_str("                            --boot against an existing, unhosted home with different text is a local\n");
    s.push_str("                            edit — the boot is replaced and the persona's revision bumps)\n");
    s.push_str("  qexo host                 list hosted personas\n");
    s.push_str("  qexo unhost <persona>     unload a hosted persona\n");
    s.push_str("  qexo message <persona> <text...>   one request/response exchange with a hosted persona\n\n");

    s.push_str("System:\n");
    s.push_str("  qexo distributed-builds   upgrade this node into the fleet's distributed build cluster:\n");
    s.push_str("                            sccache in front (fleet-shared cache), icecream behind it\n");
    s.push_str("                            (compiles spread across the fleet). Idempotent; the recipes\n");
    s.push_str("                            live in the exocortical tree (/source/exocortical), this\n");
    s.push_str("                            command fetches that tree current and runs them\n");
    s.push_str("  qexo distributed-builds --status\n");
    s.push_str("                            report only: sccache, iceccd, scheduler — probed, not remembered\n");
    s.push_str("  qexo distributed-builds --scheduler\n");
    s.push_str("                            also run the scheduler recipe (the fleet's one scheduler host;\n");
    s.push_str("                            implied on aletheia-core itself)\n");
    s.push_str("  qexo distributed-builds --branch <name>\n");
    s.push_str("                            take the recipes from a branch of the exocortical tree\n");
    s.push_str("                            (the rollout rides the feature branch until it merges to exo)\n");
    s.push_str("  qexo serve [port]         the phext HTTP API (payloads are scroll text)\n");
    s.push_str("  qexo wiring-check [dir]   every src/*.rs declared as a module, every declaration filed\n");
    s.push_str("  qexo --version            version\n\n");

    s.push_str("Phext files:\n");
    s.push_str("  qexo parse <file>         scroll inventory of a phext file\n");
    s.push_str("  qexo explode <file> [dir] one .scroll file per scroll\n");
    s.push_str("  qexo implode <file>       parse + re-serialize (canonical form check)\n");
    s.push_str("  qexo seal <file>          write <file>.xz (stored-form seal)\n");
    s.push_str("  qexo unseal <file.xz>     print the unsealed text\n");
    s.push_str("  qexo select <file> <coord>   print one scroll from a phext file\n");
    s.push_str("  qexo select <coord>          print one scroll from this node's state\n");
    s.push_str("  qexo get ...                 same as select, both forms\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_covers_the_surface() {
        assert_eq!(parse(&v(&[])), Command::Bare);
        assert_eq!(parse(&v(&["--help"])), Command::Help);
        assert_eq!(parse(&v(&["-h"])), Command::Help);
        assert_eq!(parse(&v(&["help"])), Command::Help);
        assert_eq!(parse(&v(&["config"])), Command::Config { auto_update: false, evolve: false, release_signer: None });
        assert_eq!(parse(&v(&["config", "--auto-update"])), Command::Config { auto_update: true, evolve: false, release_signer: None });
        assert_eq!(parse(&v(&["config", "--evolve"])), Command::Config { auto_update: false, evolve: true, release_signer: None });
        assert_eq!(parse(&v(&["config", "--release-signer", "A4E7277FB3EB0E589604BE5B506DBE3FC209E755"])), Command::Config { auto_update: false, evolve: false, release_signer: Some("A4E7277FB3EB0E589604BE5B506DBE3FC209E755".to_string()) });
        assert_eq!(parse(&v(&["release"])), Command::Release);
        assert_eq!(parse(&v(&["update"])), Command::Update { check: false });
        assert_eq!(parse(&v(&["update", "--check"])), Command::Update { check: true });
        assert_eq!(parse(&v(&["evolve"])), Command::Evolve { check: false });
        assert_eq!(parse(&v(&["evolve", "--check"])), Command::Evolve { check: true });
        assert_eq!(parse(&v(&["doctor"])), Command::Doctor { fix: false });
        assert_eq!(parse(&v(&["doctor", "--fix"])), Command::Doctor { fix: true });
        assert_eq!(parse(&v(&["list"])), Command::List { json: false });
        assert_eq!(parse(&v(&["list", "--json"])), Command::List { json: true });
        assert_eq!(parse(&v(&["learn"])), Command::Learn { status: false, app: false });
        assert_eq!(parse(&v(&["learn", "--status"])), Command::Learn { status: true, app: false });
        assert_eq!(parse(&v(&["learn", "--app"])), Command::Learn { status: false, app: true });
        assert_eq!(parse(&v(&["inventory"])), Command::Inventory { name: None, install: None });
        assert_eq!(parse(&v(&["catalog"])), Command::Inventory { name: None, install: None });
        assert_eq!(parse(&v(&["inventory", "sq"])), Command::Inventory { name: Some("sq".into()), install: None });
        assert_eq!(parse(&v(&["inventory", "--install", "sq"])), Command::Inventory { name: None, install: Some("sq".into()) });
        assert_eq!(parse(&v(&["register", "keyfile.asc"])), Command::Register(v(&["keyfile.asc"])));
        assert_eq!(parse(&v(&["enqueue", "brief.md", "--title", "T"])), Command::Enqueue { args: v(&["brief.md", "--title", "T"]), app: false });
        assert_eq!(parse(&v(&["enqueue", "-"])), Command::Enqueue { args: v(&["-"]), app: false });
        assert_eq!(parse(&v(&["queue"])), Command::Queue { all: false, node: None, explain: None, app: false });
        assert_eq!(parse(&v(&["queue", "--all"])), Command::Queue { all: true, node: None, explain: None, app: false });
        assert_eq!(parse(&v(&["queue", "--node", "aletheia-core"])), Command::Queue { all: false, node: Some("aletheia-core".into()), explain: None, app: false });
        assert_eq!(
            parse(&v(&["queue", "--explain", "11.11.11/2.1.4/1.1.1"])),
            Command::Queue { all: false, node: None, explain: Some("11.11.11/2.1.4/1.1.1".into()), app: false }
        );
        assert_eq!(parse(&v(&["pause", "11.11.11/2.1.4/1.1.1"])), Command::Pause { args: v(&["11.11.11/2.1.4/1.1.1"]), app: false });
        assert_eq!(parse(&v(&["pause", "11.11.11/2.1.4/1.1.1", "--note", "later"])), Command::Pause { args: v(&["11.11.11/2.1.4/1.1.1", "--note", "later"]), app: false });
        assert_eq!(parse(&v(&["resume", "11.11.11/2.1.4/1.1.1"])), Command::Resume { args: v(&["11.11.11/2.1.4/1.1.1"]), app: false });
        assert_eq!(parse(&v(&["cancel", "11.11.11/2.1.4/1.1.1", "--note", "moot"])), Command::Cancel(v(&["11.11.11/2.1.4/1.1.1", "--note", "moot"])));
        assert_eq!(parse(&v(&["drain"])), Command::Drain { check: false, url: None, app: false });
        assert_eq!(parse(&v(&["drain", "--check"])), Command::Drain { check: true, url: None, app: false });
        assert_eq!(
            parse(&v(&["drain", "--url", "https://register.phext.io"])),
            Command::Drain { check: false, url: Some("https://register.phext.io".into()), app: false }
        );
        assert_eq!(
            parse(&v(&["drain", "--check", "--url", "http://127.0.0.1:8791"])),
            Command::Drain { check: true, url: Some("http://127.0.0.1:8791".into()), app: false }
        );
        assert_eq!(parse(&v(&["bridge"])), Command::Bridge { args: v(&[]), write: false });
        assert_eq!(
            parse(&v(&["bridge", "--add", "claude", "--kind", "claude", "--endpoint", "https://claude.example", "--burn", "never"])),
            Command::Bridge { args: v(&["--add", "claude", "--kind", "claude", "--endpoint", "https://claude.example", "--burn", "never"]), write: true }
        );
        assert_eq!(parse(&v(&["bridge", "--check", "claude"])), Command::Bridge { args: v(&["--check", "claude"]), write: true });
        assert_eq!(parse(&v(&["audit"])), Command::Audit { args: v(&[]), app: false });
        assert_eq!(parse(&v(&["audit", "show", "6.6.6/9.1.1/1.1.1"])), Command::Audit { args: v(&["show", "6.6.6/9.1.1/1.1.1"]), app: false });
        assert_eq!(parse(&v(&["show", "11.11.11/2.1.4/1.1.1"])), Command::Show { addr: "11.11.11/2.1.4/1.1.1".into(), app: false });
        assert_eq!(parse(&v(&["show", "$2:11.11.11/2.1.4/1.1.1"])), Command::Show { addr: "$2:11.11.11/2.1.4/1.1.1".into(), app: false });
        assert_eq!(parse(&v(&["host"])), Command::Host { persona: None, boot: None });
        assert_eq!(
            parse(&v(&["host", "Emise", "--boot", "emise.phext"])),
            Command::Host { persona: Some("Emise".into()), boot: Some("emise.phext".into()) }
        );
        assert_eq!(parse(&v(&["unhost", "Emise"])), Command::Unhost("Emise".into()));
        assert_eq!(
            parse(&v(&["message", "Emise", "hello", "there"])),
            Command::Message { persona: "Emise".into(), text: "hello there".into() }
        );
        assert_eq!(parse(&v(&["status"])), Command::Status);
        assert_eq!(parse(&v(&["plan"])), Command::Plan);
        assert_eq!(parse(&v(&["serve"])), Command::Serve(None));
        assert_eq!(parse(&v(&["serve", "8484"])), Command::Serve(Some(8484)));
        assert_eq!(parse(&v(&["wiring-check"])), Command::WiringCheck(None));
        assert_eq!(parse(&v(&["select", "f.phext", "1.1.1/1.1.1/1.1.1"])), Command::FileOp(v(&["select", "f.phext", "1.1.1/1.1.1/1.1.1"])));
        assert_eq!(parse(&v(&["frobnicate"])), Command::Unknown("frobnicate".into()));
        // Slice 7 (§2.3): every dispatch verb's --app form —
        // the flag is the session's door, stripped from the tail.
        assert_eq!(parse(&v(&["enqueue", "--app"])), Command::Enqueue { args: v(&[]), app: true });
        assert_eq!(parse(&v(&["enqueue", "brief.md", "--app"])), Command::Enqueue { args: v(&["brief.md"]), app: true });
        assert_eq!(parse(&v(&["queue", "--app"])), Command::Queue { all: false, node: None, explain: None, app: true });
        assert_eq!(parse(&v(&["dispatch", "--app"])), Command::Dispatch { item: None, to: None, check: false, app: true });
        assert_eq!(parse(&v(&["dispatch", "--check", "--app"])), Command::Dispatch { item: None, to: None, check: true, app: true });
        assert_eq!(parse(&v(&["show", "11.11.11/2.1.4/1.1.1", "--app"])), Command::Show { addr: "11.11.11/2.1.4/1.1.1".into(), app: true });
        assert_eq!(parse(&v(&["show", "--app", "11.11.11/2.1.4/1.1.1"])), Command::Show { addr: "11.11.11/2.1.4/1.1.1".into(), app: true });
        assert_eq!(parse(&v(&["pause", "11.11.11/2.1.4/1.1.1", "--app"])), Command::Pause { args: v(&["11.11.11/2.1.4/1.1.1"]), app: true });
        assert_eq!(parse(&v(&["resume", "11.11.11/2.1.4/1.1.1", "--app"])), Command::Resume { args: v(&["11.11.11/2.1.4/1.1.1"]), app: true });
        assert_eq!(parse(&v(&["audit", "--app"])), Command::Audit { args: v(&[]), app: true });
        assert_eq!(parse(&v(&["drain", "--app"])), Command::Drain { check: false, url: None, app: true });
        assert_eq!(parse(&v(&["drain", "--check", "--app"])), Command::Drain { check: true, url: None, app: true });
    }

    #[test]
    fn distributed_builds_parses() {
        assert_eq!(parse(&v(&["distributed-builds"])),
            Command::DistributedBuilds { scheduler: false, status: false, branch: None });
        assert_eq!(parse(&v(&["distributed-builds", "--status"])),
            Command::DistributedBuilds { scheduler: false, status: true, branch: None });
        assert_eq!(parse(&v(&["distributed-builds", "--scheduler", "--branch", "emise/distributed-builds"])),
            Command::DistributedBuilds { scheduler: true, status: false, branch: Some("emise/distributed-builds".into()) });
    }

    #[test]
    fn help_banner_names_the_build_version() {
        // v1.7.2: the banner derives from the build, not a literal —
        // its first line names the current PLAN_VERSION.
        let h = help_text();
        let first = h.lines().next().unwrap_or_default();
        assert_eq!(first, format!("qexo {} — the Exocortex coordinator (phext-native)", crate::plan::PLAN_VERSION));
    }

    #[test]
    fn help_lists_every_command() {
        let h = help_text();
        for word in [
            "config", "doctor", "list", "register", "release", "update", "evolve", "host", "unhost", "message", "status",
            "plan", "serve", "wiring-check", "parse", "explode", "implode", "seal", "unseal", "distributed-builds",
            "select", "get", "--help", "/var/lib/qexo/state.phext", "learn", "inventory", "~/.qexo/state.phext",
            "enqueue", "queue", "show", "11.11.11/11.11.11/11.11.11",
            "pause", "resume", "cancel", "audit", "--explain", "drain", "11.11.11/6.1.1/1.1.1", "qexo.env",
            "bridge", "burn: never", "11.11.11/5.1.1/1.1.1",
            "--app", "end: <command>", "launcher",
        ] {
            assert!(h.contains(word), "help is missing {}", word);
        }
    }
}
