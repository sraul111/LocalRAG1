//! Search-query pipeline.
//!
//! Tiers (cheapest first; we stop at the first to reach its min_score):
//!
//! - [`tier0`] — exact + normalized filename match. < 1 ms.
//! - [`tier1`] — SQLite FTS5 over filenames and bodies. < 50 ms.
//! - [`tier2`] — metadata filters + aggregate scoring. < 20 ms.
//! - [`tier3`] — optional LLM agent loop. Opt-in only.
//!
//! Each tier implements the [`Tier`] trait. The dispatch logic in
//! [`pipeline`] walks them in order and picks the first result with
//! confidence >= `tier_min_score`.

pub mod pipeline;
pub mod query;
pub mod tier0;
pub mod tier1;
pub mod tier2;

pub use pipeline::{run, Answer, TierResult};
pub use query::Query;
