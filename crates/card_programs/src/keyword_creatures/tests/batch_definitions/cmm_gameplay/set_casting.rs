//! Exercise the everyday hand-to-stack or hand-to-battlefield route for each set card.
use super::*;
use serde::Deserialize;
use spellwake_engine::{
    attachment::AttachmentKind,
    effects::{
        LibraryChoiceKind, LibraryTopReply, OptionalEffectChoice, PileChoice,
        ResolutionPaymentReply,
    },
    game::GameError,
    mana::CastMode,
};
use std::time::Instant;

const FIXTURE_NAMES: &[&str] = &[
    "Bump in the Night",
    "Fencing Ace",
    "Forest",
    "Island",
    "Mountain",
    "Palladium Myr",
    "Plains",
    "Spitebellows",
    "Swamp",
];

#[derive(Deserialize)]
struct SetCard {
    oracle_id: String,
    name: String,
}

#[derive(Deserialize)]
struct SetManifest {
    set: String,
    cards: Vec<SetCard>,
}

#[derive(Debug)]
enum CastingProbe {
    LandPlayed,
    Resolved,
    VerifiedOutcome,
    DecisionRequired(String),
    NeedsFixture(String),
}

fn try_game(
    state: &GameState,
    actor: SeatId,
    command: GameCommand,
) -> Result<GameState, GameError> {
    state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        })
        .map(NativeCandidate::into_state)
}

fn decision_name(state: &GameState) -> Option<String> {
    let frame = format!("{:?}", state.pending_decision()?.frame());
    Some(frame.split(['(', ' ', '{']).next().unwrap().to_owned())
}

fn probe_reply(state: &GameState, card: &str) -> Option<(SeatId, GameCommand)> {
    let pending = state.pending_decision()?;
    let choice = match pending.frame() {
        RuleFrame::ResolutionPayment(_) if card == "Frightful Delusion" => {
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline)
        }
        RuleFrame::ResolutionSelect(frame) => {
            DecisionReply::SelectResolution(frame.domain()[..frame.required()].to_vec())
        }
        RuleFrame::RevealedPiles(frame) => match frame.choice() {
            PileChoice::OptionalCreatureToBattlefield { eligible, .. } => {
                DecisionReply::SelectResolution(eligible.first().copied().into_iter().collect())
            }
            PileChoice::BottomOrder(cards) | PileChoice::GraveyardOrder(cards) => {
                DecisionReply::OrderResolutionGraveyard(cards.clone())
            }
            PileChoice::OneToHand(cards) | PileChoice::OneToHandRestBottom(cards) => {
                DecisionReply::SelectResolution(vec![*cards.first()?])
            }
            _ => return None,
        },
        RuleFrame::ResolutionGraveyardOrder(frame) => {
            DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec())
        }
        RuleFrame::LinkedReturnOrder(frame) => {
            DecisionReply::LinkedReturnOrder(frame.objects().to_vec())
        }
        RuleFrame::TimestampOrder(frame) => DecisionReply::TimestampOrder(
            frame.members().iter().map(|member| member.member).collect(),
        ),
        RuleFrame::ZoneReplacement(frame) if card == "Evil Twin" => {
            let spellwake_engine::effects::ZoneReplacementRequest::EntryCopy { donors, .. } =
                frame.request()
            else {
                return None;
            };
            DecisionReply::ZoneReplacement(
                spellwake_engine::effects::ZoneReplacementReply::EntryCopy(donors.first().copied()),
            )
        }
        RuleFrame::TriggerTarget(frame) if frame.modal().is_none() => {
            if let Some((minimum, _)) = frame.permanent_target_bounds() {
                DecisionReply::TriggerPermanentTargets(frame.domain().get(..minimum)?.to_vec())
            } else if frame.ordered_any_target_bounds().is_none() {
                let preferred = match card {
                    "Midvast Protector" => Some(TargetRef::Object(physical(state, 2).id())),
                    "Kessig Malcontents" => Some(TargetRef::Player(B)),
                    "Zealous Conscripts" => Some(TargetRef::Object(physical(state, 3).id())),
                    _ => None,
                };
                DecisionReply::TriggerTarget(
                    preferred
                        .filter(|target| frame.domain().contains(target))
                        .or_else(|| frame.domain().first().copied())?,
                )
            } else {
                return None;
            }
        }
        RuleFrame::EntryChoice(frame)
            if frame.rule() == spellwake_engine::land::EntryTap::ChooseNonlandCardName =>
        {
            frame.card_names().first()?;
            DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::CardName(
                spellwake_engine::land::CardNameIndex(0),
            ))
        }
        RuleFrame::EntryChoice(frame)
            if frame.rule() == spellwake_engine::land::EntryTap::ChooseCreatureType =>
        {
            DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::CreatureType(
                state
                    .execution()
                    .creature_vocabulary()
                    .resolve_label("Human")?,
            ))
        }
        RuleFrame::LibraryTop(frame)
            if frame.kind() == LibraryChoiceKind::Search && frame.is_bounded_batch_search() =>
        {
            DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
                selected: frame
                    .eligible_results()
                    .iter()
                    .take(frame.search_batch_limit())
                    .copied()
                    .collect(),
                colors: vec![],
            })
        }
        RuleFrame::LibraryTop(frame)
            if matches!(
                card,
                "Borderland Ranger" | "Increasing Ambition" | "Memory's Journey" | "Caravan Vigil"
            ) =>
        {
            DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: frame.eligible_results().first().copied(),
            })
        }
        RuleFrame::ResolutionManaColor(frame) if frame.split().is_none() => {
            DecisionReply::ManaColor(*frame.colors().first()?)
        }
        RuleFrame::OptionalEffect(frame) => DecisionReply::OptionalEffect(
            if frame.options().contains(&OptionalEffectChoice::Take) {
                OptionalEffectChoice::Take
            } else {
                *frame.options().first()?
            },
        ),
        RuleFrame::Triggers(frame) => DecisionReply::OrderTriggers(frame.instances().to_vec()),
        _ => return None,
    };
    Some((
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    ))
}

/// Derive the ordinary payment from the pinned source, with X=0 and the first side of hybrids.
fn printed_payment(cost: &str, generic_discount: i64) -> Option<Vec<(ManaType, i64)>> {
    let mut amounts = BTreeMap::<_, i64>::new();
    let mut generic = 0;
    for symbol in cost.strip_prefix('{')?.strip_suffix('}')?.split("}{") {
        let first = symbol.split('/').next()?;
        let (kind, n) = match first {
            "W" => (ManaType::White, 1),
            "U" => (ManaType::Blue, 1),
            "B" => (ManaType::Black, 1),
            "R" => (ManaType::Red, 1),
            "G" => (ManaType::Green, 1),
            "C" => (ManaType::Colourless, 1),
            "X" => continue,
            number => {
                generic += number.parse::<i64>().ok()?;
                continue;
            }
        };
        *amounts.entry(kind).or_default() += n;
    }
    *amounts.entry(ManaType::Colourless).or_default() += (generic - generic_discount).max(0);
    Some(
        amounts
            .into_iter()
            .filter(|&(_, amount)| amount > 0)
            .collect(),
    )
}

fn set_game(
    card: SeedCard,
    price: &[(ManaType, i64)],
    fixtures: &BTreeMap<&str, &BoundProgram>,
    focused: bool,
    name: &str,
) -> GameState {
    let named = |name, number, owner, destination| SeedCard {
        card: CardId(number),
        owner,
        destination,
        program: fixtures[name].program().clone(),
    };
    let field = |id, player, program| {
        seed(
            id,
            player,
            ObjectDestination::Battlefield { controller: player },
            program,
        )
    };
    let named_at = |name, id, player| {
        named(
            name,
            id,
            player,
            ObjectDestination::Battlefield { controller: player },
        )
    };
    let mut flash_creature =
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    flash_creature.printed_keywords.insert(Flash);
    let mut cards = vec![
        card,
        creature(2, A, 2, 2, &[]),
        creature(
            3,
            B,
            if name == "Smite the Monstrous" { 4 } else { 2 },
            2,
            &[],
        ),
        creature(4, C, 2, 2, &[]),
        field(5, A, Program::artifact(None, vec![])),
        field(6, B, Program::artifact(None, vec![])),
        field(7, C, Program::enchantment(None)),
        named_at(
            if matches!(name, "Righteous Blow" | "Rebuke" | "Feeling of Dread") {
                "Plains"
            } else {
                "Forest"
            },
            8,
            A,
        ),
        named_at("Swamp", 9, B),
        named_at("Island", 10, C),
        named("Plains", 11, A, ObjectDestination::Hand),
        named("Mountain", 12, A, ObjectDestination::Graveyard),
        named("Forest", 13, B, ObjectDestination::Graveyard),
        named("Fencing Ace", 14, A, ObjectDestination::Graveyard),
        named("Fencing Ace", 15, C, ObjectDestination::Graveyard),
        named("Spitebellows", 16, A, ObjectDestination::Graveyard),
        named("Forest", 17, A, ObjectDestination::LibraryBottom),
        named("Island", 18, A, ObjectDestination::LibraryBottom),
        named("Spitebellows", 19, A, ObjectDestination::LibraryBottom),
        named("Palladium Myr", 20, A, ObjectDestination::Hand),
        seed(21, B, ObjectDestination::Hand, flash_creature),
        seed(
            22,
            B,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SingleCreature,
                Instruction::Damage { amount: 1.into() },
            ),
        ),
    ];
    if matches!(
        name,
        "Crushing Vines" | "Eaten by Spiders" | "Geistcatcher's Rig"
    ) {
        cards[2]
            .program
            .printed_keywords
            .insert(spellwake_engine::characteristics::KeywordAbility::Flying);
    }
    if name == "Zombie Apocalypse" {
        let zombie = dated_sources().1.resolve_label("Zombie").unwrap();
        for card in cards
            .iter_mut()
            .filter(|card| matches!(card.card, CardId(14 | 16)))
        {
            card.program.creature_subtypes.insert(zombie);
        }
    }
    if name == "Skaab Ruinator" {
        cards.push(named("Fencing Ace", 24, A, ObjectDestination::Graveyard));
    }
    if name == "Runic Repetition" {
        cards.push(named("Bump in the Night", 24, A, ObjectDestination::Exile));
    }
    if name == "Mystic Retrieval" {
        cards[13] = seed(
            14,
            A,
            ObjectDestination::Graveyard,
            free_instant(
                BodyTarget::None,
                Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                },
            ),
        );
    }
    if matches!(name, "Rebuke" | "Burning Oil") {
        let land = if name == "Rebuke" {
            "Plains"
        } else {
            "Mountain"
        };
        cards[7] = named_at(land, 8, A);
        cards.push(named_at("Forest", 25, A));
        cards.push(named_at("Forest", 26, A));
    }
    if name == "Night Terrors" {
        cards.push(named("Forest", 25, B, ObjectDestination::Hand));
    }
    if name == "Seek the Horizon" {
        cards.push(named("Swamp", 25, A, ObjectDestination::LibraryBottom));
    }
    if name == "Appetite for Brains" {
        cards.push(named("Spitebellows", 25, B, ObjectDestination::Hand));
    }
    if name == "Increasing Vengeance" {
        cards.push(seed(
            24,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                },
            ),
        ));
    }
    if name == "Second Guess" {
        cards.push(seed(
            24,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::None,
                Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(0.into()),
                },
            ),
        ));
    }
    if name == "Stolen Goods" {
        cards.push(named("Swamp", 26, B, ObjectDestination::LibraryBottom));
        cards.push(named(
            "Spitebellows",
            27,
            B,
            ObjectDestination::LibraryBottom,
        ));
    }
    if name == "Outwit" {
        cards.push(seed(
            23,
            B,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SinglePlayer,
                Instruction::Damage { amount: 1.into() },
            ),
        ));
    }
    if focused {
        let human = dated_sources().1.resolve_label("Human").unwrap();
        cards[1].program.creature_subtypes.insert(human);
    }
    for player in [A, B, C] {
        for i in 0..8 {
            cards.push(seed(
                30 + 8 * u64::from(player.0) + i,
                player,
                ObjectDestination::LibraryBottom,
                Program::land(BTreeSet::new(), vec![]),
            ));
        }
    }
    let step = if matches!(name, "Righteous Blow" | "Rebuke" | "Burning Oil") {
        Step::Untap
    } else {
        Step::PrecombatMain
    };
    if name == "Tyrant of Discord" {
        // One opposing permanent makes this probe independent of host entropy.
        for card in cards
            .iter_mut()
            .filter(|card| matches!(card.card, CardId(3 | 6)))
        {
            card.destination = ObjectDestination::Battlefield { controller: C };
        }
    }
    game(vec![A, B, C], cards, price, step)
}

fn cast_attempts(
    name: &str,
    state: &GameState,
    program: &Program,
    price: &[(ManaType, i64)],
    focused: bool,
) -> Vec<GameCommand> {
    let spell = physical(state, 1).id();
    let mut base = CastingPayment {
        mana: (!price.is_empty()).then(|| spend(price)),
        chosen_x: matches!(
            name,
            "Death Wind"
                | "Bonfire of the Damned"
                | "Killing Wave"
                | "Divine Deflection"
                | "Increasing Confusion"
                | "Harvest Pyre"
                | "Mikaeus, the Lunarch"
        )
        .then(|| 2.into()),
        ..Default::default()
    };
    if let ProgramKind::Spell(spell) = &program.kind
        && let Some(total) = spell.body.target_division_total()
    {
        base.target_division = vec![total];
    }
    if program.graveyard_exile_x_cost {
        let count = base
            .chosen_x
            .as_ref()
            .and_then(|x| x.to_usize())
            .unwrap_or_default();
        base.objects.extend(
            [14, 16, 24].into_iter().take(count).map(|number| {
                CastResource::ExileGraveyardCardForSpell(physical(state, number).id())
            }),
        );
    }
    if program.graveyard_creature_exile_cost > 0 {
        base.objects.extend(
            [14, 16, 24]
                .into_iter()
                .take(usize::from(program.graveyard_creature_exile_cost))
                .map(|number| {
                    CastResource::ExileGraveyardCreatureForSpell(physical(state, number).id())
                }),
        );
    }
    if program.discard_cast_cost {
        base.objects
            .push(CastResource::DiscardForSpell(physical(state, 11).id()));
    }
    if program.tap_creatures_cast_cost {
        base.objects
            .push(CastResource::TapCreatureForSpell(physical(state, 2).id()));
    }
    if program.sacrifice_creature_cast_cost {
        base.objects
            .push(CastResource::SacrificeForSpell(physical(state, 2).id()));
    }
    if program.reveal_colorless_creature_cast_cost {
        base.objects.push(CastResource::RevealColorlessCreature(
            physical(state, 20).id(),
        ));
    }
    let object = |n| TargetRef::Object(physical(state, n).id());
    let mut targets: Vec<_> = [B, C, A]
        .into_iter()
        .map(TargetRef::Player)
        .chain((2..=10).chain(12..=16).map(object))
        .chain(
            state
                .execution()
                .window()
                .entries()
                .iter()
                .map(|entry| TargetRef::Stack(entry.id())),
        )
        .collect();
    if name == "Runic Repetition" {
        targets.insert(0, object(24));
    }
    if name == "Pillar of Flame" {
        targets.insert(0, object(3));
    }
    let single = |target, payment: CastingPayment| GameCommand::CastWithResources {
        spell,
        target,
        payment,
    };
    let ProgramKind::Spell(definition) = &program.kind else {
        if matches!(
            program.permanent().and_then(|p| p.attachment),
            Some(AttachmentKind::Aura(_))
        ) {
            return targets
                .into_iter()
                .map(|target| single(Some(target), base.clone()))
                .collect();
        }
        return vec![single(None, base)];
    };
    let fixed = match name {
        "Decimate" => vec![object(5), object(3), object(7), object(9)],
        "Ram Through" | "Peel from Reality" | "Prey Upon" | "Blood Feud" => {
            vec![object(2), object(3)]
        }
        "Into the Maw of Hell" => vec![object(9), object(3)],
        "Lost in the Mist" => vec![
            TargetRef::Stack(state.execution().window().entries().last().unwrap().id()),
            object(3),
        ],
        "Grave Exchange" => vec![object(14), TargetRef::Player(B)],
        "Rise of the Eldrazi" => vec![object(6), TargetRef::Player(A)],
        _ => Vec::new(),
    };
    if !fixed.is_empty() {
        return vec![single(
            None,
            CastingPayment {
                fixed_targets: fixed,
                ..base
            },
        )];
    }
    if name == "Rain of Thorns" {
        return vec![single(
            None,
            CastingPayment {
                modes: [5, 7, 9]
                    .into_iter()
                    .enumerate()
                    .map(|(mode, card)| CastMode {
                        mode: u8::try_from(mode).unwrap(),
                        target: Some(object(card)),
                        targets: vec![],
                    })
                    .collect(),
                ..base
            },
        )];
    }
    if let Some(modes) = &definition.modes {
        if modes.minimum > 1 && !modes.allow_repeat {
            return vec![];
        }
        return std::iter::once(&definition.body)
            .chain(&modes.additional)
            .enumerate()
            .flat_map(|(mode, body)| {
                let candidates = if body.target == BodyTarget::None {
                    vec![None]
                } else {
                    targets.iter().copied().map(Some).collect()
                };
                candidates.into_iter().map(move |target| CastMode {
                    mode: u8::try_from(mode).unwrap(),
                    target,
                    targets: vec![],
                })
            })
            .map(|mode| {
                single(
                    None,
                    CastingPayment {
                        modes: vec![mode; usize::from(modes.minimum)],
                        ..base.clone()
                    },
                )
            })
            .collect();
    }
    if name == "Guff Rewrites History" {
        return vec![single(
            None,
            CastingPayment {
                permanent_targets: vec![object(5), object(6), object(4)],
                ..base
            },
        )];
    }
    if let Some((minimum, _)) = definition
        .body
        .permanent_target_bounds()
        .or(definition.body.ordered_any_target_bounds())
    {
        let groups: Vec<Vec<_>> = match minimum {
            0 if matches!(name, "Into the Void" | "Ghostform") => vec![vec![object(2), object(3)]],
            0 if focused => vec![vec![object(2)], vec![]],
            0 => vec![vec![]],
            1 => targets.iter().map(|&target| vec![target]).collect(),
            2 => vec![
                vec![object(14), object(16)],
                vec![object(3), object(4)],
                vec![object(2), object(5)],
            ],
            6 => vec![(2..=7).map(object).collect()],
            _ => vec![],
        };
        return groups
            .into_iter()
            .map(|group| {
                single(
                    None,
                    CastingPayment {
                        permanent_targets: group,
                        ..base.clone()
                    },
                )
            })
            .collect();
    }
    if definition.body.target == BodyTarget::None {
        vec![single(None, base)]
    } else {
        targets
            .into_iter()
            .map(|target| single(Some(target), base.clone()))
            .collect()
    }
}

/// The source's ordinary price reductions in this specific three-seat fixture.
fn generic_discount(name: &str) -> i64 {
    match name {
        "Blasphemous Act" => 3, // Three battlefield creatures.
        "Curtains' Call" | "Divergent Transformations" | "Sublime Exhalation" => 2,
        // Two opposing players above, a single 2/2 controlled creature or two own
        // graveyard creature cards below.
        "Ghalta, Primal Hunger" | "The Great Henge" | "Karador, Ghost Chieftain" | "Ghoultree" => 2,
        _ => 0,
    }
}

fn verify_set_outcome(name: &str, initial: &GameState, end: &GameState) -> bool {
    let zone = |id| physical(end, id).zone();
    let count = |state: &GameState, zone| {
        state
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|card| card.zone() == zone)
            .count()
    };
    let new_hand_size = count(end, ObjectZone::Hand(A));
    match name {
        "Seek the Horizon" => {
            for id in [17, 18, 25] {
                assert_eq!(zone(id), ObjectZone::Hand(A));
            }
            assert!(end.entropy().is_some_and(|tape| tape.position() > 0));
        }
        "Borderland Ranger" => assert!(
            [17, 18]
                .into_iter()
                .any(|card| zone(card) == ObjectZone::Hand(A))
        ),
        "Midvast Protector" => {
            let current = end
                .execution()
                .current_characteristics(physical(end, 2).id())
                .unwrap()
                .unwrap();
            let protection = [
                ProtectionFromWhite,
                ProtectionFromBlue,
                ProtectionFromBlack,
                ProtectionFromRed,
                ProtectionFromGreen,
            ];
            assert_eq!(
                protection
                    .into_iter()
                    .filter(|keyword| current.has_keyword(*keyword))
                    .count(),
                1
            );
        }
        "Appetite for Brains" => {
            assert_eq!(zone(25), ObjectZone::Exile);
            assert_eq!(zone(21), ObjectZone::Hand(B));
            assert_eq!(zone(22), ObjectZone::Hand(B));
        }
        "Grave Exchange" => {
            assert_eq!(zone(14), ObjectZone::Hand(A));
            assert_eq!(zone(3), ObjectZone::Graveyard(B));
        }
        "Eaten by Spiders" => assert_eq!(zone(3), ObjectZone::Graveyard(B)),
        "Stolen Goods" => {
            assert_eq!(zone(26), ObjectZone::Exile);
            assert_eq!(zone(27), ObjectZone::Exile);
            assert!(
                end.execution()
                    .may_cast_free_from_exile(physical(end, 27).id(), A)
            );
            assert!(
                !end.execution()
                    .may_cast_free_from_exile(physical(end, 27).id(), B)
            );
            assert_eq!(
                count(end, ObjectZone::Library(B)),
                count(initial, ObjectZone::Library(B)) - 2
            );
        }
        "Burn at the Stake" => {
            assert_eq!(physical(end, 2).tapped(), Some(true));
            assert_eq!(
                end.execution().life_total(B).unwrap(),
                &initial.execution().life_total(B).unwrap().minus(&3.into())
            );
        }
        "Battle Hymn" => assert_eq!(end.execution().mana(A, ManaType::Red), 1.into()),
        "Dangerous Wager" => {
            assert_eq!(zone(11), ObjectZone::Graveyard(A));
            assert_eq!(zone(20), ObjectZone::Graveyard(A));
            assert_eq!(new_hand_size, 2);
        }
        "Death Wind" | "Human Frailty" => assert_eq!(zone(2), ObjectZone::Graveyard(A)),
        "Runic Repetition" => assert_eq!(zone(24), ObjectZone::Hand(A)),
        "Dreadwaters" => assert_eq!(
            count(initial, ObjectZone::Library(B)) - count(end, ObjectZone::Library(B)),
            1
        ),
        "Geist Snatch" => {
            assert_eq!(zone(21), ObjectZone::Graveyard(B));
            let spirit = dated_sources().1.resolve_label("Spirit").unwrap();
            let token = end
                .execution()
                .window()
                .cards()
                .iter()
                .find(|card| {
                    card.is_token()
                        && card.zone() == ObjectZone::Battlefield
                        && card.controller() == Some(A)
                })
                .unwrap();
            let current = end
                .execution()
                .current_characteristics(token.id())
                .unwrap()
                .unwrap();
            assert_eq!(
                current.current_colors,
                [spellwake_engine::characteristics::Color::Blue].into()
            );
            assert!(current.current_creature_subtypes.contains(&spirit));
            assert!(current.has_keyword(Flying));
            assert_eq!(
                end.execution().creature(token.id()).unwrap().unwrap().power,
                1.into()
            );
            assert_eq!(
                end.execution()
                    .creature(token.id())
                    .unwrap()
                    .unwrap()
                    .toughness,
                1.into()
            );
        }
        "Into the Void" => {
            assert_eq!(zone(2), ObjectZone::Hand(A));
            assert_eq!(zone(3), ObjectZone::Hand(B));
        }
        "Kessig Malcontents" => assert_eq!(end.execution().life_total(B), Some(&18.into())),
        "Mass Appeal" => assert_eq!(new_hand_size, count(initial, ObjectZone::Hand(A))),
        "Mental Agony" => {
            assert_eq!(end.execution().life_total(B), Some(&18.into()));
            assert_eq!(zone(21), ObjectZone::Graveyard(B));
            assert_eq!(zone(22), ObjectZone::Graveyard(B));
        }
        "Righteous Blow" => assert_eq!(zone(2), ObjectZone::Graveyard(A)),
        "Second Guess" => assert_eq!(zone(21), ObjectZone::Graveyard(B)),
        "Dissipate" => assert_eq!(zone(22), ObjectZone::Exile),
        "Frightful Delusion" => {
            assert_eq!(zone(22), ObjectZone::Graveyard(B));
            assert_eq!(zone(21), ObjectZone::Graveyard(B));
        }
        "Mikaeus, the Lunarch" => {
            assert_eq!(zone(1), ObjectZone::Battlefield);
            assert_eq!(
                end.execution()
                    .creature(physical(end, 1).id())
                    .unwrap()
                    .unwrap()
                    .power,
                2.into()
            );
        }
        "Mulch" => {
            for land in [17, 18, 30] {
                assert_eq!(zone(land), ObjectZone::Hand(A));
            }
            assert_eq!(zone(19), ObjectZone::Graveyard(A));
        }
        "Tracker's Instincts" => {
            assert_eq!(zone(19), ObjectZone::Hand(A));
            for land in [17, 18, 30] {
                assert_eq!(zone(land), ObjectZone::Graveyard(A));
            }
        }
        "Night Terrors" => {
            let chosen = [21, 22]
                .into_iter()
                .filter(|&card| zone(card) == ObjectZone::Exile)
                .count();
            assert_eq!(chosen, 1);
            assert_eq!(zone(25), ObjectZone::Hand(B));
        }
        "Increasing Confusion" => {
            assert_eq!(
                count(end, ObjectZone::Library(B)) + 2,
                count(initial, ObjectZone::Library(B))
            );
        }
        "Make a Wish" => {
            assert_eq!(
                [12, 14, 16]
                    .into_iter()
                    .filter(|&card| zone(card) == ObjectZone::Hand(A))
                    .count(),
                2
            );
        }
        "Zombie Apocalypse" => {
            assert_eq!(zone(2), ObjectZone::Graveyard(A));
            assert_eq!(zone(14), ObjectZone::Graveyard(A));
            assert_eq!(zone(16), ObjectZone::Battlefield);
            assert_eq!(physical(end, 16).tapped(), Some(true));
        }
        "Increasing Vengeance" => {
            assert_eq!(new_hand_size, count(initial, ObjectZone::Hand(A)) + 1);
        }
        "Tribute to Hunger" => {
            assert_eq!(zone(3), ObjectZone::Graveyard(B));
            assert_eq!(end.execution().life_total(A), Some(&22.into()));
        }
        "Forge Devil" => {
            assert_eq!(end.execution().life_total(A), Some(&19.into()));
            assert_eq!(
                end.execution()
                    .battlefield_marked_damage(physical(end, 2).id()),
                Some(1.into())
            );
        }
        "Outwit" => {
            assert_eq!(zone(23), ObjectZone::Graveyard(B));
            assert_eq!(end.execution().life_total(A), Some(&20.into()));
        }
        "Pillar of Flame" => assert_eq!(zone(3), ObjectZone::Exile),
        "Rain of Thorns" => {
            assert_eq!(zone(5), ObjectZone::Graveyard(A));
            assert_eq!(zone(7), ObjectZone::Graveyard(C));
            assert_eq!(zone(9), ObjectZone::Graveyard(B));
        }
        _ => return false,
    }
    true
}

fn probe(
    card: SeedCard,
    face: &spellwake_content::CatalogFace,
    fixtures: &BTreeMap<&str, &BoundProgram>,
    focused: bool,
) -> CastingProbe {
    let program = card.program.clone();
    let printed_types = face
        .type_line
        .as_deref()
        .unwrap()
        .split(" — ")
        .next()
        .unwrap();
    let land = printed_types.split_whitespace().any(|word| word == "Land");
    let mut price = if land {
        Vec::new()
    } else {
        let Some(payment) = face
            .mana_cost
            .as_deref()
            .and_then(|cost| printed_payment(cost, generic_discount(&face.name)))
        else {
            return CastingProbe::NeedsFixture("ordinary printed price unavailable".into());
        };
        payment
    };
    match face.name.as_str() {
        "Death Wind"
        | "Killing Wave"
        | "Divine Deflection"
        | "Increasing Confusion"
        | "Mikaeus, the Lunarch" => price.push((ManaType::Colourless, 2)),
        "Bonfire of the Damned" => price.push((ManaType::Colourless, 4)),
        _ => {}
    }
    let mut initial = set_game(card, &price, fixtures, focused, &face.name);
    if matches!(
        face.name.as_str(),
        "Righteous Blow" | "Rebuke" | "Burning Oil"
    ) {
        let declare = combat_frame(&initial, true);
        initial = reply(
            &declare,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: physical(&declare, 2).id(),
                defender: B,
                defending_planeswalker: None,
            }]),
        )
        .into_state();
    }
    if face.name == "Righteous Blow" && initial.execution().mana(A, ManaType::White) == 0.into() {
        initial = command(
            &initial,
            A,
            GameCommand::ActivateMana {
                source: physical(&initial, 8).id(),
                ability: spellwake_engine::mana::ManaAbilityRef::BasicLand(
                    spellwake_engine::land::BasicLandType::Plains,
                ),
            },
        )
        .into_state();
    }
    if matches!(face.name.as_str(), "Rebuke" | "Burning Oil") {
        use spellwake_engine::{land::BasicLandType, mana::ManaAbilityRef};
        let lands = if face.name == "Rebuke" {
            vec![
                (8, BasicLandType::Plains),
                (25, BasicLandType::Forest),
                (26, BasicLandType::Forest),
            ]
        } else {
            vec![(8, BasicLandType::Mountain), (25, BasicLandType::Forest)]
        };
        for (id, kind) in lands {
            initial = command(
                &initial,
                A,
                GameCommand::ActivateMana {
                    source: physical(&initial, id).id(),
                    ability: ManaAbilityRef::BasicLand(kind),
                },
            )
            .into_state();
        }
        price = if face.name == "Rebuke" {
            vec![(ManaType::White, 1), (ManaType::Green, 2)]
        } else {
            vec![(ManaType::Red, 1), (ManaType::Green, 1)]
        };
    }
    if land {
        let object = physical(&initial, 1).id();
        return match try_game(&initial, A, GameCommand::PlayLand { land: object }) {
            Ok(mut result) => {
                if let Some((actor, reply)) = probe_reply(&result, &face.name) {
                    result = match try_game(&result, actor, reply) {
                        Ok(state) => state,
                        Err(error) => return CastingProbe::NeedsFixture(format!("{error:?}")),
                    };
                }
                if let Some(decision) = decision_name(&result) {
                    return CastingProbe::DecisionRequired(decision);
                }
                assert_eq!(physical(&result, 1).zone(), ObjectZone::Battlefield);
                CastingProbe::LandPlayed
            }
            Err(error) => CastingProbe::NeedsFixture(format!("{error:?}")),
        };
    }
    if face.name == "Second Guess" {
        initial = round(cast_untargeted(&initial, 24, &[]).into_state()).into_state();
    }
    if face.name == "Increasing Vengeance" {
        initial = command(
            &initial,
            A,
            GameCommand::CastUntargeted {
                spell: physical(&initial, 24).id(),
                payment: None,
            },
        )
        .into_state();
    }
    let respond_to = match face.name.as_str() {
        "Bone to Ash" | "Exclude" | "Geist Snatch" | "Second Guess" => Some(21),
        "Outwit" => Some(23),
        "Commandeer"
        | "Lost in the Mist"
        | "Counterspell"
        | "Counterlash"
        | "Dissipate"
        | "Frightful Delusion"
        | "Deflecting Swat"
        | "Fierce Guardianship"
        | "Imp's Mischief"
        | "Not of This World" => Some(22),
        _ => match &program.kind {
            ProgramKind::Spell(definition)
                if program.has_type(spellwake_engine::program::CardType::Instant) =>
            {
                match definition.body.target {
                    BodyTarget::SingleSpell(filter)
                        if filter == spellwake_engine::program::SpellFilter::creature() =>
                    {
                        Some(21)
                    }
                    BodyTarget::SingleSpell(filter)
                        if filter == spellwake_engine::program::SpellFilter::any()
                            || filter == spellwake_engine::program::SpellFilter::noncreature()
                            || filter
                                == spellwake_engine::program::SpellFilter::any()
                                    .single_target()
                            || filter
                                == spellwake_engine::program::SpellFilter::copyable_instruction(
                                ) =>
                    {
                        Some(22)
                    }
                    _ => None,
                }
            }
            _ => None,
        },
    };
    if let Some(other) = respond_to {
        initial = pass(&initial).into_state();
        initial = if other == 21 {
            command(
                &initial,
                B,
                GameCommand::CastUntargeted {
                    spell: physical(&initial, other).id(),
                    payment: None,
                },
            )
            .into_state()
        } else {
            let target = if other == 23 {
                TargetRef::Player(A)
            } else {
                TargetRef::Object(physical(&initial, 2).id())
            };
            targeted(&initial, B, other, target).into_state()
        };
        for _ in 0..2 {
            initial = pass(&initial).into_state();
        }
    }
    let mut first_error = None;
    for attempt in cast_attempts(&face.name, &initial, &program, &price, focused) {
        let mut cast = match try_game(&initial, A, attempt) {
            Ok(cast) => cast,
            Err(error) => {
                first_error.get_or_insert_with(|| format!("{error:?}"));
                continue;
            }
        };
        if !focused && let Some(decision) = decision_name(&cast) {
            return CastingProbe::DecisionRequired(decision);
        }
        assert_eq!(physical(&cast, 1).zone(), ObjectZone::Stack);
        for &(kind, _) in &price {
            assert_eq!(cast.execution().mana(A, kind), 0.into());
        }
        if matches!(
            face.name.as_str(),
            "Desperate Ravings" | "Archangel's Light" | "Make a Wish" | "Memory's Journey"
        ) {
            cast = cast
                .supply_entropy(cast.basis(), vec![0; 32])
                .unwrap()
                .into_state();
        }
        for _ in 0..96 {
            if let Some(decision) = decision_name(&cast) {
                if focused
                    && matches!(cast.pending_decision().map(|pending| pending.frame()),
                        Some(RuleFrame::LibraryTop(frame)) if (frame.kind() == LibraryChoiceKind::Search
                            && frame.is_bounded_batch_search()) || matches!(face.name.as_str(),
                                "Borderland Ranger" | "Increasing Ambition" | "Memory's Journey" | "Caravan Vigil"))
                {
                    // The host owns shuffle input. Supply a fixed tape for this fixture.
                    match cast.supply_entropy(cast.basis(), vec![0; 16]) {
                        Ok(next) => cast = next.into_state(),
                        Err(error) => {
                            return CastingProbe::NeedsFixture(format!(
                                "search shuffle: {error:?}"
                            ));
                        }
                    }
                }
                if focused && let Some((actor, command)) = probe_reply(&cast, &face.name) {
                    match try_game(&cast, actor, command) {
                        Ok(next) => {
                            cast = next;
                            continue;
                        }
                        Err(error) => {
                            return CastingProbe::NeedsFixture(format!("{decision}: {error:?}"));
                        }
                    }
                }
                return CastingProbe::DecisionRequired(decision);
            }
            if physical(&cast, 1).zone() != ObjectZone::Stack
                && (!focused || cast.execution().window().entries().is_empty())
            {
                assert_ne!(physical(&cast, 1).zone(), ObjectZone::Hand(A));
                return if focused && verify_set_outcome(&face.name, &initial, &cast) {
                    CastingProbe::VerifiedOutcome
                } else {
                    CastingProbe::Resolved
                };
            }
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                cast.execution().window().phase()
            else {
                return CastingProbe::NeedsFixture("game paused before resolution".into());
            };
            match try_game(&cast, holder, GameCommand::Pass) {
                Ok(next) => cast = next,
                Err(error) => return CastingProbe::NeedsFixture(format!("resolution: {error:?}")),
            }
        }
        return CastingProbe::NeedsFixture(
            "spell remained on the stack through 96 priority passes".into(),
        );
    }
    CastingProbe::NeedsFixture(
        first_error.unwrap_or_else(|| "selected cast resources required".into()),
    )
}

#[test]
#[ignore = "requires dated Oracle source, creature vocabulary and Commander Masters set manifest"]
fn cmm_set_exercises_normal_play_for_every_oracle_identity() {
    let path = std::env::var_os("SPELLWAKE_COMMANDER_MASTERS_SET").expect("dated set manifest");
    report_set_casts(path, Some(737), "CMM_CAST", true);
}

#[test]
#[ignore = "requires dated Oracle source, creature vocabulary and Avacyn Restored set manifest"]
fn avr_set_probes_normal_play_for_each_executable_definition() {
    let path = std::env::var_os("SPELLWAKE_AVACYN_RESTORED_SET").expect("dated set manifest");
    if let Some(output) = std::env::var_os("SPELLWAKE_REGISTRY_COVERAGE_OUTPUT") {
        let file = std::io::BufWriter::new(std::fs::File::create(output).unwrap());
        serde_json::to_writer(
            file,
            &serde_json::json!({"status": "success", "data": current_catalogue().2.coverage()}),
        )
        .unwrap();
    }
    let passed = report_set_casts(path, Some(234), "AVR_CAST", false);
    if std::env::var_os("SPELLWAKE_AVR_REQUIRE_ALL").as_deref() == Some(std::ffi::OsStr::new("1")) {
        assert_eq!(
            passed, 234,
            "cards that passed Avacyn Restored baseline gameplay"
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source, creature vocabulary and set manifests"]
fn set_batch_probes_normal_play_for_each_executable_definition() {
    let paths = std::env::var_os("SPELLWAKE_SET_MANIFESTS").expect("dated set manifests");
    for path in std::env::split_paths(&paths) {
        report_set_casts(path.into_os_string(), None, "SET_CAST", false);
    }
}

fn report_set_casts(
    path: std::ffi::OsString,
    expected: Option<usize>,
    prefix: &str,
    require_all: bool,
) -> usize {
    let set: SetManifest = serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();
    assert!(!set.cards.is_empty(), "empty set manifest");
    if let Some(expected) = expected {
        assert_eq!(set.cards.len(), expected);
    }
    let loaded_at = Instant::now();
    let (inventory, vocabulary) = dated_sources();
    let load = loaded_at.elapsed();
    let indexed_at = Instant::now();
    let by_id = inventory
        .cards
        .iter()
        .filter(|source| source.required_by_base_formats)
        .map(|source| (source.oracle_id.as_deref().unwrap(), source))
        .collect::<BTreeMap<_, _>>();
    let fixture_ids = by_id
        .iter()
        .filter(|(_, source)| FIXTURE_NAMES.contains(&source.name.as_str()))
        .map(|(&id, source)| {
            (
                source.name.as_str(),
                OracleId::try_from(id.to_owned()).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture_ids.len(), FIXTURE_NAMES.len());
    let mut selected: BTreeSet<_> = set
        .cards
        .iter()
        .map(|row| row.oracle_id.clone())
        .chain(fixture_ids.values().map(|id| id.as_str().to_owned()))
        .collect();
    let indexing = indexed_at.elapsed();
    let compiling_at = Instant::now();
    let focused = !require_all;
    let selected_registry;
    let mut invalid_definitions = BTreeMap::new();
    let registry = if focused {
        selected_registry = loop {
            let rejected = match Registry::compile_keyword_selection(
                inventory,
                &vocabulary,
                Profile::Current,
                Some(&selected),
            ) {
                Ok(registry) => break registry,
                Err(crate::RegistryError::InvalidKeywordCreatureDefinition {
                    oracle_id,
                    reason,
                }) => vec![(oracle_id, reason)],
                Err(crate::RegistryError::InvalidKeywordCreatureDefinitions(rejected)) => rejected,
                Err(error) => panic!("could not compile {}: {error}", set.set),
            };
            for (id, reason) in rejected {
                assert!(
                    !fixture_ids.values().any(|fixture| fixture == &id),
                    "fixture {id:?} failed definition validation: {reason:?}"
                );
                assert!(
                    selected.remove(id.as_str()),
                    "invalid card was outside the requested set"
                );
                invalid_definitions.insert(id, format!("InvalidDefinition({reason:?})"));
            }
        };
        assert_eq!(
            selected_registry.coverage().required_records,
            selected.len()
        );
        &selected_registry
    } else {
        current_catalogue().2
    };
    let compilation = compiling_at.elapsed();
    let fixtures = fixture_ids
        .iter()
        .map(|(&name, id)| (name, registry.resolve(id).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let play_started = Instant::now();
    let compiler_reasons = registry
        .coverage()
        .cards
        .iter()
        .filter_map(|card| {
            let crate::BindingStatus::Blocked { reason } = card.binding else {
                return None;
            };
            Some((card.oracle_id.as_deref()?, reason))
        })
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut passed = 0;
    for row in set.cards {
        assert!(seen.insert(row.oracle_id.clone()));
        let source = by_id[row.oracle_id.as_str()];
        assert_eq!(source.name, row.name);
        let oracle = OracleId::try_from(row.oracle_id.clone()).unwrap();
        let (status, detail) = if let Some(error) = invalid_definitions.get(&oracle) {
            ("blocked", error.clone())
        } else {
            match registry.seed(&oracle, CardId(1), A, ObjectDestination::Hand) {
                Ok(card) => match probe(card, &source.faces[0], &fixtures, focused) {
                    CastingProbe::LandPlayed => ("land_played", String::new()),
                    CastingProbe::Resolved => ("resolved", String::new()),
                    CastingProbe::VerifiedOutcome => ("outcome_verified", String::new()),
                    CastingProbe::DecisionRequired(decision) => ("decision_required", decision),
                    CastingProbe::NeedsFixture(reason) => ("needs_fixture", reason),
                },
                Err(error) if !require_all && registry.resolve(&oracle).is_none() => {
                    ("blocked", format!("{error:?}"))
                }
                Err(error) => panic!("missing set definition for {}: {error:?}", row.name),
            }
        };
        let compiler_reason = invalid_definitions
            .get(&oracle)
            .map(|error| serde_json::json!(error))
            .or_else(|| {
                compiler_reasons
                    .get(row.oracle_id.as_str())
                    .map(|reason| serde_json::json!(reason))
            });
        passed += usize::from(matches!(
            status,
            "land_played" | "resolved" | "outcome_verified"
        ));
        if prefix == "SET_CAST"
            && let Some(binding) = registry.resolve(&oracle)
        {
            println!(
                "SET_PROGRAM {}",
                serde_json::json!({
                    "set": set.set,
                    "oracle_id": row.oracle_id,
                    "name": row.name,
                    "program": format!("{:?}", binding.program()),
                }),
            );
        }
        println!(
            "{prefix} {}",
            serde_json::json!({
                "set": set.set, "oracle_id": row.oracle_id, "name": row.name, "status": status,
                "detail": detail, "compiler_reason": compiler_reason
            })
        );
    }
    println!(
        "{prefix}_TIMING {}",
        serde_json::json!({
            "set": set.set,
            "source_seconds": load.as_secs_f64(),
            "index_seconds": indexing.as_secs_f64(),
            "registry_seconds": compilation.as_secs_f64(),
            "play_seconds": play_started.elapsed().as_secs_f64(),
            "selected_entries": selected.len(),
        })
    );
    passed
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn innistrad_reuses_morbid_fateful_hour_and_flashback_in_play() {
    use ManaType::{Colourless, Red, White};
    for died in [false, true] {
        let price = [(Colourless, 2), (Red, 1)];
        let mut cards = vec![hand("Brimstone Volley"), creature(2, B, 2, 2, &[])];
        if died {
            cards.push(seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
            ));
        }
        let mut state = game(vec![A, B], cards, &price, Step::PrecombatMain);
        if died {
            let target = TargetRef::Object(physical(&state, 2).id());
            state = round(targeted(&state, A, 3, target).into_state()).into_state();
        }
        let cast = command(
            &state,
            A,
            GameCommand::CastPaid {
                spell: physical(&state, 1).id(),
                target: TargetRef::Player(B),
                payment: spend(&price),
            },
        );
        let end = round(cast.into_state());
        let expected = if died { 15 } else { 17 };
        assert_eq!(
            end.state().execution().life_total(B),
            Some(&expected.into())
        );
    }
    for fateful in [false, true] {
        let price = [(Colourless, 1), (White, 1)];
        let mut cards = vec![hand("Gather the Townsfolk")];
        if fateful {
            cards.push(seed(
                2,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::None,
                    Instruction::AdjustControllerLife {
                        adjustment: spellwake_engine::life::LifeAdjustment::Lose(15.into()),
                    },
                ),
            ));
        }
        let mut state = game(vec![A, B], cards, &price, Step::PrecombatMain);
        if fateful {
            state = round(cast_untargeted(&state, 2, &[]).into_state()).into_state();
        }
        let end = resolve_set_spell(&state, &price, "Gather the Townsfolk");
        assert_eq!(
            end.execution()
                .window()
                .cards()
                .iter()
                .filter(|card| card.is_token())
                .count(),
            if fateful { 5 } else { 2 },
        );
    }
    let price = [(Colourless, 5), (Red, 1)];
    let state = game(
        vec![A, B],
        vec![named(
            "Bump in the Night",
            1,
            A,
            ObjectDestination::Graveyard,
        )],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &state,
        A,
        GameCommand::CastWithResources {
            spell: physical(&state, 1).id(),
            target: Some(TargetRef::Player(B)),
            payment: CastingPayment {
                mana: Some(spend(&price)),
                ..Default::default()
            },
        },
    );
    let end = round(cast.into_state());
    assert_eq!(end.state().execution().life_total(B), Some(&17.into()));
    assert_eq!(physical(end.state(), 1).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn innistrad_conditional_equipment_and_tapped_token_cohort() {
    use ManaType::{Black, Colourless};
    let price = [(Colourless, 5), (Black, 3)];
    let state = game(
        vec![A, B],
        vec![hand("Army of the Damned")],
        &price,
        Step::PrecombatMain,
    );
    let end = resolve_set_spell(&state, &price, "Army of the Damned");
    let tokens = end
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|card| card.is_token())
        .collect::<Vec<_>>();
    assert_eq!(tokens.len(), 13);
    assert!(tokens.iter().all(|token| token.tapped() == Some(true)));

    let mut human = creature(2, A, 2, 2, &[]);
    let (_, vocabulary) = dated_sources();
    human
        .program
        .creature_subtypes
        .insert(vocabulary.resolve_label("Human").unwrap());
    let dagger = named(
        "Silver-Inlaid Dagger",
        1,
        A,
        ObjectDestination::Battlefield { controller: A },
    );
    let ability = dagger.program.activated[0].id;
    let price = [(Colourless, 2)];
    for (host, expected_power) in [(human, 5), (creature(2, A, 2, 2, &[]), 4)] {
        let state = game(
            vec![A, B],
            vec![dagger.clone(), host],
            &price,
            Step::PrecombatMain,
        );
        let host = physical(&state, 2).id();
        let source = physical(&state, 1).id();
        let equip = command(
            &state,
            A,
            GameCommand::ActivateAbility {
                source,
                ability,
                target: TargetRef::Object(host),
                payment: Some(spend(&price)),
            },
        );
        let end = round(equip.into_state());
        assert_eq!(
            end.state()
                .execution()
                .creature(host)
                .unwrap()
                .unwrap()
                .power,
            expected_power.into()
        );
    }
}

fn resolve_set_spell(state: &GameState, price: &[(ManaType, i64)], name: &str) -> GameState {
    let mut result = round(cast_untargeted(state, 1, price).into_state()).into_state();
    while result.pending_decision().is_some() {
        let (actor, reply) =
            probe_reply(&result, name).expect("fixture must answer the spell's choice");
        result = command(&result, actor, reply).into_state();
    }
    assert!(result.execution().window().entries().is_empty());
    result
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn increasing_confusion_pays_for_x_when_cast_with_flashback() {
    let (inventory, vocabulary) = dated_sources();
    let id = inventory
        .cards
        .iter()
        .find(|card| card.required_by_base_formats && card.name == "Increasing Confusion")
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[id.clone()].into()),
    )
    .unwrap();
    let spell = registry
        .seed(
            &OracleId::try_from(id).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Graveyard,
        )
        .unwrap();
    let mut cards = vec![spell];
    for n in 2..=7 {
        cards.push(seed(
            n,
            B,
            ObjectDestination::LibraryBottom,
            Program::land(BTreeSet::new(), vec![]),
        ));
    }
    let mana = [(ManaType::Blue, 1), (ManaType::Colourless, 2)];
    let start = game(vec![A, B], cards, &mana, Step::PrecombatMain);
    let mut casting = command(
        &start,
        A,
        GameCommand::CastWithResources {
            spell: physical(&start, 1).id(),
            target: Some(TargetRef::Player(B)),
            payment: CastingPayment {
                chosen_x: Some(2.into()),
                mana: Some(spend(&mana)),
                ..Default::default()
            },
        },
    )
    .into_state();
    let entry = casting.execution().window().entries().last().unwrap();
    assert!(entry.cast_with_flashback());
    assert_eq!(entry.chosen_x(), Some(&2.into()));
    while physical(&casting, 1).zone() == ObjectZone::Stack {
        if let Some((actor, reply)) = probe_reply(&casting, "Increasing Confusion") {
            casting = command(&casting, actor, reply).into_state();
        } else {
            casting = pass(&casting).into_state();
        }
    }
    assert_eq!(physical(&casting, 1).zone(), ObjectZone::Exile);
    assert_eq!(
        casting.execution().window().cards().library(B).len() + 4,
        start.execution().window().cards().library(B).len(),
    );
    assert_eq!(casting.execution().window().cards().graveyard(B).len(), 4);
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_moonmist_orders_transforms_in_one_event() {
    let (inventory, vocabulary) = dated_sources();
    let requested = [
        "Moonmist",
        "Village Ironsmith // Ironfang",
        "Ulvenwald Mystics // Ulvenwald Primordials",
    ];
    let ids = inventory
        .cards
        .iter()
        .filter(|card| requested.contains(&card.name.as_str()))
        .filter_map(|card| card.oracle_id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), requested.len());
    let registry =
        Registry::compile_keyword_selection(inventory, &vocabulary, Profile::Current, Some(&ids))
            .unwrap();
    let seed_named = |name: &str, id, destination| {
        let oracle = inventory
            .cards
            .iter()
            .find(|card| card.name == name && card.oracle_id.is_some())
            .and_then(|card| card.oracle_id.clone())
            .unwrap();
        registry
            .seed(
                &OracleId::try_from(oracle).unwrap(),
                CardId(id),
                A,
                destination,
            )
            .unwrap()
    };
    let price = [(ManaType::Green, 1), (ManaType::Colourless, 1)];
    let field = ObjectDestination::Battlefield { controller: A };
    let start = game(
        vec![A, B],
        vec![
            seed_named(requested[0], 1, ObjectDestination::Hand),
            seed_named(requested[1], 2, field),
            seed_named(requested[2], 3, field),
        ],
        &price,
        Step::PrecombatMain,
    );
    let first = physical(&start, 2).id();
    let second = physical(&start, 3).id();
    let casting = cast_untargeted(&start, 1, &price).into_state();
    let pending = round(casting).into_state();
    let RuleFrame::TransformOrder(order) = pending.pending_decision().unwrap().frame() else {
        panic!("expected transform timestamp order");
    };
    assert_eq!(order.chooser(), A);
    assert_eq!(order.objects(), &[first, second]);
    assert!(!physical(&pending, 2).transformed());
    assert!(!physical(&pending, 3).transformed());
    let resolved = reply(&pending, DecisionReply::TransformOrder(vec![second, first]));
    let transformed = events(&resolved)
        .filter_map(|event| {
            let ids = event
                .occurrences
                .iter()
                .filter_map(|occurrence| match &occurrence.value {
                    RulesOccurrence::Transformed { before, after } => {
                        assert_eq!(before.object.id(), after.object.id());
                        Some(after.object.id())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            (!ids.is_empty()).then_some(ids)
        })
        .collect::<Vec<_>>();
    assert_eq!(transformed, vec![vec![second, first]]);
    assert!(physical(resolved.state(), 2).transformed());
    assert!(physical(resolved.state(), 3).transformed());
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_sudden_disappearance_returns_only_exiled_cards_to_their_owners() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Sudden Disappearance" && card.oracle_id.is_some())
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let spell = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    let price = [(ManaType::White, 1), (ManaType::Colourless, 5)];
    let controlled_by_b = ObjectDestination::Battlefield { controller: B };
    let state = game(
        vec![A, B],
        vec![
            spell,
            seed(2, A, controlled_by_b, Program::artifact(None, vec![])),
            creature(3, B, 2, 2, &[]),
            creature(4, A, 2, 2, &[]),
            seed(
                5,
                B,
                controlled_by_b,
                Program::land(BTreeSet::new(), vec![]),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let untouched = [physical(&state, 4).id(), physical(&state, 5).id()];
    let cast = command(
        &state,
        A,
        GameCommand::CastPaid {
            spell: physical(&state, 1).id(),
            target: TargetRef::Player(B),
            payment: spend(&price),
        },
    );
    let exiled = round(cast.into_state()).into_state();
    assert_eq!(physical(&exiled, 2).zone(), ObjectZone::Exile);
    assert_eq!(physical(&exiled, 3).zone(), ObjectZone::Exile);
    assert_eq!(
        [physical(&exiled, 4).id(), physical(&exiled, 5).id()],
        untouched
    );
    let returned = exile_hand::until(exiled, |state| {
        physical(state, 2).zone() == ObjectZone::Battlefield
    });
    assert_eq!(physical(&returned, 2).controller(), Some(A));
    assert_eq!(physical(&returned, 3).controller(), Some(B));
    assert_eq!(physical(&returned, 2).tapped(), Some(false));
    assert_eq!(
        [physical(&returned, 4).id(), physical(&returned, 5).id()],
        untouched
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_snapcaster_uses_the_granted_cost_even_with_printed_flashback() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = |name: &str| {
        inventory
            .cards
            .iter()
            .find(|card| card.name == name && card.required_by_base_formats)
            .and_then(|card| card.oracle_id.clone())
            .unwrap()
    };
    let mage = oracle("Snapcaster Mage");
    let bump = oracle("Bump in the Night");
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[mage.clone(), bump.clone()].into()),
    )
    .unwrap();
    let prime = |id, number, destination| {
        registry
            .seed(
                &OracleId::try_from(id).unwrap(),
                CardId(number),
                A,
                destination,
            )
            .unwrap()
    };
    let mage_price = [(ManaType::Colourless, 1), (ManaType::Blue, 1)];
    let bump_price = [(ManaType::Black, 1)];
    let state = game(
        vec![A, B],
        vec![
            prime(mage, 1, ObjectDestination::Hand),
            prime(bump, 2, ObjectDestination::Graveyard),
        ],
        &[mage_price.as_slice(), bump_price.as_slice()].concat(),
        Step::PrecombatMain,
    );
    let grave_card = physical(&state, 2).id();
    let cast_bump = GameCommand::CastWithResources {
        spell: grave_card,
        target: Some(TargetRef::Player(B)),
        payment: CastingPayment {
            mana: Some(spend(&bump_price)),
            use_granted_flashback: true,
            ..Default::default()
        },
    };
    illegal(&state, A, cast_bump.clone());
    let choose_target = round(cast_untargeted(&state, 1, &mage_price).into_state()).into_state();
    let waiting = reply(
        &choose_target,
        DecisionReply::TriggerTarget(TargetRef::Object(grave_card)),
    );
    let granted = round(waiting.into_state()).into_state();
    assert!(granted.execution().has_granted_flashback(grave_card, A));
    let ended = round(command(&granted, A, cast_bump).into_state());
    assert_eq!(ended.state().execution().life_total(B), Some(&17.into()));
    assert_eq!(physical(ended.state(), 2).zone(), ObjectZone::Exile);
    assert!(
        !ended
            .state()
            .execution()
            .has_granted_flashback(grave_card, A)
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_ghost_quarter_offers_its_search_even_when_the_target_survives() {
    let (inventory, vocabulary) = dated_sources();
    let names = ["Ghost Quarter", "Forest"];
    let id = |name: &str| {
        inventory
            .cards
            .iter()
            .find(|card| card.name == name && card.required_by_base_formats)
            .and_then(|card| card.oracle_id.clone())
            .unwrap()
    };
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&names.map(id).into()),
    )
    .unwrap();
    let quarter = registry
        .seed(
            &OracleId::try_from(id(names[0])).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let forest = registry
        .seed(
            &OracleId::try_from(id(names[1])).unwrap(),
            CardId(3),
            B,
            ObjectDestination::LibraryBottom,
        )
        .unwrap();
    let mut indestructible_land = Program::land(BTreeSet::new(), Vec::new());
    indestructible_land.printed_keywords.insert(Indestructible);
    let state = game(
        vec![A, B],
        vec![
            quarter,
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                indestructible_land,
            ),
            forest,
        ],
        &[],
        Step::PrecombatMain,
    );
    let target = physical(&state, 2).id();
    let ability = spellwake_engine::activated::ActivatedAbilityId(0);
    let announced = command(
        &state,
        A,
        GameCommand::ActivateAbility {
            source: physical(&state, 1).id(),
            ability,
            target: TargetRef::Object(target),
            payment: None,
        },
    );
    let offered = round(announced.into_state()).into_state();
    assert_eq!(physical(&offered, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&offered, 2).id(), target);
    assert_eq!(offered.pending_decision().unwrap().chooser(), B);
    let accepted = reply(
        &offered,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let forest = physical(&accepted, 3).id();
    let found = reply(
        &accepted,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(forest),
        }),
    );
    assert_eq!(physical(found.state(), 3).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(found.state(), 3).controller(), Some(B));
    assert_eq!(physical(found.state(), 3).tapped(), Some(false));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_spiteful_shadows_uses_the_dead_creatures_controller() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Spiteful Shadows" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let aura = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    let burn = seed(
        3,
        A,
        ObjectDestination::Hand,
        free_instant(
            BodyTarget::SingleCreature,
            Instruction::Damage { amount: 3.into() },
        ),
    );
    let price = [(ManaType::Colourless, 1), (ManaType::Black, 1)];
    let state = game(
        vec![A, B],
        vec![aura, creature(2, B, 2, 2, &[]), burn],
        &price,
        Step::PrecombatMain,
    );
    let host = TargetRef::Object(physical(&state, 2).id());
    let cast = command(
        &state,
        A,
        GameCommand::CastPaid {
            spell: physical(&state, 1).id(),
            target: host,
            payment: spend(&price),
        },
    );
    let attached = round(cast.into_state()).into_state();
    assert_eq!(
        attached
            .execution()
            .attached_to(physical(&attached, 1).id()),
        Some(host)
    );
    let damage = round(targeted(&attached, A, 3, host).into_state()).into_state();
    assert_eq!(physical(&damage, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&damage, 2).zone(), ObjectZone::Graveyard(B));
    let reflected = round(damage);
    assert_eq!(
        reflected.state().execution().life_total(A),
        Some(&20.into())
    );
    assert_eq!(
        reflected.state().execution().life_total(B),
        Some(&17.into())
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_flayer_damage_is_dealt_by_the_creature_that_entered() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Flayer of the Hatebound" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let flayer = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let entrant = seed(
        2,
        A,
        ObjectDestination::Graveyard,
        Program::creature(Some(ManaCost::free()), 3.into(), 2.into(), BTreeSet::new()),
    );
    let reanimate = seed(
        3,
        A,
        ObjectDestination::Hand,
        free_instant(
            BodyTarget::FilteredOwnGraveyardCard(
                spellwake_engine::program::ObjectFilter::creature(),
            ),
            Instruction::ReturnOwnGraveyardCreatureToBattlefield,
        ),
    );
    let starting = game(
        vec![A, B],
        vec![flayer, entrant, reanimate],
        &[],
        Step::PrecombatMain,
    );
    let target = TargetRef::Object(physical(&starting, 2).id());
    let ready = round(targeted(&starting, A, 3, target).into_state()).into_state();
    let new_entrant = physical(&ready, 2).id();
    assert_ne!(TargetRef::Object(new_entrant), target);
    let waiting = reply(&ready, DecisionReply::TriggerTarget(TargetRef::Player(B)));
    let done = round(waiting.into_state());
    assert_eq!(done.state().execution().life_total(B), Some(&17.into()));
    assert!(events(&done).flat_map(|event| &event.occurrences).any(|row| matches!(
        &row.value, RulesOccurrence::DamageDealt { damage, .. }
            if damage.source.id() == spellwake_engine::damage::DamageSourceId::Object(new_entrant)
    )));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_fiend_hunter_has_independent_entry_and_leave_triggers() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Fiend Hunter" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let hunter = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    let removal = seed(
        3,
        A,
        ObjectDestination::Hand,
        free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
    );
    let price = [(ManaType::White, 2), (ManaType::Colourless, 1)];
    let state = game(
        vec![A, B],
        vec![hunter, creature(2, B, 2, 2, &[]), removal],
        &price,
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&state, 1, &price).into_state()).into_state();
    let hunted = physical(&entered, 2).id();
    let ready = reply(
        &entered,
        DecisionReply::TriggerTarget(TargetRef::Object(hunted)),
    )
    .into_state();
    let destroy_hunter = |state: &GameState| {
        round(targeted(state, A, 3, TargetRef::Object(physical(state, 1).id())).into_state())
            .into_state()
    };
    let accept_exile = |state: GameState| {
        let offer = round(state).into_state();
        let taken = reply(
            &offer,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        );
        assert_eq!(physical(taken.state(), 2).zone(), ObjectZone::Exile);
        taken.into_state()
    };

    let exiled = accept_exile(ready.clone());
    let returned = round(destroy_hunter(&exiled));
    assert_eq!(
        physical(returned.state(), 2).zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(physical(returned.state(), 2).controller(), Some(B));

    let gone = destroy_hunter(&ready);
    assert_eq!(physical(&gone, 1).zone(), ObjectZone::Graveyard(A));
    let leave_first = round(gone).into_state();
    assert_eq!(physical(&leave_first, 2).id(), hunted);
    assert_eq!(
        physical(&accept_exile(leave_first), 2).zone(),
        ObjectZone::Exile
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_tree_exchanges_life_and_sets_only_toughness_in_one_event() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Tree of Redemption" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let tree = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let counter = seed(
        2,
        A,
        ObjectDestination::Hand,
        free_instant(
            BodyTarget::SingleCreature,
            Instruction::PutPowerToughnessCounters {
                counters: spellwake_engine::characteristics::PowerToughnessCounters {
                    plus_one: 1.into(),
                    minus_one: 0.into(),
                },
            },
        ),
    );
    let state = game(vec![A, B], vec![tree, counter], &[], Step::PrecombatMain);
    let object = physical(&state, 1).id();
    let state = round(targeted(&state, A, 2, TargetRef::Object(object)).into_state()).into_state();
    let state = exile_hand::until(state, |state| {
        let position = state.turn().unwrap().position();
        position.number == 3 && position.step == Step::PrecombatMain
    });
    let activation = command(
        &state,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: object,
            ability: spellwake_engine::activated::ActivatedAbilityId(0),
            payment: None,
        },
    );
    let result = round(activation.into_state());
    assert_eq!(result.state().execution().life_total(A), Some(&14.into()));
    let stats = result
        .state()
        .execution()
        .power_toughness(object)
        .unwrap()
        .unwrap()
        .derived;
    assert_eq!(stats.power, 1.into());
    assert_eq!(stats.toughness, 21.into());
    assert!(events(&result).any(|event| {
        let life_changed = event
            .occurrences
            .iter()
            .any(|row| matches!(row.value, RulesOccurrence::LifeChanged(_)));
        let changed_toughness = event
            .occurrences
            .iter()
            .any(|row| matches!(row.value, RulesOccurrence::ContinuousEffectCreated { .. }));
        life_changed && changed_toughness
    }));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_loyal_cathar_enters_on_its_back_face_under_its_last_controllers_control() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name.starts_with("Loyal Cathar //") && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let cathar = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            B,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let removal = seed(
        2,
        A,
        ObjectDestination::Hand,
        free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
    );
    let start = game(vec![A, B], vec![cathar, removal], &[], Step::PrecombatMain);
    let target = TargetRef::Object(physical(&start, 1).id());
    let died = round(targeted(&start, A, 2, target).into_state()).into_state();
    assert_eq!(physical(&died, 1).zone(), ObjectZone::Graveyard(B));
    let scheduled = round(died).into_state();
    let end = exile_hand::until(scheduled, |state| {
        state.turn().unwrap().position().step == Step::End
    });
    let returned = round(end);
    let back = physical(returned.state(), 1);
    assert_eq!(back.zone(), ObjectZone::Battlefield);
    assert_eq!(back.controller(), Some(A));
    assert!(back.transformed());
    assert!(
        events(&returned)
            .flat_map(|event| &event.occurrences)
            .any(|row| matches!(
                &row.value, RulesOccurrence::ZoneChange(change)
                    if change.after.object.id() == back.id() && change.after.object.transformed()
            ))
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_entry_counters_use_the_relevant_battlefield_and_graveyard() {
    let (inventory, vocabulary) = dated_sources();
    let find = |name| {
        inventory
            .cards
            .iter()
            .find(|card| card.name == name && card.required_by_base_formats)
            .and_then(|card| card.oracle_id.clone())
            .unwrap()
    };
    let departed = find("Dearly Departed");
    let horde = find("Unbreathing Horde");
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[departed.clone(), horde.clone()].into()),
    )
    .unwrap();
    let card = |oracle, id, destination| {
        registry
            .seed(
                &OracleId::try_from(oracle).unwrap(),
                CardId(id),
                A,
                destination,
            )
            .unwrap()
    };
    let zombie = vocabulary.resolve_label("Zombie").unwrap();
    let human = vocabulary.resolve_label("Human").unwrap();
    let creature_program = |subtype| {
        let mut program =
            Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
        program.creature_subtypes.insert(subtype);
        program
    };
    let strike = seed(
        7,
        A,
        ObjectDestination::Hand,
        free_instant(
            BodyTarget::SingleCreature,
            Instruction::Damage { amount: 3.into() },
        ),
    );
    let price = [(ManaType::Black, 1), (ManaType::Colourless, 2)];
    let start = game(
        vec![A, B],
        vec![
            card(departed, 1, ObjectDestination::Graveyard),
            card(horde, 2, ObjectDestination::Hand),
            seed(3, A, ObjectDestination::Hand, creature_program(human)),
            seed(4, A, ObjectDestination::Graveyard, creature_program(zombie)),
            seed(
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
                creature_program(zombie),
            ),
            seed(6, B, ObjectDestination::Graveyard, creature_program(zombie)),
            strike,
        ],
        &price,
        Step::PrecombatMain,
    );
    let human_entered = round(cast_untargeted(&start, 3, &[]).into_state()).into_state();
    let counters = |state: &GameState, card| {
        state
            .execution()
            .power_toughness_counters(physical(state, card).id())
            .unwrap()
            .plus_one
    };
    assert_eq!(counters(&human_entered, 3), 1.into());

    let horde_entered = round(cast_untargeted(&human_entered, 2, &price).into_state()).into_state();
    let horde_id = physical(&horde_entered, 2).id();
    assert_eq!(counters(&horde_entered, 2), 2.into());
    let result = round(targeted(&horde_entered, A, 7, TargetRef::Object(horde_id)).into_state());
    assert_eq!(physical(result.state(), 2).zone(), ObjectZone::Battlefield);
    assert_eq!(counters(result.state(), 2), 1.into());
    assert!(
        events(&result)
            .flat_map(|event| &event.occurrences)
            .any(|row| matches!(&row.value,
                RulesOccurrence::PowerToughnessCountersRemoved { object, .. } if *object == horde_id
            ))
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_altar_mana_works_for_flashback_and_rejects_hand_casts() {
    use spellwake_engine::mana::{
        ManaAbilityId, ManaAbilityRef, ManaRestriction, RestrictedManaSpend,
    };
    let (inventory, vocabulary) = dated_sources();
    let find = |name| {
        inventory
            .cards
            .iter()
            .find(|card| card.name == name && card.required_by_base_formats)
            .and_then(|card| card.oracle_id.clone())
            .unwrap()
    };
    let altar = find("Altar of the Lost");
    let bump = find("Bump in the Night");
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[altar.clone(), bump.clone()].into()),
    )
    .unwrap();
    let card = |oracle, id, destination| {
        registry
            .seed(
                &OracleId::try_from(oracle).unwrap(),
                CardId(id),
                A,
                destination,
            )
            .unwrap()
    };
    let (black, red, colorless) = (ManaType::Black, ManaType::Red, ManaType::Colourless);
    let start = game(
        vec![A, B],
        vec![
            card(altar, 1, ObjectDestination::Battlefield { controller: A }),
            card(bump.clone(), 2, ObjectDestination::Graveyard),
            card(bump, 3, ObjectDestination::Hand),
        ],
        &[(colorless, 4)],
        Step::PrecombatMain,
    );
    let tap = |bundle: &[(ManaType, i64)]| GameCommand::ActivateManaWithBundle {
        source: physical(&start, 1).id(),
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        bundle: bundle.iter().map(|&(color, n)| (color, n.into())).collect(),
    };
    illegal(&start, A, tap(&[(colorless, 1), (red, 1)]));
    let restriction = ManaRestriction::SpellWithFlashbackFromGraveyard;
    let same_color = command(&start, A, tap(&[(red, 2)])).into_state();
    assert_eq!(
        same_color.execution().restricted_mana()[&(A, red, restriction)],
        2.into()
    );
    let funded = command(&start, A, tap(&[(red, 1), (black, 1)])).into_state();
    let cast =
        |id, spends: &[(ManaType, i64)], colors: &[ManaType]| GameCommand::CastWithResources {
            spell: physical(&funded, id).id(),
            target: Some(TargetRef::Player(B)),
            payment: CastingPayment {
                mana: Some(spend(spends)),
                restricted_mana: colors
                    .iter()
                    .map(|&mana_type| RestrictedManaSpend {
                        mana_type,
                        restriction,
                        amount: 1.into(),
                    })
                    .collect(),
                ..Default::default()
            },
        };
    illegal(&funded, A, cast(3, &[(black, 1)], &[black]));
    let played = command(
        &funded,
        A,
        cast(2, &[(colorless, 4), (black, 1), (red, 1)], &[black, red]),
    );
    let resolved = round(played.into_state());
    assert_eq!(physical(resolved.state(), 2).zone(), ObjectZone::Exile);
    assert_eq!(resolved.state().execution().life_total(B), Some(&17.into()));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_soul_seizer_transforms_attaches_and_controls_the_target() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name.starts_with("Soul Seizer //") && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let soul = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let start = game(
        vec![A, B],
        vec![soul, creature(2, B, 2, 2, &[])],
        &[],
        Step::PrecombatMain,
    );
    let start = exile_hand::until(start, |state| {
        let position = state.turn().unwrap().position();
        position.number == 3 && position.step == Step::PrecombatMain
    });
    let attackers = combat_frame(&start, true);
    let source = physical(&attackers, 1).id();
    let host = physical(&attackers, 2).id();
    let attacking = reply(
        &attackers,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: source,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let after_damage = exile_hand::until(attacking, |state| {
        matches!(
            state.pending_decision().map(|decision| decision.frame()),
            Some(RuleFrame::TriggerTarget(_))
        )
    });
    let ready = reply(
        &after_damage,
        DecisionReply::TriggerTarget(TargetRef::Object(host)),
    );
    let resolving = round(ready.into_state()).into_state();
    let transformed = reply(
        &resolving,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(physical(transformed.state(), 1).id(), source);
    assert!(physical(transformed.state(), 1).transformed());
    assert_eq!(
        transformed.state().execution().attached_to(source),
        Some(TargetRef::Object(host))
    );
    assert_eq!(physical(transformed.state(), 2).controller(), Some(A));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_essence_of_the_wild_applies_to_creature_cards_and_tokens() {
    let (inventory, vocabulary) = dated_sources();
    let find = |name| {
        inventory
            .cards
            .iter()
            .find(|card| card.name == name && card.required_by_base_formats)
            .and_then(|card| card.oracle_id.clone())
            .unwrap()
    };
    let essence = find("Essence of the Wild");
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[essence.clone()].into()),
    )
    .unwrap();
    let make_spirit = crate::keyword_creatures::ordinary_text::create_tokens(
        "a 1/1 white Spirit creature token with flying",
        &vocabulary,
    )
    .unwrap();
    let essence = registry
        .seed(
            &OracleId::try_from(essence).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let start = game(
        vec![A, B],
        vec![
            essence,
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
            ),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::None, make_spirit),
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&start, 3, &[]).into_state()).into_state();
    let essence_definition = entered
        .execution()
        .copiable_program(physical(&entered, 1).id())
        .unwrap();
    assert_eq!(
        entered
            .execution()
            .copiable_program(physical(&entered, 3).id()),
        Some(essence_definition)
    );

    let pending = round(cast_untargeted(&entered, 4, &[]).into_state()).into_state();
    let RuleFrame::ZoneReplacement(frame) = pending.pending_decision().unwrap().frame() else {
        panic!("expected a choice between the two Essence copies");
    };
    let selected = frame.reply_at(0).unwrap();
    let resolved = reply(&pending, DecisionReply::ZoneReplacement(selected));
    let [token] = tokens(&resolved, A, 1).try_into().unwrap();
    assert_eq!(
        resolved.state().execution().copiable_program(token),
        Some(essence_definition)
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_ravager_still_damages_its_creature_target_after_the_planeswalker_leaves() {
    let (inventory, vocabulary) = dated_sources();
    let find = |prefix: &str| {
        inventory
            .cards
            .iter()
            .find(|card| card.name.starts_with(prefix) && card.required_by_base_formats)
            .and_then(|card| card.oracle_id.clone())
            .unwrap()
    };
    let huntmaster = find("Huntmaster of the Fells //");
    let moonmist = find("Moonmist");
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[huntmaster.clone(), moonmist.clone()].into()),
    )
    .unwrap();
    let card = |oracle, id, destination| {
        registry
            .seed(
                &OracleId::try_from(oracle).unwrap(),
                CardId(id),
                A,
                destination,
            )
            .unwrap()
    };
    let mut walker = Program::new(
        BTreeSet::from([CardType::Planeswalker]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(spellwake_engine::program::PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: Some(ManaCost::free()),
        }),
    );
    walker.printed_loyalty = Some(4.into());
    let price = [(ManaType::Green, 1), (ManaType::Colourless, 1)];
    let start = game(
        vec![A, B],
        vec![
            card(
                huntmaster,
                1,
                ObjectDestination::Battlefield { controller: A },
            ),
            card(moonmist, 2, ObjectDestination::Hand),
            seed(3, A, ObjectDestination::Hand, walker),
            creature(4, A, 2, 2, &[]),
            creature(5, B, 2, 2, &[]),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::ReturnToOwnersHand),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let start = round(cast_untargeted(&start, 3, &[]).into_state()).into_state();
    let transformed = round(cast_untargeted(&start, 2, &price).into_state()).into_state();
    assert!(physical(&transformed, 1).transformed());
    let walker = TargetRef::Object(physical(&transformed, 3).id());
    let creature = TargetRef::Object(physical(&transformed, 4).id());
    let other = TargetRef::Object(physical(&transformed, 5).id());
    let RuleFrame::TriggerTarget(frame) = transformed.pending_decision().unwrap().frame() else {
        panic!("expected Ravager's two target clauses");
    };
    let choices = frame.secondary_targets().unwrap();
    assert!(choices[&walker].contains(&creature));
    assert!(!choices[&walker].contains(&other));
    let armed = reply(
        &transformed,
        DecisionReply::TriggerPermanentTargets(vec![walker, creature]),
    );
    let removed = round(targeted(armed.state(), A, 6, walker).into_state()).into_state();
    assert_eq!(physical(&removed, 3).zone(), ObjectZone::Hand(A));
    let dealt = round(removed);
    assert_eq!(physical(dealt.state(), 4).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(dealt.state(), 5).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_curse_searches_share_entry_and_keep_the_right_player_and_names() {
    let (inventory, vocabulary) = dated_sources();
    let find = |name| {
        inventory
            .cards
            .iter()
            .find(|card| card.name == name && card.required_by_base_formats)
            .and_then(|card| card.oracle_id.clone())
            .unwrap()
    };
    let witch = find("Bitterheart Witch");
    let misfortunes = find("Curse of Misfortunes");
    let tome = find("Curse of the Bloody Tome");
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[witch.clone(), misfortunes.clone(), tome.clone()].into()),
    )
    .unwrap();
    let card = |oracle, id, destination| {
        registry
            .seed(
                &OracleId::try_from(oracle).unwrap(),
                CardId(id),
                A,
                destination,
            )
            .unwrap()
    };
    let start = game(
        vec![A, B],
        vec![
            card(witch, 1, ObjectDestination::Battlefield { controller: A }),
            card(misfortunes.clone(), 2, ObjectDestination::LibraryBottom),
            card(misfortunes, 3, ObjectDestination::LibraryBottom),
            card(tome, 4, ObjectDestination::LibraryBottom),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let target = TargetRef::Object(physical(&start, 1).id());
    let died = round(targeted(&start, A, 5, target).into_state()).into_state();
    let announced = reply(&died, DecisionReply::TriggerTarget(TargetRef::Player(B)));
    let offer = round(announced.into_state()).into_state();
    let search = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let search = search
        .supply_entropy(search.basis(), vec![0; 16])
        .unwrap()
        .into_state();
    let selected = physical(&search, 2).id();
    let attached = reply(
        &search,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(selected),
        }),
    )
    .into_state();
    assert_eq!(
        attached
            .execution()
            .attached_to(physical(&attached, 2).id()),
        Some(TargetRef::Player(B))
    );

    let next_upkeep = exile_hand::until(attached, |state| {
        let position = state.turn().unwrap().position();
        position.number == 3 && position.step == Step::Upkeep
    });
    let offer = round(next_upkeep).into_state();
    let search = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let RuleFrame::LibraryTop(frame) = search.pending_decision().unwrap().frame() else {
        panic!("expected Misfortunes to search on its controller's upkeep");
    };
    assert!(
        !frame
            .eligible_results()
            .contains(&physical(&search, 3).id())
    );
    let tome = physical(&search, 4).id();
    assert!(frame.eligible_results().contains(&tome));
    let complete = reply(
        &search,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(tome),
        }),
    );
    assert_eq!(
        complete
            .state()
            .execution()
            .attached_to(physical(complete.state(), 4).id()),
        Some(TargetRef::Player(B))
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_divine_reckoning_lets_each_player_keep_one_creature() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Divine Reckoning" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let spell = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    let price = [(ManaType::White, 2), (ManaType::Colourless, 2)];
    let start = game(
        vec![A, B],
        vec![
            spell,
            creature(2, A, 2, 2, &[]),
            creature(3, A, 3, 3, &[]),
            creature(4, B, 2, 2, &[]),
            creature(5, B, 3, 3, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let first = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    assert_eq!(first.pending_decision().unwrap().chooser(), A);
    let second = reply(
        &first,
        DecisionReply::SelectResolution(vec![physical(&first, 2).id()]),
    )
    .into_state();
    assert_eq!(second.pending_decision().unwrap().chooser(), B);
    assert_eq!(physical(&second, 3).zone(), ObjectZone::Battlefield);
    let end = reply(
        &second,
        DecisionReply::SelectResolution(vec![physical(&second, 5).id()]),
    );
    assert_eq!(physical(end.state(), 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(end.state(), 5).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(end.state(), 3).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(end.state(), 4).zone(), ObjectZone::Graveyard(B));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_cellar_door_moves_the_bottom_card_and_checks_the_moved_card() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Cellar Door" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let oracle = OracleId::try_from(oracle).unwrap();
    let door = |id| {
        registry
            .seed(
                &oracle,
                CardId(id),
                A,
                ObjectDestination::Battlefield { controller: A },
            )
            .unwrap()
    };
    let price = [(ManaType::Colourless, 3)];
    let start = game(
        vec![A, B],
        vec![
            door(1),
            door(2),
            seed(
                3,
                B,
                ObjectDestination::LibraryBottom,
                Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
            ),
        ],
        &[(ManaType::Colourless, 6)],
        Step::PrecombatMain,
    );
    let activate = |state: &GameState, id| {
        round(
            command(
                state,
                A,
                GameCommand::ActivateAbility {
                    source: physical(state, id).id(),
                    ability: spellwake_engine::activated::ActivatedAbilityId(0),
                    target: TargetRef::Player(B),
                    payment: Some(spend(&price)),
                },
            )
            .into_state(),
        )
    };
    let first = activate(&start, 1);
    assert_eq!(physical(first.state(), 3).zone(), ObjectZone::Library(B));
    assert_eq!(
        physical(first.state(), 101).zone(),
        ObjectZone::Graveyard(B)
    );
    tokens(&first, A, 0);
    let second = activate(first.state(), 2);
    assert_eq!(physical(second.state(), 3).zone(), ObjectZone::Graveyard(B));
    tokens(&second, A, 1);
    assert!(events(&second).flat_map(|event| &event.occurrences).any(|row| matches!(&row.value,
        RulesOccurrence::ZoneChange(change) if change.reason == ZoneReason::PutLibraryCohortIntoGraveyard
    )));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_trepanation_blade_counts_the_revealed_land_and_moves_the_same_cards() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Trepanation Blade" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let blade = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let price = [(ManaType::Colourless, 2)];
    let start = game(
        vec![A, B],
        vec![
            blade,
            creature(2, A, 2, 2, &[KeywordAbility::Haste]),
            seed(
                3,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                4,
                B,
                ObjectDestination::LibraryBottom,
                Program::land(BTreeSet::new(), vec![]),
            ),
            seed(
                5,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let creature = physical(&start, 2).id();
    let equip = command(
        &start,
        A,
        GameCommand::ActivateAbility {
            source: physical(&start, 1).id(),
            ability: spellwake_engine::activated::ActivatedAbilityId(1),
            target: TargetRef::Object(creature),
            payment: Some(spend(&price)),
        },
    );
    let equipped = round(equip.into_state()).into_state();
    let attackers = combat_frame(&equipped, true);
    let attacking = reply(
        &attackers,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: creature,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let ready = round(attacking).into_state();
    let order = [physical(&ready, 3).id(), physical(&ready, 4).id()];
    assert_eq!(
        ready
            .execution()
            .power_toughness(creature)
            .unwrap()
            .unwrap()
            .derived
            .power,
        4.into()
    );
    let result = reply(
        &ready,
        DecisionReply::OrderResolutionGraveyard(order.to_vec()),
    );
    for id in [3, 4] {
        assert_eq!(
            physical(result.state(), id).zone(),
            ObjectZone::Graveyard(B)
        );
    }
    assert_eq!(physical(result.state(), 5).zone(), ObjectZone::Library(B));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_graveyard_clauses_use_distinct_targets_and_order_cards_at_resolution() {
    use spellwake_engine::program::CardType;

    let (inventory, vocabulary) = dated_sources();
    let oracle_for = |name| {
        inventory
            .cards
            .iter()
            .find(|card| card.name == name && card.required_by_base_formats)
            .and_then(|card| card.oracle_id.clone())
            .unwrap()
    };
    let chant = oracle_for("Ghoulcaller's Chant");
    let purge = oracle_for("Gravepurge");
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[chant.clone(), purge.clone()].into()),
    )
    .unwrap();
    let card = |id, number| {
        registry
            .seed(
                &OracleId::try_from(id).unwrap(),
                CardId(number),
                A,
                ObjectDestination::Hand,
            )
            .unwrap()
    };
    let zombie = vocabulary.resolve_label("Zombie").unwrap();
    let creature_card = || Program::creature(None, 2.into(), 2.into(), BTreeSet::from([zombie]));
    let mut kindred = Program::enchantment(None);
    kindred.card_types.insert(CardType::Kindred);
    kindred.creature_subtypes.insert(zombie);
    let mana = [(ManaType::Black, 1), (ManaType::Colourless, 2)];
    let start = game(
        vec![A, B],
        vec![
            card(chant, 1),
            card(purge, 2),
            seed(3, A, ObjectDestination::Graveyard, creature_card()),
            seed(4, A, ObjectDestination::Graveyard, creature_card()),
            seed(5, A, ObjectDestination::Graveyard, kindred),
            seed(6, B, ObjectDestination::Graveyard, creature_card()),
            seed(
                7,
                A,
                ObjectDestination::LibraryTop,
                Program::land(BTreeSet::new(), vec![]),
            ),
        ],
        &mana,
        Step::PrecombatMain,
    );
    let target = |id| TargetRef::Object(physical(&start, id).id());
    let chant_cost = [(ManaType::Black, 1)];
    let chant_command = |targets| GameCommand::CastWithResources {
        spell: physical(&start, 1).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(&chant_cost)),
            modes: vec![CastMode {
                mode: 1,
                target: None,
                targets,
            }],
            ..Default::default()
        },
    };
    illegal(&start, A, chant_command(vec![target(3), target(3)]));
    illegal(&start, A, chant_command(vec![target(3), target(6)]));
    let returned =
        round(command(&start, A, chant_command(vec![target(3), target(5)])).into_state())
            .into_state();
    assert_eq!(physical(&returned, 3).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&returned, 5).zone(), ObjectZone::Hand(A));

    let purge_command = |permanent_targets| GameCommand::CastWithResources {
        spell: physical(&start, 2).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(&mana)),
            permanent_targets,
            ..Default::default()
        },
    };
    illegal(&start, A, purge_command(vec![target(5)]));
    let empty = round(command(&start, A, purge_command(vec![])).into_state()).into_state();
    assert_eq!(physical(&empty, 7).zone(), ObjectZone::Hand(A));
    let ordering =
        round(command(&start, A, purge_command(vec![target(3), target(4)])).into_state())
            .into_state();
    let top_to_bottom = [physical(&ordering, 4).id(), physical(&ordering, 3).id()];
    let ordered = reply(
        &ordering,
        DecisionReply::OrderResolutionGraveyard(top_to_bottom.to_vec()),
    )
    .into_state();
    assert_eq!(physical(&ordered, 4).zone(), ObjectZone::Hand(A));
    assert!(
        ordered
            .execution()
            .window()
            .cards()
            .library(A)
            .starts_with(&[physical(&ordered, 3).id(), physical(&ordered, 7).id(),])
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_targeted_shuffle_moves_only_the_legal_cards_and_can_shuffle_none() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Memory's Journey" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let journey = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Graveyard,
        )
        .unwrap();
    let artifact = || Program::artifact(None, vec![]);
    let mana = [(ManaType::Green, 1)];
    let start = game(
        vec![A, B],
        vec![
            journey,
            seed(2, B, ObjectDestination::Graveyard, artifact()),
            seed(3, B, ObjectDestination::Graveyard, artifact()),
            seed(4, A, ObjectDestination::Graveyard, artifact()),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::AnyGraveyardCard,
                    Instruction::ExileGraveyardTarget,
                ),
            ),
        ],
        &mana,
        Step::PrecombatMain,
    );
    let target = |id| TargetRef::Object(physical(&start, id).id());
    let cast = |permanent_targets| GameCommand::CastWithResources {
        spell: physical(&start, 1).id(),
        target: Some(TargetRef::Player(B)),
        payment: CastingPayment {
            mana: Some(spend(&mana)),
            permanent_targets,
            ..Default::default()
        },
    };
    illegal(&start, A, cast(vec![target(4)]));
    illegal(&start, A, cast(vec![target(2), target(2)]));
    let resolve = |state: GameState| {
        let state = state
            .supply_entropy(state.basis(), vec![0; 16])
            .unwrap()
            .into_state();
        round(state)
    };
    let empty = resolve(command(&start, A, cast(vec![])).into_state());
    assert_eq!(empty.shuffle_journal().len(), 1);
    assert_eq!(empty.shuffle_journal()[0].player, B);
    assert_eq!(physical(empty.state(), 2).zone(), ObjectZone::Graveyard(B));
    let cast_state = command(&start, A, cast(vec![target(2), target(3)])).into_state();
    let exiled = round(targeted(&cast_state, A, 5, target(2)).into_state()).into_state();
    let result = resolve(exiled).into_state();
    assert_eq!(physical(&result, 1).zone(), ObjectZone::Exile);
    assert_eq!(physical(&result, 2).zone(), ObjectZone::Exile);
    assert_eq!(physical(&result, 3).zone(), ObjectZone::Library(B));
    assert_eq!(physical(&result, 4).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_seance_copies_an_exiled_creature_with_an_added_subtype_until_end_step() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Séance" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let seance = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let zombie = vocabulary.resolve_label("Zombie").unwrap();
    let spirit = vocabulary.resolve_label("Spirit").unwrap();
    let dead = Program::creature(None, 2.into(), 3.into(), BTreeSet::from([zombie]));
    let start = game(
        vec![A, B],
        vec![
            seance,
            seed(2, A, ObjectDestination::Graveyard, dead.clone()),
            seed(3, B, ObjectDestination::Graveyard, dead),
        ],
        &[],
        Step::PrecombatMain,
    );
    let upkeep = exile_hand::until(start, |state| {
        state.turn().unwrap().position().step == Step::Upkeep
    });
    assert_eq!(upkeep.execution().window().active_player(), B);
    let RuleFrame::TriggerTarget(frame) = upkeep.pending_decision().unwrap().frame() else {
        panic!("expected the upkeep trigger to request a graveyard creature");
    };
    let own = TargetRef::Object(physical(&upkeep, 2).id());
    assert_eq!(frame.domain(), &[own]);
    let chosen = reply(&upkeep, DecisionReply::TriggerTarget(own)).into_state();
    let offer = round(chosen).into_state();
    let declined = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(
        physical(declined.state(), 2).zone(),
        ObjectZone::Graveyard(A)
    );
    let made = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(physical(made.state(), 2).zone(), ObjectZone::Exile);
    let token = tokens(&made, A, 1)[0];
    let current = made
        .state()
        .execution()
        .current_characteristics(token)
        .unwrap()
        .unwrap();
    assert_eq!(
        current.current_creature_subtypes,
        BTreeSet::from([zombie, spirit])
    );
    assert_eq!(current.power_toughness.unwrap().derived.toughness, 3.into());
    let end = exile_hand::until(made.into_state(), |state| {
        state.turn().unwrap().position().step == Step::End
    });
    let resolved = round(end);
    assert!(
        resolved
            .state()
            .execution()
            .window()
            .cards()
            .get(token)
            .is_none()
    );
    assert!(
        events(&resolved)
            .flat_map(|event| &event.occurrences)
            .any(|row| matches!(&row.value,
                RulesOccurrence::ZoneChange(change) if change.before.object.id() == token
                    && change.after.object.zone() == ObjectZone::Exile
            ))
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_morbid_search_offers_the_same_land_for_hand_or_battlefield() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Caravan Vigil" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let spell = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    let mut land = Program::land(BTreeSet::new(), vec![]);
    land.supertypes
        .insert(spellwake_engine::program::Supertype::Basic);
    let mana = [(ManaType::Green, 1)];
    let start = game(
        vec![A, B],
        vec![
            spell,
            creature(2, B, 2, 2, &[]),
            seed(3, A, ObjectDestination::LibraryTop, land.clone()),
            seed(4, A, ObjectDestination::LibraryBottom, land),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
            ),
        ],
        &mana,
        Step::PrecombatMain,
    );
    let search = |state: &GameState| {
        let pending = round(cast_untargeted(state, 1, &mana).into_state()).into_state();
        pending
            .supply_entropy(pending.basis(), vec![0; 16])
            .unwrap()
            .into_state()
    };
    let ordinary = search(&start);
    let selected = physical(&ordinary, 3).id();
    let to_hand = reply(
        &ordinary,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(selected),
        }),
    );
    assert_eq!(physical(to_hand.state(), 3).zone(), ObjectZone::Hand(A));

    let killed =
        round(targeted(&start, A, 5, TargetRef::Object(physical(&start, 2).id())).into_state())
            .into_state();
    let offered = search(&killed);
    let RuleFrame::LibraryTop(frame) = offered.pending_decision().unwrap().frame() else {
        panic!("expected the basic land search")
    };
    assert!(frame.offers_search_destination());
    let selected = physical(&offered, 3).id();
    for (battlefield, hand, zone) in [
        (Some(selected), None, ObjectZone::Battlefield),
        (None, Some(selected), ObjectZone::Hand(A)),
    ] {
        let result = reply(
            &offered,
            DecisionReply::LibraryTop(LibraryTopReply::SearchSplit { battlefield, hand }),
        );
        let card = physical(result.state(), 3);
        assert_eq!(card.zone(), zone);
        if battlefield.is_some() {
            assert_eq!(card.tapped(), Some(false));
        }
        assert_eq!(result.shuffle_journal().len(), 1);
    }
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_activation_uses_the_exiled_creatures_own_mana_cost_and_copiable_values() {
    use spellwake_engine::mana::{HybridAlternative, HybridSymbol};

    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Back from the Brink" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let brink = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let mut donor = Program::creature(
        Some(ManaCost::one(ManaType::Blue)),
        3.into(),
        4.into(),
        BTreeSet::new(),
    );
    donor.hybrid_symbols.push(HybridSymbol {
        first: ManaType::Blue,
        alternative: HybridAlternative::Mana(ManaType::Green),
    });
    donor.printed_keywords.insert(Flying);
    let price = [(ManaType::Green, 1)];
    let start = game(
        vec![A, B],
        vec![
            brink,
            seed(2, A, ObjectDestination::Graveyard, donor.clone()),
            seed(3, B, ObjectDestination::Graveyard, donor),
            seed(
                4,
                A,
                ObjectDestination::Graveyard,
                Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let activate = |id, payment| GameCommand::ActivateWithCostObjects {
        source: physical(&start, 1).id(),
        ability: spellwake_engine::activated::ActivatedAbilityId(0),
        granting_effect: None,
        target: None,
        objects: vec![physical(&start, id).id()],
        payment,
    };
    illegal(&start, A, activate(2, Some(spend(&[(ManaType::Blue, 1)]))));
    illegal(&start, A, activate(3, Some(spend(&price))));
    illegal(&start, A, activate(4, None));
    let paid = command(&start, A, activate(2, Some(spend(&price)))).into_state();
    assert_eq!(paid.execution().mana(A, ManaType::Green), 0.into());
    assert_eq!(physical(&paid, 2).zone(), ObjectZone::Exile);
    let result = round(paid);
    let token = tokens(&result, A, 1)[0];
    let current = result
        .state()
        .execution()
        .current_characteristics(token)
        .unwrap()
        .unwrap();
    assert!(current.has_keyword(Flying));
    assert_eq!(current.power_toughness.unwrap().derived.power, 3.into());
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_zone_restriction_keeps_the_exact_graveyard_card_and_blocks_flashback() {
    use spellwake_engine::program::ObjectFilter;

    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Grafdigger's Cage" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let cage = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            B,
            ObjectDestination::Battlefield { controller: B },
        )
        .unwrap();
    let reanimate = || {
        free_instant(
            BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature()),
            Instruction::ReturnOwnGraveyardCreatureToBattlefield,
        )
    };
    let mut flashback = free_instant(
        BodyTarget::None,
        Instruction::DrawController {
            amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
        },
    );
    flashback.flashback = Some(ManaCost::free());
    let start = game(
        vec![A, B],
        vec![
            cage,
            seed(
                2,
                A,
                ObjectDestination::Graveyard,
                Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
            ),
            seed(3, A, ObjectDestination::Hand, reanimate()),
            seed(4, A, ObjectDestination::Graveyard, flashback),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
            ),
            seed(6, A, ObjectDestination::Hand, reanimate()),
        ],
        &[],
        Step::PrecombatMain,
    );
    let creature = physical(&start, 2).id();
    let blocked = round(targeted(&start, A, 3, TargetRef::Object(creature)).into_state());
    assert_eq!(physical(blocked.state(), 2).id(), creature);
    assert_eq!(
        physical(blocked.state(), 2).zone(),
        ObjectZone::Graveyard(A)
    );
    let flashback = GameCommand::CastWithResources {
        spell: physical(blocked.state(), 4).id(),
        target: None,
        payment: CastingPayment::default(),
    };
    assert!(matches!(
        try_game(blocked.state(), A, flashback),
        Err(GameError::Effect(
            spellwake_engine::effects::EffectError::CastingFromZoneProhibited
        ))
    ));
    let without_cage = round(
        targeted(
            blocked.state(),
            A,
            5,
            TargetRef::Object(physical(blocked.state(), 1).id()),
        )
        .into_state(),
    )
    .into_state();
    let enters = round(targeted(&without_cage, A, 6, TargetRef::Object(creature)).into_state());
    assert_eq!(physical(enters.state(), 2).zone(), ObjectZone::Battlefield);
    assert_ne!(physical(enters.state(), 2).id(), creature);
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_named_counters_fund_the_library_choice_from_the_paid_count() {
    use spellwake_engine::effects::CounterKind;

    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Jar of Eyeballs" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let jar = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let price = [(ManaType::Colourless, 3)];
    let start = game(
        vec![A, B],
        vec![
            jar,
            creature(2, A, 2, 2, &[]),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
            ),
            seed(
                4,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                5,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                6,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                7,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let jar = physical(&start, 1).id();
    let killed =
        round(targeted(&start, A, 3, TargetRef::Object(physical(&start, 2).id())).into_state())
            .into_state();
    let counted = round(killed).into_state();
    assert_eq!(
        counted
            .execution()
            .counters_of_kind(jar, CounterKind::Eyeball),
        2.into()
    );
    let activated = command(
        &counted,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: jar,
            ability: spellwake_engine::activated::ActivatedAbilityId(0),
            payment: Some(spend(&price)),
        },
    )
    .into_state();
    assert_eq!(
        activated
            .execution()
            .counters_of_kind(jar, CounterKind::Eyeball),
        0.into()
    );
    let removed =
        round(targeted(&activated, A, 7, TargetRef::Object(jar)).into_state()).into_state();
    assert_eq!(physical(&removed, 1).zone(), ObjectZone::Graveyard(A));
    let choice = round(removed).into_state();
    let RuleFrame::RevealedPiles(frame) = choice.pending_decision().unwrap().frame() else {
        panic!("expected the private library choice")
    };
    assert_eq!(
        frame.choice(),
        &PileChoice::OneToHandRestBottom(vec![
            physical(&choice, 4).id(),
            physical(&choice, 5).id()
        ])
    );
    let took = reply(
        &choice,
        DecisionReply::SelectResolution(vec![physical(&choice, 5).id()]),
    );
    assert_eq!(physical(took.state(), 5).zone(), ObjectZone::Hand(A));
    assert_eq!(
        took.state().execution().window().cards().library(A).last(),
        Some(&physical(took.state(), 4).id())
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_study_counters_pay_for_one_return_batch_from_both_graveyards() {
    use spellwake_engine::{
        effects::CounterKind,
        mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput},
    };

    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Grimoire of the Dead" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let grimoire = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let goblin = vocabulary.resolve_label("Goblin").unwrap();
    let mut preserved = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([goblin]));
    preserved.color_indicator = Some(BTreeSet::from([
        spellwake_engine::characteristics::Color::Red,
    ]));
    let mut cards = vec![
        grimoire,
        seed(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::land(
                BTreeSet::new(),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())])),
                }],
            ),
        ),
        seed(
            3,
            A,
            ObjectDestination::Graveyard,
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        ),
        seed(4, B, ObjectDestination::Graveyard, preserved),
        seed(
            20,
            B,
            ObjectDestination::Graveyard,
            Program::artifact(None, vec![]),
        ),
    ];
    cards.extend((5..=7).map(|id| {
        seed(
            id,
            A,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        )
    }));
    for id in 8..20 {
        cards.push(seed(
            id,
            if id % 2 == 0 { A } else { B },
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
    }
    let mut state = game(vec![A, B], cards, &[], Step::PrecombatMain);
    let grimoire = physical(&state, 1).id();
    let raise = || GameCommand::ActivateUntargetedAbility {
        source: grimoire,
        ability: spellwake_engine::activated::ActivatedAbilityId(1),
        payment: None,
    };
    illegal(&state, A, raise());
    for id in 5..=7 {
        let mana = command(
            &state,
            A,
            GameCommand::ActivateMana {
                source: physical(&state, 2).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        )
        .into_state();
        let ability = command(
            &mana,
            A,
            GameCommand::ActivateWithCostObjects {
                source: grimoire,
                ability: spellwake_engine::activated::ActivatedAbilityId(0),
                granting_effect: None,
                target: None,
                objects: vec![physical(&mana, id).id()],
                payment: Some(spend(&[(ManaType::Colourless, 1)])),
            },
        )
        .into_state();
        let studied = round(ability).into_state();
        assert_eq!(
            studied
                .execution()
                .counters_of_kind(grimoire, CounterKind::Study),
            i64::try_from(id - 4).unwrap().into()
        );
        let next_turn = studied.turn().unwrap().position().number + 2;
        state = exile_hand::until(studied, |game| {
            let position = game.turn().unwrap().position();
            position.number == next_turn && position.step == Step::PrecombatMain
        });
    }
    let paid = command(&state, A, raise()).into_state();
    assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
    let pending = round(paid).into_state();
    let entering = [physical(&pending, 3).id(), physical(&pending, 4).id()];
    let returned = reply(
        &pending,
        DecisionReply::LinkedReturnOrder(entering.to_vec()),
    );
    assert!(events(&returned).any(|event| {
        event
            .occurrences
            .iter()
            .filter(|row| {
                matches!(&row.value, RulesOccurrence::ZoneChange(change)
            if change.reason == ZoneReason::ReturnFromGraveyardToBattlefield)
            })
            .count()
            == 2
    }));
    let raised = returned.into_state();
    assert_eq!(physical(&raised, 20).zone(), ObjectZone::Graveyard(B));
    let zombie = vocabulary.resolve_label("Zombie").unwrap();
    for id in [3, 4] {
        let card = physical(&raised, id);
        assert_eq!(card.zone(), ObjectZone::Battlefield);
        assert_eq!(card.controller(), Some(A));
        let value = raised
            .execution()
            .current_characteristics(card.id())
            .unwrap()
            .unwrap();
        assert!(value.current_creature_subtypes.contains(&zombie));
        assert!(
            value
                .current_colors
                .contains(&spellwake_engine::characteristics::Color::Black)
        );
        if id == 4 {
            assert!(value.current_creature_subtypes.contains(&goblin));
            assert!(
                value
                    .current_colors
                    .contains(&spellwake_engine::characteristics::Color::Red)
            );
        }
    }
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_counter_sized_tokens_track_the_source_and_retain_its_last_count() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Gutter Grime" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let grime = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let destroy = || free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget);
    let start = game(
        vec![A, B],
        vec![
            grime,
            creature(2, A, 2, 2, &[]),
            creature(3, A, 2, 2, &[]),
            seed(4, A, ObjectDestination::Hand, destroy()),
            seed(5, A, ObjectDestination::Hand, destroy()),
            seed(6, A, ObjectDestination::Hand, destroy()),
        ],
        &[],
        Step::PrecombatMain,
    );
    let remove = |state: &GameState, spell, object| {
        round(
            targeted(
                state,
                A,
                spell,
                TargetRef::Object(physical(state, object).id()),
            )
            .into_state(),
        )
        .into_state()
    };
    let power = |state: &GameState, object| {
        state
            .execution()
            .power_toughness(object)
            .unwrap()
            .unwrap()
            .derived
            .power
    };
    let first = round(remove(&start, 4, 2));
    let first_token = tokens(&first, A, 1)[0];
    assert_eq!(power(first.state(), first_token), 1.into());
    let second = round(remove(first.state(), 5, 3));
    let second_token = second
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .find(|card| {
            card.is_token() && card.zone() == ObjectZone::Battlefield && card.id() != first_token
        })
        .unwrap()
        .id();
    assert_eq!(power(second.state(), first_token), 2.into());
    assert_eq!(power(second.state(), second_token), 2.into());
    let gone = remove(second.state(), 6, 1);
    assert_eq!(physical(&gone, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(power(&gone, first_token), 2.into());
    assert_eq!(power(&gone, second_token), 2.into());
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_kindred_look_keeps_the_private_cohort_and_uses_the_last_host() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Call to the Kindred" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let call = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    let human = vocabulary.resolve_label("Human").unwrap();
    let zombie = vocabulary.resolve_label("Zombie").unwrap();
    let typed = |kind| Program::creature(None, 2.into(), 2.into(), BTreeSet::from([kind]));
    let price = [(ManaType::Colourless, 3), (ManaType::Blue, 1)];
    let start = game(
        vec![A, B],
        vec![
            call,
            seed(
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
                typed(human),
            ),
            seed(3, A, ObjectDestination::LibraryBottom, typed(zombie)),
            seed(4, A, ObjectDestination::LibraryBottom, typed(human)),
            seed(
                5,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &start,
        A,
        GameCommand::CastPaid {
            spell: physical(&start, 1).id(),
            target: TargetRef::Object(physical(&start, 2).id()),
            payment: spend(&price),
        },
    );
    let attached = round(cast.into_state()).into_state();
    let upkeep = exile_hand::until(attached, |state| {
        let position = state.turn().unwrap().position();
        position.number == 3 && position.step == Step::Upkeep
    });
    let destroyed =
        round(targeted(&upkeep, A, 6, TargetRef::Object(physical(&upkeep, 1).id())).into_state())
            .into_state();
    let offer = round(destroyed).into_state();
    let looked = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    let RuleFrame::RevealedPiles(frame) = looked.state().pending_decision().unwrap().frame() else {
        panic!("expected the private look")
    };
    let PileChoice::OptionalCreatureToBattlefield {
        looked: cards,
        eligible,
    } = frame.choice()
    else {
        panic!("expected the matching creature choice")
    };
    assert_eq!(cards.len(), 4);
    let matching = physical(looked.state(), 4).id();
    assert_eq!(eligible, &[matching]);
    assert!(
        !events(&looked)
            .flat_map(|event| &event.occurrences)
            .any(|row| matches!(&row.value, RulesOccurrence::LibraryCardRevealed { .. }))
    );
    let pending = reply(
        looked.state(),
        DecisionReply::SelectResolution(vec![matching]),
    )
    .into_state();
    let bottom = [100, 5, 3].map(|id| physical(&pending, id).id());
    let result = reply(
        &pending,
        DecisionReply::OrderResolutionGraveyard(bottom.to_vec()),
    )
    .into_state();
    assert_eq!(physical(&result, 4).zone(), ObjectZone::Battlefield);
    assert_eq!(result.execution().window().cards().library(A), bottom);
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_equipment_grant_pays_with_the_equipment_and_deals_damage_as_that_source() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Blazing Torch" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let torch = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let start = game(
        vec![A, B],
        vec![torch, creature(2, A, 2, 2, &[KeywordAbility::Haste])],
        &[(ManaType::Colourless, 1)],
        Step::PrecombatMain,
    );
    let torch = physical(&start, 1).id();
    let host = physical(&start, 2).id();
    let equipped = round(
        command(
            &start,
            A,
            GameCommand::ActivateAbility {
                source: torch,
                ability: ActivatedAbilityId(1),
                target: TargetRef::Object(host),
                payment: Some(spend(&[(ManaType::Colourless, 1)])),
            },
        )
        .into_state(),
    )
    .into_state();
    let current = equipped
        .execution()
        .current_characteristics(host)
        .unwrap()
        .unwrap();
    let [grant] = current
        .granted_activated
        .keys()
        .copied()
        .collect::<Vec<_>>()[..]
    else {
        panic!("expected one activated ability granted by the Equipment");
    };
    let announced = command(
        &equipped,
        A,
        GameCommand::ActivateGranted {
            source: host,
            effect: grant,
            target: Some(TargetRef::Player(B)),
            payment: None,
        },
    );
    assert_eq!(physical(announced.state(), 2).tapped(), Some(true));
    assert_eq!(
        physical(announced.state(), 1).zone(),
        ObjectZone::Graveyard(A)
    );
    let dealt = round(announced.into_state());
    assert!(dealt.damage_journal().applied.iter().any(|record| {
        record.source.id() == spellwake_engine::damage::DamageSourceId::Object(torch)
            && record.target == spellwake_engine::damage::DamageRecipient::Player(B)
            && record.amount == 2.into()
    }));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_attached_player_damage_is_a_resolution_choice() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Curse of the Pierced Heart" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let curse = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    let mut walker = Program::new(
        BTreeSet::from([CardType::Planeswalker]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(spellwake_engine::program::PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: Some(ManaCost::free()),
        }),
    );
    walker.printed_loyalty = Some(3.into());
    let price = [(ManaType::Colourless, 1), (ManaType::Red, 1)];
    let start = game(
        vec![A, B],
        vec![
            curse,
            seed(2, A, ObjectDestination::Hand, walker.clone()),
            seed(3, B, ObjectDestination::Hand, walker),
        ],
        &price,
        Step::PrecombatMain,
    );
    let start = round(cast_untargeted(&start, 2, &[]).into_state()).into_state();
    let attached = round(
        command(
            &start,
            A,
            GameCommand::CastPaid {
                spell: physical(&start, 1).id(),
                target: TargetRef::Player(A),
                payment: spend(&price),
            },
        )
        .into_state(),
    )
    .into_state();
    let main = exile_hand::until(attached, |state| {
        let position = state.turn().unwrap().position();
        position.number == 2 && position.step == Step::PrecombatMain
    });
    let entered = round(
        command(
            &main,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&main, 3).id(),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    let upkeep = exile_hand::until(entered, |state| {
        let position = state.turn().unwrap().position();
        position.number == 3 && position.step == Step::Upkeep
    });
    assert!(
        upkeep.pending_decision().is_none(),
        "no target is announced with the trigger"
    );
    let pending = round(upkeep).into_state();
    let walker = physical(&pending, 2).id();
    let RuleFrame::DamageRecipientChoice(frame) = pending.pending_decision().unwrap().frame()
    else {
        panic!("expected to choose the recipient of the damage");
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(
        frame.recipients(),
        &[TargetRef::Player(A), TargetRef::Object(walker)]
    );
    let dealt = reply(
        &pending,
        DecisionReply::ChooseDamageRecipient(TargetRef::Object(walker)),
    );
    assert!(dealt.damage_journal().applied.iter().any(|record| {
        record.target == spellwake_engine::damage::DamageRecipient::Planeswalker(walker)
            && record.amount == 1.into()
    }));
    assert_eq!(dealt.state().execution().life_total(A), Some(&20.into()));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_permanent_partition_uses_two_choosers_and_one_sacrifice_event() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Liliana of the Veil" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let mut walker = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    walker.program.printed_loyalty = Some(7.into());
    let price = [(ManaType::Colourless, 1), (ManaType::Black, 2)];
    let start = game(
        vec![A, B],
        vec![
            walker,
            creature(2, B, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            creature(4, B, 2, 2, &[]),
            creature(5, A, 2, 2, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let start = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let selected = round(
        command(
            &start,
            A,
            GameCommand::ActivateAbility {
                source: physical(&start, 1).id(),
                ability: ActivatedAbilityId(2),
                target: TargetRef::Player(B),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    let own = physical(&selected, 5).id();
    let [left, middle, right] = [2, 3, 4].map(|id| physical(&selected, id).id());
    let RuleFrame::RevealedPiles(frame) = selected.pending_decision().unwrap().frame() else {
        panic!("expected the controller to divide the permanents");
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(
        frame.choice(),
        &PileChoice::DividePermanents(vec![left, middle, right])
    );
    let divided = reply(
        &selected,
        DecisionReply::SelectResolution(vec![middle, right]),
    )
    .into_state();
    let RuleFrame::RevealedPiles(frame) = divided.pending_decision().unwrap().frame() else {
        panic!("expected the affected player to choose a pile");
    };
    assert_eq!(frame.chooser(), B);
    assert_eq!(
        frame.choice(),
        &PileChoice::Sacrifice {
            first: vec![middle, right],
            second: vec![left]
        }
    );
    let ordered = reply(&divided, DecisionReply::ChoosePile(1)).into_state();
    let RuleFrame::ResolutionGraveyardOrder(frame) = ordered.pending_decision().unwrap().frame()
    else {
        panic!("expected the owner to order the simultaneous graveyard arrivals");
    };
    assert_eq!(frame.chooser(), B);
    let sacrificed = reply(
        &ordered,
        DecisionReply::OrderResolutionGraveyard(vec![middle, right]),
    );
    assert!(events(&sacrificed).any(|event| {
        event
            .occurrences
            .iter()
            .filter(|row| {
                matches!(&row.value, RulesOccurrence::ZoneChange(change)
            if matches!(change.reason, ZoneReason::Sacrifice { player } if player == B))
            })
            .count()
            == 2
    }));
    assert_eq!(physical(sacrificed.state(), 2).id(), left);
    assert_eq!(physical(sacrificed.state(), 5).id(), own);
    assert_eq!(
        physical(sacrificed.state(), 3).zone(),
        ObjectZone::Graveyard(B)
    );
    assert_eq!(
        physical(sacrificed.state(), 4).zone(),
        ObjectZone::Graveyard(B)
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_destruction_returns_only_cards_actually_put_into_graveyards() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Sorin, Lord of Innistrad" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let mut sorin = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    sorin.program.printed_loyalty = Some(7.into());
    let mut walker = Program::new(
        BTreeSet::from([CardType::Planeswalker]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(spellwake_engine::program::PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: Some(ManaCost::free()),
        }),
    );
    walker.printed_loyalty = Some(3.into());
    let price = [
        (ManaType::Colourless, 2),
        (ManaType::White, 1),
        (ManaType::Black, 1),
    ];
    let start = game(
        vec![A, B],
        vec![
            sorin,
            creature(2, B, 2, 2, &[]),
            creature(3, B, 2, 2, &[KeywordAbility::Indestructible]),
            seed(4, A, ObjectDestination::Hand, walker),
        ],
        &price,
        Step::PrecombatMain,
    );
    let start = round(cast_untargeted(&start, 4, &[]).into_state()).into_state();
    let start = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let [sorin, creature, indestructible, walker] =
        [1, 2, 3, 4].map(|id| physical(&start, id).id());
    let activate = |targets: &[ObjectId]| GameCommand::ActivatePermanentTargets {
        source: sorin,
        ability: ActivatedAbilityId(2),
        targets: targets.iter().copied().map(TargetRef::Object).collect(),
        payment: None,
    };
    assert!(matches!(
        try_game(&start, A, activate(&[sorin])),
        Err(GameError::Effect(EffectError::IllegalTarget))
    ));
    let mut returned =
        round(command(&start, A, activate(&[creature, indestructible, walker])).into_state());
    if let Some(frame) = returned.state().pending_decision()
        && let RuleFrame::LinkedReturnOrder(order) = frame.frame()
    {
        returned = reply(
            returned.state(),
            DecisionReply::LinkedReturnOrder(order.objects().to_vec()),
        );
    }
    for (number, original) in [(2, creature), (4, walker)] {
        let card = physical(returned.state(), number);
        assert_eq!(card.zone(), ObjectZone::Battlefield);
        assert_eq!(card.controller(), Some(A));
        assert_ne!(card.id(), original);
    }
    assert_eq!(physical(returned.state(), 3).id(), indestructible);
    assert_eq!(physical(returned.state(), 3).controller(), Some(B));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_exiled_hand_card_can_be_played_by_the_ability_controller_after_cleanup() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Fiend of the Shadows" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let mut fiend = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    fiend.program.printed_keywords.insert(KeywordAbility::Haste);
    let start = game(
        vec![A, B],
        vec![
            fiend,
            named("Plains", 2, B, ObjectDestination::Hand),
            seed(
                3,
                B,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            ),
            creature(4, B, 2, 2, &[KeywordAbility::Flying]),
        ],
        &[],
        Step::PrecombatMain,
    );
    let attacks = combat_frame(&start, true);
    let attacking = reply(
        &attacks,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: physical(&attacks, 1).id(),
            defender: B,
            defending_planeswalker: None,
        }]),
    );
    let blocks = combat_frame(attacking.state(), false);
    let dealt =
        round(reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state()).into_state();
    let resolving = round(dealt).into_state();
    let RuleFrame::ResolutionSelect(choice) = resolving.pending_decision().unwrap().frame() else {
        panic!("expected the damaged player to exile a card");
    };
    let plains = physical(&resolving, 2).id();
    assert_eq!(choice.chooser(), B);
    assert_eq!(choice.domain(), &[plains, physical(&resolving, 3).id()]);
    let exiled = reply(&resolving, DecisionReply::SelectResolution(vec![plains])).into_state();
    let plains = physical(&exiled, 2).id();
    assert_eq!(physical(&exiled, 2).zone(), ObjectZone::Exile);
    assert!(!exiled.execution().may_play_from_exile(plains, B));
    assert_eq!(exiled.execution().flexible_exile_mana(plains, A), None);
    let next_main = exile_hand::until(exiled, |state| {
        let position = state.turn().unwrap().position();
        position.number == 3 && position.step == Step::PrecombatMain
    });
    assert!(next_main.execution().may_play_from_exile(plains, A));
    let played = command(&next_main, A, GameCommand::PlayLand { land: plains });
    assert_eq!(physical(played.state(), 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(played.state(), 2).controller(), Some(A));
    assert!(!played.state().execution().may_play_from_exile(plains, A));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_empty_draw_victory_ends_the_resolving_instruction_immediately() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Laboratory Maniac" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let maniac = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let start = game(
        vec![A, B],
        vec![
            maniac,
            seed(
                2,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::None,
                    Instruction::DrawEachPlayer {
                        amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                    },
                ),
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let source = physical(&start, 1).id();
    let result = round(cast_untargeted(&start, 2, &[]).into_state());
    let end = result.state();
    assert_eq!(
        end.outcome(),
        Some(spellwake_engine::draw::GameOutcome::Winner(A))
    );
    assert!(matches!(end.conclusion().unwrap().cause(),
        spellwake_engine::game::GameConclusionCause::WonByEffect { winner, source: exact, .. }
            if *winner == A && *exact == source));
    assert_eq!(physical(end, 100).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(end, 101).zone(), ObjectZone::Library(B));
    assert_eq!(physical(end, 2).zone(), ObjectZone::Stack);
    assert!(end.execution().state_based_losses().is_empty());
    assert!(matches!(
        result.rules_journal().records.last(),
        Some(RulesRecord::GameConcluded { .. })
    ));
    assert!(matches!(
        try_game(end, A, GameCommand::Pass),
        Err(GameError::GameEnded)
    ));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_fateful_protection_uses_one_color_for_the_player_and_current_permanents() {
    use spellwake_engine::program::DamageGroup;
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Faith's Shield" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let shield = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    for fateful in [false, true] {
        let red = |target, effect| {
            let mut spell = free_instant(target, effect);
            spell.color_indicator = Some(BTreeSet::from([
                spellwake_engine::characteristics::Color::Red,
            ]));
            spell
        };
        let price = [(ManaType::White, 1)];
        let mut state = game(
            vec![A, B],
            vec![
                shield.clone(),
                creature(2, A, 2, 2, &[]),
                creature(3, A, 2, 2, &[]),
                creature(4, B, 2, 2, &[]),
                seed(
                    5,
                    A,
                    ObjectDestination::Hand,
                    free_instant(
                        BodyTarget::None,
                        Instruction::AdjustControllerLife {
                            adjustment: spellwake_engine::life::LifeAdjustment::Lose(15.into()),
                        },
                    ),
                ),
                seed(
                    6,
                    A,
                    ObjectDestination::Hand,
                    red(
                        BodyTarget::SinglePlayer,
                        Instruction::Damage { amount: 1.into() },
                    ),
                ),
                seed(
                    7,
                    A,
                    ObjectDestination::Hand,
                    red(
                        BodyTarget::None,
                        Instruction::DamageGroup {
                            group: DamageGroup::Players,
                            amount: 1.into(),
                        },
                    ),
                ),
            ],
            &price,
            Step::PrecombatMain,
        );
        if fateful {
            state = round(cast_untargeted(&state, 5, &[]).into_state()).into_state();
        }
        let shield = |target| GameCommand::CastPaid {
            spell: physical(&state, 1).id(),
            target,
            payment: spend(&price),
        };
        let opponent = physical(&state, 4).id();
        assert!(matches!(
            try_game(&state, A, shield(TargetRef::Object(opponent))),
            Err(GameError::Effect(EffectError::IllegalTarget))
        ));
        let chosen = physical(&state, 2).id();
        let prompt =
            round(command(&state, A, shield(TargetRef::Object(chosen))).into_state()).into_state();
        let protected = reply(&prompt, DecisionReply::ManaColor(ManaType::Red)).into_state();
        let current = protected.execution().evaluated_characteristics().unwrap();
        assert!(
            current
                .get(chosen)
                .unwrap()
                .has_keyword(KeywordAbility::ProtectionFromRed)
        );
        let other = physical(&protected, 3).id();
        assert_eq!(
            current
                .get(other)
                .unwrap()
                .has_keyword(KeywordAbility::ProtectionFromRed),
            fateful
        );
        assert!(
            !current
                .get(opponent)
                .unwrap()
                .has_keyword(KeywordAbility::ProtectionFromRed)
        );
        let targeted = try_game(
            &protected,
            A,
            GameCommand::Cast {
                spell: physical(&protected, 6).id(),
                target: TargetRef::Player(A),
            },
        );
        assert_eq!(targeted.is_err(), fateful);
        let dealt = round(cast_untargeted(&protected, 7, &[]).into_state());
        assert_eq!(
            dealt.state().execution().life_total(A),
            Some(&if fateful { 5.into() } else { 19.into() })
        );
        assert_eq!(dealt.state().execution().life_total(B), Some(&19.into()));
        assert_eq!(
            dealt.damage_journal().protections.iter().any(
                |entry| entry.recipient == spellwake_engine::damage::DamageRecipient::Player(A)
            ),
            fateful
        );
        let next = exile_hand::until(dealt.into_state(), |game| {
            let position = game.turn().unwrap().position();
            position.number == 3 && position.step == Step::PrecombatMain
        });
        let current = next.execution().evaluated_characteristics().unwrap();
        assert!(current.player_protection_colors(A).next().is_none());
        assert!(
            !current
                .get(chosen)
                .unwrap()
                .has_keyword(KeywordAbility::ProtectionFromRed)
        );
    }
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_source_control_duration_expires_permanently_when_control_changes() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Olivia Voldaren" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let olivia = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let goblin = vocabulary.resolve_label("Goblin").unwrap();
    let vampire = vocabulary.resolve_label("Vampire").unwrap();
    let steal = free_instant(
        BodyTarget::SingleCreature,
        Instruction::CreateContinuousEffect {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                ControlRecipient::ResolvingController,
            )]),
            duration: ResolvedDuration::UntilEndOfTurn,
        },
    );
    let price = [
        (ManaType::Colourless, 4),
        (ManaType::Red, 1),
        (ManaType::Black, 2),
    ];
    let start = game(
        vec![A, B],
        vec![
            olivia,
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::creature(None, 2.into(), 4.into(), BTreeSet::from([goblin])),
            ),
            seed(3, B, ObjectDestination::Hand, steal),
        ],
        &price,
        Step::PrecombatMain,
    );
    let source = physical(&start, 1).id();
    let target = physical(&start, 2).id();
    let gain_control = || GameCommand::ActivateAbility {
        source,
        ability: ActivatedAbilityId(1),
        target: TargetRef::Object(target),
        payment: Some(spend(&[(ManaType::Colourless, 3), (ManaType::Black, 2)])),
    };
    assert!(matches!(
        try_game(&start, A, gain_control()),
        Err(GameError::Effect(EffectError::IllegalTarget))
    ));
    let changed = round(
        command(
            &start,
            A,
            GameCommand::ActivateAbility {
                source,
                ability: ActivatedAbilityId(0),
                target: TargetRef::Object(target),
                payment: Some(spend(&[(ManaType::Colourless, 1), (ManaType::Red, 1)])),
            },
        )
        .into_state(),
    )
    .into_state();
    let current = changed
        .execution()
        .current_characteristics(target)
        .unwrap()
        .unwrap();
    assert_eq!(
        current.current_creature_subtypes,
        BTreeSet::from([goblin, vampire])
    );
    assert_eq!(
        changed
            .execution()
            .counters_of_kind(source, spellwake_engine::effects::CounterKind::PlusOne),
        1.into()
    );
    let captured = round(command(&changed, A, gain_control()).into_state()).into_state();
    assert_eq!(physical(&captured, 2).controller(), Some(A));
    let other_priority = pass(&captured).into_state();
    let lost = round(targeted(&other_priority, B, 3, TargetRef::Object(source)).into_state());
    assert_eq!(physical(lost.state(), 1).controller(), Some(B));
    assert_eq!(physical(lost.state(), 2).controller(), Some(B));
    assert!(
        events(&lost)
            .flat_map(|event| &event.occurrences)
            .any(|row| matches!(row.value, RulesOccurrence::SourceControlEffectsEnded { .. }))
    );
    let regained = exile_hand::until(lost.into_state(), |game| {
        let position = game.turn().unwrap().position();
        position.number == 3 && position.step == Step::PrecombatMain
    });
    assert_eq!(physical(&regained, 1).controller(), Some(A));
    assert_eq!(physical(&regained, 2).controller(), Some(B));
    let current = regained
        .execution()
        .current_characteristics(target)
        .unwrap()
        .unwrap();
    assert_eq!(
        current.current_creature_subtypes,
        BTreeSet::from([goblin, vampire])
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_counter_followup_casts_a_matching_hand_spell_even_if_the_target_survives() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = OracleId::try_from("16de96d0-a58e-42a7-9a13-45358542a246".to_owned()).unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.as_str().to_owned()].into()),
    )
    .unwrap();
    let counter = registry
        .seed(&oracle, CardId(1), A, ObjectDestination::Hand)
        .unwrap();
    let followup = Program::instant(
        ManaCost::generic(9.into()),
        ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![Instruction::Damage { amount: 2.into() }],
        ),
    );
    let artifact = Program::artifact(Some(ManaCost::free()), vec![]);
    let mana = [(ManaType::Blue, 2), (ManaType::Colourless, 4)];
    for uncounterable in [false, true] {
        let mut original = free_instant(
            BodyTarget::SinglePlayer,
            Instruction::Damage { amount: 1.into() },
        );
        original.cannot_be_countered = uncounterable;
        let start = game(
            vec![A, B],
            vec![
                counter.clone(),
                seed(2, B, ObjectDestination::Hand, original),
                seed(3, A, ObjectDestination::Hand, followup.clone()),
                seed(4, A, ObjectDestination::Hand, artifact.clone()),
            ],
            &mana,
            Step::PrecombatMain,
        );
        let other_priority = pass(&start).into_state();
        let waiting = targeted(&other_priority, B, 2, TargetRef::Player(A)).into_state();
        let target = waiting.execution().window().entries().last().unwrap().id();
        let ready = pass(&waiting).into_state();
        let casting = command(
            &ready,
            A,
            GameCommand::CastWithResources {
                spell: physical(&ready, 1).id(),
                target: Some(TargetRef::Stack(target)),
                payment: CastingPayment {
                    mana: Some(spend(&mana)),
                    ..Default::default()
                },
            },
        )
        .into_state();
        let choose = round(casting).into_state();
        let RuleFrame::ResolutionSelect(frame) = choose.pending_decision().unwrap().frame() else {
            panic!("expected a choice among matching spells in hand");
        };
        let spell = physical(&choose, 3).id();
        assert_eq!(frame.domain(), &[spell]);
        assert_eq!(
            physical(&choose, 2).zone(),
            if uncounterable {
                ObjectZone::Stack
            } else {
                ObjectZone::Graveyard(B)
            }
        );
        let offer = reply(&choose, DecisionReply::SelectResolution(vec![spell])).into_state();
        let cast = reply(
            &offer,
            DecisionReply::ImpulseCast {
                target: Some(TargetRef::Player(B)),
                payment: CastingPayment::default(),
                mana_abilities: vec![],
            },
        )
        .into_state();
        assert_eq!(physical(&cast, 1).zone(), ObjectZone::Graveyard(A));
        assert_eq!(physical(&cast, 3).zone(), ObjectZone::Stack);
        assert_eq!(physical(&cast, 4).zone(), ObjectZone::Hand(A));
        let result = round(cast);
        assert_eq!(result.state().execution().life_total(B), Some(&18.into()));
    }
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_source_shuffle_uses_the_owner_and_only_reveals_through_the_match() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Mirror-Mad Phantasm" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let mirror = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            B,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let remove = free_instant(BodyTarget::SingleCreature, Instruction::ExileTarget);
    let mana = [(ManaType::Blue, 1), (ManaType::Colourless, 1)];
    let start = game(
        vec![A, B],
        vec![mirror, seed(2, A, ObjectDestination::Hand, remove)],
        &mana,
        Step::PrecombatMain,
    );
    let source = physical(&start, 1).id();
    let activated = command(
        &start,
        A,
        GameCommand::ActivateUntargetedAbility {
            source,
            ability: ActivatedAbilityId(0),
            payment: Some(spend(&mana)),
        },
    )
    .into_state();
    let activated = activated
        .supply_entropy(activated.basis(), vec![0; 16])
        .unwrap()
        .into_state();
    let result = round(activated.clone());
    assert_eq!(result.shuffle_journal().len(), 1);
    assert_eq!(result.shuffle_journal()[0].player, B);
    let result_state = result.state();
    assert_eq!(physical(result_state, 1).controller(), Some(B));
    assert_ne!(physical(result_state, 1).id(), source);
    assert_eq!(
        result_state.execution().window().cards().library(A).len(),
        1
    );
    assert_eq!(
        result_state.execution().window().cards().graveyard(B).len(),
        1
    );
    assert!(
        result_state
            .execution()
            .window()
            .cards()
            .library(B)
            .is_empty()
    );
    assert_eq!(
        events(&result)
            .flat_map(|event| &event.occurrences)
            .filter(|row| matches!(row.value, RulesOccurrence::LibraryCardRevealed { .. }))
            .count(),
        2
    );

    let exiled =
        round(targeted(&activated, A, 2, TargetRef::Object(source)).into_state()).into_state();
    let removed = round(exiled);
    assert!(removed.shuffle_journal().is_empty());
    assert_eq!(physical(removed.state(), 1).zone(), ObjectZone::Exile);
    assert_eq!(
        removed
            .state()
            .execution()
            .window()
            .cards()
            .library(B)
            .len(),
        1
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_low_loyalty_transforms_the_same_permanent_after_sequential_damage() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| {
            card.name.starts_with("Garruk Relentless // ") && card.required_by_base_formats
        })
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let walker = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    let mana = [(ManaType::Green, 1), (ManaType::Colourless, 3)];
    let start = game(
        vec![A, B],
        vec![walker, creature(2, B, 1, 3, &[])],
        &mana,
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&start, 1, &mana).into_state()).into_state();
    let garruk = physical(&entered, 1).id();
    let loyalty = |game: &GameState| {
        game.execution()
            .counters_of_kind(garruk, spellwake_engine::effects::CounterKind::Loyalty)
    };
    assert_eq!(loyalty(&entered), 3.into());
    let used = command(
        &entered,
        A,
        GameCommand::ActivateAbility {
            source: garruk,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(physical(&entered, 2).id()),
            payment: None,
        },
    )
    .into_state();
    let fought = round(used);
    assert_eq!(physical(fought.state(), 2).zone(), ObjectZone::Graveyard(B));
    assert_eq!(loyalty(fought.state()), 2.into());
    assert_eq!(fought.state().execution().window().entries().len(), 1);
    let complete = round(fought.into_state()).into_state();
    assert_eq!(physical(&complete, 1).id(), garruk);
    assert!(physical(&complete, 1).transformed());
    assert_eq!(loyalty(&complete), 2.into());
    let plus_one = || GameCommand::ActivateUntargetedAbility {
        source: garruk,
        ability: ActivatedAbilityId(0),
        payment: None,
    };
    assert!(try_game(&complete, A, plus_one()).is_err());
    let next = exile_hand::until(complete, |game| {
        let position = game.turn().unwrap().position();
        position.number == 3 && position.step == Step::PrecombatMain
    });
    let generated = round(command(&next, A, plus_one()).into_state());
    assert_eq!(loyalty(generated.state()), 3.into());
    let [token] = tokens(&generated, A, 1).try_into().unwrap();
    assert!(
        generated
            .state()
            .execution()
            .current_characteristics(token)
            .unwrap()
            .unwrap()
            .has_keyword(KeywordAbility::Deathtouch)
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_each_other_player_can_copy_and_retarget_the_same_departed_spell() {
    use spellwake_engine::effects::CopyTargetChoice;
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Curse of Echoes" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let curse = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    let original = free_instant(
        BodyTarget::SinglePlayer,
        Instruction::Damage { amount: 2.into() },
    );
    let counter = free_instant(
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
        Instruction::CounterSpell,
    );
    let mana = [(ManaType::Blue, 1), (ManaType::Colourless, 4)];
    let start = game(
        vec![A, B, C],
        vec![
            curse,
            seed(2, B, ObjectDestination::Hand, original),
            seed(3, A, ObjectDestination::Hand, counter),
        ],
        &mana,
        Step::PrecombatMain,
    );
    let cursed = command(
        &start,
        A,
        GameCommand::CastWithResources {
            spell: physical(&start, 1).id(),
            target: Some(TargetRef::Player(B)),
            payment: CastingPayment {
                mana: Some(spend(&mana)),
                ..Default::default()
            },
        },
    )
    .into_state();
    let cursed = round(cursed).into_state();
    let ready = pass(&cursed).into_state();
    let cast = targeted(&ready, B, 2, TargetRef::Player(A)).into_state();
    let original = cast.execution().window().entries().first().unwrap().id();
    let ready = exile_hand::until(cast, |state| {
        matches!(
            state.execution().window().phase(),
            WindowPhase::Priority(PriorityPhase::Ready { holder: A })
        )
    });
    let countered =
        round(targeted(&ready, A, 3, TargetRef::Stack(original)).into_state()).into_state();
    assert_eq!(physical(&countered, 2).zone(), ObjectZone::Graveyard(B));
    let choose = round(countered).into_state();
    assert_eq!(choose.pending_decision().unwrap().chooser(), A);

    let decline = reply(
        &choose,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(decline.pending_decision().unwrap().chooser(), C);
    let neither = reply(
        &decline,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert!(neither.execution().window().entries().is_empty());

    let a_targets = reply(
        &choose,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert_eq!(a_targets.pending_decision().unwrap().chooser(), A);
    let c_choice = reply(&a_targets, CopyTargetChoice::Retain.into()).into_state();
    assert_eq!(c_choice.pending_decision().unwrap().chooser(), C);
    let c_targets = reply(
        &c_choice,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    assert_eq!(c_targets.pending_decision().unwrap().chooser(), C);
    let copies = reply(
        &c_targets,
        CopyTargetChoice::Change(TargetRef::Player(B)).into(),
    );
    let controllers = events(&copies)
        .flat_map(|event| &event.occurrences)
        .filter_map(|row| match &row.value {
            RulesOccurrence::CopyCreated { controller, .. } => Some(*controller),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(controllers, [A, C]);
    let resolved = round(copies.into_state()).into_state();
    assert_eq!(resolved.execution().life_total(B), Some(&18.into()));
    let resolved = round(resolved).into_state();
    assert_eq!(resolved.execution().life_total(A), Some(&18.into()));
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_zombie_combat_replaces_unpreventable_damage_and_triggers_once_per_milled_creature() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Undead Alchemist" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let alchemist = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let zombie = vocabulary.resolve_label("Zombie").unwrap();
    let mut attacker = creature(
        2,
        A,
        2,
        2,
        &[
            KeywordAbility::Haste,
            KeywordAbility::DamageCannotBePrevented,
        ],
    );
    attacker.program.creature_subtypes.insert(zombie);
    let start = game(
        vec![A, B],
        vec![
            alchemist,
            attacker,
            creature(3, A, 1, 1, &[KeywordAbility::Haste]),
            seed(
                4,
                B,
                ObjectDestination::LibraryTop,
                Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            ),
            seed(
                5,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            creature(6, B, 1, 1, &[]),
        ],
        &[],
        Step::PrecombatMain,
    );
    let declare = combat_frame(&start, true);
    let attacking = reply(
        &declare,
        DecisionReply::DeclareAttackers(
            [2, 3]
                .map(|id| AttackDeclaration {
                    attacker: physical(&declare, id).id(),
                    defender: B,
                    defending_planeswalker: None,
                })
                .to_vec(),
        ),
    );
    let blocks = combat_frame(attacking.state(), false);
    let order =
        round(reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state()).into_state();
    let creature = physical(&order, 4).id();
    let other = physical(&order, 5).id();
    let decision = order.pending_decision().unwrap();
    assert_eq!(decision.chooser(), B);
    let RuleFrame::RevealedPiles(frame) = decision.frame() else {
        panic!("expected mill order");
    };
    let PileChoice::GraveyardOrder(domain) = frame.choice() else {
        panic!("expected a graveyard order");
    };
    assert_eq!(
        domain.iter().copied().collect::<BTreeSet<_>>(),
        [creature, other].into()
    );
    let milled = reply(
        &order,
        DecisionReply::OrderResolutionGraveyard(vec![other, creature]),
    )
    .into_state();
    assert_eq!(milled.execution().life_total(B), Some(&19.into()));
    assert_eq!(
        milled.execution().window().cards().graveyard(B),
        &[physical(&milled, 5).id(), physical(&milled, 4).id()]
    );
    assert_eq!(milled.execution().window().entries().len(), 1);
    let result = round(milled);
    assert_eq!(physical(result.state(), 4).zone(), ObjectZone::Exile);
    assert_eq!(physical(result.state(), 5).zone(), ObjectZone::Graveyard(B));
    let [created] = tokens(&result, A, 1).try_into().unwrap();
    assert!(
        result
            .state()
            .execution()
            .current_characteristics(created)
            .unwrap()
            .unwrap()
            .current_creature_subtypes
            .contains(&zombie)
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_a_graveyard_cast_grants_the_exact_cards_abilities_after_the_delayed_trigger() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Havengul Lich" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let lich = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let mut donor = Program::creature(
        Some(ManaCost::generic(2.into())),
        1.into(),
        2.into(),
        BTreeSet::new(),
    );
    donor.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::generic(1.into()),
        source_cost: spellwake_engine::activated::SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
            }],
        ),
    });
    let mana = [(ManaType::Colourless, 4)];
    let start = game(
        vec![A, B],
        vec![
            lich,
            seed(2, B, ObjectDestination::Graveyard, donor.clone()),
            seed(3, B, ObjectDestination::Graveyard, donor),
            seed(
                4,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
        ],
        &mana,
        Step::PrecombatMain,
    );
    let source = physical(&start, 1).id();
    let chosen = physical(&start, 2).id();
    let other = physical(&start, 3).id();
    let grants = |game: &GameState| {
        game.execution()
            .current_characteristics(source)
            .unwrap()
            .unwrap()
            .granted_activated
    };
    let activated = command(
        &start,
        A,
        GameCommand::ActivateAbility {
            source,
            ability: ActivatedAbilityId(0),
            target: TargetRef::Object(chosen),
            payment: Some(spend(&[(ManaType::Colourless, 1)])),
        },
    )
    .into_state();
    let permitted = round(activated).into_state();
    assert!(
        permitted
            .execution()
            .may_cast_from_graveyard_by_permission(chosen, A)
    );
    assert!(
        !permitted
            .execution()
            .may_cast_from_graveyard_by_permission(chosen, B)
    );
    assert!(
        !permitted
            .execution()
            .may_cast_from_graveyard_by_permission(other, A)
    );
    assert!(grants(&permitted).is_empty());
    let cast = cast_untargeted(&permitted, 2, &[(ManaType::Colourless, 2)]).into_state();
    assert!(grants(&cast).is_empty());
    assert_eq!(cast.execution().window().entries().len(), 2);
    let inherited = round(cast).into_state();
    let [effect] = grants(&inherited)
        .into_keys()
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let used = command(
        &inherited,
        A,
        GameCommand::ActivateGranted {
            source,
            effect,
            target: None,
            payment: Some(spend(&[(ManaType::Colourless, 1)])),
        },
    )
    .into_state();
    let drawn = round(used).into_state();
    assert_eq!(physical(&drawn, 4).zone(), ObjectZone::Hand(A));
    let entered = round(drawn).into_state();
    assert_eq!(physical(&entered, 2).owner(), B);
    assert_eq!(physical(&entered, 2).controller(), Some(A));
    let next = exile_hand::until(entered, |game| {
        let position = game.turn().unwrap().position();
        position.number == 2 && position.step == Step::PrecombatMain
    });
    assert!(grants(&next).is_empty());
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_nonland_name_choice_blocks_that_spell_until_the_source_leaves() {
    use spellwake_contracts::names::{EnglishCardName, ObjectNames};
    use spellwake_engine::land::{CardNameIndex, EntryChoice};
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| card.name == "Nevermore" && card.required_by_base_formats)
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let nevermore = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Hand,
        )
        .unwrap();
    let instant = |name: &str| {
        let mut program = free_instant(
            BodyTarget::SinglePlayer,
            Instruction::Damage { amount: 2.into() },
        );
        program.names = ObjectNames::single(EnglishCardName::new(name.to_owned()).unwrap());
        program
    };
    let remove = free_instant(BodyTarget::SinglePermanent, Instruction::ExileTarget);
    let mana = [(ManaType::White, 2), (ManaType::Colourless, 1)];
    let start = game(
        vec![A, B],
        vec![
            nevermore,
            seed(2, A, ObjectDestination::Hand, instant("Lightning Bolt")),
            seed(3, A, ObjectDestination::Hand, instant("Shock")),
            seed(4, A, ObjectDestination::Hand, remove),
        ],
        &mana,
        Step::PrecombatMain,
    );
    let pending = round(cast_untargeted(&start, 1, &mana).into_state()).into_state();
    let RuleFrame::EntryChoice(frame) = pending.pending_decision().unwrap().frame() else {
        panic!("expected a qualified name choice");
    };
    let names = frame.card_names();
    assert!(names.iter().any(|name| name.as_str() == "Black Lotus"));
    assert!(!names.iter().any(|name| name.as_str() == "Plains"));
    let index = names
        .iter()
        .position(|name| name.as_str() == "Lightning Bolt")
        .unwrap();
    let illegal_choice = GameCommand::Reply {
        decision: pending.pending_decision().unwrap().id(),
        choice: DecisionReply::EntryChoice(EntryChoice::CardName(CardNameIndex(names.len()))),
    };
    assert!(try_game(&pending, A, illegal_choice).is_err());
    let entered = reply(
        &pending,
        DecisionReply::EntryChoice(EntryChoice::CardName(CardNameIndex(index))),
    )
    .into_state();
    let attempt = |game: &GameState| GameCommand::Cast {
        spell: physical(game, 2).id(),
        target: TargetRef::Player(B),
    };
    assert!(matches!(
        try_game(&entered, A, attempt(&entered)),
        Err(GameError::Effect(EffectError::ChosenNameProhibitsCasting))
    ));
    let damaged = round(targeted(&entered, A, 3, TargetRef::Player(B)).into_state()).into_state();
    let left = round(
        targeted(
            &damaged,
            A,
            4,
            TargetRef::Object(physical(&damaged, 1).id()),
        )
        .into_state(),
    )
    .into_state();
    let cast = command(&left, A, attempt(&left)).into_state();
    assert_eq!(
        round(cast).state().execution().life_total(B),
        Some(&16.into())
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn innistrad_equipment_unattaches_transforms_and_grows_after_a_player_leaves() {
    let (inventory, vocabulary) = dated_sources();
    let oracle = inventory
        .cards
        .iter()
        .find(|card| {
            card.name.starts_with("Elbrus, the Binding Blade // ") && card.required_by_base_formats
        })
        .and_then(|card| card.oracle_id.clone())
        .unwrap();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[oracle.clone()].into()),
    )
    .unwrap();
    let equipment = registry
        .seed(
            &OracleId::try_from(oracle).unwrap(),
            CardId(1),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let ability = equipment.program.activated[0].id;
    let finisher = free_instant(
        BodyTarget::SinglePlayer,
        Instruction::Damage { amount: 30.into() },
    );
    let mana = [(ManaType::Colourless, 1)];
    let start = game(
        vec![A, B, C],
        vec![
            equipment,
            creature(2, A, 1, 2, &[KeywordAbility::Haste]),
            creature(3, B, 2, 2, &[]),
            seed(4, A, ObjectDestination::Hand, finisher),
        ],
        &mana,
        Step::PrecombatMain,
    );
    let item = physical(&start, 1).id();
    let creature = physical(&start, 2).id();
    let equip = command(
        &start,
        A,
        GameCommand::ActivateAbility {
            source: item,
            ability,
            target: TargetRef::Object(creature),
            payment: Some(spend(&mana)),
        },
    )
    .into_state();
    let attached = round(equip).into_state();
    assert_eq!(
        attached
            .execution()
            .creature(creature)
            .unwrap()
            .unwrap()
            .power,
        2.into()
    );
    let attacks = combat_frame(&attached, true);
    let attacking = reply(
        &attacks,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: creature,
            defender: B,
            defending_planeswalker: None,
        }]),
    );
    let blocks = combat_frame(attacking.state(), false);
    let hit =
        round(reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state()).into_state();
    let transformed = round(hit).into_state();
    assert_eq!(physical(&transformed, 1).id(), item);
    assert!(physical(&transformed, 1).transformed());
    assert_eq!(
        transformed
            .execution()
            .creature(creature)
            .unwrap()
            .unwrap()
            .power,
        1.into()
    );
    let demon = transformed
        .execution()
        .current_characteristics(item)
        .unwrap()
        .unwrap();
    assert_eq!(demon.attached_to, None);
    assert!(demon.has_keyword(KeywordAbility::Intimidate));
    let loss = round(targeted(&transformed, A, 4, TargetRef::Player(B)).into_state());
    assert_eq!(loss.state().execution().window().participants(), &[A, C]);
    assert!(
        loss.state()
            .execution()
            .window()
            .cards()
            .current_identity(CardId(3))
            .is_none()
    );
    assert!(
        loss.rules_journal()
            .records
            .iter()
            .any(|record| match record {
                RulesRecord::TriggerDiscovered(waiting) =>
                    waiting.source.object.id() == item
                        && waiting.body.instructions
                            == [Instruction::PutCountersOnSource { amount: 13.into() }],
                _ => false,
            })
    );
    let grown = round(loss.into_state()).into_state();
    let counters = grown.execution().power_toughness_counters(item).unwrap();
    assert_eq!(counters.plus_one, 13.into());
    assert_eq!(
        grown.execution().creature(item).unwrap().unwrap().power,
        26.into()
    );
}
