# Third-Party Notices

Hadron itself is licensed under the Apache License 2.0 (see [LICENSE](LICENSE)).
It also bundles or depends on third-party material, credited here. Each is used
under its own licence.

---

## Superpowers (skill procedures)

The Markdown skill procedures in
[`crates/hadron-gluon/invariants/skills/`](crates/hadron-gluon/invariants/skills/)
are ported from **Superpowers**, an agentic-skills framework and software-development
methodology by Jesse Vincent.

- **Project:** Superpowers — https://github.com/obra/superpowers
- **Author:** Jesse Vincent ([@obra](https://github.com/obra))
- **Licence:** MIT

Per the MIT licence, the original copyright and permission notice is retained
below and travels with the ported files:

```
MIT License

Copyright (c) 2025 Jesse Vincent

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

> Note: the ported skill bodies have been adapted for Hadron's engine-level
> injection (which replaces Superpowers' Claude Code session hooks). Some files
> retain upstream `superpowers:`-prefixed cross-references from the original text.

---

## Rust dependencies

All crate dependencies declared in the workspace `Cargo.toml` files are used
under their respective licences (predominantly MIT / Apache-2.0). Notable
git-tracked dependencies:

- **GPUI** and **gpui_platform** — Zed Industries, Apache-2.0
  (https://github.com/zed-industries/zed). Tracked from git.
- **gpui-component** / **gpui-component-macros** / **gpui-component-assets** —
  Longbridge, Apache-2.0 (https://github.com/longbridge/gpui-component). Hadron
  runs a small fork adding `TextMark::color`, branched from the pinned upstream
  commit and intended for upstreaming.
- **agent-client-protocol** (Rust SDK) — Zed, Apache-2.0
  (https://github.com/agentclientprotocol/rust-sdk).

See each dependency's own repository for its full licence text.

---

## Inter

Copyright (c) 2016 The Inter Project Authors. Licensed under the SIL Open Font
License 1.1. Bundled as `crates/hadron-chamber/assets/fonts/Inter-{Regular,Bold}.ttf`;
full licence at `crates/hadron-chamber/assets/fonts/Inter-OFL.txt`.

## Cascadia Code

Copyright (c) 2019 Microsoft Corporation. Licensed under the SIL Open Font
License 1.1. Bundled as `crates/hadron-chamber/assets/fonts/CascadiaCode-{Regular,Bold}.ttf`;
full licence at `crates/hadron-chamber/assets/fonts/CascadiaCode-OFL.txt`.

---

## Symbols Icon Theme

The SVG file and folder icons bundled under `crates/hadron-chamber/assets/symbols/`
are sourced from **Symbols for VS Code** by Miguel Solorio and the **zed-symbols**
port by Joan S. Garcia.

- **Original Project:** Symbols for VS Code — https://github.com/miguelsolorio/vscode-symbols
- **Zed Extension:** zed-symbols — https://github.com/joangarciaa-dev/zed-symbols
- **Author:** Miguel Solorio ([@miguelsolorio](https://github.com/miguelsolorio))
- **Contributor / Zed Port:** Joan S. Garcia ([@joangarciaa-dev](https://github.com/joangarciaa-dev))
- **Licence:** MIT

Per the MIT licence, the original copyright and permission notice is retained below:

```
MIT License

Copyright (c) 2020-2026 Miguel Solorio

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
