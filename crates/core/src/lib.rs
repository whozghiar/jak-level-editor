//! Jak Level Editor's engine: a level editor for Jak 3 (OpenGOAL), without its interface.
//!
//! It reads the levels the user extracted from their own disc (ogle-extract, `extractor/`), keeps
//! the document open with its history, runs every tool on it, and exports it as a custom level of
//! OpenGOAL. `docs/rust-architecture.md` explains how it is split; the interface is `ui/`.

pub mod editor;
pub mod game;
pub mod history;
pub mod host;
pub mod i18n;
pub mod io;
pub mod log;
pub mod math;
pub mod navmesh;
pub mod prefs;
pub mod scene;
