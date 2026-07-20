# Progress Ledger - Hadron Features Implementation

## Tasks
- [x] Task 1: Focus Hover-Selection Bugfix
- [x] Task 2: Slash Commands in Chamber
- [x] Task 3: No-Human-Mode Adjudication Loop in Gluon
- [x] Task 4: Worktree Isolation & Merge Gate Activation
- [x] Task 5: Live Mid-Turn Stream UI
- [x] Task 6: Budget Ceilings
- [x] Task 7: Foldable Plan Tab
- [x] Role Preference & locks: Task 1 (deny_skills plumbing), Task 2 (task-to-role preferred mapping), Task 3 (deny_skills dispatch hard lock), Task 4 (soft role preference resolution), Task 5 (role prompt body injection), Task 6 (Settings UI edit inputs)

## Changelog
- Task 1: complete (commits 8154e12a..448c2d16, review clean)
- Task 2: complete (commits d9fba36..1caac47, review clean)
- Task 3: complete (commit 36c726e, review clean)
- Task 4: complete (commit 6eb8e23, review clean)
- Task 5: complete (commit 4d0931c, review clean. Fixed mid-turn repaint issue in reload.rs by polling live activities and calling cx.notify() on change, and removed the unused Activity tab from the right rail.)
- Task 6: complete (commits 6f22bde..24a356c, review clean)
- Task 7: complete (commit 340f0b0, review clean)
- Security: Sanitized path traversal in active plan loading and workspace file reading (commit b8c50b4, review clean)
- Role Preference & locks: complete (all tasks 1–6 implemented, verified by passing unit tests across engine, prompt, personas, and chamber UI)
- Task 1: complete (commits d27f548..0af746d, review clean)
- Task 2: complete (commits 0af746d..1cf6848, review clean)
- Task 3: complete (commits 1cf6848..82210e0, review clean)

## Minor Findings / Cleanup List
- `crates/hadron-chamber/src/app/widgets.rs:L56`: The `wash_layer` helper function is unused and generates a compiler warning.
- `assets/background.jpeg`: Unrequested 9 MB image asset was added by the user; keep/ignore since it was committed by the user.

## Memory Feature Map & Invariants Registry
- [x] Task 1: Initialize Invariants Registry
- [x] Task 2: Initialize Feature Map
- [x] Task 3: Update Standard Model Rules

