//! Named card games using the current catalogue and the ordinary command boundary.
use super::*;
use spellwake_engine::{
    game::{DecisionReply, NativeCandidate},
    object_identity::{GameObject, ObjectId},
    occurrence::{EntryCompletion, RulesOccurrence, RulesRecord},
    stack_window::StackEntryId,
    targets::TargetRef,
};

mod darksteel_garrison;
mod forked_branch_garami;
mod sacrifice_exile;
mod target_departure;

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn seed(name: &str, card: u64, owner: SeatId, destination: ObjectDestination) -> SeedCard {
    let (inventory, _, registry) = current_catalogue();
    let row = inventory
        .cards
        .iter()
        .find(|row| row.required_by_base_formats && row.name == name)
        .unwrap_or_else(|| panic!("missing catalogue row for {name}"));
    let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
    registry
        .seed(&id, CardId(card), owner, destination)
        .unwrap_or_else(|error| panic!("unable to seed {name}: {error:?}"))
}

fn game(cards: Vec<SeedCard>, mana: &[(SeatId, ManaType, i64)]) -> GameState {
    let (_, vocabulary, registry) = current_catalogue();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        format!(
            "keyword-gameplay-registry{}",
            registry.coverage.registry_version
        ),
        "named-attachment-and-soulshift".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let battlefield = cards
        .iter()
        .filter(|seed| matches!(seed.destination, ObjectDestination::Battlefield { .. }))
        .map(|seed| seed.card)
        .collect();
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        battlefield,
        mana.iter()
            .map(|&(player, mana_type, quantity)| FloatingMana {
                player,
                mana_type,
                amount: quantity.into(),
            })
            .collect(),
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    GameState::initialize_scheduled(
        GameId(191),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}

fn physical(state: &GameState, card: u64) -> &GameObject {
    let cards = state.execution().window().cards();
    cards
        .get(cards.current_identity(CardId(card)).unwrap())
        .unwrap()
}

fn object(state: &GameState, card: u64) -> ObjectId {
    physical(state, card).id()
}

fn payment(amounts: &[(ManaType, i64)]) -> ManaPayment {
    ManaPayment {
        from_lots: Default::default(),
        spend: amounts
            .iter()
            .map(|&(mana_type, quantity)| ManaSpend {
                mana_type,
                amount: quantity.into(),
            })
            .collect(),
    }
}

fn targeted_spell(
    state: &GameState,
    spell: u64,
    target: ObjectId,
    amounts: &[(ManaType, i64)],
) -> GameCommand {
    GameCommand::CastPaid {
        spell: object(state, spell),
        target: TargetRef::Object(target),
        payment: payment(amounts),
    }
}

fn priority(state: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = state.execution().window().phase()
    else {
        panic!("expected priority, got {:?}", state.pending_decision());
    };
    holder
}

fn resolve_top(state: &GameState) -> NativeCandidate {
    assert!(!state.execution().window().entries().is_empty());
    let passed = advance(state, priority(state), GameCommand::Pass);
    passed
        .advance(GameInput {
            basis: passed.basis().clone(),
            actor: priority(&passed),
            command: GameCommand::Pass,
        })
        .unwrap()
}

fn reply(state: &GameState, chooser: SeatId, choice: DecisionReply) -> GameState {
    let pending = state.pending_decision().unwrap();
    assert_eq!(pending.chooser(), chooser);
    advance(
        state,
        chooser,
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
}

fn assert_all_targets_illegal(candidate: &NativeCandidate, expected: StackEntryId) {
    assert!(candidate.rules_journal().records.iter().any(|record| {
        let RulesRecord::Event(event) = record else {
            return false;
        };
        event.occurrences.iter().any(|occurrence| {
            matches!(occurrence.value, RulesOccurrence::EntryCompleted {
                entry,
                completion: EntryCompletion::AllTargetsIllegal,
                ..
            } if entry == expected)
        })
    }));
    assert!(candidate.state().pending_decision().is_none());
}
