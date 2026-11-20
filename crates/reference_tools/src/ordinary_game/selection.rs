//! Saved source9 qualification inputs and native witness bridge. This is not
//! public gameplay transport, release/deck admission or complete-card support.
mod registry_witness;
mod source;
pub use source::SelectionSource;
pub(in crate::ordinary_game) mod data;
pub(in crate::ordinary_game) mod prepared;
pub(in crate::ordinary_game) mod recipes;
pub use data::SelectionCase;
pub(in crate::ordinary_game) mod authority;
pub(in crate::ordinary_game) mod probes;
pub use authority::{
    ForeignHelperCall, ForeignHelperTrace, SelectionBindingError, SelectionBindingEvidence,
};

#[cfg(test)]
pub(crate) mod policy_tests;
