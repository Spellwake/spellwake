//! Finite authored ordinary-card definitions shared by the selected teaching pools.
//! Full source bytes are checked at intake; these guards check every materialized
//! selected face. No unlisted Oracle text is parsed or inferred executable.
use super::{
    Definition, INTERACTION_REGISTRY_VERSION, MANDATORY_TRIGGER_REGISTRY_VERSION, OracleId,
    ProgramKey, RegistryError, Scope,
};
use spellwake_content::{Inventory, Legality};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    characteristics::Color,
    effects::Program,
    land::LandType,
    life::LifeAdjustment,
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    program::{
        BodyTarget, ExecutableBody, Instruction, PlayerRelation, ResolvingPlayer, Supertype,
        TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
    },
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSubtype},
};
use std::collections::{BTreeMap, BTreeSet};

pub use spellwake_contracts::teaching_profile::{
    OPTIONAL_PAYMENT_PROFILE, OPTIONAL_PAYMENT_REGISTRY_VERSION, TEACHING_PROFILE,
    TEACHING_REGISTRY_VERSION, TEACHING_SOURCE_SHA256, TEACHING_VOCABULARY_ID,
};

/// Explicit selected pools share one authored identity table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TeachingPool {
    Vanilla,
    Interaction,
    Mandatory,
    OptionalPayment,
}
enum AdmissionFailure {
    Source,
    Vocabulary,
    Missing(&'static str),
    Duplicate(&'static str),
    Printing(&'static str),
    Semantic(&'static str),
    Legality(&'static str),
}
impl TeachingPool {
    pub(super) fn registry_version(self) -> u32 {
        match self {
            Self::Vanilla => TEACHING_REGISTRY_VERSION,
            Self::Interaction => INTERACTION_REGISTRY_VERSION,
            Self::Mandatory => MANDATORY_TRIGGER_REGISTRY_VERSION,
            Self::OptionalPayment => OPTIONAL_PAYMENT_REGISTRY_VERSION,
        }
    }
    fn scope(self) -> Scope {
        match self {
            Self::Vanilla => Scope::TeachingOrdinaryCardV1,
            Self::Interaction => Scope::InteractionOrdinaryCardV1,
            Self::Mandatory => Scope::MandatoryTriggerOrdinaryCardV1,
            Self::OptionalPayment => Scope::OptionalPaymentOrdinaryCardV1,
        }
    }
    fn error(self, failure: AdmissionFailure) -> RegistryError {
        match (self, failure) {
            (Self::OptionalPayment, AdmissionFailure::Source) => {
                RegistryError::OptionalPaymentSourceMismatch
            }
            (Self::OptionalPayment, AdmissionFailure::Vocabulary) => {
                RegistryError::OptionalPaymentVocabularyMismatch
            }
            (Self::OptionalPayment, AdmissionFailure::Missing(oracle_id)) => {
                RegistryError::OptionalPaymentMissingIdentity { oracle_id }
            }
            (Self::OptionalPayment, AdmissionFailure::Duplicate(oracle_id)) => {
                RegistryError::OptionalPaymentDuplicateIdentity { oracle_id }
            }
            (Self::OptionalPayment, AdmissionFailure::Printing(oracle_id)) => {
                RegistryError::OptionalPaymentPrintingMismatch { oracle_id }
            }
            (Self::OptionalPayment, AdmissionFailure::Semantic(oracle_id)) => {
                RegistryError::OptionalPaymentSemanticMismatch { oracle_id }
            }
            (Self::OptionalPayment, AdmissionFailure::Legality(oracle_id)) => {
                RegistryError::OptionalPaymentLegalityMismatch { oracle_id }
            }
            // Earlier pools do not have a separate printing admission step.
            (Self::Vanilla, AdmissionFailure::Printing(oracle_id)) => {
                RegistryError::TeachingSemanticMismatch { oracle_id }
            }
            (Self::Interaction, AdmissionFailure::Printing(oracle_id)) => {
                RegistryError::InteractionSemanticMismatch { oracle_id }
            }
            (Self::Mandatory, AdmissionFailure::Printing(oracle_id)) => {
                RegistryError::MandatorySemanticMismatch { oracle_id }
            }
            (Self::Mandatory, AdmissionFailure::Source) => RegistryError::MandatorySourceMismatch,
            (Self::Mandatory, AdmissionFailure::Vocabulary) => {
                RegistryError::MandatoryVocabularyMismatch
            }
            (Self::Mandatory, AdmissionFailure::Missing(oracle_id)) => {
                RegistryError::MandatoryMissingIdentity { oracle_id }
            }
            (Self::Mandatory, AdmissionFailure::Duplicate(oracle_id)) => {
                RegistryError::MandatoryDuplicateIdentity { oracle_id }
            }
            (Self::Mandatory, AdmissionFailure::Semantic(oracle_id)) => {
                RegistryError::MandatorySemanticMismatch { oracle_id }
            }
            (Self::Mandatory, AdmissionFailure::Legality(oracle_id)) => {
                RegistryError::MandatoryLegalityMismatch { oracle_id }
            }
            (Self::Vanilla, AdmissionFailure::Source) => RegistryError::TeachingSourceMismatch,
            (Self::Interaction, AdmissionFailure::Source) => {
                RegistryError::InteractionSourceMismatch
            }
            (Self::Vanilla, AdmissionFailure::Vocabulary) => {
                RegistryError::TeachingVocabularyMismatch
            }
            (Self::Interaction, AdmissionFailure::Vocabulary) => {
                RegistryError::InteractionVocabularyMismatch
            }
            (Self::Vanilla, AdmissionFailure::Missing(oracle_id)) => {
                RegistryError::TeachingMissingIdentity { oracle_id }
            }
            (Self::Interaction, AdmissionFailure::Missing(oracle_id)) => {
                RegistryError::InteractionMissingIdentity { oracle_id }
            }
            (Self::Vanilla, AdmissionFailure::Duplicate(oracle_id)) => {
                RegistryError::TeachingDuplicateIdentity { oracle_id }
            }
            (Self::Interaction, AdmissionFailure::Duplicate(oracle_id)) => {
                RegistryError::InteractionDuplicateIdentity { oracle_id }
            }
            (Self::Vanilla, AdmissionFailure::Semantic(oracle_id)) => {
                RegistryError::TeachingSemanticMismatch { oracle_id }
            }
            (Self::Interaction, AdmissionFailure::Semantic(oracle_id)) => {
                RegistryError::InteractionSemanticMismatch { oracle_id }
            }
            (Self::Vanilla, AdmissionFailure::Legality(oracle_id)) => {
                RegistryError::TeachingLegalityMismatch { oracle_id }
            }
            (Self::Interaction, AdmissionFailure::Legality(oracle_id)) => {
                RegistryError::InteractionLegalityMismatch { oracle_id }
            }
        }
    }
}

struct AuthoredCard {
    membership: &'static [TeachingPool],
    oracle_id: &'static str,
    name: &'static str,
    mana: &'static str,
    type_line: &'static str,
    text: &'static str,
    power: Option<&'static str>,
    toughness: Option<&'static str>,
    kind: AuthoredKind,
}
enum AuthoredKind {
    BasicLand(LandType),
    Creature {
        generic: u8,
        colored: u8,
        mana_type: ManaType,
        power: u8,
        toughness: u8,
        subtypes: &'static [&'static str],
        trigger: Option<AuthoredTrigger>,
        printed_mana: Option<ManaType>,
    },
    Artifact {
        generic: u8,
        mana_amount: u8,
        mana_colors: &'static [ManaType],
    },
    Enchantment {
        generic: u8,
        colored: u8,
        mana_type: ManaType,
        trigger: AuthoredTrigger,
    },
    Spell {
        generic: u8,
        colored: u8,
        mana_type: ManaType,
        kind: AuthoredSpellKind,
        effect: AuthoredEffect,
    },
}
enum AuthoredSpellKind {
    Instant,
    Sorcery,
}
enum AuthoredEffect {
    BounceCreature,
    CounterSpell,
    DrawTwo,
    CreateSoldiers,
    CounterUnlessPaidThree,
    CopySpell,
}
impl AuthoredEffect {
    fn key(&self, pool: TeachingPool) -> ProgramKey {
        match self {
            Self::BounceCreature if pool == TeachingPool::OptionalPayment => {
                ProgramKey::OptionalPaymentBounceCreatureV1
            }
            Self::CounterSpell if pool == TeachingPool::OptionalPayment => {
                ProgramKey::OptionalPaymentCounterSpellV1
            }
            Self::DrawTwo if pool == TeachingPool::OptionalPayment => {
                ProgramKey::OptionalPaymentDrawTwoV1
            }
            Self::CreateSoldiers if pool == TeachingPool::OptionalPayment => {
                ProgramKey::OptionalPaymentTokenCreationSpellV1
            }
            Self::CounterUnlessPaidThree => ProgramKey::OptionalPaymentUnlessCounterSpellV1,
            Self::CopySpell => ProgramKey::OptionalPaymentCopySpellV1,
            Self::BounceCreature if pool == TeachingPool::Mandatory => {
                ProgramKey::MandatoryBounceCreatureV1
            }
            Self::BounceCreature => ProgramKey::InteractionBounceCreatureV1,
            Self::CounterSpell if pool == TeachingPool::Mandatory => {
                ProgramKey::MandatoryCounterSpellV1
            }
            Self::CounterSpell => ProgramKey::InteractionCounterSpellV1,
            Self::DrawTwo => ProgramKey::InteractionDrawTwoV1,
            Self::CreateSoldiers => ProgramKey::MandatoryTokenCreationSpellV1,
        }
    }
    fn body(&self, vocabulary: &CreatureTypeVocabulary) -> Result<ExecutableBody, RegistryError> {
        Ok(match self {
            Self::BounceCreature => ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::ReturnToOwnersHand],
            ),
            Self::CounterUnlessPaidThree => ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![
                    Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::TargetSpellController,
                        cost: ManaCost::generic(3.into()),
                        paid_destination: 2,
                        declined_destination: 1,
                    },
                    Instruction::CounterSpell,
                ],
            ),
            Self::CopySpell => ExecutableBody::new(
                BodyTarget::SingleSpell(
                    spellwake_engine::program::SpellFilter::copyable_instruction(),
                ),
                vec![Instruction::CopySpell],
            ),
            Self::CounterSpell => ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::CounterSpell],
            ),
            Self::DrawTwo => ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                }],
            ),
            Self::CreateSoldiers => ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        definition: token_template("Soldier", Color::White, 1, vocabulary)?,
                        amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                        base_size: None,
                        enters_tapped: false,
                        recipients: TokenRecipients::Controller,
                    }],
                }],
            ),
        })
    }
}

struct AuthoredTrigger {
    condition: TriggerSpec,
    effect: AuthoredTriggerEffect,
}
enum AuthoredTriggerEffect {
    GainOne,
    DrawOne,
    DrawOneLoseOne,
    TargetLoseOneGainOne,
    CreateZombie,
    OptionalGainOne,
    OptionalDrawOne,
    DrawUnlessRelatedPaysOne,
}
impl AuthoredTrigger {
    fn ability(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<TriggeredAbility, RegistryError> {
        let draw = Instruction::DrawController {
            amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
        };
        let gain = Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        };
        let (target, instructions) = match self.effect {
            AuthoredTriggerEffect::OptionalGainOne => (
                BodyTarget::None,
                vec![
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    gain,
                ],
            ),
            AuthoredTriggerEffect::OptionalDrawOne => (
                BodyTarget::None,
                vec![
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    draw,
                ],
            ),
            AuthoredTriggerEffect::DrawUnlessRelatedPaysOne => (
                BodyTarget::None,
                vec![
                    Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::UniqueRelatedPlayer,
                        cost: ManaCost::generic(1.into()),
                        paid_destination: 3,
                        declined_destination: 1,
                    },
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    draw,
                ],
            ),
            AuthoredTriggerEffect::GainOne => (BodyTarget::None, vec![gain]),
            AuthoredTriggerEffect::DrawOne => (BodyTarget::None, vec![draw]),
            AuthoredTriggerEffect::DrawOneLoseOne => (
                BodyTarget::None,
                vec![
                    draw,
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Lose(1.into()),
                    },
                ],
            ),
            AuthoredTriggerEffect::TargetLoseOneGainOne => (
                BodyTarget::SinglePlayer,
                vec![
                    Instruction::AdjustTargetLife {
                        adjustment: LifeAdjustment::Lose(1.into()),
                    },
                    gain,
                ],
            ),
            AuthoredTriggerEffect::CreateZombie => (
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation::one(token_template(
                        "Zombie",
                        Color::Black,
                        2,
                        vocabulary,
                    )?)],
                }],
            ),
        };
        Ok(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: self.condition.clone(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(target, instructions),
        })
    }
}

fn token_template(
    label: &str,
    color: Color,
    size: u8,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<TokenDefinition, RegistryError> {
    let subtype = vocabulary
        .resolve_label(label)
        .ok_or(RegistryError::MissingCreatureType)?;
    let mut program = Program::creature(
        None,
        i64::from(size).into(),
        i64::from(size).into(),
        BTreeSet::from([subtype]),
    );
    program.effect_defined_colors = Some(BTreeSet::from([color]));
    TokenDefinition::new(
        program,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(subtype)],
        },
        vocabulary,
    )
    .map_err(RegistryError::TokenDefinition)
}

// Fixed 46-card teaching pool table referenced directly by compile-time lookup.
#[allow(clippy::large_const_arrays)]
const CARDS: [AuthoredCard; 46] = [
    AuthoredCard {
        membership: &[TeachingPool::Vanilla, TeachingPool::Interaction],
        oracle_id: "86e3c86c-d50e-4aa9-b080-f68b6afee87c",
        name: "Alpine Grizzly",
        mana: "{2}{G}",
        type_line: "Creature — Bear",
        text: "",
        power: Some("4"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 2,
            colored: 1,
            mana_type: ManaType::Green,
            power: 4,
            toughness: 2,
            subtypes: &["Bear"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[
            TeachingPool::Vanilla,
            TeachingPool::Interaction,
            TeachingPool::Mandatory,
        ],
        oracle_id: "2f5bf099-2e01-4e1c-9ebf-0ce0ac66939e",
        name: "Centaur Courser",
        mana: "{2}{G}",
        type_line: "Creature — Centaur Warrior",
        text: "",
        power: Some("3"),
        toughness: Some("3"),
        kind: AuthoredKind::Creature {
            generic: 2,
            colored: 1,
            mana_type: ManaType::Green,
            power: 3,
            toughness: 3,
            subtypes: &["Centaur", "Warrior"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla, TeachingPool::OptionalPayment],
        oracle_id: "b3b9a87d-cb95-435c-90b6-037406cab32e",
        name: "Elite Vanguard",
        mana: "{W}",
        type_line: "Creature — Human Soldier",
        text: "",
        power: Some("2"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 0,
            colored: 1,
            mana_type: ManaType::White,
            power: 2,
            toughness: 1,
            subtypes: &["Human", "Soldier"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[
            TeachingPool::Vanilla,
            TeachingPool::Interaction,
            TeachingPool::Mandatory,
        ],
        oracle_id: "b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6",
        name: "Forest",
        mana: "",
        type_line: "Basic Land — Forest",
        text: "({T}: Add {G}.)",
        power: None,
        toughness: None,
        kind: AuthoredKind::BasicLand(LandType::Forest),
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla],
        oracle_id: "55c60b29-52c5-428c-9229-11ed0e26bd71",
        name: "Glory Seeker",
        mana: "{1}{W}",
        type_line: "Creature — Human Soldier",
        text: "",
        power: Some("2"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::White,
            power: 2,
            toughness: 2,
            subtypes: &["Human", "Soldier"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[
            TeachingPool::Vanilla,
            TeachingPool::Interaction,
            TeachingPool::Mandatory,
        ],
        oracle_id: "14c8f55d-d177-4c25-a931-ebeb9e6062a0",
        name: "Grizzly Bears",
        mana: "{1}{G}",
        type_line: "Creature — Bear",
        text: "",
        power: Some("2"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Green,
            power: 2,
            toughness: 2,
            subtypes: &["Bear"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[
            TeachingPool::Vanilla,
            TeachingPool::Interaction,
            TeachingPool::Mandatory,
        ],
        oracle_id: "df7f697e-6886-4897-a024-61ae225c1b34",
        name: "Kalonian Tusker",
        mana: "{G}{G}",
        type_line: "Creature — Beast",
        text: "",
        power: Some("3"),
        toughness: Some("3"),
        kind: AuthoredKind::Creature {
            generic: 0,
            colored: 2,
            mana_type: ManaType::Green,
            power: 3,
            toughness: 3,
            subtypes: &["Beast"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla],
        oracle_id: "cd990c63-ba29-46f8-afa5-4cd5fe0b0166",
        name: "Loxodon Wayfarer",
        mana: "{2}{W}",
        type_line: "Creature — Elephant Monk",
        text: "",
        power: Some("1"),
        toughness: Some("5"),
        kind: AuthoredKind::Creature {
            generic: 2,
            colored: 1,
            mana_type: ManaType::White,
            power: 1,
            toughness: 5,
            subtypes: &["Elephant", "Monk"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla, TeachingPool::Interaction],
        oracle_id: "e876d1fc-3acd-41a3-a34b-2bfa83204393",
        name: "Nessian Courser",
        mana: "{2}{G}",
        type_line: "Creature — Centaur Warrior",
        text: "",
        power: Some("3"),
        toughness: Some("3"),
        kind: AuthoredKind::Creature {
            generic: 2,
            colored: 1,
            mana_type: ManaType::Green,
            power: 3,
            toughness: 3,
            subtypes: &["Centaur", "Warrior"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla],
        oracle_id: "014d00fc-434a-4b06-84b2-afd930677d61",
        name: "Oreskos Swiftclaw",
        mana: "{1}{W}",
        type_line: "Creature — Cat Warrior",
        text: "",
        power: Some("3"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::White,
            power: 3,
            toughness: 1,
            subtypes: &["Cat", "Warrior"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla],
        oracle_id: "0f7c70c4-9795-4c76-8b27-043942a963c6",
        name: "Pillarfield Ox",
        mana: "{3}{W}",
        type_line: "Creature — Ox",
        text: "",
        power: Some("2"),
        toughness: Some("4"),
        kind: AuthoredKind::Creature {
            generic: 3,
            colored: 1,
            mana_type: ManaType::White,
            power: 2,
            toughness: 4,
            subtypes: &["Ox"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[
            TeachingPool::Vanilla,
            TeachingPool::Mandatory,
            TeachingPool::OptionalPayment,
        ],
        oracle_id: "bc71ebf6-2056-41f7-be35-b2e5c34afa99",
        name: "Plains",
        mana: "",
        type_line: "Basic Land — Plains",
        text: "({T}: Add {W}.)",
        power: None,
        toughness: None,
        kind: AuthoredKind::BasicLand(LandType::Plains),
    },
    AuthoredCard {
        membership: &[
            TeachingPool::Vanilla,
            TeachingPool::Interaction,
            TeachingPool::Mandatory,
        ],
        oracle_id: "3191c6ca-4d25-4ba3-bfe1-4aeab1295573",
        name: "Rumbling Baloth",
        mana: "{2}{G}{G}",
        type_line: "Creature — Beast",
        text: "",
        power: Some("4"),
        toughness: Some("4"),
        kind: AuthoredKind::Creature {
            generic: 2,
            colored: 2,
            mana_type: ManaType::Green,
            power: 4,
            toughness: 4,
            subtypes: &["Beast"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla, TeachingPool::Interaction],
        oracle_id: "ec49dfcf-d16d-4621-af4b-4a6f09043221",
        name: "Runeclaw Bear",
        mana: "{1}{G}",
        type_line: "Creature — Bear",
        text: "",
        power: Some("2"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Green,
            power: 2,
            toughness: 2,
            subtypes: &["Bear"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla],
        oracle_id: "60ba93eb-39e6-4af2-9c66-cd38f72daff2",
        name: "Savannah Lions",
        mana: "{W}",
        type_line: "Creature — Cat",
        text: "",
        power: Some("2"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 0,
            colored: 1,
            mana_type: ManaType::White,
            power: 2,
            toughness: 1,
            subtypes: &["Cat"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla],
        oracle_id: "e3c0b11e-7340-4e0a-98b7-91b38439d4f9",
        name: "Silvercoat Lion",
        mana: "{1}{W}",
        type_line: "Creature — Cat",
        text: "",
        power: Some("2"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::White,
            power: 2,
            toughness: 2,
            subtypes: &["Cat"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla, TeachingPool::Interaction],
        oracle_id: "05425f2f-7228-4bf5-8fe1-6fe99107e8e0",
        name: "Spined Wurm",
        mana: "{4}{G}",
        type_line: "Creature — Wurm",
        text: "",
        power: Some("5"),
        toughness: Some("4"),
        kind: AuthoredKind::Creature {
            generic: 4,
            colored: 1,
            mana_type: ManaType::Green,
            power: 5,
            toughness: 4,
            subtypes: &["Wurm"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla, TeachingPool::Interaction],
        oracle_id: "3a9b9fe0-1846-41fc-b7c3-ed9abef27a6a",
        name: "Terrain Elemental",
        mana: "{1}{G}",
        type_line: "Creature — Elemental",
        text: "",
        power: Some("3"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Green,
            power: 3,
            toughness: 2,
            subtypes: &["Elemental"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla],
        oracle_id: "1adc4691-20cf-4f55-af85-3132fe1ff2a8",
        name: "Traveling Philosopher",
        mana: "{1}{W}",
        type_line: "Creature — Human Advisor",
        text: "",
        power: Some("2"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::White,
            power: 2,
            toughness: 2,
            subtypes: &["Human", "Advisor"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Vanilla],
        oracle_id: "19b4cf0b-e1e4-4e50-80bc-673bc6ef8bb8",
        name: "Yoked Ox",
        mana: "{W}",
        type_line: "Creature — Ox",
        text: "",
        power: Some("0"),
        toughness: Some("4"),
        kind: AuthoredKind::Creature {
            generic: 0,
            colored: 1,
            mana_type: ManaType::White,
            power: 0,
            toughness: 4,
            subtypes: &["Ox"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Interaction],
        oracle_id: "445bb343-9758-459b-970f-47f1705f1e55",
        name: "Armored Cancrix",
        mana: "{4}{U}",
        type_line: "Creature — Crab",
        text: "",
        power: Some("2"),
        toughness: Some("5"),
        kind: AuthoredKind::Creature {
            generic: 4,
            colored: 1,
            mana_type: ManaType::Blue,
            power: 2,
            toughness: 5,
            subtypes: &["Crab"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Interaction],
        oracle_id: "7d00fb28-ea6c-49a9-b4af-ffb38860a9a7",
        name: "Cancel",
        mana: "{1}{U}{U}",
        type_line: "Instant",
        text: "Counter target spell.",
        power: None,
        toughness: None,
        kind: AuthoredKind::Spell {
            generic: 1,
            colored: 2,
            mana_type: ManaType::Blue,
            kind: AuthoredSpellKind::Instant,
            effect: AuthoredEffect::CounterSpell,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Interaction, TeachingPool::OptionalPayment],
        oracle_id: "4ed27607-21a8-4bc3-997e-6d2242313f6d",
        name: "Coral Merfolk",
        mana: "{1}{U}",
        type_line: "Creature — Merfolk",
        text: "",
        power: Some("2"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Blue,
            power: 2,
            toughness: 1,
            subtypes: &["Merfolk"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[
            TeachingPool::Interaction,
            TeachingPool::Mandatory,
            TeachingPool::OptionalPayment,
        ],
        oracle_id: "cc187110-1148-4090-bbb8-e205694a39f5",
        name: "Counterspell",
        mana: "{U}{U}",
        type_line: "Instant",
        text: "Counter target spell.",
        power: None,
        toughness: None,
        kind: AuthoredKind::Spell {
            generic: 0,
            colored: 2,
            mana_type: ManaType::Blue,
            kind: AuthoredSpellKind::Instant,
            effect: AuthoredEffect::CounterSpell,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Interaction, TeachingPool::OptionalPayment],
        oracle_id: "273b339c-964b-4a18-8eb5-ceb8abcdfd9e",
        name: "Divination",
        mana: "{2}{U}",
        type_line: "Sorcery",
        text: "Draw two cards.",
        power: None,
        toughness: None,
        kind: AuthoredKind::Spell {
            generic: 2,
            colored: 1,
            mana_type: ManaType::Blue,
            kind: AuthoredSpellKind::Sorcery,
            effect: AuthoredEffect::DrawTwo,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Interaction],
        oracle_id: "381fa2a9-69fc-4558-a0ac-fb99c6f8d77f",
        name: "Fugitive Wizard",
        mana: "{U}",
        type_line: "Creature — Human Wizard",
        text: "",
        power: Some("1"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 0,
            colored: 1,
            mana_type: ManaType::Blue,
            power: 1,
            toughness: 1,
            subtypes: &["Human", "Wizard"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[
            TeachingPool::Interaction,
            TeachingPool::Mandatory,
            TeachingPool::OptionalPayment,
        ],
        oracle_id: "b2c6aa39-2d2a-459c-a555-fb48ba993373",
        name: "Island",
        mana: "",
        type_line: "Basic Land — Island",
        text: "({T}: Add {U}.)",
        power: None,
        toughness: None,
        kind: AuthoredKind::BasicLand(LandType::Island),
    },
    AuthoredCard {
        membership: &[TeachingPool::Interaction],
        oracle_id: "0bb1c704-47f1-4d0c-8158-cfcd0b5e4d06",
        name: "Maritime Guard",
        mana: "{1}{U}",
        type_line: "Creature — Merfolk Soldier",
        text: "",
        power: Some("1"),
        toughness: Some("3"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Blue,
            power: 1,
            toughness: 3,
            subtypes: &["Merfolk", "Soldier"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Interaction],
        oracle_id: "218d9277-c179-4de3-9c7f-79b5a6d4fa38",
        name: "Merfolk of the Pearl Trident",
        mana: "{U}",
        type_line: "Creature — Merfolk",
        text: "",
        power: Some("1"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 0,
            colored: 1,
            mana_type: ManaType::Blue,
            power: 1,
            toughness: 1,
            subtypes: &["Merfolk"],
            trigger: None,
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[
            TeachingPool::Interaction,
            TeachingPool::Mandatory,
            TeachingPool::OptionalPayment,
        ],
        oracle_id: "837182db-1bf3-4a2c-bd01-1af9d9873561",
        name: "Unsummon",
        mana: "{U}",
        type_line: "Instant",
        text: "Return target creature to its owner's hand.",
        power: None,
        toughness: None,
        kind: AuthoredKind::Spell {
            generic: 0,
            colored: 1,
            mana_type: ManaType::Blue,
            kind: AuthoredSpellKind::Instant,
            effect: AuthoredEffect::BounceCreature,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Mandatory, TeachingPool::OptionalPayment],
        oracle_id: "310f141c-7f37-4729-aed6-dd9c09db448d",
        name: "Blood Artist",
        mana: "{1}{B}",
        type_line: "Creature — Vampire",
        text: "Whenever this creature or another creature dies, target player loses 1 life and you gain 1 life.",
        power: Some("0"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Black,
            power: 0,
            toughness: 1,
            subtypes: &["Vampire"],
            trigger: Some(AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::creature(),
                ),
                effect: AuthoredTriggerEffect::TargetLoseOneGainOne,
            }),
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Mandatory, TeachingPool::OptionalPayment],
        oracle_id: "11cb509d-21af-48f1-b355-135ebd3e4bd1",
        name: "Doomed Dissenter",
        mana: "{1}{B}",
        type_line: "Creature — Human",
        text: "When this creature dies, create a 2/2 black Zombie creature token.",
        power: Some("1"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Black,
            power: 1,
            toughness: 1,
            subtypes: &["Human"],
            trigger: Some(AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                effect: AuthoredTriggerEffect::CreateZombie,
            }),
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Mandatory],
        oracle_id: "c6a3a882-a127-4590-93d7-679ef4313efe",
        name: "Elvish Visionary",
        mana: "{1}{G}",
        type_line: "Creature — Elf Shaman",
        text: "When this creature enters, draw a card.",
        power: Some("1"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Green,
            power: 1,
            toughness: 1,
            subtypes: &["Elf", "Shaman"],
            trigger: Some(AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                effect: AuthoredTriggerEffect::DrawOne,
            }),
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Mandatory],
        oracle_id: "c7107a2d-2dcf-42e9-9ea9-d0fc0d6d2ec6",
        name: "Impassioned Orator",
        mana: "{1}{W}",
        type_line: "Creature — Human Cleric",
        text: "Whenever another creature you control enters, you gain 1 life.",
        power: Some("2"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::White,
            power: 2,
            toughness: 2,
            subtypes: &["Human", "Cleric"],
            trigger: Some(AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::creature()
                        .controlled()
                        .other_than_ability_source(),
                ),
                effect: AuthoredTriggerEffect::GainOne,
            }),
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Mandatory],
        oracle_id: "f75ed312-3a23-4624-80c5-03980aa22d0b",
        name: "Llanowar Visionary",
        mana: "{2}{G}",
        type_line: "Creature — Elf Druid",
        text: "When this creature enters, draw a card.\n{T}: Add {G}.",
        power: Some("2"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 2,
            colored: 1,
            mana_type: ManaType::Green,
            power: 2,
            toughness: 2,
            subtypes: &["Elf", "Druid"],
            trigger: Some(AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                effect: AuthoredTriggerEffect::DrawOne,
            }),
            printed_mana: Some(ManaType::Green),
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Mandatory, TeachingPool::OptionalPayment],
        oracle_id: "e51f5b41-5465-4b1d-b96a-544762657ca5",
        name: "Novice Occultist",
        mana: "{1}{B}",
        type_line: "Creature — Human Wizard",
        text: "When this creature dies, you draw a card and you lose 1 life.",
        power: Some("1"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Black,
            power: 1,
            toughness: 2,
            subtypes: &["Human", "Wizard"],
            trigger: Some(AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                effect: AuthoredTriggerEffect::DrawOneLoseOne,
            }),
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Mandatory, TeachingPool::OptionalPayment],
        oracle_id: "e409c9be-0c9a-43c3-adb4-8c47afc1d551",
        name: "Phyrexian Rager",
        mana: "{2}{B}",
        type_line: "Creature — Phyrexian Horror",
        text: "When this creature enters, you draw a card and you lose 1 life.",
        power: Some("2"),
        toughness: Some("2"),
        kind: AuthoredKind::Creature {
            generic: 2,
            colored: 1,
            mana_type: ManaType::Black,
            power: 2,
            toughness: 2,
            subtypes: &["Phyrexian", "Horror"],
            trigger: Some(AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                effect: AuthoredTriggerEffect::DrawOneLoseOne,
            }),
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Mandatory, TeachingPool::OptionalPayment],
        oracle_id: "5b2364d7-a811-4595-a1b4-224c70555ffa",
        name: "Raise the Alarm",
        mana: "{1}{W}",
        type_line: "Instant",
        text: "Create two 1/1 white Soldier creature tokens.",
        power: None,
        toughness: None,
        kind: AuthoredKind::Spell {
            generic: 1,
            colored: 1,
            mana_type: ManaType::White,
            kind: AuthoredSpellKind::Instant,
            effect: AuthoredEffect::CreateSoldiers,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Mandatory],
        oracle_id: "f3fad295-1af2-4ecc-8546-b121ad6be27b",
        name: "Soul Warden",
        mana: "{W}",
        type_line: "Creature — Human Cleric",
        text: "Whenever another creature enters, you gain 1 life.",
        power: Some("1"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 0,
            colored: 1,
            mana_type: ManaType::White,
            power: 1,
            toughness: 1,
            subtypes: &["Human", "Cleric"],
            trigger: Some(AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::creature().other_than_ability_source(),
                ),
                effect: AuthoredTriggerEffect::GainOne,
            }),
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::Mandatory, TeachingPool::OptionalPayment],
        oracle_id: "56719f6a-1a6c-4c0a-8d21-18f7d7350b68",
        name: "Swamp",
        mana: "",
        type_line: "Basic Land — Swamp",
        text: "({T}: Add {B}.)",
        power: None,
        toughness: None,
        kind: AuthoredKind::BasicLand(LandType::Swamp),
    },
    AuthoredCard {
        membership: &[TeachingPool::OptionalPayment],
        oracle_id: "045a9d3d-c20d-427f-a77b-0bffc6f55526",
        name: "Soul's Attendant",
        mana: "{W}",
        type_line: "Creature — Human Cleric",
        text: "Whenever another creature enters, you may gain 1 life.",
        power: Some("1"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 0,
            colored: 1,
            mana_type: ManaType::White,
            power: 1,
            toughness: 1,
            subtypes: &["Human", "Cleric"],
            trigger: Some(AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::creature().other_than_ability_source(),
                ),
                effect: AuthoredTriggerEffect::OptionalGainOne,
            }),
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::OptionalPayment],
        oracle_id: "e793ede9-044a-44e9-bc9c-99b0034503eb",
        name: "Oculus",
        mana: "{1}{U}",
        type_line: "Creature — Phyrexian Homunculus",
        text: "When this creature dies, you may draw a card.",
        power: Some("1"),
        toughness: Some("1"),
        kind: AuthoredKind::Creature {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Blue,
            power: 1,
            toughness: 1,
            subtypes: &["Phyrexian", "Homunculus"],
            trigger: Some(AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                effect: AuthoredTriggerEffect::OptionalDrawOne,
            }),
            printed_mana: None,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::OptionalPayment],
        oracle_id: "c61fe162-2202-4e56-9ba0-393547f9875f",
        name: "Mana Leak",
        mana: "{1}{U}",
        type_line: "Instant",
        text: "Counter target spell unless its controller pays {3}.",
        power: None,
        toughness: None,
        kind: AuthoredKind::Spell {
            generic: 1,
            colored: 1,
            mana_type: ManaType::Blue,
            kind: AuthoredSpellKind::Instant,
            effect: AuthoredEffect::CounterUnlessPaidThree,
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::OptionalPayment],
        oracle_id: "53236dd7-845a-444c-96d5-f41ed7325d8f",
        name: "Rhystic Study",
        mana: "{2}{U}",
        type_line: "Enchantment",
        text: "Whenever an opponent casts a spell, you may draw a card unless that player pays {1}.",
        power: None,
        toughness: None,
        kind: AuthoredKind::Enchantment {
            generic: 2,
            colored: 1,
            mana_type: ManaType::Blue,
            trigger: AuthoredTrigger {
                condition: spellwake_engine::program::TriggerSpec::spell_cast(
                    PlayerRelation::Opponent,
                    spellwake_engine::program::SpellFilter::any(),
                ),
                effect: AuthoredTriggerEffect::DrawUnlessRelatedPaysOne,
            },
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::OptionalPayment],
        oracle_id: "bd9e416a-89b3-4912-be9b-49fce6a93dc9",
        name: "Manalith",
        mana: "{3}",
        type_line: "Artifact",
        text: "{T}: Add one mana of any color.",
        power: None,
        toughness: None,
        kind: AuthoredKind::Artifact {
            generic: 3,
            mana_amount: 1,
            mana_colors: &[
                ManaType::White,
                ManaType::Blue,
                ManaType::Black,
                ManaType::Red,
                ManaType::Green,
            ],
        },
    },
    AuthoredCard {
        membership: &[TeachingPool::OptionalPayment],
        oracle_id: "8f878efc-850f-43d2-a6fe-5ea8d1dd5afb",
        name: "Twincast",
        mana: "{U}{U}",
        type_line: "Instant",
        text: "Copy target instant or sorcery spell. You may choose new targets for the copy.",
        power: None,
        toughness: None,
        kind: AuthoredKind::Spell {
            generic: 0,
            colored: 2,
            mana_type: ManaType::Blue,
            kind: AuthoredSpellKind::Instant,
            effect: AuthoredEffect::CopySpell,
        },
    },
];

pub(super) fn selected(oracle_id: &str, pool: TeachingPool) -> bool {
    CARDS
        .iter()
        .any(|card| card.oracle_id == oracle_id && card.membership.contains(&pool))
}

fn cost(generic: u8, colored: u8, mana_type: ManaType) -> ManaCost {
    let mut cost = ManaCost::specific(mana_type, i64::from(colored).into());
    cost.generic = i64::from(generic).into();
    cost
}

pub(super) fn definition(
    id: &OracleId,
    vocabulary: &CreatureTypeVocabulary,
    pool: TeachingPool,
) -> Result<Option<Definition>, RegistryError> {
    let cards = CARDS;
    let Some(card) = cards
        .iter()
        .find(|card| card.oracle_id == id.as_str() && card.membership.contains(&pool))
    else {
        return Ok(None);
    };
    let (key, program) = match &card.kind {
        AuthoredKind::BasicLand(land_type) => {
            let mut program = Program::land(BTreeSet::from([*land_type]), Vec::new());
            program.supertypes.insert(Supertype::Basic);
            let key = match pool {
                TeachingPool::Vanilla => ProgramKey::TeachingBasicLandV1,
                TeachingPool::Interaction => ProgramKey::InteractionBasicLandV1,
                TeachingPool::Mandatory => ProgramKey::MandatoryBasicLandV1,
                TeachingPool::OptionalPayment => ProgramKey::OptionalPaymentBasicLandV1,
            };
            (key, program)
        }
        AuthoredKind::Creature {
            generic,
            colored,
            mana_type,
            power,
            toughness,
            subtypes,
            trigger,
            printed_mana,
        } => {
            let subtypes = subtypes
                .iter()
                .map(|label| {
                    vocabulary
                        .resolve_label(label)
                        .ok_or(RegistryError::MissingCreatureType)
                })
                .collect::<Result<BTreeSet<_>, _>>()?;
            let key = match pool {
                TeachingPool::Vanilla => ProgramKey::TeachingVanillaCreatureV1,
                TeachingPool::Interaction => ProgramKey::InteractionVanillaCreatureV1,
                TeachingPool::Mandatory if trigger.is_some() => {
                    ProgramKey::MandatoryTriggeredCreatureV1
                }
                TeachingPool::Mandatory => ProgramKey::MandatoryVanillaCreatureV1,
                TeachingPool::OptionalPayment if trigger.is_some() => {
                    ProgramKey::OptionalPaymentTriggeredCreatureV1
                }
                TeachingPool::OptionalPayment => ProgramKey::OptionalPaymentVanillaCreatureV1,
            };
            let mut program = Program::creature(
                Some(cost(*generic, *colored, *mana_type)),
                i64::from(*power).into(),
                i64::from(*toughness).into(),
                subtypes,
            );
            if let Some(trigger) = trigger {
                program.triggered.push(trigger.ability(vocabulary)?);
            }
            if let Some(mana_type) = printed_mana {
                program.mana.push(ManaAbility {
                    id: ManaAbilityId(1),
                    output: ManaOutput::Fixed(BTreeMap::from([(*mana_type, 1.into())])),
                });
            }
            (key, program)
        }
        AuthoredKind::Artifact {
            generic,
            mana_amount,
            mana_colors,
        } => {
            let mut program = Program::artifact(
                Some(ManaCost::generic(i64::from(*generic).into())),
                Vec::new(),
            );
            program.mana.push(ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::ChooseColor {
                    amount: i64::from(*mana_amount).into(),
                    colors: mana_colors.iter().copied().collect(),
                },
            });
            (ProgramKey::OptionalPaymentManaArtifactV1, program)
        }
        AuthoredKind::Enchantment {
            generic,
            colored,
            mana_type,
            trigger,
        } => {
            let mut program = Program::enchantment(Some(cost(*generic, *colored, *mana_type)));
            program.triggered.push(trigger.ability(vocabulary)?);
            (ProgramKey::OptionalPaymentTriggeredEnchantmentV1, program)
        }
        AuthoredKind::Spell {
            generic,
            colored,
            mana_type,
            kind,
            effect,
        } => {
            let cost = cost(*generic, *colored, *mana_type);
            let body = effect.body(vocabulary)?;
            let program = match kind {
                AuthoredSpellKind::Instant => Program::instant(cost, body),
                AuthoredSpellKind::Sorcery => Program::sorcery(cost, body),
            };
            (effect.key(pool), program)
        }
    };
    Ok(Some(Definition {
        key,
        scope: pool.scope(),
        name: card.name,
        mana: card.mana,
        type_line: card.type_line,
        text: card.text,
        power: card.power,
        toughness: card.toughness,
        program,
    }))
}

pub(super) fn validate_inventory(
    inventory: &Inventory,
    vocabulary: &CreatureTypeVocabulary,
    pool: TeachingPool,
) -> Result<(), RegistryError> {
    if inventory.compressed_sha256 != TEACHING_SOURCE_SHA256
        || (pool == TeachingPool::OptionalPayment && !source_metadata_matches(inventory))
    {
        return Err(pool.error(AdmissionFailure::Source));
    }
    if vocabulary.id().to_string() != TEACHING_VOCABULARY_ID {
        return Err(pool.error(AdmissionFailure::Vocabulary));
    }
    for expected in CARDS.iter().filter(|card| card.membership.contains(&pool)) {
        let mut matching = inventory
            .cards
            .iter()
            .filter(|card| card.oracle_id.as_deref() == Some(expected.oracle_id));
        let card = matching
            .next()
            .ok_or_else(|| pool.error(AdmissionFailure::Missing(expected.oracle_id)))?;
        if matching.next().is_some() {
            return Err(pool.error(AdmissionFailure::Duplicate(expected.oracle_id)));
        }
        if pool == TeachingPool::OptionalPayment {
            let printing = OPTIONAL_PAYMENT_PRINTINGS
                .iter()
                .find_map(|(oracle, printing)| (*oracle == expected.oracle_id).then_some(*printing))
                .ok_or_else(|| pool.error(AdmissionFailure::Printing(expected.oracle_id)))?;
            if card.scryfall_id != printing
                || inventory
                    .cards
                    .iter()
                    .filter(|c| c.scryfall_id == printing)
                    .count()
                    != 1
            {
                return Err(pool.error(AdmissionFailure::Printing(expected.oracle_id)));
            }
        }
        let format = if pool == TeachingPool::OptionalPayment {
            "legacy"
        } else {
            "modern"
        };
        if card.legalities.get(format) != Some(&Legality::Legal) || !card.required_by_base_formats {
            return Err(pool.error(AdmissionFailure::Legality(expected.oracle_id)));
        }
        let id = OracleId(expected.oracle_id.to_owned());
        let definition = definition(&id, vocabulary, pool)?
            .ok_or_else(|| pool.error(AdmissionFailure::Missing(expected.oracle_id)))?;
        if !super::matches(card, &definition) {
            return Err(pool.error(AdmissionFailure::Semantic(expected.oracle_id)));
        }
    }
    Ok(())
}

// Printing joins are profile-specific. The authored face and native Program still
// have one owner in CARDS, and signature() remains the sole semantic hash owner.
const OPTIONAL_PAYMENT_PRINTINGS: [(&str, &str); 19] = [
    (
        "310f141c-7f37-4729-aed6-dd9c09db448d",
        "b5275d76-2947-4219-be21-614c7421614a",
    ),
    (
        "4ed27607-21a8-4bc3-997e-6d2242313f6d",
        "97a74ccf-8165-4db1-a87c-52c2d8ea0058",
    ),
    (
        "cc187110-1148-4090-bbb8-e205694a39f5",
        "4f616706-ec97-4923-bb1e-11a69fbaa1f8",
    ),
    (
        "273b339c-964b-4a18-8eb5-ceb8abcdfd9e",
        "cb3b35b8-f321-46d8-a441-6b9a6efa9021",
    ),
    (
        "11cb509d-21af-48f1-b355-135ebd3e4bd1",
        "f7c0cf16-81ea-45e3-99cc-4424d59bb44b",
    ),
    (
        "b3b9a87d-cb95-435c-90b6-037406cab32e",
        "28a5c350-2ed1-4a25-9626-0f8da5d1aef7",
    ),
    (
        "b2c6aa39-2d2a-459c-a555-fb48ba993373",
        "1335d293-3dd2-4f13-bd2c-22181f4528ee",
    ),
    (
        "c61fe162-2202-4e56-9ba0-393547f9875f",
        "179236d9-6fe2-4db6-bdfb-f851e8d531a2",
    ),
    (
        "bd9e416a-89b3-4912-be9b-49fce6a93dc9",
        "f0d7d904-f29b-4524-9223-27910d748bdb",
    ),
    (
        "e51f5b41-5465-4b1d-b96a-544762657ca5",
        "a541ac79-a969-42b4-b989-f6e37572ec90",
    ),
    (
        "e793ede9-044a-44e9-bc9c-99b0034503eb",
        "673bebb4-9c82-40ca-8552-b9030e961005",
    ),
    (
        "e409c9be-0c9a-43c3-adb4-8c47afc1d551",
        "a9861fb3-6722-43b8-bf0a-af6dd166e59d",
    ),
    (
        "bc71ebf6-2056-41f7-be35-b2e5c34afa99",
        "09ac862f-0a51-44dd-b86f-9412b8218b13",
    ),
    (
        "5b2364d7-a811-4595-a1b4-224c70555ffa",
        "6c7c8527-55f6-494d-b4f7-c427a5735053",
    ),
    (
        "53236dd7-845a-444c-96d5-f41ed7325d8f",
        "9f37c5b6-a59c-45cd-9a99-e9357fe9ea1b",
    ),
    (
        "045a9d3d-c20d-427f-a77b-0bffc6f55526",
        "3223c0ac-cc22-4886-8919-11273b477cc7",
    ),
    (
        "56719f6a-1a6c-4c0a-8d21-18f7d7350b68",
        "b233b117-9369-45c1-80c5-cc857386b6be",
    ),
    (
        "8f878efc-850f-43d2-a6fe-5ea8d1dd5afb",
        "f38eec33-a40e-4739-ad2c-2f57008cef4e",
    ),
    (
        "837182db-1bf3-4a2c-bd01-1af9d9873561",
        "ddad9f16-52d5-49de-82b0-b1a5294a9c44",
    ),
];

pub(super) fn source_metadata_matches(inventory: &Inventory) -> bool {
    let source = &inventory.source;
    source.object == "bulk_data"
        && source.id == "27bf3214-1271-490b-bdfe-c0be6c23d02e"
        && source.kind == "oracle_cards"
        && source.updated_at == "2026-10-29T21:01:58.033+00:00"
        && source.jsonl_download_uri
            == "https://data.scryfall.io/oracle-cards/oracle-cards-20261029210158.jsonl.gz"
        && source.compressed_size == 25_229_019
}
