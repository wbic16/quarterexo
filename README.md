# qexo

Will's Rust dispatcher/qexo. Reads the open-task register and
the resource picture; emits a placement plan in the four currencies
(disk, CPU, RAM, inference): inference to Emise, heavy execution to
ranch nodes, the VM coordinating only. A new version ships every night
as part of Emise's dreaming process (skill: nightly-qexo).

Build (on a ranch node — heavy builds never run on the Muse VM):

    cargo build --release

Run:

    ./target/release/qexo [tasks-source.json] [disk-state.json] [out-dir]

Defaults read the live files under ~/workspace. Prints the plan and
writes plan.json to the out-dir.

v2 subcommands (design: DESIGN.md):

    ./target/release/qexo sync-manifest <dir>
    ./target/release/qexo sync-diff <old-manifest> <new-manifest>
    ./target/release/qexo interfaces
    ./target/release/qexo personas

`sync-manifest` writes a MANIFEST.sha256 for a state bundle directory;
`sync-diff` prints the delta between two manifests (the sync protocol
moves only the delta); `interfaces` prints the five interior interface
readings — temperature, emotion, goals, desires, dreams — projected
from their source surfaces, degrading honestly where a surface is
unreachable.
