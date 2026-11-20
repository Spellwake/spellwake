//! Literal mechanism fixtures over an explicitly supplied genuine vocabulary.
//! These are native adapter tests, never complete-card/format qualification.
use super::*;
use serde::Deserialize;
use spellwake_contracts::{
    creature_types::{CreatureTypeVocabulary, CreatureVocabularyId},
    names::NameRules,
};
use spellwake_engine::{
    combat::CombatRules,
    effects::{Program, SeedCard},
    game::{GameContext, GameId},
    object_identity::{CardId, ObjectDestination, ObjectId},
    setup::GameSeed,
};
use std::sync::Arc;
pub const A: SeatId = SeatId(17);
pub const B: SeatId = SeatId(4);
pub fn viewer_a() -> Uuid {
    Uuid::from_u128(101)
}
pub fn viewer_b() -> Uuid {
    Uuid::from_u128(102)
}
pub fn context_id() -> Uuid {
    Uuid::from_u128(103)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VocabularyInput {
    rules_revision: String,
    rules_sha256: String,
    members: Vec<String>,
    id: String,
}
pub fn vocabulary() -> Arc<CreatureTypeVocabulary> {
    let path = std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT").expect(
        "native bridge composition requires explicit genuine vocabulary input; no fallback",
    );
    let bytes = std::fs::read(path).expect("read retained genuine vocabulary input");
    let raw: VocabularyInput =
        crate::decode_wire_object(&bytes).expect("strict flat vocabulary input");
    let rules_hash =
        CreatureVocabularyId::try_from(raw.rules_sha256).expect("canonical rules digest");
    let declared = CreatureVocabularyId::try_from(raw.id).expect("canonical vocabulary identity");
    let value =
        CreatureTypeVocabulary::new(raw.rules_revision, *rules_hash.as_bytes(), raw.members)
            .unwrap();
    assert_eq!(value.id(), declared);
    Arc::new(value)
}
pub fn context(vocabulary: &CreatureTypeVocabulary) -> GameContext {
    GameContext::new(
        "bridge-mechanism-fixture".into(),
        vocabulary.rules_revision().to_owned(),
        "literal-program-fixtures-not-a-catalogue".into(),
        "private-bridge-test".into(),
        "controlled-native-interaction".into(),
        vocabulary.id(),
        NameRules::literal_names().id(),
    )
    .unwrap()
}
pub fn seed(cards: Vec<SeedCard>) -> GameSeed {
    let order = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary(),
        Arc::new(NameRules::literal_names()),
    )
}
pub fn state(cards: Vec<SeedCard>) -> GameState {
    let seed = seed(cards);
    GameState::initialize(GameId(90), context(&seed.creature_vocabulary), seed)
        .unwrap()
        .into_state()
}
pub fn card(id: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program,
    }
}
pub fn object(state: &GameState, card: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
pub fn binding(state: &GameState, viewer: Uuid) -> NativeBinding {
    NativeBinding::new(
        context_id(),
        state.basis().clone(),
        vec![
            NativeSeatBinding {
                seat_id: viewer_a(),
                native_seat: A,
            },
            NativeSeatBinding {
                seat_id: viewer_b(),
                native_seat: B,
            },
        ],
        viewer,
    )
    .unwrap()
}
pub fn scope(state: &GameState, viewer: Uuid) -> NativeScope<'_> {
    NativeScope::new(state, binding(state, viewer)).unwrap()
}
pub fn command(scope: &NativeScope<'_>, action: crate::player_wire::PlayerAction) -> PlayerCommand {
    PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: scope.basis().clone(),
        action,
    }
}
pub fn main_state(cards: Vec<SeedCard>) -> GameState {
    let seed = seed(cards);
    GameState::initialize_scheduled(
        GameId(90),
        context(&seed.creature_vocabulary),
        seed,
        spellwake_engine::turn::TurnStart {
            number: 1,
            step: spellwake_engine::turn::Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}

pub fn bridged(
    source: &GameState,
    viewer: Uuid,
    action: impl FnOnce(&mut NativeScope<'_>) -> crate::player_wire::PlayerAction,
) -> GameState {
    let before = source.clone();
    let mut view = scope(source, viewer);
    let action = action(&mut view);
    let original = command(&view, action);
    super::projection::verify_view(&mut view, Some(&original));
    let prepared = view.propose(&original).unwrap();
    super::input_codec::verify(&view, &prepared);
    assert_eq!(source, &before);
    prepared.into_parts().3.into_state()
}
