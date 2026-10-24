---
name: release
description: Execute project release procedure (updating CHANGELOG, bumping version, merging main, tagging, and pushing)
---

# Release Procedure

Execute the repository release workflow strictly following instructions in `.hadron/nucleus/release.md`.

## Procedure Steps:
1. **Pre-flight Check:** Verify working tree is clean and up-to-date with `origin/main`.
2. **Documentation & Changelog:** Audit and update project documentation (`docs/`, `README.md`, command references) to reflect any new features, tools, commands, or workflows. Update `docs/CHANGELOG.md` with release notes, update in-app `RELEASES` in `crates/hadron-chamber/src/app/render/overlays.rs`, and bump project version in workspace manifests (`Cargo.toml`, `Cargo.lock`).
3. **Commit Prep:** Commit version bump, changelog, documentation updates, and overlay updates with commit message `chore(release): prepare vX.Y.Z`.
4. **Tagging with Full Notes:** Create annotated release tag `vX.Y.Z` on HEAD with title `Release vX.Y.Z` and the complete changelog body from `docs/CHANGELOG.md`:
   ```bash
   git tag -a -m "Hadron vX.Y.Z

$(python3 -c 'import re; c=open("docs/CHANGELOG.md").read(); m=re.search(r"^## \[X\.Y\.Z\].*?\n(.*?)(?=^## \[|\Z)", c, re.S|re.M); print(m.group(1).strip() if m else "")')" vX.Y.Z HEAD
   ```
5. **Push:** Push current worktree commit to remote `main` branch (`git push origin HEAD:main`) and push release tag (`git push origin vX.Y.Z`).
6. **CI-Gated GitHub Release Publication:** Do **NOT** run `gh release create` manually at tag-push time. The `publish-release` job in `.github/workflows/release.yml` (`needs: build-release`) automatically creates `Hadron vX.Y.Z` with `docs/CHANGELOG.md` notes and attaches all archives only after all 4 platform builds succeed.

