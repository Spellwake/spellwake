//! Versioned current-state locations. No native private identity constructors.
use super::*;
use crate::player_wire::{ManaColor, PlayerZoneKind};
use spellwake_engine::{
    checks::CheckDecision,
    effects::{
        CopyTargetChoice, DamageDecision, OptionalEffectChoice, PendingWorkKind, ZoneApplication,
        ZoneOptionalAnswer, ZoneReplacementRequest,
    },
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
fn library_owner(scope: &NativeScope<'_>) -> Result<(), NativeCodecError> {
    if scope.source.execution().pending_work_kind() != Some(PendingWorkKind::LibraryTop) {
        return Err(NativeCodecError::InvalidRecord);
    }
    Ok(())
}
fn library_cards<'a>(scope: &'a NativeScope<'_>) -> Result<&'a [ObjectId], NativeCodecError> {
    library_owner(scope)?;
    match frame(scope)? {
        RuleFrame::LibraryTop(f) => Ok(f.domain_top_first()),
        RuleFrame::ResolutionGraveyardOrder(f) => Ok(f.domain()),
        _ => Err(NativeCodecError::InvalidRecord),
    }
}
fn replacement_owner(scope: &NativeScope<'_>) -> Result<(), NativeCodecError> {
    if !crate::native_game::handles::supports_zone_replacement(scope.source) {
        return Err(NativeCodecError::InvalidRecord);
    }
    Ok(())
}
fn zone_applications<'a>(
    scope: &'a NativeScope<'_>,
) -> Result<&'a [ZoneApplication], NativeCodecError> {
    replacement_owner(scope)?;
    if let RuleFrame::ZoneReplacement(f) = frame(scope)?
        && let ZoneReplacementRequest::Select { applications } = f.request()
    {
        return Ok(applications);
    }
    Err(NativeCodecError::InvalidRecord)
}
fn zone_optional(scope: &NativeScope<'_>) -> Result<(), NativeCodecError> {
    replacement_owner(scope)?;
    if matches!(frame(scope)?,RuleFrame::ZoneReplacement(f) if matches!(f.request(),ZoneReplacementRequest::ConfirmOptional {..}))
    {
        return Ok(());
    }
    Err(NativeCodecError::InvalidRecord)
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
    LibraryCard {
        index: NativeRevision,
    },
    ZoneApplication {
        index: NativeRevision,
    },
    ZoneOptional {
        apply: bool,
    },
    ReplacementChooser {
        #[serde(deserialize_with = "crate::provider_json::object")]
        target: Target,
    },
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
}
impl Location {
    pub(super) fn encode(
        scope: &NativeScope<'_>,
        reference: NativeReference,
    ) -> Result<Self, NativeCodecError> {
        Ok(match reference {
            NativeReference::LibraryCard(value) => Self::LibraryCard {
                index: find(library_cards(scope)?, &value)?,
            },
            NativeReference::ZoneApplication(value) => Self::ZoneApplication {
                index: find(zone_applications(scope)?, &value)?,
            },
            NativeReference::ZoneOptional(value) => {
                zone_optional(scope)?;
                Self::ZoneOptional {
                    apply: value == ZoneOptionalAnswer::Apply,
                }
            }
            NativeReference::ReplacementChooser(player) => Self::ReplacementChooser {
                target: Target::encode(scope, TargetRef::Player(player))?,
            },
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
                if current(scope, source)?.abilities.continuous != Some(ability) {
                    return Err(NativeCodecError::InvalidRecord);
                }
                Self::Continuous {
                    object: object_index(scope, source)?,
                }
            }
            NativeReference::Object(value) => Self::Object {
                index: object_index(scope, value)?,
            },
            NativeReference::Stack(value) => Self::Stack {
                index: stack_index(scope, value)?,
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
            NativeReference::OptionalEffect(value) => Self::Optional {
                take: match value {
                    OptionalEffectChoice::Take => true,
                    OptionalEffectChoice::Decline => false,
                },
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
            NativeReference::CreatureType(value) => Self::CreatureType {
                index: u64::from(value.ordinal()).into(),
            },
        })
    }
    pub(super) fn resolve(
        &self,
        scope: &NativeScope<'_>,
    ) -> Result<NativeReference, NativeCodecError> {
        Ok(match self {
            Self::LibraryCard { index } => {
                NativeReference::LibraryCard(at(library_cards(scope)?, *index)?)
            }
            Self::ZoneApplication { index } => {
                NativeReference::ZoneApplication(at(zone_applications(scope)?, *index)?)
            }
            Self::ZoneOptional { apply } => {
                zone_optional(scope)?;
                NativeReference::ZoneOptional(if *apply {
                    ZoneOptionalAnswer::Apply
                } else {
                    ZoneOptionalAnswer::Decline
                })
            }
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
            Self::ReplacementChooser { target } => {
                let TargetRef::Player(player) = target.resolve(scope)? else {
                    return Err(NativeCodecError::InvalidRecord);
                };
                NativeReference::ReplacementChooser(player)
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
            Self::CreatureType { index } => {
                let vocabulary = scope.source.execution().creature_vocabulary();
                let member = u16::try_from(index.get())
                    .ok()
                    .and_then(|ordinal| vocabulary.resolve_member(vocabulary.id(), ordinal))
                    .ok_or(NativeCodecError::InvalidRecord)?;
                NativeReference::CreatureType(member)
            }
        })
    }
}
