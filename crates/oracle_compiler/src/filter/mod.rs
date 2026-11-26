//! Strict zero-regex parsers for `ObjectFilterAst`, `SubjectRefAst`, `ValueExprAst`,
//! `ConditionAst`, `ManaSpecAst`, and `TokenSpecAst`.
//!
//! Organized by grammatical responsibility:
//! - [`zones`]: compositional zone-suffix and controller/owner-suffix parsers
//! - [`object`]: object noun-phrase filters, stat bounds, counter/keyword/relative predicates
//! - [`subject`]: player references, target/subject noun phrases, keyword lists
//! - [`value`]: numeric value expressions and `for each` scaling counts
//! - [`condition`]: boolean game-state and event conditions
//! - [`mana_token`]: mana specifications and token creation descriptors

mod condition;
mod mana_token;
mod object;
mod subject;
mod value;
pub(crate) mod zones;

pub use condition::parse_condition;
pub use mana_token::{
    RichTokenCreationAst, parse_mana_spec, parse_token_creation, parse_token_creation_rich,
    split_keyword_tokens,
};
pub use object::parse_object_filter;
pub use subject::{parse_keyword_list, parse_player_ref, parse_subject_or_target};
pub use value::{parse_for_each_value_expr, parse_value_expr};
