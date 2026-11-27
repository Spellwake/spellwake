//! Canonical constructed dungeons, room tables, and Incubator token builder (CR 309, 701.46, 701.50, 725).
use crate::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{Color, DungeonId, KeywordAbility},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaSacrificeCost},
    program::{
        AffectedPlayers, ArtifactSubtype, BodyTarget, CardType, EntryCounterAbility,
        ExecutableBody, Instruction, LibrarySearch, ObjectFilter, PermanentTypeFilter, Program,
        ResolvingPlayer, SearchCardMatcher, ValueExpr,
    },
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSubtype},
};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary};
use std::collections::BTreeSet;

/// One room in a dungeon with its display name, valid next-room indices, and room ability body.
#[derive(Debug, Clone)]
pub struct DungeonRoom {
    pub name: &'static str,
    pub exits: &'static [u8],
    pub body: ExecutableBody,
}

/// Names of the three standard dungeons selectable by "venture into the dungeon" (CR 309.2a).
pub const STANDARD_DUNGEONS: [(DungeonId, &str); 3] = [
    (DungeonId::LostMineOfPhandelver, "Lost Mine of Phandelver"),
    (DungeonId::DungeonOfTheMadMage, "Dungeon of the Mad Mage"),
    (DungeonId::TombOfAnnihilation, "Tomb of Annihilation"),
];

fn treasure_token_creation(vocabulary: &CreatureTypeVocabulary) -> Option<TokenCreation> {
    let mut def = Program::artifact(None, vec![]);
    def.artifact_subtypes.insert(ArtifactSubtype::Treasure);
    def.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::ChooseColor {
            amount: 1.into(),
            colors: ManaOutput::all_colors(),
        }
        .sacrifice(ManaSacrificeCost::Source { tap: true }),
    });
    let td = TokenDefinition::new(
        def,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Treasure)],
        },
        vocabulary,
    )
    .ok()?;
    Some(TokenCreation::one(td))
}

fn creature_token_creation(
    colors: BTreeSet<Color>,
    power: i64,
    toughness: i64,
    keywords: BTreeSet<KeywordAbility>,
    subtype_label: &str,
    amount: i64,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<TokenCreation> {
    let id = vocabulary.resolve_label(subtype_label)?;
    let mut def = Program::creature(None, power.into(), toughness.into(), BTreeSet::from([id]));
    def.effect_defined_colors = Some(colors);
    def.printed_keywords.extend(keywords);
    let td = TokenDefinition::new(
        def,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(id)],
        },
        vocabulary,
    )
    .ok()?;
    Some(TokenCreation {
        enters_tapped: false,
        definition: td,
        amount: ValueExpr::literal(amount),
        base_size: None,
        recipients: TokenRecipients::Controller,
        attacking_defender: None,
    })
}

fn single_type_filter(card_type: CardType) -> PermanentTypeFilter {
    PermanentTypeFilter {
        any_types: BTreeSet::from([card_type]),
        required_types: BTreeSet::new(),
        forbidden_types: BTreeSet::new(),
        object_filter: None,
    }
}

fn token_body(creation: Option<TokenCreation>) -> ExecutableBody {
    match creation {
        Some(c) => ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens { creations: vec![c] }],
        ),
        None => ExecutableBody::new(BodyTarget::None, vec![]),
    }
}

/// Return the ordered rooms of `dungeon` (index 0 is the top entrance room; empty `exits` = bottom room).
#[must_use]
pub fn dungeon_rooms(dungeon: DungeonId, vocabulary: &CreatureTypeVocabulary) -> Vec<DungeonRoom> {
    match dungeon {
        DungeonId::LostMineOfPhandelver => vec![
            DungeonRoom {
                name: "Cave Entrance",
                exits: &[1, 2],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        crate::toolbox::MutatePlayerSpec::scry(
                            ResolvingPlayer::Controller,
                            ValueExpr::literal(1),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Goblin Lair",
                exits: &[3, 4],
                body: token_body(creature_token_creation(
                    BTreeSet::from([Color::Red]),
                    1,
                    1,
                    BTreeSet::new(),
                    "Goblin",
                    1,
                    vocabulary,
                )),
            },
            DungeonRoom {
                name: "Mine Tunnels",
                exits: &[4, 5],
                body: token_body(treasure_token_creation(vocabulary)),
            },
            DungeonRoom {
                name: "Storeroom",
                exits: &[6],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        crate::toolbox::MutatePlayerSpec::bolster(1),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Dark Pool",
                exits: &[6],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustLife(Box::new(
                        crate::toolbox::AdjustLifeSpec::drain(
                            crate::toolbox::PlayerSelector::Group(AffectedPlayers::EachOpponent),
                            ValueExpr::literal(1),
                            crate::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            false,
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Fungi Cavern",
                exits: &[6],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        crate::toolbox::MutatePlayerSpec::scry(
                            ResolvingPlayer::Controller,
                            ValueExpr::literal(1),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Temple of Dumathoin",
                exits: &[],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::DrawCards(Box::new(
                        crate::toolbox::DrawCardsSpec::simple(
                            crate::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            ValueExpr::literal(1),
                        ),
                    ))],
                ),
            },
        ],
        DungeonId::DungeonOfTheMadMage => vec![
            DungeonRoom {
                name: "Yawning Portal",
                exits: &[1],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustLife(Box::new(
                        crate::toolbox::AdjustLifeSpec::gain(
                            crate::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            ValueExpr::literal(1),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Dungeon Level",
                exits: &[2, 3],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        crate::toolbox::MutatePlayerSpec::scry(
                            ResolvingPlayer::Controller,
                            ValueExpr::literal(1),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Goblin Bazaar",
                exits: &[4],
                body: token_body(treasure_token_creation(vocabulary)),
            },
            DungeonRoom {
                name: "Twisted Caverns",
                exits: &[4],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        crate::toolbox::MutatePlayerSpec::scry(
                            ResolvingPlayer::Controller,
                            ValueExpr::literal(1),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Lost Level",
                exits: &[5, 6],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        crate::toolbox::MutatePlayerSpec::scry(
                            ResolvingPlayer::Controller,
                            ValueExpr::literal(2),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Runestone Caverns",
                exits: &[7],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::ZoneOp(Box::new(
                            crate::toolbox::ZoneOpSpec::exile_top_library(
                                crate::toolbox::PlayerSelector::Group(AffectedPlayers::One(
                                    ResolvingPlayer::Controller,
                                )),
                                ValueExpr::literal(2),
                                false,
                            ),
                        )),
                        Instruction::ApplyContinuous(Box::new(
                            crate::toolbox::ApplyContinuousSpec::impulse_play_permission(
                                crate::toolbox::ObjectQuery::from_move_subject(
                                    crate::program::MoveSubjectRef::LastMoved,
                                ),
                                crate::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                                crate::program::ImpulsePlayDuration::ThisTurn,
                                false,
                                false,
                                false,
                            ),
                        )),
                    ],
                ),
            },
            DungeonRoom {
                name: "Muiral's Graveyard",
                exits: &[7],
                body: token_body(creature_token_creation(
                    BTreeSet::from([Color::Black]),
                    1,
                    1,
                    BTreeSet::new(),
                    "Skeleton",
                    2,
                    vocabulary,
                )),
            },
            DungeonRoom {
                name: "Deep Mines",
                exits: &[8],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        crate::toolbox::MutatePlayerSpec::scry(
                            ResolvingPlayer::Controller,
                            ValueExpr::literal(3),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Mad Mage's Lair",
                exits: &[],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::DrawCards(Box::new(
                        crate::toolbox::DrawCardsSpec::simple(
                            crate::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            ValueExpr::literal(3),
                        ),
                    ))],
                ),
            },
        ],
        DungeonId::TombOfAnnihilation => vec![
            DungeonRoom {
                name: "Trapped Entry",
                exits: &[1, 2],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustLife(Box::new(
                        crate::toolbox::AdjustLifeSpec::lose(
                            crate::toolbox::PlayerSelector::Group(AffectedPlayers::EachPlayer),
                            ValueExpr::literal(1),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Veils of Fear",
                exits: &[3],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustLife(Box::new(
                        crate::toolbox::AdjustLifeSpec::lose(
                            crate::toolbox::PlayerSelector::Group(AffectedPlayers::EachPlayer),
                            ValueExpr::literal(2),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Oubliette",
                exits: &[4],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::discard(
                            AffectedPlayers::One(ResolvingPlayer::Controller),
                            ValueExpr::literal(1),
                        ))),
                        Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::sacrifice(
                            AffectedPlayers::One(ResolvingPlayer::Controller),
                            ValueExpr::literal(1),
                            single_type_filter(CardType::Creature),
                        ))),
                        Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::sacrifice(
                            AffectedPlayers::One(ResolvingPlayer::Controller),
                            ValueExpr::literal(1),
                            single_type_filter(CardType::Artifact),
                        ))),
                        Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::sacrifice(
                            AffectedPlayers::One(ResolvingPlayer::Controller),
                            ValueExpr::literal(1),
                            single_type_filter(CardType::Land),
                        ))),
                    ],
                ),
            },
            DungeonRoom {
                name: "Sandfall Cell",
                exits: &[4],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustLife(Box::new(
                        crate::toolbox::AdjustLifeSpec::lose(
                            crate::toolbox::PlayerSelector::Group(AffectedPlayers::EachPlayer),
                            ValueExpr::literal(2),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Cradle of the Death God",
                exits: &[],
                body: token_body(creature_token_creation(
                    BTreeSet::from([Color::Black]),
                    4,
                    4,
                    BTreeSet::from([KeywordAbility::Deathtouch]),
                    "Horror",
                    1,
                    vocabulary,
                )),
            },
        ],
        DungeonId::Undercity => vec![
            DungeonRoom {
                name: "Secret Entrance",
                exits: &[1, 2],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::SearchLibrary {
                        filter: Box::new(LibrarySearch::to_hand(SearchCardMatcher::BasicLand)),
                    }],
                ),
            },
            DungeonRoom {
                name: "Forge",
                exits: &[3, 4],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        crate::toolbox::MutatePlayerSpec::bolster(2),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Lost Well",
                exits: &[4, 5],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        crate::toolbox::MutatePlayerSpec::scry(
                            ResolvingPlayer::Controller,
                            ValueExpr::literal(2),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Trap!",
                exits: &[6],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustLife(Box::new(
                        crate::toolbox::AdjustLifeSpec::lose(
                            crate::toolbox::PlayerSelector::Group(AffectedPlayers::EachOpponent),
                            ValueExpr::literal(5),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Arena",
                exits: &[6, 7],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        crate::toolbox::MutateObjectSpec::all(
                            crate::toolbox::ObjectQuery::zone(
                                crate::toolbox::QueryZone::Battlefield,
                                crate::program::ObjectFilter::creature().not_controlled(),
                            ),
                            crate::toolbox::ObjectMutation::Goad {
                                by: crate::toolbox::PlayerSelector::Resolving(
                                    crate::program::ResolvingPlayer::Controller,
                                ),
                                permanent: false,
                            },
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Stash",
                exits: &[7],
                body: token_body(treasure_token_creation(vocabulary)),
            },
            DungeonRoom {
                name: "Archives",
                exits: &[8],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::DrawCards(Box::new(
                        crate::toolbox::DrawCardsSpec::simple(
                            crate::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            ValueExpr::literal(1),
                        ),
                    ))],
                ),
            },
            DungeonRoom {
                name: "Catacombs",
                exits: &[8],
                body: token_body(creature_token_creation(
                    BTreeSet::from([Color::Black]),
                    4,
                    1,
                    BTreeSet::from([KeywordAbility::Menace]),
                    "Skeleton",
                    1,
                    vocabulary,
                )),
            },
            DungeonRoom {
                name: "Throne of the Dead Three",
                exits: &[],
                body: ExecutableBody::new(
                    BodyTarget::None,
                    crate::toolbox::zone_op::look_top_choose_instructions(
                        ResolvingPlayer::Controller,
                        ResolvingPlayer::Controller,
                        ValueExpr::literal(10),
                        Some(ObjectFilter::creature()),
                        1,
                        false,
                        crate::program::LookTopKeepDest::Battlefield {
                            tapped: false,
                            attacking: false,
                        },
                        crate::program::LookTopRestDest::BottomRandom,
                        false,
                        true,
                        false,
                        None,
                    ),
                ),
            },
        ],
    }
}

/// Build the standard colorless Incubator artifact token definition with `{2}: TransformSource`,
/// `count` entry `+1/+1` counters, and a 0/0 Phyrexian Artifact Creature back face (CR 111.10i, 701.50).
#[must_use]
pub fn incubator_token_creation(
    vocabulary: &CreatureTypeVocabulary,
    count: RulesInteger,
) -> Option<TokenCreation> {
    let mut back_subtypes = BTreeSet::new();
    if let Some(id) = vocabulary.resolve_label("Phyrexian") {
        back_subtypes.insert(id);
    }
    let mut back = Program::creature(None, 0.into(), 0.into(), back_subtypes);
    back.card_types.insert(CardType::Artifact);
    back.effect_defined_colors = Some(BTreeSet::new());
    let mut front = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::generic(2.into()),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::MutateObject(Box::new(
                    crate::toolbox::MutateObjectSpec::all(
                        crate::toolbox::ObjectQuery::Source,
                        crate::toolbox::ObjectMutation::Orientation(
                            crate::toolbox::OrientationOp::Transform,
                        ),
                    ),
                ))],
            ),
        }],
    );
    front.artifact_subtypes.insert(ArtifactSubtype::Incubator);
    front.effect_defined_colors = Some(BTreeSet::new());
    if count.is_positive() {
        front
            .entry_counter_abilities
            .push(EntryCounterAbility::FixedPlusOne(count));
    }
    front.transform_back = Some(Box::new(back));
    let td = TokenDefinition::new(
        front,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Incubator)],
        },
        vocabulary,
    )
    .ok()?;
    Some(TokenCreation {
        enters_tapped: false,
        definition: td,
        amount: ValueExpr::literal(1),
        base_size: None,
        recipients: TokenRecipients::Controller,
        attacking_defender: None,
    })
}
