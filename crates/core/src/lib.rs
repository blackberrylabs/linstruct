//! Linstruct, core: the instruction files of a machine as each AI host reads them. No UI,
//! no network.
//!
//! The Blackberrylabs promises, the same for every tool: read-only (nothing written outside
//! `~/.config/linstruct/`, and only by the app's preferences); no network; secrets masked the
//! moment they are read; actions shown, never run; every rule sourced.
//!
//! `tests/promises.rs` checks that the source of this crate never mentions a write or a
//! network API, and that `report` conforms to `bbl-report/1`.

#![forbid(unsafe_code)]

pub mod model;
pub mod projects;
pub mod report;
pub mod rules;
pub mod scan;
pub mod text;

pub use model::{Finding, Inventory, Kind};
pub use scan::{scan, scan_with};
