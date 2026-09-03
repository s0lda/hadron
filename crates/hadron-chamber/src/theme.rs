//! The chamber's color system — **neutral dark surfaces floating on a black field**.
//! The housing is near-black; instrument panels use a restrained gray ladder, with state hues
//! reserved for status indicators rather than surface tint. The quark-state hues (blue =
//! working, purple = thinking, green = available, amber = waiting) survive only as a whisper
//! in the corners, so the field reads black but still has life.
//!
//! **What translucency is allowed — and what is NOT.** Under WSL the app software-renders
//! (llvmpipe, no GPU), so a repaint is expensive; the discipline is to keep repaints RARE,
//! not to ban translucency. Two things are the true CPU killers and are forbidden here:
//!   1. **Continuous animation** (a live `.with_animation`/repeating loop) — GPUI re-renders
//!      the whole window every frame it runs. There is none.
//!   2. **Blur** (backdrop-blur, blurred drop-shadows) — a large-kernel per-frame resample.
//!      There is none; "glass" here is alpha + a hairline sheen, never a blur of what's behind.
//! With those gone, repaints only happen on real change (or a throttled ~10fps while the
//! terminal streams), so alpha layers and static linear-gradient washes are affordable. GPUI
//! has no radial gradient, so the field is built from *layered* linear washes, not orbs.
//!
//! The full palette is exposed even where not yet consumed, so callers reach for the
//! named token, not a raw hex.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use gpui::{rgb, rgba, Hsla, Rgba};

use hadron_lattice::QuarkState;

static ACTIVE_PRESET: AtomicU8 = AtomicU8::new(0); // 0: Obsidian, 1: Oled, 2: Midnight, 3: Tokyo
static ACTIVE_ACCENT: AtomicU32 = AtomicU32::new(0xc084fc); // 0xRRGGBB (soft amethyst default)
static ACTIVE_CUSTOM_THEME: std::sync::RwLock<Option<ResolvedTheme>> = std::sync::RwLock::new(None);

/// Fully resolved, GPU-ready theme colors parsed from a `ThemeDefinition`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTheme {
    pub id: String,
    pub name: String,
    pub is_dark: bool,
    // surfaces
    pub canvas_base: Rgba,
    pub bg_base: Rgba,
    pub bg_surface: Rgba,
    pub bg_surface_raised: Rgba,
    pub bg_elevated: Rgba,
    pub input_bg: Rgba,
    pub border: Rgba,
    pub popover: Rgba,
    pub term_bg: Rgba,
    pub term_fg: Rgba,
    pub term_prompt: Rgba,
    // accents
    pub primary_accent: Rgba,
    pub glow_blue: Rgba,
    pub glow_pink: Rgba,
    pub glow_green: Rgba,
    pub glow_amber: Rgba,
    // text
    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    pub text_muted: Rgba,
    // syntax
    pub syntax_keyword: Rgba,
    pub syntax_function: Rgba,
    pub syntax_type: Rgba,
    pub syntax_string: Rgba,
    pub syntax_number: Rgba,
    pub syntax_comment: Rgba,
    pub syntax_operator: Rgba,
    pub syntax_variable: Rgba,
    pub syntax_constant: Rgba,
    pub syntax_attribute: Rgba,
    pub syntax_tag: Rgba,
    pub syntax_boolean: Rgba,
    pub syntax_delimiter: Rgba,
    pub syntax_punctuation: Rgba,
}

impl ResolvedTheme {
    pub fn from_definition(def: &crate::config::ThemeDefinition) -> Self {
        use crate::config::parse_hex_color;
        let c_base = parse_hex_color(&def.surfaces.canvas_base).unwrap_or_else(|| rgb(0x050505));
        let b_base = parse_hex_color(&def.surfaces.bg_base).unwrap_or_else(|| rgb(0x0b0b0b));
        let b_surf = parse_hex_color(&def.surfaces.bg_surface).unwrap_or_else(|| rgb(0x101010));
        let b_raised = parse_hex_color(&def.surfaces.bg_surface_raised).unwrap_or_else(|| rgb(0x1c1c1c));
        let b_elevated = parse_hex_color(&def.surfaces.bg_elevated).unwrap_or_else(|| rgb(0x242424));
        let in_bg = parse_hex_color(&def.surfaces.input_bg).unwrap_or_else(|| rgb(0x181818));
        let brd = parse_hex_color(&def.surfaces.border).unwrap_or_else(|| rgb(0x444444));
        let pop = parse_hex_color(&def.surfaces.popover).unwrap_or_else(|| rgb(0x101010));

        let t_bg = parse_hex_color(&def.terminal.bg).unwrap_or_else(|| rgb(0x080808));
        let t_fg = parse_hex_color(&def.terminal.fg).unwrap_or_else(|| rgb(0xe8e8e8));
        let t_prompt = parse_hex_color(&def.terminal.prompt).unwrap_or_else(|| rgb(0x4ade80));

        let p_accent = parse_hex_color(&def.accents.primary).unwrap_or_else(|| rgb(0xc084fc));
        let g_blue = parse_hex_color(&def.accents.glow_blue).unwrap_or_else(|| rgba(0x4f83f01c));
        let g_pink = parse_hex_color(&def.accents.glow_pink).unwrap_or_else(|| rgba(0xc084fc15));
        let g_green = parse_hex_color(&def.accents.glow_green).unwrap_or_else(|| rgba(0x2fcf8a1a));
        let g_amber = parse_hex_color(&def.accents.glow_amber).unwrap_or_else(|| rgba(0xfbbf241a));

        let txt_pri = parse_hex_color(&def.text.primary).unwrap_or_else(|| rgb(0xe8e8e8));
        let txt_sec = parse_hex_color(&def.text.secondary).unwrap_or_else(|| rgb(0xa8a8a8));
        let txt_mut = parse_hex_color(&def.text.muted).unwrap_or_else(|| rgb(0x707070));

        let syn_kw = parse_hex_color(&def.syntax.keyword).unwrap_or_else(|| rgb(0xf97583));
        let syn_fn = parse_hex_color(&def.syntax.function).unwrap_or_else(|| rgb(0xb392f0));
        let syn_ty = parse_hex_color(&def.syntax.r#type).unwrap_or_else(|| rgb(0x79b8ff));
        let syn_str = parse_hex_color(&def.syntax.string).unwrap_or_else(|| rgb(0x9ecbff));
        let syn_num = parse_hex_color(&def.syntax.number).unwrap_or_else(|| rgb(0x79b8ff));
        let syn_cmt = parse_hex_color(&def.syntax.comment).unwrap_or_else(|| rgb(0x7e888c));
        let syn_op = parse_hex_color(&def.syntax.operator).unwrap_or_else(|| rgb(0xf97583));
        let syn_var = parse_hex_color(&def.syntax.variable).unwrap_or_else(|| rgb(0xe1e4e8));
        let syn_cst = parse_hex_color(&def.syntax.constant).unwrap_or_else(|| rgb(0x79b8ff));
        let syn_att = parse_hex_color(&def.syntax.attribute).unwrap_or_else(|| rgb(0xb392f0));
        let syn_tag = parse_hex_color(&def.syntax.tag).unwrap_or_else(|| rgb(0x85e89d));
        let syn_bool = parse_hex_color(&def.syntax.boolean).unwrap_or_else(|| rgb(0x79b8ff));
        let syn_del = parse_hex_color(&def.syntax.delimiter).unwrap_or_else(|| rgb(0xf97583));
        let syn_punc = parse_hex_color(&def.syntax.punctuation).unwrap_or_else(|| rgb(0xbbbebf));

        Self {
            id: def.id.clone(),
            name: def.name.clone(),
            is_dark: def.is_dark,
            canvas_base: c_base,
            bg_base: b_base,
            bg_surface: b_surf,
            bg_surface_raised: b_raised,
            bg_elevated: b_elevated,
            input_bg: in_bg,
            border: brd,
            popover: pop,
            term_bg: t_bg,
            term_fg: t_fg,
            term_prompt: t_prompt,
            primary_accent: p_accent,
            glow_blue: g_blue,
            glow_pink: g_pink,
            glow_green: g_green,
            glow_amber: g_amber,
            text_primary: txt_pri,
            text_secondary: txt_sec,
            text_muted: txt_mut,
            syntax_keyword: syn_kw,
            syntax_function: syn_fn,
            syntax_type: syn_ty,
            syntax_string: syn_str,
            syntax_number: syn_num,
            syntax_comment: syn_cmt,
            syntax_operator: syn_op,
            syntax_variable: syn_var,
            syntax_constant: syn_cst,
            syntax_attribute: syn_att,
            syntax_tag: syn_tag,
            syntax_boolean: syn_bool,
            syntax_delimiter: syn_del,
            syntax_punctuation: syn_punc,
        }
    }
}

pub fn set_active_custom_theme(def: Option<crate::config::ThemeDefinition>) {
    let resolved = def.map(|d| ResolvedTheme::from_definition(&d));
    if let Ok(mut lock) = ACTIVE_CUSTOM_THEME.write() {
        *lock = resolved;
    }
}

pub fn active_custom_theme() -> Option<ResolvedTheme> {
    ACTIVE_CUSTOM_THEME.read().ok().and_then(|lock| lock.clone())
}

pub fn custom_themes_dir() -> Option<std::path::PathBuf> {
    Some(hadron_lattice::user_hadron_dir()?.join("themes"))
}

pub fn load_custom_themes() -> Vec<crate::config::ThemeDefinition> {
    let Some(dir) = custom_themes_dir() else {
        return Vec::new();
    };
    if !dir.exists() {
        return Vec::new();
    }
    let mut themes = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(theme) = serde_json::from_str::<crate::config::ThemeDefinition>(&content) {
                        themes.push(theme);
                    }
                }
            }
        }
    }
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    themes
}

pub fn save_custom_theme(theme: &crate::config::ThemeDefinition) -> std::io::Result<()> {
    let Some(dir) = custom_themes_dir() else {
        return Err(std::io::Error::other("Could not resolve user themes directory"));
    };
    std::fs::create_dir_all(&dir)?;
    let file_path = dir.join(format!("{}.json", theme.id));
    let json = serde_json::to_string_pretty(theme).map_err(std::io::Error::other)?;
    std::fs::write(file_path, json)
}

pub fn delete_custom_theme(id: &str) -> std::io::Result<()> {
    let Some(dir) = custom_themes_dir() else {
        return Ok(());
    };
    let file_path = dir.join(format!("{id}.json"));
    if file_path.exists() {
        std::fs::remove_file(file_path)?;
    }
    Ok(())
}

pub fn set_active_preset(preset: crate::config::ThemePreset) {
    let val = match preset {
        crate::config::ThemePreset::Obsidian => 0,
        crate::config::ThemePreset::Oled => 1,
        crate::config::ThemePreset::Midnight => 2,
        crate::config::ThemePreset::Tokyo => 3,
    };
    ACTIVE_PRESET.store(val, Ordering::Relaxed);
    // Clear custom theme override when selecting a curated preset
    set_active_custom_theme(None);
}

pub fn active_preset() -> crate::config::ThemePreset {
    match ACTIVE_PRESET.load(Ordering::Relaxed) {
        1 => crate::config::ThemePreset::Oled,
        2 => crate::config::ThemePreset::Midnight,
        3 => crate::config::ThemePreset::Tokyo,
        _ => crate::config::ThemePreset::Obsidian,
    }
}

pub fn set_active_accent(choice: crate::config::AccentChoice) {
    set_active_accent_rgb(choice.rgb());
}

pub fn set_active_accent_rgb(color: Rgba) {
    let r = (color.r * 255.0).round() as u32;
    let g = (color.g * 255.0).round() as u32;
    let b = (color.b * 255.0).round() as u32;
    let val = (r << 16) | (g << 8) | b;
    ACTIVE_ACCENT.store(val, Ordering::Relaxed);
}

pub fn active_accent() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        return custom.primary_accent;
    }
    let val = ACTIVE_ACCENT.load(Ordering::Relaxed);
    let r = ((val >> 16) & 0xff) as f32 / 255.0;
    let g = ((val >> 8) & 0xff) as f32 / 255.0;
    let b = (val & 0xff) as f32 / 255.0;
    Rgba { r, g, b, a: 1.0 }
}

// --- the ambient field: a flat black housing (the frosted-glass-on-black look) ---
/// Layer 0 (Canvas Base): Deep obsidian canvas base fill (`#050505`).
pub fn canvas_base() -> Hsla {
    if let Some(custom) = active_custom_theme() {
        custom.canvas_base.into()
    } else {
        palette_for_preset(active_preset()).canvas_base.into()
    }
}

/// The near-black base — the opaque tone painted behind the rounded corners and the dark
/// end of the field wash (`#050505`).
pub fn field_base() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.canvas_base
    } else {
        palette_for_preset(active_preset()).canvas_base
    }
}
/// The top of the field wash (`#050505`).
pub fn field_bright() -> Rgba {
    field_base()
}
/// The near-black the wash settles into at the bottom / behind the panels (`#050505`).
pub fn field_deep() -> Rgba {
    field_base()
}

/// The quark-state hues, kept as a faint corner whisper — the same palette the presence
/// dots use, so the black field still carries the colours of the swarm's states, but at a
/// low enough alpha that the backdrop reads black, not as an aurora. Each is anchored to
/// one corner (see `app.rs`).
pub fn glow_blue() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.glow_blue
    } else {
        rgba(0x4f83f01c) // working / excited — top-left
    }
}
pub fn glow_pink() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.glow_pink
    } else {
        rgba(0xc084fc15) // thinking / reasoning — top-right
    }
}
pub fn glow_green() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.glow_green
    } else {
        rgba(0x2fcf8a1a) // available — bottom-left
    }
}

// --- surfaces (recessed → raised) --- borderless glass surface hierarchy over cosmic obsidian base.
/// Recessed inner surface (deepest wells), the neutral panel tone (`#0b0b0b`).
pub fn bg_base() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.bg_base
    } else {
        palette_for_preset(active_preset()).bg_base
    }
}

/// The window/content backdrop token — the opaque housing behind the whole scene. (Kept
/// under the old name so the one call site that sets the root fill still reads a token.)
pub fn window_glint() -> Rgba {
    field_base()
}
/// A step-lighter neutral tone for lifted chrome and selected controls (`#242424`).
pub fn bg_elevated() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.bg_elevated
    } else {
        palette_for_preset(active_preset()).bg_elevated
    }
}

/// Layer 1 (Panels & Rails): Neutral dark panel layer (`#0b0b0bf2`).
pub fn glass_surface() -> Hsla {
    let base = bg_base();
    rgba(
        ((base.r * 255.0).round() as u32) << 24
            | ((base.g * 255.0).round() as u32) << 16
            | ((base.b * 255.0).round() as u32) << 8
            | 0xf2,
    )
    .into()
}

/// Tab bar background token matching main obsidian field (`#050505`).
pub fn tab_bar_bg() -> Rgba {
    canvas_base().into()
}

/// Layer 2 (Floating Cards & Modals): Elevated neutral dark cards (`#0b0b0bf8`).
pub fn glass_card() -> Hsla {
    let base = bg_base();
    rgba(
        ((base.r * 255.0).round() as u32) << 24
            | ((base.g * 255.0).round() as u32) << 16
            | ((base.b * 255.0).round() as u32) << 8
            | 0xf8,
    )
    .into()
}

/// Highlights / rims: Neutral border sheen (`rgba(96, 96, 96, 0.22)`).
pub fn glass_highlight() -> Hsla {
    rgba(0x60606038).into()
}

/// Subtle 1px hairline border for dark elevated cards (`rgba(255, 255, 255, 0.07)`).
pub fn hairline_border() -> Hsla {
    rgba(0xffffff12).into()
}

// --- vector status halo indicators ---
/// Active status halo (Soft Sapphire Blue `#60a5fa`) for tool execution / active state.
pub fn halo_active() -> Hsla {
    rgb(0x60a5fa).into()
}

/// Reasoning status halo (Soft Amethyst `#a78bfa`) for reasoning / thinking state.
pub fn halo_reasoning() -> Hsla {
    rgb(0xa78bfa).into()
}

/// Idle status halo (Soft Emerald `#34d399`) for ground / waiting / available state.
pub fn halo_idle() -> Hsla {
    rgb(0x34d399).into()
}

/// Error status halo (Soft Coral `#f87171`) for blocked / error state.
pub fn halo_error() -> Hsla {
    rgb(0xf87171).into()
}

/// Resolves the 8px GPU-native vector status halo indicator color for a given `QuarkState`.
pub fn halo_dot(state: QuarkState) -> Hsla {
    match state {
        QuarkState::Ground => halo_idle(),
        QuarkState::Excited => halo_active(),
        QuarkState::Thinking => halo_reasoning(),
        QuarkState::Waiting => halo_idle(),
        QuarkState::Blocked | QuarkState::Error => halo_error(),
    }
}

/// The fill for a **focused modal** (Settings card, Processes overlay, app menu) (`#101010`).
pub fn modal_surface() -> Rgba {
    bg_surface()
}

// --- terminal (a Zed-like screen) ---
/// The terminal screen surface — `#080808` main bg.
pub fn term_bg() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.term_bg
    } else {
        palette_for_preset(active_preset()).term_bg
    }
}
/// Default terminal output foreground — softened primary text (`#e8e8e8`).
pub fn term_fg() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.term_fg
    } else {
        rgb(0xe8e8e8)
    }
}
/// The shell prompt (`user@host: cwd$`) — the classic terminal green.
pub fn term_prompt() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.term_prompt
    } else {
        rgb(0x4ade80)
    }
}
pub fn bg_surface() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.bg_surface
    } else {
        palette_for_preset(active_preset()).bg_surface
    }
}
pub fn bg_surface_raised() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.bg_surface_raised
    } else {
        palette_for_preset(active_preset()).bg_surface_raised
    }
}
pub fn input_bg() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.input_bg
    } else {
        palette_for_preset(active_preset()).input_bg
    }
}
pub fn popover() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.popover
    } else {
        palette_for_preset(active_preset()).bg_surface
    }
}
pub fn border() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.border
    } else {
        palette_for_preset(active_preset()).border
    }
}

// --- text tiers ---
pub fn text() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.text_primary
    } else {
        rgb(0xe8e8e8)
    }
}
pub fn text_secondary() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.text_secondary
    } else {
        rgb(0xa8a8a8)
    }
}
pub fn text_muted() -> Rgba {
    if let Some(custom) = active_custom_theme() {
        custom.text_muted
    } else {
        rgb(0x707070)
    }
}

// --- GitHub Semantic Syntax Tokens ---
pub fn syntax_attribute() -> Rgba { active_custom_theme().map(|t| t.syntax_attribute).unwrap_or_else(|| rgb(0xb392f0)) }
pub fn syntax_boolean() -> Rgba { active_custom_theme().map(|t| t.syntax_boolean).unwrap_or_else(|| rgb(0x79b8ff)) }
pub fn syntax_comment() -> Rgba { active_custom_theme().map(|t| t.syntax_comment).unwrap_or_else(|| rgb(0x7e888c)) }
pub fn syntax_constant() -> Rgba { active_custom_theme().map(|t| t.syntax_constant).unwrap_or_else(|| rgb(0x79b8ff)) }
pub fn syntax_constructor() -> Rgba { active_custom_theme().map(|t| t.syntax_function).unwrap_or_else(|| rgb(0xb392f0)) }
pub fn syntax_embedded() -> Rgba { active_custom_theme().map(|t| t.text_primary).unwrap_or_else(|| rgb(0xe6edf3)) }
pub fn syntax_emphasis() -> Rgba { active_custom_theme().map(|t| t.syntax_variable).unwrap_or_else(|| rgb(0xe1e4e8)) }
pub fn syntax_enum() -> Rgba { active_custom_theme().map(|t| t.syntax_type).unwrap_or_else(|| rgb(0x79b8ff)) }
pub fn syntax_function() -> Rgba { active_custom_theme().map(|t| t.syntax_function).unwrap_or_else(|| rgb(0xb392f0)) }
pub fn syntax_hint() -> Rgba { active_custom_theme().map(|t| t.syntax_comment).unwrap_or_else(|| rgb(0x7e888c)) }
pub fn syntax_keyword() -> Rgba { active_custom_theme().map(|t| t.syntax_keyword).unwrap_or_else(|| rgb(0xf97583)) }
pub fn syntax_label() -> Rgba { active_custom_theme().map(|t| t.syntax_function).unwrap_or_else(|| rgb(0xb392f0)) }
pub fn syntax_link_text() -> Rgba { active_custom_theme().map(|t| t.primary_accent).unwrap_or_else(|| rgb(0x48a0c7)) }
pub fn syntax_link_uri() -> Rgba { active_custom_theme().map(|t| t.syntax_string).unwrap_or_else(|| rgb(0x9ecbff)) }
pub fn syntax_number() -> Rgba { active_custom_theme().map(|t| t.syntax_number).unwrap_or_else(|| rgb(0x79b8ff)) }
pub fn syntax_operator() -> Rgba { active_custom_theme().map(|t| t.syntax_operator).unwrap_or_else(|| rgb(0xf97583)) }
pub fn syntax_predictive() -> Rgba { active_custom_theme().map(|t| t.text_muted).unwrap_or_else(|| rgb(0x555555)) }
pub fn syntax_preproc() -> Rgba { active_custom_theme().map(|t| t.syntax_keyword).unwrap_or_else(|| rgb(0xf97583)) }
pub fn syntax_primary() -> Rgba { active_custom_theme().map(|t| t.syntax_punctuation).unwrap_or_else(|| rgb(0xbbbebf)) }
pub fn syntax_property() -> Rgba { active_custom_theme().map(|t| t.syntax_type).unwrap_or_else(|| rgb(0x79b8ff)) }
pub fn syntax_punctuation() -> Rgba { active_custom_theme().map(|t| t.syntax_punctuation).unwrap_or_else(|| rgb(0xbbbebf)) }
pub fn syntax_delimiter() -> Rgba { active_custom_theme().map(|t| t.syntax_delimiter).unwrap_or_else(|| rgb(0xf97583)) }
pub fn syntax_list_marker() -> Rgba { active_custom_theme().map(|t| t.syntax_tag).unwrap_or_else(|| rgb(0x85e89d)) }
pub fn syntax_special() -> Rgba { active_custom_theme().map(|t| t.syntax_keyword).unwrap_or_else(|| rgb(0xf97583)) }
pub fn syntax_string() -> Rgba { active_custom_theme().map(|t| t.syntax_string).unwrap_or_else(|| rgb(0x9ecbff)) }
pub fn syntax_string_escape() -> Rgba { active_custom_theme().map(|t| t.syntax_tag).unwrap_or_else(|| rgb(0x85e89d)) }
pub fn syntax_tag() -> Rgba { active_custom_theme().map(|t| t.syntax_tag).unwrap_or_else(|| rgb(0x85e89d)) }
pub fn syntax_literal() -> Rgba { active_custom_theme().map(|t| t.syntax_tag).unwrap_or_else(|| rgb(0x85e89d)) }
pub fn syntax_title() -> Rgba { active_custom_theme().map(|t| t.syntax_type).unwrap_or_else(|| rgb(0x79b8ff)) }
pub fn syntax_type() -> Rgba { active_custom_theme().map(|t| t.syntax_type).unwrap_or_else(|| rgb(0x79b8ff)) }
pub fn syntax_variable() -> Rgba { active_custom_theme().map(|t| t.syntax_variable).unwrap_or_else(|| rgb(0xe1e4e8)) }
pub fn syntax_variable_special() -> Rgba { active_custom_theme().map(|t| t.primary_accent).unwrap_or_else(|| rgb(0xffab70)) }
pub fn syntax_variant() -> Rgba { active_custom_theme().map(|t| t.syntax_type).unwrap_or_else(|| rgb(0x79b8ff)) }

/// Semantic color for file category icons in the file tree and workspace viewers.
pub fn file_icon_color_for_path(path: &str) -> Rgba {
    let p = std::path::Path::new(path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let name = p.file_name().and_then(|f| f.to_str()).unwrap_or("").to_lowercase();

    match ext.as_str() {
        // Code / Logic
        "rs" | "go" | "c" | "cpp" | "cc" | "h" | "hpp" | "zig" => rgb(0xb392f0),
        "ts" | "tsx" | "js" | "jsx" | "py" | "java" | "kt" | "swift" | "rb" | "php" | "lua" => rgb(0x79b8ff),
        "sh" | "bash" | "zsh" => rgb(0xf97583),
        // Prose / Docs
        "md" | "markdown" | "txt" | "org" | "adoc" | "rst" => rgb(0x85e89d),
        // Config / Data
        "json" | "toml" | "yaml" | "yml" | "xml" | "ini" | "env" | "lock" | "csv" | "sql" => rgb(0xffab70),
        // Web / Styling
        "html" | "htm" | "css" | "scss" | "sass" | "less" => rgb(0xf97583),
        // Media / Assets / Binary
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" | "wasm" | "ttf" | "woff" | "woff2" => rgb(0x48a0c7),
        _ => {
            if name == "dockerfile" || name == "makefile" || name == "cargo.lock" {
                rgb(0xffab70)
            } else {
                rgb(0xbbbebf)
            }
        }
    }
}

/// Accent color gradient and token ramp.
pub fn accent() -> Rgba {
    active_accent()
}

/// Palette tokens resolved from a curated ThemePreset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresetPalette {
    pub canvas_base: Rgba,
    pub bg_base: Rgba,
    pub bg_surface: Rgba,
    pub bg_surface_raised: Rgba,
    pub bg_elevated: Rgba,
    pub input_bg: Rgba,
    pub term_bg: Rgba,
    pub border: Rgba,
}

pub fn palette_for_preset(preset: crate::config::ThemePreset) -> PresetPalette {
    match preset {
        crate::config::ThemePreset::Obsidian => PresetPalette {
            canvas_base: rgb(0x050505),
            bg_base: rgb(0x0b0b0b),
            bg_surface: rgb(0x101010),
            bg_surface_raised: rgb(0x1c1c1c),
            bg_elevated: rgb(0x242424),
            input_bg: rgb(0x181818),
            term_bg: rgb(0x080808),
            border: rgb(0x444444),
        },
        crate::config::ThemePreset::Oled => PresetPalette {
            canvas_base: rgb(0x000000),
            bg_base: rgb(0x050505),
            bg_surface: rgb(0x0a0a0a),
            bg_surface_raised: rgb(0x141414),
            bg_elevated: rgb(0x1a1a1a),
            input_bg: rgb(0x101010),
            term_bg: rgb(0x000000),
            border: rgb(0x383838),
        },
        crate::config::ThemePreset::Midnight => PresetPalette {
            canvas_base: rgb(0x090d16),
            bg_base: rgb(0x0f172a),
            bg_surface: rgb(0x1e293b),
            bg_surface_raised: rgb(0x283548),
            bg_elevated: rgb(0x334155),
            input_bg: rgb(0x172033),
            term_bg: rgb(0x0b1120),
            border: rgb(0x475569),
        },
        crate::config::ThemePreset::Tokyo => PresetPalette {
            canvas_base: rgb(0x0d0f18),
            bg_base: rgb(0x131622),
            bg_surface: rgb(0x1a1e2e),
            bg_surface_raised: rgb(0x24293e),
            bg_elevated: rgb(0x2f354f),
            input_bg: rgb(0x181c2b),
            term_bg: rgb(0x0f121d),
            border: rgb(0x414868),
        },
    }
}
/// A muted, low-alpha amethyst for chrome that should whisper rather than shout —
/// the focused chat-input border.
pub fn accent_soft() -> Rgba {
    active_accent().opacity(0.40)
}
pub fn accent_secondary() -> Rgba {
    rgb(0xa855f7) // purple — thinking
}
pub fn danger() -> Rgba {
    rgb(0xef4444) // red — close-button hover
}
/// Markdown link colour in chat — light blue (sky), distinct from the amethyst accent.
pub fn link() -> Rgba {
    rgb(0x7dd3fc) // sky-300
}
pub fn link_hover() -> Rgba {
    rgb(0xbae6fd) // sky-200 — brighter on hover
}
pub fn link_active() -> Rgba {
    rgb(0x38bdf8) // sky-400 — darker while pressed
}

/// Roster chip color for a quark's lifecycle state (the status ramp).
pub fn quark_state(state: QuarkState) -> Rgba {
    match state {
        QuarkState::Ground => rgb(0x9ca2ad),   // neutral grey
        QuarkState::Excited => rgb(0xc084fc),  // soft amethyst — active
        QuarkState::Thinking => rgb(0xa855f7), // purple
        QuarkState::Waiting => rgb(0xfbbf24),  // amber
        QuarkState::Blocked | QuarkState::Error => rgb(0xf87171), // red
    }
}

/// Presence-dot color for the roster user-list — Jake's traffic-light semantics
/// (green available · blue working · amber waiting · red unavailable), distinct
/// from the [`quark_state`] accent ramp used elsewhere.
pub fn presence(state: QuarkState) -> Rgba {
    match state {
        QuarkState::Ground => rgb(0x22c55e), // green — available
        QuarkState::Excited | QuarkState::Thinking => rgb(0x3b82f6), // blue — working
        QuarkState::Waiting => rgb(0xf59e0b), // amber — waiting on a decision
        QuarkState::Blocked | QuarkState::Error => rgb(0xef4444), // red — unavailable
    }
}

pub fn presence_disabled() -> Rgba {
    rgb(0x71717a) // zinc-500 gray
}

/// One-word presence label matching [`presence`], for tooltips/subtitles.
pub fn presence_label(state: QuarkState) -> &'static str {
    match state {
        QuarkState::Ground => "available",
        QuarkState::Excited | QuarkState::Thinking => "working",
        QuarkState::Waiting => "waiting",
        QuarkState::Blocked | QuarkState::Error => "unavailable",
    }
}

/// The auto-assignment palette: distinct, legible hues a quark cycles through by name
/// when it has no custom colour. Exposed so a colour picker can offer them as presets.
pub const AUTO_PALETTE: [u32; 12] = [
    0x60a5fa, // soft sapphire blue
    0x34d399, // soft mint emerald
    0xa78bfa, // soft amethyst
    0xf59e0b, // soft warm amber
    0xf87171, // soft coral rose
    0x94a3b8, // cool slate
    0x2dd4bf, // soft ice teal
    0x10b981, // muted sage green
    0xf97316, // soft terracotta orange
    0x818cf8, // soft dusk indigo
    0xc084fc, // soft orchid
    0x9333ea, // muted royal purple
];

/// A stable hue for a chat author's header, so who-said-what scans at a glance.
/// Human/gluon are fixed; quarks cycle through [`AUTO_PALETTE`] by name. This is the
/// **fallback** — a quark with a custom colour resolves to that instead (see
/// `ChamberView`/`Chamber::color_for`).
pub fn actor_hue(actor: &str) -> Rgba {
    match actor {
        "human" => rgb(0xf5f5f6), // bright — the human
        "gluon" => rgb(0x60a5fa), // info blue — the system
        other => {
            let idx = (other.bytes().fold(0u32, |a, b| a.wrapping_add(b as u32)) as usize)
                % AUTO_PALETTE.len();
            rgb(AUTO_PALETTE[idx])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{rgb, rgba, Hsla};

    #[test]
    fn test_canvas_base_token() {
        set_active_preset(crate::config::ThemePreset::Obsidian);
        let base = canvas_base();
        let expected: Hsla = rgb(0x050505).into();
        assert_eq!(base, expected);
        assert_eq!(base.a, 1.0);
    }

    #[test]
    fn test_glass_surface_token() {
        let surface = glass_surface();
        let expected: Hsla = rgba(0x0b0b0bf2).into();
        assert_eq!(surface, expected);
    }

    #[test]
    fn test_tab_bar_bg_token() {
        let bg = tab_bar_bg();
        let expected = rgb(0x050505);
        assert_eq!(bg, expected);
    }

    #[test]
    fn test_glass_card_token() {
        let card = glass_card();
        let expected: Hsla = rgba(0x0b0b0bf8).into();
        assert_eq!(card, expected);
    }

    #[test]
    fn test_neutral_palette_tokens() {
        assert_eq!(modal_surface(), rgb(0x101010));
        assert_eq!(bg_surface(), rgb(0x101010));
        assert_eq!(popover(), rgb(0x101010));
        assert_eq!(input_bg(), rgb(0x181818));
        assert_eq!(bg_surface_raised(), rgb(0x1c1c1c));
        assert_eq!(border(), rgb(0x444444));
        assert_eq!(text(), rgb(0xe8e8e8));
        assert_eq!(term_fg(), rgb(0xe8e8e8));
        assert_eq!(text_secondary(), rgb(0xa8a8a8));
        assert_eq!(text_muted(), rgb(0x707070));
    }

    #[test]
    fn test_neutral_surface_tokens_have_no_color_tint() {
        for surface in [
            modal_surface(),
            bg_surface(),
            popover(),
            input_bg(),
            bg_surface_raised(),
            border(),
            field_base(),
            bg_base(),
        ] {
            let r = (surface.r * 255.0).round() as u8;
            let g = (surface.g * 255.0).round() as u8;
            let b = (surface.b * 255.0).round() as u8;
            assert_eq!(r, g, "Red and Green must match on neutral surfaces");
            assert_eq!(g, b, "Green and Blue must match on neutral surfaces");
        }
    }

    #[test]
    fn test_glass_highlight_token() {
        let highlight = glass_highlight();
        let expected: Hsla = rgba(0x60606038).into();
        assert_eq!(highlight, expected);
    }

    #[test]
    fn test_halo_color_tokens() {
        assert_eq!(halo_active(), Hsla::from(rgb(0x60a5fa)));
        assert_eq!(halo_reasoning(), Hsla::from(rgb(0xa78bfa)));
        assert_eq!(halo_idle(), Hsla::from(rgb(0x34d399)));
        assert_eq!(halo_error(), Hsla::from(rgb(0xf87171)));
    }

    #[test]
    fn test_halo_dot_mapping() {
        assert_eq!(halo_dot(QuarkState::Excited), halo_active());
        assert_eq!(halo_dot(QuarkState::Thinking), halo_reasoning());
        assert_eq!(halo_dot(QuarkState::Ground), halo_idle());
        assert_eq!(halo_dot(QuarkState::Waiting), halo_idle());
        assert_eq!(halo_dot(QuarkState::Blocked), halo_error());
        assert_eq!(halo_dot(QuarkState::Error), halo_error());
    }

    #[test]
    fn test_glass_elevation_hierarchy() {
        let base = canvas_base();
        let surface = glass_surface();
        let card = glass_card();

        assert_ne!(base, surface, "Canvas base and glass surface must be distinct");
        assert_ne!(surface, card, "Glass surface and glass card must be distinct");
        assert_ne!(base, card, "Canvas base and glass card must be distinct");
    }

    #[test]
    fn test_halo_colors_mutually_distinct() {
        let active = halo_active();
        let reasoning = halo_reasoning();
        let idle = halo_idle();
        let error = halo_error();

        assert_ne!(active, reasoning);
        assert_ne!(active, idle);
        assert_ne!(active, error);
        assert_ne!(reasoning, idle);
        assert_ne!(reasoning, error);
        assert_ne!(idle, error);
    }

    #[test]
    fn test_translucency_invariants() {
        assert_eq!(canvas_base().a, 1.0, "Canvas base must be opaque");
        assert!(
            glass_surface().a > 0.8 && glass_surface().a <= 1.0,
            "Glass surface must be ~85-95% opacity"
        );
        assert!(
            glass_card().a > 0.8 && glass_card().a <= 1.0,
            "Glass card must be ~85-95% opacity"
        );
        assert!(
            glass_highlight().a < 0.25,
            "Glass highlight rim must be low-alpha white (<25%)"
        );
        assert!(
            hairline_border().a < 0.20,
            "Hairline border must be subtle (<20%)"
        );
        // The dropdown/menu surface. The fork's `Select` fills its popup from a
        // theme token, so anything less than fully opaque here lets the panel
        // underneath read through the open model list.
        assert_eq!(popover().a, 1.0, "Popover surface must be opaque");
        assert_eq!(modal_surface().a, 1.0, "Modal surface must be opaque");
    }

    #[test]
    fn test_theme_presets_have_distinct_palettes() {
        let obsidian = palette_for_preset(crate::config::ThemePreset::Obsidian);
        let oled = palette_for_preset(crate::config::ThemePreset::Oled);
        let midnight = palette_for_preset(crate::config::ThemePreset::Midnight);
        let tokyo = palette_for_preset(crate::config::ThemePreset::Tokyo);

        assert_ne!(obsidian.canvas_base, oled.canvas_base);
        assert_ne!(obsidian.bg_base, midnight.bg_base);
        assert_ne!(midnight.bg_surface, tokyo.bg_surface);
        assert_eq!(oled.canvas_base, rgb(0x000000));
    }

    static THEME_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_dynamic_preset_and_accent_switching() {
        let _guard = THEME_TEST_MUTEX.lock().unwrap();
        set_active_preset(crate::config::ThemePreset::Obsidian);
        set_active_accent(crate::config::AccentChoice::Amethyst);
        assert_eq!(active_preset(), crate::config::ThemePreset::Obsidian);
        assert_eq!(accent(), rgb(0xc084fc));

        set_active_preset(crate::config::ThemePreset::Oled);
        assert_eq!(active_preset(), crate::config::ThemePreset::Oled);
        assert_eq!(canvas_base(), rgb(0x000000).into());
        assert_eq!(bg_base(), rgb(0x050505));

        set_active_accent(crate::config::AccentChoice::Emerald);
        assert_eq!(accent(), rgb(0x34d399));

        // Restore default obsidian for subsequent tests
        set_active_preset(crate::config::ThemePreset::Obsidian);
        set_active_accent(crate::config::AccentChoice::Amethyst);
        set_active_custom_theme(None);
    }

    #[test]
    fn test_github_semantic_syntax_tokens() {
        let _guard = THEME_TEST_MUTEX.lock().unwrap();
        assert_eq!(syntax_function(), rgb(0xb392f0));
        assert_eq!(syntax_keyword(), rgb(0xf97583));
        assert_eq!(syntax_type(), rgb(0x79b8ff));
        assert_eq!(syntax_string(), rgb(0x9ecbff));
        assert_eq!(syntax_literal(), rgb(0x85e89d));
        assert_eq!(syntax_comment(), rgb(0x7e888c));
        assert_eq!(syntax_variable(), rgb(0xe1e4e8));
        assert_eq!(syntax_variable_special(), rgb(0xffab70));
    }

    #[test]
    fn test_file_icon_color_for_path() {
        assert_eq!(file_icon_color_for_path("src/main.rs"), rgb(0xb392f0));
        assert_eq!(file_icon_color_for_path("frontend/app.ts"), rgb(0x79b8ff));
        assert_eq!(file_icon_color_for_path("README.md"), rgb(0x85e89d));
        assert_eq!(file_icon_color_for_path("Cargo.toml"), rgb(0xffab70));
        assert_eq!(file_icon_color_for_path("package.json"), rgb(0xffab70));
        assert_eq!(file_icon_color_for_path("assets/logo.png"), rgb(0x48a0c7));
        assert_eq!(file_icon_color_for_path("styles/main.css"), rgb(0xf97583));
        assert_eq!(file_icon_color_for_path("random.unknown"), rgb(0xbbbebf));
    }

    #[test]
    fn test_custom_theme_switching_and_token_override() {
        let _guard = THEME_TEST_MUTEX.lock().unwrap();
        let mut custom = crate::config::ThemeDefinition::preset_obsidian();
        custom.id = "custom-neon".to_string();
        custom.name = "Neon Cyber".to_string();
        custom.surfaces.canvas_base = "#120024".to_string();
        custom.surfaces.bg_surface = "#1e003a".to_string();
        custom.accents.primary = "#00ffcc".to_string();
        custom.syntax.keyword = "#ff007f".to_string();
        custom.syntax.function = "#00e5ff".to_string();

        set_active_custom_theme(Some(custom));

        assert_eq!(canvas_base(), rgb(0x120024).into());
        assert_eq!(bg_surface(), rgb(0x1e003a));
        assert_eq!(accent(), rgb(0x00ffcc));
        assert_eq!(syntax_keyword(), rgb(0xff007f));
        assert_eq!(syntax_function(), rgb(0x00e5ff));

        // Reverting to preset clears custom theme override
        set_active_preset(crate::config::ThemePreset::Obsidian);
        assert_eq!(active_custom_theme(), None);
        assert_eq!(canvas_base(), rgb(0x050505).into());
        assert_eq!(syntax_keyword(), rgb(0xf97583));
    }
}

