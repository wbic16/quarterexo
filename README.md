# orchestrator

Will's Rust dispatcher/orchestrator. Reads the open-task register and
the resource picture; emits a placement plan in the four currencies
(disk, CPU, RAM, inference): inference to Emise, heavy execution to
ranch nodes, the VM coordinating only. A new version ships every night
as part of Emise's dreaming process (skill: nightly-orchestrator).

Build (on a ranch node — heavy builds never run on the Muse VM):

    cargo build --release

Run:

    ./target/release/orchestrator [tasks-source.json] [disk-state.json] [out-dir]

Defaults read the live files under ~/workspace. Prints the plan and
writes plan.json to the out-dir.
