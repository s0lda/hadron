# Feasibility Research: Google Noto Emoji (`noto-emoji`) in GPUI on Linux (Software/Lavapipe)

**Date**: 2026-07-23  
**Target Repo**: `https://github.com/googlefonts/noto-emoji`  
**Crate**: `hadron-chamber` (`crates/hadron-chamber`)  
**Environment**: Linux (WSLg / X11 / Wayland) using GPUI + `font-kit` + FreeType with CPU software rasterization (`lavapipe`).

---

## 1. Executive Summary & Verdict

- **Can GPUI on Linux render full-color glyphs from `NotoColorEmoji.ttf` directly?**  
  **NO (Color COLRv1 / CBDT bitmaps are unsupported in GPUI's Linux atlas pipeline).**
- **Can GPUI render monochrome vector emoji glyphs from `NotoEmoji-Regular.ttf`?**  
  **YES.**

---

## 2. Technical Root Cause Analysis

1. **Table Format Incompatibility (`CBDT/CBLC` & `COLRv1`)**:
   - `NotoColorEmoji.ttf` from `googlefonts/noto-emoji` uses **CBDT/CBLC** (embedded PNG color bitmaps) and **COLRv1** (vector color gradient tables).
   - Linux font rendering in `gpui` uses `font-kit` + FreeType. FreeType reads `CBDT` PNGs if compiled with `libpng`, but GPUI's text pipeline (`TextSystem`) expects single-channel (A8 / R8) monochrome glyph alpha masks.

2. **GPUI Texture Atlas Constraints**:
   - GPUI's glyph atlas texture stores glyph masks in a 1-byte-per-pixel alpha format.
   - Standard text shaders sample the atlas to multiply foreground text color by alpha.
   - GPUI lacks an RGBA color glyph atlas texture format and COLRv1 shader pass for Linux/X11/Wayland. As a result, color bitmap or vector color table glyphs from `NotoColorEmoji.ttf` render as missing glyph boxes or empty spaces.

3. **Monochrome Vector Outlines (`NotoEmoji-Regular.ttf`)**:
   - Google Fonts publishes monochrome vector outline fonts (`NotoEmoji-Regular.ttf` / `NotoEmoji-VariableFont_wght.ttf`) in the same `noto-emoji` repository (`fonts/` directory).
   - These outline fonts use standard OpenType `glyf` / `CFF` tables.
   - FreeType rasterizes `glyf` outlines directly into GPUI's monochrome A8 atlas, rendering clean emoji glyphs matching the active UI text color (`theme::text()`).

---

## 3. Concrete Implementation Recipe

### Strategy: Bundle & Serve Monochrome Noto Emoji Outlines

#### Step 1: Obtain `NotoEmoji-Regular.ttf`
Download `NotoEmoji-Regular.ttf` from `https://github.com/googlefonts/noto-emoji/raw/main/fonts/NotoEmoji-Regular.ttf` and save to `crates/hadron-chamber/assets/fonts/NotoEmoji-Regular.ttf` (or register via `gpui-component-assets`).

#### Step 2: Register Font Family in `hadron-chamber`
In `crates/hadron-chamber/src/app/mod.rs` (around line 735), prepend `Noto Emoji` to `t.font_family`:

```rust
t.font_family = "Inter, Segoe UI, sans-serif, Noto Emoji, Noto Color Emoji, Apple Color Emoji, Segoe UI Emoji".into();
```

#### Step 3: Embed Font Assets
If running on systems where `Noto Emoji` is not installed system-wide via fontconfig, load font bytes into GPUI's asset loader or `FontSystem`:

```rust
cx.text_system().add_fonts(vec![
    std::sync::Arc::new(include_bytes!("../assets/fonts/NotoEmoji-Regular.ttf").to_vec())
]).ok();
```

---

## 4. Alternative for Full Color Emoji (Future Roadmap)

If full-color emoji rendering is strictly required in the future:
1. **Inline SVG / PNG Blitting**: Render emojis as small inline GPUI `img()` or SVG `Icon` elements using shortcodes from `emojis` crate (`emojis = "0.9.0"` in `Cargo.toml`).
2. **GPUI Color Atlas Upgrade**: Add an RGBA texture atlas format and multi-channel glyph rendering pipeline to `zed-industries/gpui`.
