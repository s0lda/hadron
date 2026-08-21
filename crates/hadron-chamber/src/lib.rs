#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub mod model;
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub mod config;
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub mod vcs;
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub mod text;
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub mod sys;
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub mod pty;
pub mod fonts;
pub mod mermaid;
#[cfg(feature = "gui")]
pub mod app;
#[cfg(feature = "gui")]
pub mod theme;
#[cfg(feature = "gui")]
pub mod window_frame;
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub mod symbols;
