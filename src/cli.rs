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
    Config { auto_update: bool, evolve: bool },
    Doctor { fix: bool },
    List,
    Register(Vec<String>),
    Release,
    Update { check: bool },
    Evolve { check: bool },
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
    match w(0) {
        "" => Command::Bare,
        "--help" | "-h" | "help" => Command::Help,
        "--version" | "version" => Command::Version,
        "wiring-check" => Command::WiringCheck(args.get(1).cloned()),
        "parse" | "explode" | "implode" | "seal" | "unseal" | "select" | "get" => {
            Command::FileOp(args.to_vec())
        }
        "config" => Command::Config {
            auto_update: args.iter().any(|a| a == "--auto-update"),
            evolve: args.iter().any(|a| a == "--evolve"),
        },
        "doctor" => Command::Doctor { fix: args.iter().any(|a| a == "--fix") },
        "list" => Command::List,
        "register" => Command::Register(args[1..].to_vec()),
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
    s.push_str("qexo v1.2.1 — the Exocortex coordinator (phext-native)\n");
    s.push_str("All node state lives in one file: /var/lib/qexo/state.phext\n");
    s.push_str("(QEXO_STATE overrides the path for development and tests.)\n\n");

    s.push_str("Everyday:\n");
    s.push_str("  qexo                      the current plan, human rendering\n");
    s.push_str("  qexo plan                 the same, explicitly\n");
    s.push_str("  qexo status               node, version, state path, key, peers, hosted, doctor headline\n");
    s.push_str("  qexo doctor [--fix]       health report; --fix repairs what is safely repairable\n");
    s.push_str("  qexo --help               this text\n\n");

    s.push_str("Identity & peers:\n");
    s.push_str("  qexo config               birth the node: create state.phext, set up the node GPG key\n");
    s.push_str("  qexo config --auto-update turn automatic updates on (config record) and install the systemd timer where possible\n");
    s.push_str("  qexo config --evolve      set evolve: on — the automatic timer's trigger becomes a full evolve pass\n");
    s.push_str("                            instead of update alone (update is one stage of evolve; see below)\n");
    s.push_str("  qexo list                 the registered peers\n");
    s.push_str("  qexo register <pubkey> [--endpoint <url>]   register a peer's public key (armored text, a file path, or - for stdin);\n");
    s.push_str("                            the endpoint (its qexo serve base URL) is recorded on the peer so update can poll it\n\n");

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

    s.push_str("Personas & inference:\n");
    s.push_str("  qexo host <persona>       load a persona into local inference (--boot <path> for a boot text file;\n");
    s.push_str("                            --boot against an existing, unhosted home with different text is a local\n");
    s.push_str("                            edit — the boot is replaced and the persona's revision bumps)\n");
    s.push_str("  qexo host                 list hosted personas\n");
    s.push_str("  qexo unhost <persona>     unload a hosted persona\n");
    s.push_str("  qexo message <persona> <text...>   one request/response exchange with a hosted persona\n\n");

    s.push_str("System:\n");
    s.push_str("  qexo serve [port]         the phext HTTP API (payloads are scroll text)\n");
    s.push_str("  qexo wiring-check [dir]   every src/*.rs declared as a module, every declaration filed\n");
    s.push_str("  qexo --version            version\n\n");

    s.push_str("Phext files:\n");
    s.push_str("  qexo parse <file>         scroll inventory of a phext file\n");
    s.push_str("  qexo explode <file> [dir] one .scroll file per scroll\n");
    s.push_str("  qexo implode <file>       parse + re-serialize (canonical form check)\n");
    s.push_str("  qexo seal <file>          write <file>.xz (stored-form seal)\n");
    s.push_str("  qexo unseal <file.xz>     print the unsealed text\n");
    s.push_str("  qexo select <file> <coord>   print one scroll\n");
    s.push_str("  qexo get <file> <coord>      same as select\n");
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
        assert_eq!(parse(&v(&["config"])), Command::Config { auto_update: false, evolve: false });
        assert_eq!(parse(&v(&["config", "--auto-update"])), Command::Config { auto_update: true, evolve: false });
        assert_eq!(parse(&v(&["config", "--evolve"])), Command::Config { auto_update: false, evolve: true });
        assert_eq!(parse(&v(&["release"])), Command::Release);
        assert_eq!(parse(&v(&["update"])), Command::Update { check: false });
        assert_eq!(parse(&v(&["update", "--check"])), Command::Update { check: true });
        assert_eq!(parse(&v(&["evolve"])), Command::Evolve { check: false });
        assert_eq!(parse(&v(&["evolve", "--check"])), Command::Evolve { check: true });
        assert_eq!(parse(&v(&["doctor"])), Command::Doctor { fix: false });
        assert_eq!(parse(&v(&["doctor", "--fix"])), Command::Doctor { fix: true });
        assert_eq!(parse(&v(&["list"])), Command::List);
        assert_eq!(parse(&v(&["register", "keyfile.asc"])), Command::Register(v(&["keyfile.asc"])));
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
    }

    #[test]
    fn help_lists_every_command() {
        let h = help_text();
        for word in [
            "config", "doctor", "list", "register", "release", "update", "evolve", "host", "unhost", "message", "status",
            "plan", "serve", "wiring-check", "parse", "explode", "implode", "seal", "unseal",
            "select", "get", "--help", "/var/lib/qexo/state.phext",
        ] {
            assert!(h.contains(word), "help is missing {}", word);
        }
    }
}
