//! Search core for Lightning: sources, matching and ranking. No UI code.

pub mod apps;
pub mod calc;
pub mod engine;
pub mod files;
pub mod fuzzy;
pub mod power;
pub mod settings;

pub use engine::{Engine, Hit, Kind, Results, Sources};
