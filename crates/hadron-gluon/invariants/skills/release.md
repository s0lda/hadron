---
name: release
description: Execute project release procedure (updating CHANGELOG, bumping version, merging main, tagging, and pushing)
---

# Release Procedure

Execute the repository release workflow strictly following instructions in `.hadron/nucleus/release.md`.

## Procedure Steps:
1. **Pre-flight Check:** Verify working tree is clean, on `main`, and up-to-date with `origin/main`.
2. **Version & Changelog:** Update `CHANGELOG.md` (or `docs/CHANGELOG.md`) with release notes and bump project version in workspace manifests (e.g., `Cargo.toml`).
3. **Commit Prep:** Commit version bump and changelog updates with commit message `chore(release): prepare vX.Y.Z`.
4. **Merge Sync:** Sync and merge local `main` with `origin/main`.
5. **Tagging:** Create annotated release tag `vX.Y.Z` with commit message `Release vX.Y.Z`.
6. **Push:** Push `main` and release tags to remote `origin`.
