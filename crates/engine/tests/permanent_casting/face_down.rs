#![allow(dead_code, unused_imports)]
use super::*;
use spellwake_engine::{
    characteristics::{
        AbilityChange, AbilityRemoval, KeywordAbility, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec,
    },
    face_down::{FaceDownKind, FaceUpAbility},
    occurrence::{RulesOccurrence, RulesRecord},
};

fn hide(game: &GameState) -> GameCommand {
    GameCommand::CastFaceDown {
        spell: object(game, 100),
        kind: FaceDownKind::Morph,
        payment: Some(payment(&[(ManaType::Colourless, 3)])),
    }
}
fn turn_up(game: &GameState) -> GameCommand {
    GameCommand::TurnFaceUp {
        permanent: object(game, 100),
        ability: 0,
        payment: Some(payment(&[(ManaType::Black, 1)])),
    }
}
fn reveals(candidate: &NativeCandidate) -> Vec<&GameObject> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::Event(event) = record {
                Some(event)
            } else {
                None
            }
        })
        .flat_map(|event| &event.occurrences)
        .filter_map(|record| match &record.value {
            RulesOccurrence::FaceDownRevealed { object } => Some(object),
            _ => None,
        })
        .collect()
}
