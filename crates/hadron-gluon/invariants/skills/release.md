---
name: release
description: Execute project release procedure (updating CHANGELOG, bumping version, merging main, tagging, and pushing)
---

# Release Procedure

Execute the repository release workflow strictly following instructions in `.hadron/nucleus/release.md`.

## Procedure Steps:
1. **Pre-flight Check:** Verify working tree is clean and up-to-date with `origin/main`.
2. **Version & Changelog:** Update `docs/CHANGELOG.md` with release notes, update in-app `RELEASES` in `crates/hadron-chamber/src/app/render/overlays.rs`, and bump project version in workspace manifests (`Cargo.toml`, `Cargo.lock`).
3. **Commit Prep:** Commit version bump, changelog, and overlay updates with commit message `chore(release): prepare vX.Y.Z`.
4. **Tagging:** Create annotated release tag `vX.Y.Z` on HEAD with message `Release vX.Y.Z`.
5. **Push:** Push current worktree commit to remote `main` branch (`git push origin HEAD:main`) and push release tag (`git push origin vX.Y.Z`).

