//! Dynamic nucleus prompt injector.
//!
//! Loads `.hadron/nucleus/notes/*.md` and ranks them by overlap with the
//! target file paths and the current query, returning the top notes that fit
//! within a byte budget. The chamber (or any caller) then splices those notes
//! into the prompt that goes to a quark.
//!
//! See `Dynamic Smart Nucleus Prompt Injector` in
//! `.hadron/docs/plans/2026-08-13-hadron-next-gen-capabilities.md`.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One nucleus note as stored on disk and injected into a prompt.
///
/// `slug` and `description` are the routing metadata (the index line in
/// `index.md` is exactly these two); `content` is the body of the note that
/// only gets paid for on the turns its line says matters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NucleusNote {
    pub slug: String,
    pub description: String,
    pub content: String,
}

/// Ranks and slices a corpus of [`NucleusNote`]s to fit a prompt byte budget.
pub struct DynamicNucleusInjector {
    notes: Vec<NucleusNote>,
}
