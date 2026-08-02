//! `sl` library root. The binary `main.rs` re-uses everything we expose
//! here, so integration tests (in `tests/`) can also re-use them.
//!
//! Public modules are deliberately flat — keep the surface small so it's
//! easy to reason about which internal pieces the user could plug into a
//! shell or a TUI.
//
// Crate-wide attribute lints. We allow hidden lifetimes and lifetime-elision
// warnings to keep the scaffold readable while we learn the conventions.
#![allow(hidden_lifetime_in_path)]
#![allow(mismatched_lifetime_syntaxes)]

pub mod cli;
pub mod config;
pub mod crawler;
pub mod error;
pub mod index;
pub mod llm;
pub mod query;
pub mod tools;
pub mod util;
