//! Viewer query path: `sessions`, `view`, `search`, `version`.
//!
//! This module is intentionally independent of the legacy exporter. The legacy path keeps its
//! schema, flags and exit codes; the query path returns provenance, bounded output and its own
//! 0/1/2 exit contract.

pub mod cursor;
pub mod detect;
pub mod filter;
pub mod model;
pub mod normalize;
pub mod output;
pub mod reader;
pub mod runner;
pub mod select;
pub mod sessions;
pub mod source;
pub mod toolpair;
pub mod version;
