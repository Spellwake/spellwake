//! Strict zero-regex recursive-descent parsers for Costs, Triggers, Effects, and Statics/Replacements.
//!
//! Organized by MTG grammatical responsibility:
//! - [`costs`]: activated-ability and additional cost lists
//! - [`triggers`]: triggered-ability headers and event grammar
//! - [`effects`]: effect bodies, atomic instructions, zone ops, combat/stats, cast/mana, choices
//! - [`statics`]: static abilities, continuous layer effects, restrictions, and replacements

pub(crate) mod costs;
pub(crate) mod effects;
pub(crate) mod statics;
pub(crate) mod triggers;

pub use costs::{parse_cost_atom, parse_cost_list};
pub use effects::{parse_atomic_effect, parse_effect_body};
pub use statics::{StaticOrReplacement, parse_static_or_replacement};
pub use triggers::parse_trigger_header;
