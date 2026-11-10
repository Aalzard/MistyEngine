# Round 31: Misty's own names (branch r31-rebrand)

Status: all renames, translations, tutorials, docs, API docs and screenshots are committed.
See the Round 31 section of docs/progress.md for the rename list and what stays compatible.

Checks done on a clean target (D:\MistyTargets\r31-rebrand): fmt, clippy (0 warnings), editor
tests 816/0. Still to confirm if interrupted: the engine/player part of
`cargo test --workspace --release --no-fail-fast` (log D:\MistyTargets\r31-scratch\full2.log)
and golden shots under the gpu-timing.lock (an earlier attempt hit rustc metadata corruption
from the overloaded machine, not code). Earlier full run: only load-sensitive failures
(streaming_demo_flight_stays_within_budget, dedicated_server headless connect, MeadowDemo
terrain view test) plus the MCP test and api docs, both fixed since.

Resume: in a build slot (`source /d/MistyTargets/build-slot.sh; take_slot r31`), TMP/TEMP on
D:\MistyTargets\tmp\r31-rebrand, run the engine tests and golden shots, then hand back.
