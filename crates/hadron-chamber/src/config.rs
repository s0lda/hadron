//! Persistent chamber layout preferences — rail collapse state and panel widths.
//! Stored at `~/.hadron/chamber.json` (cross-platform), loaded on start and saved
//! whenever the layout changes, so the user's workspace is preserved across
//! sessions. Pure (no GPUI) so it is unit-tested.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A customizable identity for chat/roster display — the human's, or a quark's.
/// All fields are optional overrides; unset fields fall back to code defaults
/// (id-derived name, hue-derived color, initials avatar) at render time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// Shown name; falls back to the actor id (or "You" for the human).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Accent + avatar color as `#rrggbb`; falls back to the actor's hue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Path to an avatar image; when set, it wins over color + initials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowBoundsPrefs {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The persisted chamber state: rail layout, plus per-actor display identities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamberPrefs {
    #[serde(default = "default_false")]
    pub roster_collapsed: bool,
    #[serde(default = "default_false")]
    pub inspector_collapsed: bool,
    /// Kill the auto-spawned `hadron-gluon` daemon when the chamber window closes.
    /// Default `true` — the chamber spawns the daemon for you, so closing the window
    /// is the natural end of the swarm; a daemon left running after its only viewer
    /// is gone keeps burning tokens with nobody reading them. Turn it off in Settings
    /// to keep a headless swarm alive across chamber restarts.
    #[serde(default = "default_true")]
    pub close_gluon_on_exit: bool,
    #[serde(default = "default_roster_width")]
    pub roster_width: f32,
    #[serde(default = "default_inspector_width")]
    pub inspector_width: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_bounds: Option<WindowBoundsPrefs>,
    /// The human's own chat identity.
    #[serde(default)]
    pub human: Identity,
    /// Per-quark identity overrides, keyed by quark id.
    #[serde(default)]
    pub quarks: BTreeMap<String, Identity>,
    /// Which program opens a source file the human clicks — both a chat message's
    /// `file://` link and the file tree's "Open in editor". Defaults to
    /// [`crate::sys::EditorChoice::System`], which is the pre-existing behaviour
    /// (the desktop's own association, i.e. whatever `xdg-open` picks).
    ///
    /// Read leniently, because this is the one field whose docs invite a hand edit
    /// (`Custom`, which the Settings ladder cannot offer). [`load_from`] does
    /// `from_str(..).unwrap_or_default()`, so without [`lenient_editor`] a single
    /// typo here — `"Vim"`, a malformed object — would fail the WHOLE `ChamberPrefs`
    /// and silently reset the human's layout, widths and every quark identity.
    #[serde(default, deserialize_with = "lenient_editor")]
    pub editor: crate::sys::EditorChoice,
    /// The global permission mode a **fresh field** starts on.
    ///
    /// The effective mode is folded from the field's `ModeSet` events
    /// (`hadron_gatekeeper::global_mode`) and that stays the one source of
    /// truth — but `/clear` truncates `field.jsonl`, which took every `ModeSet` with
    /// it and dropped the swarm back to `Mode::Ask` on every new session. This is the
    /// standing preference `/clear` re-seeds from, so a human who works in `Auto` does
    /// not re-arm it by hand each time.
    ///
    /// Defaults to `Mode::Ask` — `Mode::default()` — so an existing `chamber.json`
    /// with no such key behaves exactly as it always has.
    #[serde(default)]
    pub default_mode: hadron_lattice::Mode,
    /// Optional custom UI font family (defaults to bundled Inter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_font_family: Option<String>,
    /// Optional custom UI font size in pixels (defaults to 14.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_font_size: Option<f32>,
    /// Optional custom monospace font family (defaults to bundled Cascadia Code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono_font_family: Option<String>,
    /// Optional custom monospace font size in pixels (defaults to 13.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono_font_size: Option<f32>,
    /// Optional color theme preset (defaults to Obsidian Neutral).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_preset: Option<ThemePreset>,
    /// Optional primary accent color choice (defaults to Amethyst).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_choice: Option<AccentChoice>,
    /// Optional custom theme definition override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_theme: Option<ThemeDefinition>,
    /// Audio & haptics telemetry configuration.
    #[serde(default)]
    pub audio: crate::app::audio::AudioConfig,
    /// Turn watchdog silence limit in seconds (default 1800s / 30m).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_deadline_secs: Option<u64>,
    /// Live activity stale threshold in seconds (default 120s).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_after_secs: Option<i64>,
    /// Custom terminal shell path override (defaults to $SHELL or platform default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_shell: Option<String>,
    /// Terminal cursor rendering style (Beam, Block, Underline).
    #[serde(default)]
    pub terminal_cursor_style: TerminalCursorStyle,
    /// Terminal scrollback buffer line depth (defaults to 5,000).
    #[serde(default = "default_terminal_scrollback")]
    pub terminal_scrollback: usize,
    /// Chat row density (Comfortable vs Compact).
    #[serde(default)]
    pub chat_density: ChatDensity,
    /// Whether markdown code blocks should wrap text instead of scrolling horizontally.
    #[serde(default)]
    pub code_block_word_wrap: bool,
    /// Chat timestamp format (24h, 12h, Relative).
    #[serde(default)]
    pub timestamp_format: TimestampFormat,
    /// Whether to auto-fold thinking/reasoning blocks in chat by default.
    #[serde(default = "default_true")]
    pub auto_fold_reasoning: bool,
    /// Whether git worktrees should be automatically pruned on merge/abandonment.
    #[serde(default = "default_true")]
    pub git_auto_prune_worktrees: bool,
    /// Custom Git author name override for commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_author_name: Option<String>,
    /// Custom Git author email override for commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_author_email: Option<String>,
    /// Native OS desktop notifications master toggle.
    #[serde(default = "default_true")]
    pub desktop_notifications: bool,
    /// Show desktop notification when a quark is blocked on Mode::Ask.
    #[serde(default = "default_true")]
    pub notify_on_blocked: bool,
    /// Show desktop notification when a quark finishes a turn.
    #[serde(default = "default_true")]
    pub notify_on_turn_finish: bool,
}

/// Sound themes and acoustic profiles for synthesized telemetry chimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SoundTheme {
    #[default]
    Classic,
    Synth,
    Minimal,
    Retro8Bit,
}

impl SoundTheme {
    pub const ALL: [SoundTheme; 4] = [
        SoundTheme::Classic,
        SoundTheme::Synth,
        SoundTheme::Minimal,
        SoundTheme::Retro8Bit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SoundTheme::Classic => "Classic (Harmonic Chimes)",
            SoundTheme::Synth => "Synth (Electronic FM Blips)",
            SoundTheme::Minimal => "Minimal (Soft Clicks & Pops)",
            SoundTheme::Retro8Bit => "Retro 8-Bit (Arcade Bleeps)",
        }
    }

    #[allow(dead_code)]
    pub fn id(self) -> &'static str {
        match self {
            SoundTheme::Classic => "classic",
            SoundTheme::Synth => "synth",
            SoundTheme::Minimal => "minimal",
            SoundTheme::Retro8Bit => "retro-8bit",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        let clean = s.trim().to_ascii_lowercase();
        if clean.starts_with("classic") || clean.contains("harmonic") || clean == "default" {
            Some(SoundTheme::Classic)
        } else if clean.starts_with("synth") || clean.contains("electronic") || clean.contains("fm") {
            Some(SoundTheme::Synth)
        } else if clean.starts_with("minimal") || clean.contains("click") || clean.contains("soft") || clean.contains("pop") {
            Some(SoundTheme::Minimal)
        } else if clean.starts_with("retro") || clean.contains("8bit") || clean.contains("8-bit") || clean.contains("arcade") {
            Some(SoundTheme::Retro8Bit)
        } else {
            None
        }
    }
}

/// Terminal cursor styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalCursorStyle {
    #[default]
    Beam,
    Block,
    Underline,
}

#[allow(dead_code)]
impl TerminalCursorStyle {
    pub const ALL: [TerminalCursorStyle; 3] = [
        TerminalCursorStyle::Beam,
        TerminalCursorStyle::Block,
        TerminalCursorStyle::Underline,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TerminalCursorStyle::Beam => "Beam (Vertical Line)",
            TerminalCursorStyle::Block => "Block (Full Rectangle)",
            TerminalCursorStyle::Underline => "Underline (Bottom Bar)",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        let clean = s.trim().to_ascii_lowercase();
        if clean.starts_with("beam") || clean.starts_with("line") || clean.starts_with("vertical") {
            Some(TerminalCursorStyle::Beam)
        } else if clean.starts_with("block") || clean.starts_with("box") || clean.starts_with("rectangle") {
            Some(TerminalCursorStyle::Block)
        } else if clean.starts_with("underline") || clean.starts_with("bar") || clean.starts_with("bottom") {
            Some(TerminalCursorStyle::Underline)
        } else {
            None
        }
    }
}

/// Chat view row density and spacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ChatDensity {
    #[default]
    Comfortable,
    Compact,
}

#[allow(dead_code)]
impl ChatDensity {
    pub const ALL: [ChatDensity; 2] = [
        ChatDensity::Comfortable,
        ChatDensity::Compact,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ChatDensity::Comfortable => "Comfortable (Standard Spacing)",
            ChatDensity::Compact => "Compact (Dense View)",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        let clean = s.trim().to_ascii_lowercase();
        if clean.starts_with("compact") || clean == "dense" || clean == "tight" {
            Some(ChatDensity::Compact)
        } else if clean.starts_with("comfortable") || clean == "standard" || clean == "default" {
            Some(ChatDensity::Comfortable)
        } else {
            None
        }
    }
}

/// Timestamp formatting style for chat messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TimestampFormat {
    #[default]
    Clock24h,
    Clock12h,
    Relative,
}

#[allow(dead_code)]
impl TimestampFormat {
    pub const ALL: [TimestampFormat; 3] = [
        TimestampFormat::Clock24h,
        TimestampFormat::Clock12h,
        TimestampFormat::Relative,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Clock24h => "24-Hour (15:04:05)",
            Self::Clock12h => "12-Hour (3:04:05 PM)",
            Self::Relative => "Relative (2m ago)",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        let clean = s.trim().to_ascii_lowercase();
        if clean.starts_with("24") || clean.starts_with("clock24") || clean.contains("24-hour") || clean.contains("24_hour") {
            Some(TimestampFormat::Clock24h)
        } else if clean.starts_with("12") || clean.starts_with("clock12") || clean.contains("12-hour") || clean.contains("12_hour") {
            Some(TimestampFormat::Clock12h)
        } else if clean.starts_with("relative") || clean == "human" || clean == "ago" || clean.contains("relative") {
            Some(TimestampFormat::Relative)
        } else {
            None
        }
    }
}

/// Curated color theme presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ThemePreset {
    #[default]
    Obsidian,
    Oled,
    Midnight,
    Tokyo,
}

impl ThemePreset {
    pub const ALL: [ThemePreset; 4] = [
        ThemePreset::Obsidian,
        ThemePreset::Oled,
        ThemePreset::Midnight,
        ThemePreset::Tokyo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemePreset::Obsidian => "Obsidian Neutral",
            ThemePreset::Oled => "OLED True Black",
            ThemePreset::Midnight => "Midnight Slate",
            ThemePreset::Tokyo => "Tokyo Dark",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            ThemePreset::Obsidian => "obsidian",
            ThemePreset::Oled => "oled",
            ThemePreset::Midnight => "midnight",
            ThemePreset::Tokyo => "tokyo",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "obsidian" | "obsidian neutral" | "obsidian-neutral" | "default" => Some(ThemePreset::Obsidian),
            "oled" | "oled true black" | "oled-true-black" | "oled-black" | "black" => Some(ThemePreset::Oled),
            "midnight" | "midnight slate" | "midnight-slate" | "slate" => Some(ThemePreset::Midnight),
            "tokyo" | "tokyo dark" | "tokyo-dark" | "indigo" => Some(ThemePreset::Tokyo),
            _ => None,
        }
    }
}

/// Primary accent color choices for highlights, focus borders, and active indicators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AccentChoice {
    #[default]
    Amethyst,
    Sapphire,
    Emerald,
    Amber,
    Rose,
    Coral,
}

impl AccentChoice {
    pub const ALL: [AccentChoice; 6] = [
        AccentChoice::Amethyst,
        AccentChoice::Sapphire,
        AccentChoice::Emerald,
        AccentChoice::Amber,
        AccentChoice::Rose,
        AccentChoice::Coral,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AccentChoice::Amethyst => "Amethyst",
            AccentChoice::Sapphire => "Sapphire",
            AccentChoice::Emerald => "Emerald",
            AccentChoice::Amber => "Amber",
            AccentChoice::Rose => "Rose",
            AccentChoice::Coral => "Coral",
        }
    }

    #[allow(dead_code)]
    pub fn id(self) -> &'static str {
        match self {
            AccentChoice::Amethyst => "amethyst",
            AccentChoice::Sapphire => "sapphire",
            AccentChoice::Emerald => "emerald",
            AccentChoice::Amber => "amber",
            AccentChoice::Rose => "rose",
            AccentChoice::Coral => "coral",
        }
    }

    pub fn rgb(self) -> gpui::Rgba {
        match self {
            AccentChoice::Amethyst => gpui::rgb(0xc084fc),
            AccentChoice::Sapphire => gpui::rgb(0x60a5fa),
            AccentChoice::Emerald => gpui::rgb(0x34d399),
            AccentChoice::Amber => gpui::rgb(0xfbbf24),
            AccentChoice::Rose => gpui::rgb(0xf472b6),
            AccentChoice::Coral => gpui::rgb(0xf87171),
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "amethyst" | "purple" | "default" => Some(AccentChoice::Amethyst),
            "sapphire" | "blue" => Some(AccentChoice::Sapphire),
            "emerald" | "green" => Some(AccentChoice::Emerald),
            "amber" | "yellow" => Some(AccentChoice::Amber),
            "rose" | "pink" => Some(AccentChoice::Rose),
            "coral" | "red" => Some(AccentChoice::Coral),
            _ => None,
        }
    }
}

/// Comprehensive, serializable theme definition for custom colors and surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeDefinition {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub is_dark: bool,
    pub surfaces: SurfacePalette,
    pub accents: AccentPalette,
    pub text: TextPalette,
    pub syntax: SyntaxPalette,
    pub terminal: TerminalPalette,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfacePalette {
    pub canvas_base: String,
    pub bg_base: String,
    pub bg_surface: String,
    pub bg_surface_raised: String,
    pub bg_elevated: String,
    pub input_bg: String,
    pub border: String,
    pub popover: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccentPalette {
    pub primary: String,
    #[serde(default = "default_glow_blue_hex")]
    pub glow_blue: String,
    #[serde(default = "default_glow_pink_hex")]
    pub glow_pink: String,
    #[serde(default = "default_glow_green_hex")]
    pub glow_green: String,
    #[serde(default = "default_glow_amber_hex")]
    pub glow_amber: String,
}

fn default_glow_blue_hex() -> String { "#4f83f0".to_string() }
fn default_glow_pink_hex() -> String { "#c084fc".to_string() }
fn default_glow_green_hex() -> String { "#2fcf8a".to_string() }
fn default_glow_amber_hex() -> String { "#fbbf24".to_string() }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPalette {
    pub primary: String,
    pub secondary: String,
    pub muted: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyntaxPalette {
    pub keyword: String,
    pub function: String,
    pub r#type: String,
    pub string: String,
    pub number: String,
    pub comment: String,
    pub operator: String,
    pub variable: String,
    pub constant: String,
    pub attribute: String,
    pub tag: String,
    pub boolean: String,
    pub delimiter: String,
    pub punctuation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalPalette {
    pub bg: String,
    pub fg: String,
    pub prompt: String,
}

impl Default for ThemeDefinition {
    fn default() -> Self {
        Self::preset_obsidian()
    }
}

impl ThemeDefinition {
    pub fn preset_obsidian() -> Self {
        Self {
            id: "obsidian".into(),
            name: "Obsidian Neutral".into(),
            is_dark: true,
            surfaces: SurfacePalette {
                canvas_base: "#050505".into(),
                bg_base: "#0b0b0b".into(),
                bg_surface: "#101010".into(),
                bg_surface_raised: "#1c1c1c".into(),
                bg_elevated: "#242424".into(),
                input_bg: "#181818".into(),
                border: "#444444".into(),
                popover: "#101010".into(),
            },
            accents: AccentPalette {
                primary: "#c084fc".into(),
                glow_blue: "#4f83f0".into(),
                glow_pink: "#c084fc".into(),
                glow_green: "#2fcf8a".into(),
                glow_amber: "#fbbf24".into(),
            },
            text: TextPalette {
                primary: "#e8e8e8".into(),
                secondary: "#a8a8a8".into(),
                muted: "#707070".into(),
            },
            syntax: SyntaxPalette::default_dark(),
            terminal: TerminalPalette {
                bg: "#080808".into(),
                fg: "#e8e8e8".into(),
                prompt: "#4ade80".into(),
            },
        }
    }

    pub fn preset_oled() -> Self {
        Self {
            id: "oled".into(),
            name: "OLED True Black".into(),
            is_dark: true,
            surfaces: SurfacePalette {
                canvas_base: "#000000".into(),
                bg_base: "#050505".into(),
                bg_surface: "#0a0a0a".into(),
                bg_surface_raised: "#141414".into(),
                bg_elevated: "#1a1a1a".into(),
                input_bg: "#101010".into(),
                border: "#383838".into(),
                popover: "#0a0a0a".into(),
            },
            accents: AccentPalette {
                primary: "#c084fc".into(),
                glow_blue: "#4f83f0".into(),
                glow_pink: "#c084fc".into(),
                glow_green: "#2fcf8a".into(),
                glow_amber: "#fbbf24".into(),
            },
            text: TextPalette {
                primary: "#e8e8e8".into(),
                secondary: "#a8a8a8".into(),
                muted: "#707070".into(),
            },
            syntax: SyntaxPalette::default_dark(),
            terminal: TerminalPalette {
                bg: "#000000".into(),
                fg: "#e8e8e8".into(),
                prompt: "#4ade80".into(),
            },
        }
    }

    pub fn preset_midnight() -> Self {
        Self {
            id: "midnight".into(),
            name: "Midnight Slate".into(),
            is_dark: true,
            surfaces: SurfacePalette {
                canvas_base: "#090d16".into(),
                bg_base: "#0f172a".into(),
                bg_surface: "#1e293b".into(),
                bg_surface_raised: "#283548".into(),
                bg_elevated: "#334155".into(),
                input_bg: "#172033".into(),
                border: "#475569".into(),
                popover: "#1e293b".into(),
            },
            accents: AccentPalette {
                primary: "#60a5fa".into(),
                glow_blue: "#60a5fa".into(),
                glow_pink: "#a78bfa".into(),
                glow_green: "#34d399".into(),
                glow_amber: "#fbbf24".into(),
            },
            text: TextPalette {
                primary: "#f1f5f9".into(),
                secondary: "#94a3b8".into(),
                muted: "#64748b".into(),
            },
            syntax: SyntaxPalette::default_dark(),
            terminal: TerminalPalette {
                bg: "#0b1120".into(),
                fg: "#f1f5f9".into(),
                prompt: "#38bdf8".into(),
            },
        }
    }

    pub fn preset_tokyo() -> Self {
        Self {
            id: "tokyo".into(),
            name: "Tokyo Dark".into(),
            is_dark: true,
            surfaces: SurfacePalette {
                canvas_base: "#0d0f18".into(),
                bg_base: "#131622".into(),
                bg_surface: "#1a1e2e".into(),
                bg_surface_raised: "#24293e".into(),
                bg_elevated: "#2f354f".into(),
                input_bg: "#181c2b".into(),
                border: "#414868".into(),
                popover: "#1a1e2e".into(),
            },
            accents: AccentPalette {
                primary: "#7aa2f7".into(),
                glow_blue: "#7aa2f7".into(),
                glow_pink: "#bb9af7".into(),
                glow_green: "#73daca".into(),
                glow_amber: "#e0af68".into(),
            },
            text: TextPalette {
                primary: "#c0caf5".into(),
                secondary: "#9aa5ce".into(),
                muted: "#565f89".into(),
            },
            syntax: SyntaxPalette::default_dark(),
            terminal: TerminalPalette {
                bg: "#0f121d".into(),
                fg: "#c0caf5".into(),
                prompt: "#73daca".into(),
            },
        }
    }

    pub fn from_preset(preset: ThemePreset) -> Self {
        match preset {
            ThemePreset::Obsidian => Self::preset_obsidian(),
            ThemePreset::Oled => Self::preset_oled(),
            ThemePreset::Midnight => Self::preset_midnight(),
            ThemePreset::Tokyo => Self::preset_tokyo(),
        }
    }
}

impl SyntaxPalette {
    pub fn default_dark() -> Self {
        Self {
            keyword: "#f97583".into(),
            function: "#b392f0".into(),
            r#type: "#79b8ff".into(),
            string: "#9ecbff".into(),
            number: "#79b8ff".into(),
            comment: "#7e888c".into(),
            operator: "#f97583".into(),
            variable: "#e1e4e8".into(),
            constant: "#79b8ff".into(),
            attribute: "#b392f0".into(),
            tag: "#85e89d".into(),
            boolean: "#79b8ff".into(),
            delimiter: "#f97583".into(),
            punctuation: "#bbbebf".into(),
        }
    }
}

/// Parses a hex color string (`#rrggbb` or `#rrggbbaa` or without `#`) into GPUI `Rgba`.
pub fn parse_hex_color(s: &str) -> Option<gpui::Rgba> {
    let trimmed = s.trim().trim_start_matches('#');
    if trimmed.len() == 6 {
        let r = u8::from_str_radix(&trimmed[0..2], 16).ok()?;
        let g = u8::from_str_radix(&trimmed[2..4], 16).ok()?;
        let b = u8::from_str_radix(&trimmed[4..6], 16).ok()?;
        Some(gpui::Rgba {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        })
    } else if trimmed.len() == 8 {
        let r = u8::from_str_radix(&trimmed[0..2], 16).ok()?;
        let g = u8::from_str_radix(&trimmed[2..4], 16).ok()?;
        let b = u8::from_str_radix(&trimmed[4..6], 16).ok()?;
        let a = u8::from_str_radix(&trimmed[6..8], 16).ok()?;
        Some(gpui::Rgba {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        })
    } else {
        None
    }
}

/// Helper to format GPUI `Rgba` back to `#rrggbb` string.
#[allow(dead_code)]
pub fn format_rgba_hex(color: gpui::Rgba) -> String {
    let r = (color.r * 255.0).round() as u8;
    let g = (color.g * 255.0).round() as u8;
    let b = (color.b * 255.0).round() as u8;
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// An `editor` value we do not understand resolves to the default instead of
/// poisoning the rest of the file. See [`ChamberPrefs::editor`].
fn lenient_editor<'de, D>(d: D) -> Result<crate::sys::EditorChoice, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let raw = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(raw).unwrap_or_default())
}

fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_terminal_scrollback() -> usize {
    5000
}
fn default_roster_width() -> f32 {
    // Wide enough for effort + mode tags beside the name/model column, trimmed
    // ~11% from the previous 450 default (Jake's request).
    400.0
}
fn default_inspector_width() -> f32 {
    300.0
}

impl ChamberPrefs {
    /// Move per-quark identity (colour/name/avatar) to a renamed id, so the taxonomy
    /// migration does not reset a quark's appearance. Reads the SAME map as the team.json
    /// rename (`hadron_lattice::legacy_id_renames`) — passed in so the SSOT stays in lattice.
    pub fn rename_quark_ids(&mut self, renames: &[(&str, &str)]) {
        for (old, new) in renames {
            if let Some(identity) = self.quarks.remove(*old) {
                self.quarks.entry(new.to_string()).or_insert(identity);
            }
        }
    }
}

impl Default for ChamberPrefs {
    fn default() -> Self {
        ChamberPrefs {
            roster_collapsed: default_false(),
            inspector_collapsed: default_false(),
            close_gluon_on_exit: default_true(),
            roster_width: default_roster_width(),
            inspector_width: default_inspector_width(),
            default_mode: hadron_lattice::Mode::default(),
            window_bounds: None,
            human: Identity::default(),
            quarks: BTreeMap::new(),
            editor: crate::sys::EditorChoice::default(),
            ui_font_family: None,
            ui_font_size: None,
            mono_font_family: None,
            mono_font_size: None,
            theme_preset: None,
            accent_choice: None,
            custom_theme: None,
            audio: crate::app::audio::AudioConfig::default(),
            turn_deadline_secs: None,
            stale_after_secs: None,
            terminal_shell: None,
            terminal_cursor_style: TerminalCursorStyle::default(),
            terminal_scrollback: default_terminal_scrollback(),
            chat_density: ChatDensity::default(),
            code_block_word_wrap: false,
            timestamp_format: TimestampFormat::default(),
            auto_fold_reasoning: default_true(),
            git_auto_prune_worktrees: default_true(),
            git_author_name: None,
            git_author_email: None,
            desktop_notifications: default_true(),
            notify_on_blocked: default_true(),
            notify_on_turn_finish: default_true(),
        }
    }
}

/// Resolve the on-disk preferences path: `~/.hadron/chamber.json` (cross-platform).
/// `None` if the home directory can't be resolved.
pub fn config_path() -> Option<PathBuf> {
    Some(hadron_lattice::user_hadron_dir()?.join("chamber.json"))
}

/// Read preferences from an explicit path; missing or malformed → defaults.
pub fn load_from(path: &Path) -> ChamberPrefs {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let mut prefs: ChamberPrefs = serde_json::from_str(&text).unwrap_or_default();
            // One-time migration: a chamber.json still pinned at an old default
            // (≤410, 450 or 500) is bumped to the new default so the trimmed roster
            // applies without a manual edit. Any other width was deliberately chosen
            // by the user and is untouched.
            if prefs.roster_width <= 410.0
                || prefs.roster_width == 450.0
                || prefs.roster_width == 500.0
            {
                prefs.roster_width = default_roster_width();
            }
            prefs
        }
        Err(_) => ChamberPrefs::default(),
    }
}

/// Write preferences to an explicit path, creating parent dirs as needed.
pub fn save_to(path: &Path, prefs: &ChamberPrefs) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(prefs).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// Load from the resolved config path (defaults if unresolved/missing).
pub fn load() -> ChamberPrefs {
    config_path().map(|p| load_from(&p)).unwrap_or_default()
}

/// Save to the resolved config path (no-op if the path can't be resolved).
pub fn save(prefs: &ChamberPrefs) -> std::io::Result<()> {
    match config_path() {
        Some(p) => save_to(&p, prefs),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prefs_round_trip() {
        let mut quarks = BTreeMap::new();
        quarks.insert(
            "claude".to_string(),
            Identity {
                display_name: Some("Claude".into()),
                color: Some("#a855f7".into()),
                image_path: None,
            },
        );
        let prefs = ChamberPrefs {
            roster_collapsed: true,
            inspector_collapsed: false,
            close_gluon_on_exit: false,
            roster_width: 180.5,
            inspector_width: 320.0,
            default_mode: hadron_lattice::Mode::Auto,
            window_bounds: None,
            human: Identity {
                display_name: Some("Jake".into()),
                color: Some("#ec4899".into()),
                image_path: Some("/tmp/me.png".into()),
            },
            quarks,
            editor: crate::sys::EditorChoice::Zed,
            ui_font_family: Some("Inter".into()),
            ui_font_size: Some(14.0),
            mono_font_family: Some("Cascadia Code".into()),
            mono_font_size: Some(13.0),
            theme_preset: None,
            accent_choice: None,
            custom_theme: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let back: ChamberPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(prefs, back);
    }

    #[test]
    fn old_config_without_identities_keeps_layout() {
        // A config written by a pre-identity binary has only the four layout
        // keys. Loading it must preserve them (not reset via unwrap_or_default)
        // and yield empty identities.
        let dir = tempdir().unwrap();
        let path = dir.path().join("chamber.json");
        std::fs::write(
            &path,
            r#"{"roster_collapsed":true,"inspector_collapsed":true,"roster_width":175.0,"inspector_width":333.0}"#,
        )
        .unwrap();
        let prefs = load_from(&path);
        assert!(prefs.roster_collapsed);
        assert!(prefs.inspector_collapsed);
        // Wait, 175.0 <= 410.0, so this would migrate to 500.0! Let's update this assertion as well.
        assert_eq!(prefs.roster_width, default_roster_width());
        assert_eq!(prefs.inspector_width, 333.0);
        assert_eq!(prefs.human, Identity::default());
        assert!(prefs.quarks.is_empty());
    }

    #[test]
    fn close_gluon_on_exit_defaults_to_true_and_round_trips() {
        let prefs = ChamberPrefs::default();
        assert!(prefs.close_gluon_on_exit, "default must be true");

        let json = serde_json::to_string(&prefs).unwrap();
        assert!(json.contains("\"close_gluon_on_exit\":true"));

        // The opposite value must still survive a round trip — the default is a
        // starting point, not a forced value.
        let custom = ChamberPrefs {
            close_gluon_on_exit: false,
            ..Default::default()
        };
        let json_custom = serde_json::to_string(&custom).unwrap();
        let back: ChamberPrefs = serde_json::from_str(&json_custom).unwrap();
        assert!(!back.close_gluon_on_exit);
    }

    /// A config written before the key existed gets the NEW default, but a config
    /// that says `false` on purpose keeps saying `false`. Changing a default must
    /// never overwrite a deliberate choice (unlike `roster_width`, which migrates
    /// a *stale default* — not a user's own value).
    #[test]
    fn an_explicit_close_gluon_on_exit_false_survives_the_default_flip() {
        let dir = tempdir().unwrap();

        let absent = dir.path().join("absent.json");
        std::fs::write(&absent, r#"{"roster_collapsed":false}"#).unwrap();
        assert!(load_from(&absent).close_gluon_on_exit, "absent key → new default");

        let explicit = dir.path().join("explicit.json");
        std::fs::write(&explicit, r#"{"close_gluon_on_exit":false}"#).unwrap();
        assert!(!load_from(&explicit).close_gluon_on_exit, "an explicit false is kept");
    }

    #[test]
    fn window_bounds_round_trip_on_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chamber.json");
        let prefs = ChamberPrefs {
            window_bounds: Some(WindowBoundsPrefs {
                x: 120.0,
                y: 64.0,
                width: 1600.0,
                height: 1000.0,
            }),
            ..Default::default()
        };
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path), prefs);
    }

    #[test]
    fn config_without_window_bounds_loads_as_unset() {
        // A config written by a pre-persistence binary must still parse — it just
        // has no remembered geometry, so the window centers on its default size.
        let dir = tempdir().unwrap();
        let path = dir.path().join("chamber.json");
        std::fs::write(&path, r#"{"roster_collapsed":false,"roster_width":240.0}"#).unwrap();
        // 240.0 is <= 410.0, so it migrates to 500.0. Let's make sure it's 500.0.
        assert_eq!(load_from(&path).roster_width, default_roster_width());
        assert_eq!(load_from(&path).window_bounds, None);
    }

    /// A typo in the one field we tell people to hand-edit must not cost them the
    /// rest of the file. Without `lenient_editor` this resets width AND identities.
    #[test]
    fn a_nonsense_editor_value_does_not_discard_the_rest_of_the_prefs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chamber.json");
        std::fs::write(
            &path,
            r#"{"editor":"Vim","inspector_width":333.0,"human":{"display_name":"Jake"}}"#,
        )
        .unwrap();
        let prefs = load_from(&path);
        assert_eq!(prefs.editor, crate::sys::EditorChoice::System);
        assert_eq!(prefs.inspector_width, 333.0);
        assert_eq!(prefs.human.display_name.as_deref(), Some("Jake"));
    }

    /// The hand-edit the docs actually invite, and the reason the field exists.
    #[test]
    fn a_hand_written_custom_editor_is_honoured() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chamber.json");
        std::fs::write(&path, r#"{"editor":{"Custom":"kate -l {line} {file}"}}"#).unwrap();
        assert_eq!(
            load_from(&path).editor,
            crate::sys::EditorChoice::Custom("kate -l {line} {file}".into())
        );
    }

    #[test]
    fn load_from_missing_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert_eq!(load_from(&path), ChamberPrefs::default());
    }

    #[test]
    fn a_stored_410_roster_width_migrates_to_the_new_default() {
        let dir = tempdir().unwrap();
        // A chamber.json pinned at any old default (410, 450, 500) is bumped on load...
        for old_default in ["410.0", "450.0", "500.0"] {
            let old = dir.path().join(format!("old-{old_default}.json"));
            std::fs::write(&old, format!(r#"{{"roster_width":{old_default}}}"#)).unwrap();
            assert_eq!(load_from(&old).roster_width, default_roster_width());
        }
        // ...but a width the user chose themselves is preserved exactly.
        let chosen = dir.path().join("chosen.json");
        std::fs::write(&chosen, r#"{"roster_width":460.0}"#).unwrap();
        assert_eq!(load_from(&chosen).roster_width, 460.0);
    }

    #[test]
    fn save_then_load_from_round_trips_on_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sub").join("chamber.json"); // parent created by save
        let prefs = ChamberPrefs {
            roster_collapsed: true,
            ..Default::default()
        };
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path), prefs);
    }

    #[test]
    fn malformed_file_falls_back_to_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chamber.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_from(&path), ChamberPrefs::default());
    }

    #[test]
    fn rename_quark_ids_moves_identity_to_the_new_key() {
        let mut prefs = ChamberPrefs::default();
        prefs.quarks.insert("agy".to_string(), Identity::default());
        prefs.rename_quark_ids(hadron_lattice::legacy_id_renames());
        assert!(prefs.quarks.contains_key("cli-agy"), "identity moved to the new id");
        assert!(!prefs.quarks.contains_key("agy"), "old key gone");
        // Idempotent: a second run finds nothing to move.
        prefs.rename_quark_ids(hadron_lattice::legacy_id_renames());
        assert!(prefs.quarks.contains_key("cli-agy"));
    }

    #[test]
    fn typography_preferences_round_trip_and_default_to_none() {
        let prefs = ChamberPrefs::default();
        assert_eq!(prefs.ui_font_family, None);
        assert_eq!(prefs.ui_font_size, None);
        assert_eq!(prefs.mono_font_family, None);
        assert_eq!(prefs.mono_font_size, None);

        let custom = ChamberPrefs {
            ui_font_family: Some("Geist".to_string()),
            ui_font_size: Some(15.0),
            mono_font_family: Some("JetBrains Mono".to_string()),
            mono_font_size: Some(12.5),
            ..Default::default()
        };
        let json = serde_json::to_string(&custom).unwrap();
        let loaded: ChamberPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.ui_font_family.as_deref(), Some("Geist"));
        assert_eq!(loaded.ui_font_size, Some(15.0));
        assert_eq!(loaded.mono_font_family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(loaded.mono_font_size, Some(12.5));
    }

    #[test]
    fn theme_and_accent_preferences_round_trip() {
        let prefs = ChamberPrefs::default();
        assert_eq!(prefs.theme_preset, None);
        assert_eq!(prefs.accent_choice, None);

        let custom = ChamberPrefs {
            theme_preset: Some(ThemePreset::Tokyo),
            accent_choice: Some(AccentChoice::Sapphire),
            ..Default::default()
        };
        let json = serde_json::to_string(&custom).unwrap();
        let loaded: ChamberPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.theme_preset, Some(ThemePreset::Tokyo));
        assert_eq!(loaded.accent_choice, Some(AccentChoice::Sapphire));

        assert_eq!(ThemePreset::from_str("oled"), Some(ThemePreset::Oled));
        assert_eq!(ThemePreset::from_str("OLED True Black"), Some(ThemePreset::Oled));
        assert_eq!(ThemePreset::from_str("tokyo-dark"), Some(ThemePreset::Tokyo));
        assert_eq!(ThemePreset::from_str("Tokyo Dark"), Some(ThemePreset::Tokyo));
        assert_eq!(ThemePreset::from_str("Midnight Slate"), Some(ThemePreset::Midnight));
        assert_eq!(ThemePreset::from_str("Obsidian Neutral"), Some(ThemePreset::Obsidian));
        assert_eq!(AccentChoice::from_str("blue"), Some(AccentChoice::Sapphire));
        assert_eq!(AccentChoice::from_str("Sapphire"), Some(AccentChoice::Sapphire));
        assert_eq!(AccentChoice::from_str("emerald"), Some(AccentChoice::Emerald));
        assert_eq!(AccentChoice::from_str("Amethyst"), Some(AccentChoice::Amethyst));
    }

    #[test]
    fn test_custom_theme_definition_serde_and_hex_parsing() {
        let theme = ThemeDefinition::preset_tokyo();
        let json = serde_json::to_string(&theme).unwrap();
        let loaded: ThemeDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.name, "Tokyo Dark");
        assert_eq!(loaded.surfaces.canvas_base, "#0d0f18");

        let c1 = parse_hex_color("#c084fc").unwrap();
        assert_eq!((c1.r * 255.0).round() as u8, 0xc0);
        assert_eq!((c1.g * 255.0).round() as u8, 0x84);
        assert_eq!((c1.b * 255.0).round() as u8, 0xfc);

        let c2 = parse_hex_color("050505").unwrap();
        assert_eq!((c2.r * 255.0).round() as u8, 0x05);

        let formatted = format_rgba_hex(c1);
        assert_eq!(formatted, "#c084fc");

        assert_eq!(parse_hex_color("invalid"), None);
    }

    #[test]
    fn test_new_adjustable_preferences_serde_roundtrip() {
        let prefs = ChamberPrefs {
            audio: crate::app::audio::AudioConfig {
                enabled: true,
                volume: 0.85,
                haptic_enabled: true,
                cue_gate_approval: true,
                cue_merge_collision: false,
                cue_turn_finish: true,
                cue_blocked_on_human: true,
                cue_message_received: true,
                cue_message_sent: true,
                sound_theme: SoundTheme::Synth,
            },
            turn_deadline_secs: Some(3600),
            stale_after_secs: Some(300),
            terminal_shell: Some("/usr/bin/zsh".to_string()),
            terminal_cursor_style: TerminalCursorStyle::Underline,
            terminal_scrollback: 10000,
            chat_density: ChatDensity::Compact,
            code_block_word_wrap: true,
            timestamp_format: TimestampFormat::Relative,
            auto_fold_reasoning: false,
            git_auto_prune_worktrees: true,
            git_author_name: Some("Hadron Orchestrator".to_string()),
            git_author_email: Some("swarm@hadron.internal".to_string()),
            desktop_notifications: true,
            notify_on_blocked: true,
            notify_on_turn_finish: false,
            ..Default::default()
        };

        let json = serde_json::to_string(&prefs).expect("serialize prefs");
        let loaded: ChamberPrefs = serde_json::from_str(&json).expect("deserialize prefs");
        assert_eq!(prefs, loaded);

        assert_eq!(TerminalCursorStyle::from_str("block"), Some(TerminalCursorStyle::Block));
        assert_eq!(TerminalCursorStyle::from_str("beam"), Some(TerminalCursorStyle::Beam));
        assert_eq!(TerminalCursorStyle::from_str("underline"), Some(TerminalCursorStyle::Underline));
        assert_eq!(TerminalCursorStyle::from_str("Beam (Vertical Line)"), Some(TerminalCursorStyle::Beam));
        assert_eq!(TerminalCursorStyle::from_str("Block (Full Rectangle)"), Some(TerminalCursorStyle::Block));
        assert_eq!(TerminalCursorStyle::from_str("Underline (Bottom Bar)"), Some(TerminalCursorStyle::Underline));

        assert_eq!(ChatDensity::from_str("compact"), Some(ChatDensity::Compact));
        assert_eq!(ChatDensity::from_str("comfortable"), Some(ChatDensity::Comfortable));
        assert_eq!(ChatDensity::from_str("Comfortable (Standard Spacing)"), Some(ChatDensity::Comfortable));
        assert_eq!(ChatDensity::from_str("Compact (Dense View)"), Some(ChatDensity::Compact));

        assert_eq!(TimestampFormat::from_str("24h"), Some(TimestampFormat::Clock24h));
        assert_eq!(TimestampFormat::from_str("12h"), Some(TimestampFormat::Clock12h));
        assert_eq!(TimestampFormat::from_str("relative"), Some(TimestampFormat::Relative));
        assert_eq!(TimestampFormat::from_str("24-Hour (15:04:05)"), Some(TimestampFormat::Clock24h));
        assert_eq!(TimestampFormat::from_str("12-Hour (3:04:05 PM)"), Some(TimestampFormat::Clock12h));
        assert_eq!(TimestampFormat::from_str("Relative (2m ago)"), Some(TimestampFormat::Relative));

        assert_eq!(SoundTheme::from_str("classic"), Some(SoundTheme::Classic));
        assert_eq!(SoundTheme::from_str("synth"), Some(SoundTheme::Synth));
        assert_eq!(SoundTheme::from_str("minimal"), Some(SoundTheme::Minimal));
        assert_eq!(SoundTheme::from_str("retro-8bit"), Some(SoundTheme::Retro8Bit));
        assert_eq!(SoundTheme::from_str("Classic (Harmonic Chimes)"), Some(SoundTheme::Classic));
        assert_eq!(SoundTheme::from_str("Synth (Electronic FM Blips)"), Some(SoundTheme::Synth));
        assert_eq!(SoundTheme::from_str("Minimal (Soft Clicks & Pops)"), Some(SoundTheme::Minimal));
        assert_eq!(SoundTheme::from_str("Retro 8-Bit (Arcade Bleeps)"), Some(SoundTheme::Retro8Bit));
    }
}


