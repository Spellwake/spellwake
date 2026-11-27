//! Ground-up, strictly-typed, fail-closed compositional Oracle parser (`v2`).
//!
//! Zero legacy `v1` waterfall matchers, zero raw-prose `String` escape hatches, zero `.contains()`
//! clause hijacks. Every sentence is tokenized with source byte spans and parsed by composable
//! recursive-descent combinators that enforce `expect_eof` (fail-closed).

pub mod card;
pub mod clause;
pub mod condition;
pub mod cost;
pub mod filter;
pub mod ir;
pub mod lexer;
pub mod parser;
pub mod replacement;
pub mod static_ability;
pub mod trigger;
pub mod value;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use card::{compile_card_v2, compile_face_v2};
pub use ir::{CompiledCardV2, CompiledFaceV2, ParseError, ParseStage, Span};

/// Honest corpus-wide summary for `audit-v2-corpus`.
///
/// Because `v2::ir` has zero raw-prose `String` escape hatches and enforces fail-closed
/// `expect_eof` on every clause, `strictly_compiled_cards` is 100% honest, strictly-typed
/// compositional coverage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct V2CorpusSummary {
    pub total_cards: usize,
    pub strictly_compiled_cards: usize,
    pub failed_cards: usize,
    pub error_stage_counts: BTreeMap<String, usize>,
    pub sample_failures: Vec<String>,
}

impl V2CorpusSummary {
    /// Record one compiled `v2` card into the corpus summary.
    pub fn record(&mut self, card: &CompiledCardV2) {
        self.total_cards += 1;
        if card.strictly_compiled {
            self.strictly_compiled_cards += 1;
        } else {
            self.failed_cards += 1;
            for face in &card.faces {
                for err in &face.errors {
                    let key = format!("{:?}", err.stage);
                    *self.error_stage_counts.entry(key).or_insert(0) += 1;
                    if self.sample_failures.len() < 25 {
                        self.sample_failures.push(format!(
                            "{} [{:?} @ {}..{}]: {} ({})",
                            card.name,
                            err.stage,
                            err.span.start,
                            err.span.end,
                            err.message,
                            err.text
                        ));
                    }
                }
            }
        }
    }
}
