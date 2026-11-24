//! Versioned current-state locations. No native private identity constructors.
use super::*;
use crate::player_wire::{ManaColor, PlayerZoneKind};
use spellwake_engine::{
    checks::CheckDecision,
    effects::{CopyTargetChoice, DamageDecision, OptionalEffectChoice},
    game::RuleFrame,
    mana::ManaType,
    object_identity::{ObjectId, ObjectZone},
    opening::MulliganChoice,
    targets::TargetRef,
};
fn ordinal(index: usize) -> Result<NativeRevision, NativeCodecError> {
    u64::try_from(index)
        .map(NativeRevision::from)
        .map_err(|_| NativeCodecError::InvalidRecord)
}
fn index(value: NativeRevision) -> Result<usize, NativeCodecError> {
    usize::try_from(value.get()).map_err(|_| NativeCodecError::InvalidRecord)
}
fn find<T: PartialEq>(values: &[T], target: &T) -> Result<NativeRevision, NativeCodecError> {
    ordinal(
        values
            .iter()
            .position(|value| value == target)
            .ok_or(NativeCodecError::InvalidRecord)?,
    )
}
fn at<T: Copy>(values: &[T], value: NativeRevision) -> Result<T, NativeCodecError> {
    values
        .get(index(value)?)
        .copied()
        .ok_or(NativeCodecError::InvalidRecord)
}
fn object_index(
    scope: &NativeScope<'_>,
    object: ObjectId,
) -> Result<NativeRevision, NativeCodecError> {
    ordinal(
        scope
            .source
            .execution()
            .window()
            .cards()
            .iter()
            .position(|entry| entry.id() == object)
            .ok_or(NativeCodecError::InvalidRecord)?,
    )
}
fn object_at(scope: &NativeScope<'_>, value: NativeRevision) -> Result<ObjectId, NativeCodecError> {
    scope
        .source
        .execution()
        .window()
        .cards()
        .iter()
        .nth(index(value)?)
        .map(|entry| entry.id())
        .ok_or(NativeCodecError::InvalidRecord)
}
fn stack_index(
    scope: &NativeScope<'_>,
    stack: spellwake_engine::stack_window::StackEntryId,
) -> Result<NativeRevision, NativeCodecError> {
    ordinal(
        scope
            .source
            .execution()
            .window()
            .entries()
            .iter()
            .position(|entry| entry.id() == stack)
            .ok_or(NativeCodecError::InvalidRecord)?,
    )
}
fn stack_at(
    scope: &NativeScope<'_>,
    value: NativeRevision,
) -> Result<spellwake_engine::stack_window::StackEntryId, NativeCodecError> {
    scope
        .source
        .execution()
        .window()
        .entries()
        .get(index(value)?)
        .map(|entry| entry.id())
        .ok_or(NativeCodecError::InvalidRecord)
}
fn frame<'a>(scope: &'a NativeScope<'_>) -> Result<&'a RuleFrame, NativeCodecError> {
    scope.frame().map_err(NativeCodecError::Bridge)
}
fn current(
    scope: &NativeScope<'_>,
    object: ObjectId,
) -> Result<spellwake_engine::characteristics::ObjectCharacteristics, NativeCodecError> {
    scope
        .source
        .execution()
        .current_characteristics(object)
        .map_err(|cause| NativeBridgeError::Observation(Box::new(cause)))?
        .ok_or(NativeCodecError::InvalidRecord)
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Target {
    Object { index: NativeRevision },
    Stack { index: NativeRevision },
    Player { index: NativeRevision },
}
impl Target {
    fn encode(scope: &NativeScope<'_>, target: TargetRef) -> Result<Self, NativeCodecError> {
        Ok(match target {
            TargetRef::Object(value) => Self::Object {
                index: object_index(scope, value)?,
            },
            TargetRef::Stack(value) => Self::Stack {
                index: stack_index(scope, value)?,
            },
            TargetRef::Player(value) => Self::Player {
                index: ordinal(
                    scope
                        .binding
                        .seats
                        .iter()
                        .position(|entry| entry.native_seat == value)
                        .ok_or(NativeCodecError::InvalidRecord)?,
                )?,
            },
        })
    }
    fn resolve(&self, scope: &NativeScope<'_>) -> Result<TargetRef, NativeCodecError> {
        Ok(match self {
            Self::Object { index } => TargetRef::Object(object_at(scope, *index)?),
            Self::Stack { index } => TargetRef::Stack(stack_at(scope, *index)?),
            Self::Player { index: value } => TargetRef::Player(
                scope
                    .binding
                    .seats
                    .get(index(*value)?)
                    .ok_or(NativeCodecError::InvalidRecord)?
                    .native_seat,
            ),
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Location {
    Zone {
        zone: PlayerZoneKind,
        #[serde(deserialize_with = "Option::deserialize")]
        owner: Option<NativeRevision>,
    },
    Triggered {
        object: NativeRevision,
        index: NativeRevision,
    },
    Continuous {
        object: NativeRevision,
    },
    Object {
        index: NativeRevision,
    },
    Stack {
        index: NativeRevision,
    },
    GrantedAbility {
        object: NativeRevision,
        index: NativeRevision,
    },
    AdditionalContinuous {
        object: NativeRevision,
        index: NativeRevision,
    },
    Activated {
        object: NativeRevision,
        index: NativeRevision,
    },
    Mana {
        object: NativeRevision,
        index: NativeRevision,
    },
    Decision {},
    Legend {
        index: NativeRevision,
    },
    Trigger {
        index: NativeRevision,
    },
    Token {
        index: NativeRevision,
    },
    DamageApplication {
        index: NativeRevision,
    },
    DamageAttempt {
        index: NativeRevision,
    },
    ZoneReplacement {
        index: NativeRevision,
    },
    DrawLandKind {
        land: Option<bool>,
    },
    MiracleReveal {
        reveal: bool,
    },
    PlaneswalkerType {
        label: String,
    },
    AttackDirection {
        left: bool,
    },
    PermanentTypeOrder {
        index: NativeRevision,
    },
    PermanentType {
        index: NativeRevision,
    },
    Optional {
        take: bool,
    },
    Mulligan {
        keep: bool,
    },
    RetainCopyTargets {},
    ChangeCopyTarget {
        #[serde(deserialize_with = "crate::provider_json::object")]
        target: Target,
    },
    Color {
        color: ManaColor,
    },
    CreatureType {
        index: NativeRevision,
    },
    CardName {
        index: NativeRevision,
    },
    EntryPayLife {
        pay: bool,
    },
    OptionIndex {
        index: NativeRevision,
    },
}
impl Location {
    pub(super) fn encode(
        scope: &NativeScope<'_>,
        reference: NativeReference,
    ) -> Result<Self, NativeCodecError> {
        Ok(match reference {
            NativeReference::Zone(zone) => {
                let (zone, owner) = match zone {
                    ObjectZone::Library(seat) => (PlayerZoneKind::Library, Some(seat)),
                    ObjectZone::Hand(seat) => (PlayerZoneKind::Hand, Some(seat)),
                    ObjectZone::Graveyard(seat) => (PlayerZoneKind::Graveyard, Some(seat)),
                    ObjectZone::Battlefield => (PlayerZoneKind::Battlefield, None),
                    ObjectZone::Stack => (PlayerZoneKind::Stack, None),
                    ObjectZone::Exile => (PlayerZoneKind::Exile, None),
                    ObjectZone::Command => (PlayerZoneKind::Command, None),
                };
                let owner = owner
                    .map(|seat| {
                        ordinal(
                            scope
                                .binding
                                .seats
                                .iter()
                                .position(|entry| entry.native_seat == seat)
                                .ok_or(NativeCodecError::InvalidRecord)?,
                        )
                    })
                    .transpose()?;
                Self::Zone { zone, owner }
            }
            NativeReference::TriggeredAbility { source, ability } => Self::Triggered {
                object: object_index(scope, source)?,
                index: find(&current(scope, source)?.abilities.triggered, &ability)?,
            },
            NativeReference::ContinuousAbility { source, ability } => {
                let roster = current(scope, source)?.abilities;
                if roster.continuous == Some(ability) {
                    Self::Continuous {
                        object: object_index(scope, source)?,
                    }
                } else {
                    Self::AdditionalContinuous {
                        object: object_index(scope, source)?,
                        index: find(&roster.additional_battlefield_abilities, &ability)?,
                    }
                }
            }
            NativeReference::Object(value) => Self::Object {
                index: object_index(scope, value)?,
            },
            NativeReference::Stack(value) => Self::Stack {
                index: stack_index(scope, value)?,
            },
            NativeReference::GrantedAbility { source, effect } => Self::GrantedAbility {
                object: object_index(scope, source)?,
                index: ordinal(
                    current(scope, source)?
                        .granted_activated
                        .keys()
                        .position(|key| *key == effect)
                        .ok_or(NativeCodecError::InvalidRecord)?,
                )?,
            },
            NativeReference::ActivatedAbility { source, ability } => Self::Activated {
                object: object_index(scope, source)?,
                index: find(&current(scope, source)?.abilities.activated, &ability)?,
            },
            NativeReference::ManaAbility { source, ability } => Self::Mana {
                object: object_index(scope, source)?,
                index: ordinal(
                    current(scope, source)?
                        .mana_abilities()
                        .iter()
                        .position(|value| *value == ability)
                        .ok_or(NativeCodecError::InvalidRecord)?,
                )?,
            },
            NativeReference::Decision(value) => {
                frame(scope)?;
                if !scope
                    .source
                    .pending_decision()
                    .is_some_and(|pending| pending.id() == value)
                {
                    return Err(NativeCodecError::InvalidRecord);
                }
                Self::Decision {}
            }
            NativeReference::Legend(value) => {
                let RuleFrame::Checks(CheckDecision::Legend(choice)) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                Self::Legend {
                    index: ordinal(
                        choice
                            .conflicts()
                            .iter()
                            .position(|entry| entry.id() == value)
                            .ok_or(NativeCodecError::InvalidRecord)?,
                    )?,
                }
            }
            NativeReference::Trigger(value) => {
                let RuleFrame::Triggers(choice) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                Self::Trigger {
                    index: find(choice.instances(), &value)?,
                }
            }
            NativeReference::TokenMember(value) => {
                let RuleFrame::TimestampOrder(choice) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                Self::Token {
                    index: ordinal(
                        choice
                            .members()
                            .iter()
                            .position(|entry| entry.member == value)
                            .ok_or(NativeCodecError::InvalidRecord)?,
                    )?,
                }
            }
            NativeReference::DamageApplication(value) => {
                let RuleFrame::Damage(DamageDecision::Order(choice)) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                Self::DamageApplication {
                    index: find(choice.applications(), &value)?,
                }
            }
            NativeReference::DamageAttempt(value) => {
                let RuleFrame::Damage(DamageDecision::Allocation(choice)) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                Self::DamageAttempt {
                    index: ordinal(
                        choice
                            .eligible()
                            .iter()
                            .position(|entry| entry.attempt() == value)
                            .ok_or(NativeCodecError::InvalidRecord)?,
                    )?,
                }
            }
            NativeReference::ZoneReplacement(index) => Self::ZoneReplacement {
                index: ordinal(index)?,
            },
            NativeReference::MiracleReveal(reveal) => Self::MiracleReveal { reveal },
            NativeReference::DrawLandKind(land) => Self::DrawLandKind { land },
            NativeReference::PlaneswalkerType(kind) => Self::PlaneswalkerType {
                label: kind.label().to_owned(),
            },
            NativeReference::AttackDirection(value) => Self::AttackDirection {
                left: value == spellwake_engine::effects::AttackDirection::Left,
            },
            NativeReference::OptionalEffect(value) => match value {
                OptionalEffectChoice::Take => Self::Optional { take: true },
                OptionalEffectChoice::Decline => Self::Optional { take: false },
                OptionalEffectChoice::PermanentType(_) => {
                    let RuleFrame::OptionalEffect(choice) = frame(scope)? else {
                        return Err(NativeCodecError::InvalidRecord);
                    };
                    Self::PermanentType {
                        index: ordinal(
                            choice
                                .options()
                                .iter()
                                .position(|option| *option == value)
                                .ok_or(NativeCodecError::InvalidRecord)?,
                        )?,
                    }
                }
                OptionalEffectChoice::PermanentTypeOrder(_) => {
                    let RuleFrame::OptionalEffect(choice) = frame(scope)? else {
                        return Err(NativeCodecError::InvalidRecord);
                    };
                    Self::PermanentTypeOrder {
                        index: ordinal(
                            choice
                                .options()
                                .iter()
                                .position(|option| *option == value)
                                .ok_or(NativeCodecError::InvalidRecord)?,
                        )?,
                    }
                }
            },
            NativeReference::Mulligan(value) => Self::Mulligan {
                keep: match value {
                    MulliganChoice::Keep => true,
                    MulliganChoice::Mulligan => false,
                },
            },
            NativeReference::CopyTargets(value) => match value {
                CopyTargetChoice::Retain => Self::RetainCopyTargets {},
                CopyTargetChoice::Change(value) => Self::ChangeCopyTarget {
                    target: Target::encode(scope, value)?,
                },
            },
            NativeReference::ManaColor(value) => Self::Color {
                color: match value {
                    ManaType::White => ManaColor::White,
                    ManaType::Blue => ManaColor::Blue,
                    ManaType::Black => ManaColor::Black,
                    ManaType::Red => ManaColor::Red,
                    ManaType::Green => ManaColor::Green,
                    ManaType::Colourless => ManaColor::Colourless,
                },
            },
            NativeReference::CardName(index) => Self::CardName {
                index: u64::try_from(index.0)
                    .map_err(|_| NativeCodecError::InvalidRecord)?
                    .into(),
            },
            NativeReference::CreatureType(value) => Self::CreatureType {
                index: u64::from(value.ordinal()).into(),
            },
            NativeReference::EntryPayLife(pay) => Self::EntryPayLife { pay },
            NativeReference::OptionIndex(index) => Self::OptionIndex {
                index: u64::from(index).into(),
            },
        })
    }
    pub(super) fn resolve(
        &self,
        scope: &NativeScope<'_>,
    ) -> Result<NativeReference, NativeCodecError> {
        Ok(match self {
            Self::Zone { zone, owner } => {
                let seat = owner
                    .map(|value| {
                        scope
                            .binding
                            .seats
                            .get(index(value)?)
                            .map(|entry| entry.native_seat)
                            .ok_or(NativeCodecError::InvalidRecord)
                    })
                    .transpose()?;
                NativeReference::Zone(match (zone, seat) {
                    (PlayerZoneKind::Library, Some(seat)) => ObjectZone::Library(seat),
                    (PlayerZoneKind::Hand, Some(seat)) => ObjectZone::Hand(seat),
                    (PlayerZoneKind::Graveyard, Some(seat)) => ObjectZone::Graveyard(seat),
                    (PlayerZoneKind::Battlefield, None) => ObjectZone::Battlefield,
                    (PlayerZoneKind::Stack, None) => ObjectZone::Stack,
                    (PlayerZoneKind::Exile, None) => ObjectZone::Exile,
                    (PlayerZoneKind::Command, None) => ObjectZone::Command,
                    _ => return Err(NativeCodecError::InvalidRecord),
                })
            }
            Self::Triggered { object, index } => {
                let source = object_at(scope, *object)?;
                NativeReference::TriggeredAbility {
                    source,
                    ability: at(&current(scope, source)?.abilities.triggered, *index)?,
                }
            }
            Self::Continuous { object } => {
                let source = object_at(scope, *object)?;
                NativeReference::ContinuousAbility {
                    source,
                    ability: current(scope, source)?
                        .abilities
                        .continuous
                        .ok_or(NativeCodecError::InvalidRecord)?,
                }
            }
            Self::Object { index } => NativeReference::Object(object_at(scope, *index)?),
            Self::Stack { index } => NativeReference::Stack(stack_at(scope, *index)?),
            Self::GrantedAbility { object, index } => {
                let source = object_at(scope, *object)?;
                let effects = current(scope, source)?
                    .granted_activated
                    .keys()
                    .copied()
                    .collect::<Vec<_>>();
                NativeReference::GrantedAbility {
                    source,
                    effect: at(&effects, *index)?,
                }
            }
            Self::AdditionalContinuous { object, index } => {
                let source = object_at(scope, *object)?;
                NativeReference::ContinuousAbility {
                    source,
                    ability: at(
                        &current(scope, source)?
                            .abilities
                            .additional_battlefield_abilities,
                        *index,
                    )?,
                }
            }
            Self::Activated { object, index } => {
                let source = object_at(scope, *object)?;
                NativeReference::ActivatedAbility {
                    source,
                    ability: at(&current(scope, source)?.abilities.activated, *index)?,
                }
            }
            Self::Mana { object, index } => {
                let source = object_at(scope, *object)?;
                NativeReference::ManaAbility {
                    source,
                    ability: current(scope, source)?
                        .mana_abilities()
                        .iter()
                        .nth(
                            index
                                .get()
                                .try_into()
                                .map_err(|_| NativeCodecError::InvalidRecord)?,
                        )
                        .copied()
                        .ok_or(NativeCodecError::InvalidRecord)?,
                }
            }
            Self::Decision {} => {
                frame(scope)?;
                NativeReference::Decision(
                    scope
                        .source
                        .pending_decision()
                        .ok_or(NativeCodecError::InvalidRecord)?
                        .id(),
                )
            }
            Self::Legend { index: value } => {
                let RuleFrame::Checks(CheckDecision::Legend(choice)) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                NativeReference::Legend(
                    choice
                        .conflicts()
                        .get(index(*value)?)
                        .ok_or(NativeCodecError::InvalidRecord)?
                        .id(),
                )
            }
            Self::Trigger { index } => {
                let RuleFrame::Triggers(choice) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                NativeReference::Trigger(at(choice.instances(), *index)?)
            }
            Self::Token { index: value } => {
                let RuleFrame::TimestampOrder(choice) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                NativeReference::TokenMember(
                    choice
                        .members()
                        .get(index(*value)?)
                        .ok_or(NativeCodecError::InvalidRecord)?
                        .member,
                )
            }
            Self::DamageApplication { index } => {
                let RuleFrame::Damage(DamageDecision::Order(choice)) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                NativeReference::DamageApplication(at(choice.applications(), *index)?)
            }
            Self::DamageAttempt { index: value } => {
                let RuleFrame::Damage(DamageDecision::Allocation(choice)) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                NativeReference::DamageAttempt(
                    choice
                        .eligible()
                        .get(index(*value)?)
                        .ok_or(NativeCodecError::InvalidRecord)?
                        .attempt(),
                )
            }
            Self::ZoneReplacement { index: value } => {
                NativeReference::ZoneReplacement(index(*value)?)
            }
            Self::MiracleReveal { reveal } => NativeReference::MiracleReveal(*reveal),
            Self::DrawLandKind { land } => NativeReference::DrawLandKind(*land),
            Self::PlaneswalkerType { label } => NativeReference::PlaneswalkerType(
                spellwake_engine::program::PlaneswalkerSubtype::from_label(label)
                    .ok_or(NativeCodecError::InvalidRecord)?,
            ),
            Self::AttackDirection { left } => NativeReference::AttackDirection(if *left {
                spellwake_engine::effects::AttackDirection::Left
            } else {
                spellwake_engine::effects::AttackDirection::Right
            }),
            Self::PermanentType { index: selected } => {
                let RuleFrame::OptionalEffect(choice) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                let Some(value @ OptionalEffectChoice::PermanentType(_)) =
                    choice.options().iter().copied().nth(index(*selected)?)
                else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                NativeReference::OptionalEffect(value)
            }
            Self::PermanentTypeOrder { index: selected } => {
                let RuleFrame::OptionalEffect(choice) = frame(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                let Some(value @ OptionalEffectChoice::PermanentTypeOrder(_)) =
                    choice.options().iter().copied().nth(index(*selected)?)
                else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                NativeReference::OptionalEffect(value)
            }
            Self::Optional { take } => NativeReference::OptionalEffect(if *take {
                OptionalEffectChoice::Take
            } else {
                OptionalEffectChoice::Decline
            }),
            Self::Mulligan { keep } => NativeReference::Mulligan(if *keep {
                MulliganChoice::Keep
            } else {
                MulliganChoice::Mulligan
            }),
            Self::RetainCopyTargets {} => NativeReference::CopyTargets(CopyTargetChoice::Retain),
            Self::ChangeCopyTarget { target } => {
                NativeReference::CopyTargets(CopyTargetChoice::Change(target.resolve(scope)?))
            }
            Self::Color { color } => NativeReference::ManaColor(match color {
                ManaColor::White => ManaType::White,
                ManaColor::Blue => ManaType::Blue,
                ManaColor::Black => ManaType::Black,
                ManaColor::Red => ManaType::Red,
                ManaColor::Green => ManaType::Green,
                ManaColor::Colourless => ManaType::Colourless,
            }),
            Self::CardName { index } => {
                NativeReference::CardName(spellwake_engine::land::CardNameIndex(
                    usize::try_from(index.get()).map_err(|_| NativeCodecError::InvalidRecord)?,
                ))
            }
            Self::CreatureType { index } => {
                let vocabulary = scope.source.execution().creature_vocabulary();
                let member = u16::try_from(index.get())
                    .ok()
                    .and_then(|ordinal| vocabulary.resolve_member(vocabulary.id(), ordinal))
                    .ok_or(NativeCodecError::InvalidRecord)?;
                NativeReference::CreatureType(member)
            }
            Self::EntryPayLife { pay } => NativeReference::EntryPayLife(*pay),
            Self::OptionIndex { index } => NativeReference::OptionIndex(
                u8::try_from(index.get()).map_err(|_| NativeCodecError::InvalidRecord)?,
            ),
        })
    }
}
