//! Structured native damage evidence. Identity strings are opaque handles; source,
//! recipient, event scope and parent provenance have separate tagged fields.
use super::program;
use serde::Serialize;
use serde_json::Value;
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    damage::{
        DamageApplication, DamageAttempt, DamageBatchPhase, DamageEffectCreation, DamageEffectId,
        DamageEffectTarget, DamageJournal, DamageKind, DamageOrigin, DamageOutcome,
        DamageRecipient, DamageScope, DamageSource, DamageSourceFilter, DamageSourceId,
    },
    object_identity::{GameObject, ObjectOrigin, ObjectZone},
    turn::{Step, TurnPosition},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ZoneView {
    Library { player: u32 },
    Hand { player: u32 },
    Graveyard { player: u32 },
    Battlefield,
    Stack,
    Exile,
    Command,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GameObjectOriginView {
    Card { card_id: u64 },
    PermanentSpellCopy { definition: Value },
    Token { definition: Value },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GameObjectView {
    pub object: String,
    pub origin: GameObjectOriginView,
    pub physical_card: Option<u64>,
    pub owner: u32,
    pub zone: ZoneView,
    pub controller: Option<u32>,
    pub default_controller: Option<u32>,
    pub tapped: Option<bool>,
    pub control_epoch: Option<String>,
    pub continuous_control_since_turn_start: Option<bool>,
}
pub fn game_object_view(object: &GameObject) -> GameObjectView {
    let zone = match object.zone() {
        ObjectZone::Library(player) => ZoneView::Library { player: player.0 },
        ObjectZone::Hand(player) => ZoneView::Hand { player: player.0 },
        ObjectZone::Graveyard(player) => ZoneView::Graveyard { player: player.0 },
        ObjectZone::Battlefield => ZoneView::Battlefield,
        ObjectZone::Stack => ZoneView::Stack,
        ObjectZone::Exile => ZoneView::Exile,
        ObjectZone::Command => ZoneView::Command,
    };
    GameObjectView {
        object: format!("{:?}", object.id()),
        origin: match object.origin() {
            ObjectOrigin::Card(card) => GameObjectOriginView::Card { card_id: card.0 },
            ObjectOrigin::PermanentSpellCopy { definition } => {
                GameObjectOriginView::PermanentSpellCopy {
                    definition: program::view(definition),
                }
            }
            ObjectOrigin::Token { definition } => GameObjectOriginView::Token {
                definition: program::view(definition),
            },
        },
        physical_card: object.card_id().map(|card| card.0),
        owner: object.owner().0,
        zone,
        controller: object.controller().map(|player| player.0),
        default_controller: object.default_controller().map(|player| player.0),
        tapped: object.tapped(),
        control_epoch: object.control_epoch().map(|epoch| format!("{epoch:?}")),
        continuous_control_since_turn_start: object.has_continuous_control_since_turn_start(),
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DamageSourceView {
    Emblem {
        emblem: String,
        controller: u32,
    },
    Object {
        object: GameObjectView,
        controller: u32,
    },
    SpellCopy {
        entry: String,
        controller: u32,
    },
}
pub fn damage_source_view(source: &DamageSource) -> DamageSourceView {
    match source {
        DamageSource::Emblem { emblem, controller } => DamageSourceView::Emblem {
            emblem: format!("{emblem:?}"),
            controller: controller.0,
        },
        DamageSource::Object { object, controller } => DamageSourceView::Object {
            object: game_object_view(object),
            controller: controller.0,
        },
        DamageSource::SpellCopy { entry, controller } => DamageSourceView::SpellCopy {
            entry: format!("{entry:?}"),
            controller: controller.0,
        },
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DamageSourceIdView {
    Emblem { emblem: String },
    Object { object: String },
    SpellCopy { entry: String },
}
pub fn source_id_view(source: DamageSourceId) -> DamageSourceIdView {
    match source {
        DamageSourceId::Emblem(emblem) => DamageSourceIdView::Emblem {
            emblem: format!("{emblem:?}"),
        },
        DamageSourceId::Object(object) => DamageSourceIdView::Object {
            object: format!("{object:?}"),
        },
        DamageSourceId::SpellCopy(entry) => DamageSourceIdView::SpellCopy {
            entry: format!("{entry:?}"),
        },
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DamageRecipientView {
    Creature { object: String },
    Planeswalker { object: String },
    Battle { object: String },
    Player { player: u32 },
}
pub fn damage_recipient_view(recipient: DamageRecipient) -> DamageRecipientView {
    match recipient {
        DamageRecipient::Creature(object) => DamageRecipientView::Creature {
            object: format!("{object:?}"),
        },
        DamageRecipient::Battle(object) => DamageRecipientView::Battle {
            object: format!("{object:?}"),
        },
        DamageRecipient::Planeswalker(object) => DamageRecipientView::Planeswalker {
            object: format!("{object:?}"),
        },
        DamageRecipient::Player(player) => DamageRecipientView::Player { player: player.0 },
    }
}
/// A stored effect binding is not an actual eligible damage recipient.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DamageEffectTargetView {
    PlayerAndPermanents { player: u32 },
    Permanent { object: String },
    Player { player: u32 },
}
pub fn damage_effect_target_view(target: DamageEffectTarget) -> DamageEffectTargetView {
    match target {
        DamageEffectTarget::Permanent(object) => DamageEffectTargetView::Permanent {
            object: format!("{object:?}"),
        },
        DamageEffectTarget::PlayerAndPermanents(player) => {
            DamageEffectTargetView::PlayerAndPermanents { player: player.0 }
        }
        DamageEffectTarget::Player(player) => DamageEffectTargetView::Player { player: player.0 },
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct TurnPositionView {
    pub number: u64,
    pub step: &'static str,
    pub cleanup_iteration: u64,
    pub additional_damage: bool,
}
/// Canonical spelling of a native turn step for qualification projections.
pub fn step_view(step: Step) -> &'static str {
    match step {
        Step::Untap => "untap",
        Step::Upkeep => "upkeep",
        Step::Draw => "draw",
        Step::PrecombatMain => "precombat_main",
        Step::BeginningCombat => "beginning_combat",
        Step::DeclareAttackers => "declare_attackers",
        Step::DeclareBlockers => "declare_blockers",
        Step::CombatDamage => "combat_damage",
        Step::EndCombat => "end_combat",
        Step::PostcombatMain => "postcombat_main",
        Step::End => "end",
        Step::Cleanup => "cleanup",
    }
}
pub fn turn_position_view(position: TurnPosition) -> TurnPositionView {
    TurnPositionView {
        number: position.number,
        step: step_view(position.step),
        cleanup_iteration: position.cleanup_iteration,
        additional_damage: position.additional_damage,
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DamageOriginView {
    PreventionFollowup { batch: String, creator: String },
    Resolution { entry: String, instruction: usize },
    Combat { position: TurnPositionView },
    ManaAbility { source: String, ability: String },
}
pub fn damage_origin_view(origin: DamageOrigin) -> DamageOriginView {
    match origin {
        DamageOrigin::PreventionFollowup { batch, creator } => {
            DamageOriginView::PreventionFollowup {
                batch: format!("{batch:?}"),
                creator: format!("{creator:?}"),
            }
        }
        DamageOrigin::Resolution { entry, instruction } => DamageOriginView::Resolution {
            entry: format!("{entry:?}"),
            instruction,
        },
        DamageOrigin::Combat { position } => DamageOriginView::Combat {
            position: turn_position_view(position),
        },
        DamageOrigin::ManaAbility { source, ability } => DamageOriginView::ManaAbility {
            source: format!("{source:?}"),
            ability: format!("{ability:?}"),
        },
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectIdentityView {
    StaticPreventionWithExile { source: String, ability: u64 },
    CombatDamageToMill { source: String, ability: u64 },
    RemovePlusOneToPreventDamage { source: String, ability: u64 },
    UntilEndOfTurn { creator: String, instruction: usize },
    StaticPrevention { source: String },
    StaticDouble { source: String },
    StaticHalfPrevention { permanent: String, source: String },
    EmblemCap { emblem: String, source: String },
    ProtectionMulticolored,
    ProtectionSubtype { subtype: u16, creatures_only: bool },
    Protection { color: String },
    Prevention { id: String },
    Multiplier { id: String },
}
pub fn damage_effect_view(effect: DamageEffectId) -> EffectIdentityView {
    match effect {
        DamageEffectId::UntilEndOfTurn {
            creator,
            instruction,
        } => EffectIdentityView::UntilEndOfTurn {
            creator: format!("{creator:?}"),
            instruction,
        },
        DamageEffectId::EmblemCap { emblem, source } => EffectIdentityView::EmblemCap {
            emblem: format!("{emblem:?}"),
            source: format!("{source:?}"),
        },
        DamageEffectId::StaticDouble(source)
        | DamageEffectId::DoubleDamageFromEquippedCreature(source)
        | DamageEffectId::DoubleDamageToEquippedCreature(source) => {
            EffectIdentityView::StaticDouble {
                source: format!("{source:?}"),
            }
        }
        DamageEffectId::StaticHalfPrevention { permanent, source } => {
            EffectIdentityView::StaticHalfPrevention {
                permanent: format!("{permanent:?}"),
                source: format!("{source:?}"),
            }
        }
        DamageEffectId::RemovePlusOneToPreventDamage(source, ability) => {
            EffectIdentityView::RemovePlusOneToPreventDamage {
                source: format!("{source:?}"),
                ability: ability.0,
            }
        }
        DamageEffectId::CombatDamageToMill(source, ability) => {
            EffectIdentityView::CombatDamageToMill {
                source: format!("{source:?}"),
                ability: ability.0,
            }
        }
        DamageEffectId::StaticPreventionWithExile(source, ability) => {
            EffectIdentityView::StaticPreventionWithExile {
                source: format!("{source:?}"),
                ability: ability.0,
            }
        }
        DamageEffectId::StaticPrevention(source) | DamageEffectId::ShieldCounter(source) => {
            EffectIdentityView::StaticPrevention {
                source: format!("{source:?}"),
            }
        }
        DamageEffectId::ProtectionSubtype(quality) => EffectIdentityView::ProtectionSubtype {
            subtype: quality.subtype.ordinal(),
            creatures_only: quality.creatures_only,
        },
        DamageEffectId::ProtectionMulticolored => EffectIdentityView::ProtectionMulticolored,
        DamageEffectId::Protection(color) => EffectIdentityView::Protection {
            color: format!("{color:?}"),
        },
        DamageEffectId::Prevention(id) => EffectIdentityView::Prevention {
            id: format!("{id:?}"),
        },
        DamageEffectId::Multiplier(id) => EffectIdentityView::Multiplier {
            id: format!("{id:?}"),
        },
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DamageScopeView {
    ControlledRecipients { player: u32 },
    Component { attempt: String },
    Recipient { target: DamageRecipientView },
}
#[derive(Debug, Clone, Serialize)]
pub struct DamageApplicationView {
    pub effect: EffectIdentityView,
    pub scope: DamageScopeView,
}
pub fn damage_application_view(application: DamageApplication) -> DamageApplicationView {
    DamageApplicationView {
        effect: damage_effect_view(application.effect),
        scope: match application.scope {
            DamageScope::ControlledRecipients(player) => {
                DamageScopeView::ControlledRecipients { player: player.0 }
            }
            DamageScope::Component(attempt) => DamageScopeView::Component {
                attempt: format!("{attempt:?}"),
            },
            DamageScope::Recipient(target) => DamageScopeView::Recipient {
                target: damage_recipient_view(target),
            },
        },
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceFilterView {
    Any,
    Exact { source: DamageSourceIdView },
}
pub fn filter_view(filter: DamageSourceFilter) -> SourceFilterView {
    match filter {
        DamageSourceFilter::Any => SourceFilterView::Any,
        DamageSourceFilter::Exact(source) => SourceFilterView::Exact {
            source: source_id_view(source),
        },
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct BatchView {
    pub batch: String,
    pub origin: DamageOriginView,
    pub kind: &'static str,
    pub components: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct AttemptView {
    pub batch: String,
    pub attempt: String,
    pub source: DamageSourceView,
    pub target: DamageRecipientView,
    pub amount: RulesInteger,
}
/// One actual attempted component, shared by active state and its diagnostic journal.
pub fn damage_attempt_view(attempt: &DamageAttempt) -> AttemptView {
    AttemptView {
        batch: format!("{:?}", attempt.batch),
        attempt: format!("{:?}", attempt.id),
        source: damage_source_view(&attempt.source),
        target: damage_recipient_view(attempt.target),
        amount: attempt.amount.clone(),
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DamageBatchPhaseView {
    Selecting,
    Allocating { application: DamageApplicationView },
}
pub fn damage_batch_phase_view(phase: &DamageBatchPhase) -> DamageBatchPhaseView {
    match phase {
        DamageBatchPhase::Selecting => DamageBatchPhaseView::Selecting,
        DamageBatchPhase::Allocating { application } => DamageBatchPhaseView::Allocating {
            application: damage_application_view(*application),
        },
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreationView {
    Prevention {
        shield: String,
        creator: String,
        instruction: usize,
        target: DamageEffectTargetView,
        amount: RulesInteger,
    },
    Multiplier {
        multiplier: String,
        creator: String,
        instruction: usize,
        target: DamageEffectTargetView,
        factor: RulesInteger,
        filter: SourceFilterView,
    },
}
#[derive(Debug, Clone, Serialize)]
pub struct AllocationView {
    pub attempt: String,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, Serialize)]
pub struct PreventionView {
    pub batch: String,
    pub shield: String,
    pub creator: String,
    pub target: DamageRecipientView,
    pub stage: u64,
    pub allocations: Vec<AllocationView>,
    pub amount: RulesInteger,
    pub damage_remaining: Vec<AllocationView>,
    pub shield_remaining: RulesInteger,
}
#[derive(Debug, Clone, Serialize)]
pub struct MultiplicationView {
    pub batch: String,
    pub attempt: String,
    pub multiplier: String,
    pub creator: String,
    pub target: DamageRecipientView,
    pub stage: u64,
    pub before: RulesInteger,
    pub factor: RulesInteger,
    pub after: RulesInteger,
}
#[derive(Debug, Clone, Serialize)]
pub struct CompletionView {
    pub batch: String,
    pub attempt: String,
    pub outcome: &'static str,
    pub dealt: Option<RulesInteger>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DamageJournalView {
    pub batches: Vec<BatchView>,
    pub created: Vec<CreationView>,
    pub attempts: Vec<AttemptView>,
    pub preventions: Vec<PreventionView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub protections: Vec<Value>,
    pub multiplications: Vec<MultiplicationView>,
    pub applied: Vec<AttemptView>,
    pub completions: Vec<CompletionView>,
}
/// One lossless projection of the currently returned journal, without inferred events.
pub fn damage_journal_view(journal: &DamageJournal) -> DamageJournalView {
    DamageJournalView {
        batches: journal
            .batches
            .iter()
            .map(|batch| BatchView {
                batch: format!("{:?}", batch.id),
                origin: damage_origin_view(batch.origin),
                kind: match batch.kind {
                    DamageKind::Combat => "combat",
                    DamageKind::Noncombat => "noncombat",
                },
                components: batch
                    .components
                    .iter()
                    .map(|id| format!("{id:?}"))
                    .collect(),
            })
            .collect(),
        created: journal
            .created
            .iter()
            .map(|creation| match creation {
                DamageEffectCreation::Prevention {
                    shield,
                    instruction,
                } => CreationView::Prevention {
                    shield: format!("{:?}", shield.id),
                    creator: format!("{:?}", shield.creator),
                    instruction: *instruction,
                    target: damage_effect_target_view(shield.target),
                    amount: shield.remaining.clone(),
                },
                DamageEffectCreation::Multiplier {
                    multiplier,
                    instruction,
                } => CreationView::Multiplier {
                    multiplier: format!("{:?}", multiplier.id),
                    creator: format!("{:?}", multiplier.creator),
                    instruction: *instruction,
                    target: damage_effect_target_view(multiplier.target),
                    factor: multiplier.factor.clone(),
                    filter: filter_view(multiplier.filter),
                },
            })
            .collect(),
        attempts: journal.attempts.iter().map(damage_attempt_view).collect(),
        protections: journal.protections.iter().map(protection_view).collect(),
        preventions: journal
            .preventions
            .iter()
            .map(|event| PreventionView {
                batch: format!("{:?}", event.batch),
                shield: format!("{:?}", event.shield),
                creator: format!("{:?}", event.creator),
                target: damage_recipient_view(event.target),
                stage: event.stage,
                allocations: event
                    .allocations
                    .iter()
                    .map(|allocation| AllocationView {
                        attempt: format!("{:?}", allocation.attempt),
                        amount: allocation.amount.clone(),
                    })
                    .collect(),
                amount: event.amount.clone(),
                damage_remaining: event
                    .damage_remaining
                    .iter()
                    .map(|(attempt, amount)| AllocationView {
                        attempt: format!("{attempt:?}"),
                        amount: amount.clone(),
                    })
                    .collect(),
                shield_remaining: event.shield_remaining.clone(),
            })
            .collect(),
        multiplications: journal
            .multiplications
            .iter()
            .map(|event| MultiplicationView {
                batch: format!("{:?}", event.batch),
                attempt: format!("{:?}", event.attempt),
                multiplier: format!("{:?}", event.multiplier),
                creator: format!("{:?}", event.creator),
                target: damage_recipient_view(event.target),
                stage: event.stage,
                before: event.before.clone(),
                factor: event.factor.clone(),
                after: event.after.clone(),
            })
            .collect(),
        applied: journal
            .applied
            .iter()
            .map(|event| AttemptView {
                batch: format!("{:?}", event.batch),
                attempt: format!("{:?}", event.attempt),
                source: damage_source_view(&event.source),
                target: damage_recipient_view(event.target),
                amount: event.amount.clone(),
            })
            .collect(),
        completions: journal
            .completions
            .iter()
            .map(|event| {
                let (outcome, dealt) = match &event.outcome {
                    DamageOutcome::Dealt(amount) => ("dealt", Some(amount.clone())),
                    DamageOutcome::FullyPrevented => ("fully-prevented", None),
                    DamageOutcome::RecipientUnavailable => ("recipient-unavailable", None),
                };
                CompletionView {
                    batch: format!("{:?}", event.batch),
                    attempt: format!("{:?}", event.attempt),
                    outcome,
                    dealt,
                }
            })
            .collect(),
    }
}

pub fn protection_view(value: &spellwake_engine::damage::ProtectionPrevention) -> Value {
    let recipient = match value.recipient {
        DamageRecipient::Creature(object)
        | DamageRecipient::Planeswalker(object)
        | DamageRecipient::Battle(object) => serde_json::json!(format!("{object:?}")),
        DamageRecipient::Player(player) => serde_json::json!({"player":player.0}),
    };
    let mut record = serde_json::json!({"batch":format!("{:?}",value.batch),"stage":value.stage,"recipient":recipient,"allocations":value.allocations.iter().map(|a|serde_json::json!({"attempt":format!("{:?}",a.attempt),"amount":a.amount})).collect::<Vec<_>>()});
    match value.quality {
        spellwake_engine::damage::ProtectionQuality::Color(
            spellwake_engine::characteristics::ColorFilter::Has(color),
        ) => record["color"] = serde_json::json!(format!("{color:?}")),
        spellwake_engine::damage::ProtectionQuality::Color(quality) => {
            record["quality"] = serde_json::json!(format!("{quality:?}"))
        }
        spellwake_engine::damage::ProtectionQuality::Subtype(quality) => {
            record["quality"] = serde_json::json!({"subtype":quality.subtype.ordinal(),"creatures_only":quality.creatures_only})
        }
    }
    record
}
