//! Privileged qualification projection of current native owners and actual receipts.
//! Characteristic data comes from one pure evaluation of each current card batch.
//! No canonical mutation, identity allocation, or product visibility filtering occurs here.
use super::related_player_fault::{RelatedPlayerCauseFault, RelatedPlayerFaultSetupError};
use super::{check_trace, damage_trace, mana_ref, packet::AliasContext, program};
use serde_json::{Value, json};
use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};
use spellwake_engine::{
    actions::PermanentEntryRecord,
    activated::AbilityActivationRecord,
    characteristics::{
        CharacteristicBucket, ComponentWait, EffectInstanceKey, EvaluationTrace,
        GeneratedComponent, ObjectCharacteristics, PassBinding, PowerToughnessEvaluation,
        PrintedAbilityRoster, ResolvedContinuousEffect, ResolvedPart, ResolvedParts,
    },
    combat::{
        AttackDeclaration, BlockDeclaration, CombatAssignmentRecord, CombatDamageAssignment,
        CombatDecision, CombatDeclarationRecord, CombatState, Combatant,
    },
    damage::DamageJournal,
    draw::{DrawEvent, DrawJournal},
    effects::{
        CopyTargetChoice, CreatureTypeChoice, DamageDecision, DamageParentView, EffectDecision,
        EffectWindow, PendingWorkKind, ResolutionChoiceState, ResolutionPaymentOutcome,
        ResolutionPaymentReply, ResolutionSelectionOperation, ResolutionSelectionStage,
        ResolutionSelectionState, ResolutionState, RuleCompletion, RuleProgress, RuleResult,
    },
    game::{
        CandidateBase, DecisionReply, GameBasis, GameCommand, GameContext, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    life::LifeChange,
    mana::{ManaPayment, ManaPaymentRecord, ManaProductionRecord, ManaType},
    object_identity::{CardId, GameObject, GameObjects, ObjectId},
    occurrence::{
        CastOccurrence, CharacteristicAppearanceChange, DrawOrigin, EventCause, RulesJournal,
        RulesOccurrence, RulesRecord, SupportedAppearance, TriggerBinding, TriggerRelated,
        WaitingInstance, ZoneReason,
    },
    opening::OpeningDecision,
    priority::{PriorityPhase, SeatId},
    stack_window::{SpellCopyObject, StackEntry, StackKind, StackWindow, WindowPhase},
    targets::TargetRef,
    turn::{TurnScheduler, WorkCompleted},
};
use std::collections::BTreeMap;

pub fn opaque(value: impl std::fmt::Debug) -> String {
    format!("{value:?}")
}
/// Structured context from the six actual native accessors.
pub fn context(value: &GameContext) -> Value {
    json!({"release":value.release(),"rules":value.rules(),"catalogue":value.catalogue(),"programs":value.programs(),"profile":value.profile(),"creature_vocabulary":value.creature_vocabulary_id().to_string(),"name_rules_id":value.name_rules_id().to_string()})
}
pub use super::creature_vocabulary::{member_view as creature_type_id, view as vocabulary};
pub fn creature_type_frame(value: &CreatureTypeChoice) -> Value {
    json!({"tag":"CreatureType","revision":value.revision().to_string(),"entry":opaque(value.entry()),"instruction":value.instruction().to_string(),"slot":value.slot().ordinal().to_string(),"chooser":value.chooser().0,"vocabulary":value.vocabulary().to_string(),"excluding":value.excluding().iter().map(|value|creature_type_id(*value)).collect::<Vec<_>>()})
}
pub fn resolution_state(value: ResolutionState<'_>) -> Value {
    json!({"entry":opaque(value.entry()),"instruction":value.instruction().to_string(),"locals":value.locals().iter().map(|(slot,value)|json!({"slot":slot.ordinal().to_string(),"value":creature_type_id(*value)})).collect::<Vec<_>>()})
}
pub fn damage_parent(value: DamageParentView<'_>) -> Value {
    match value {
        DamageParentView::Resolution(cursor) => {
            json!({"tag":"Resolution","cursor":resolution_state(cursor)})
        }
        DamageParentView::Combat => json!({"tag":"Combat"}),
        DamageParentView::ManaAbility => json!({"tag":"ManaAbility"}),
    }
}
pub fn turn_scheduler(value: &TurnScheduler) -> Value {
    json!({"position":damage_trace::turn_position_view(value.position()),"stage":opaque(value.stage()),"lands_played":value.lands_played()})
}
pub fn object(value: &GameObject) -> Value {
    let mut result = json!(damage_trace::game_object_view(value));
    if value.transformed() {
        result["transformed"] = json!(true);
    }
    if let Some(kind) = value.face_down() {
        result["face_down"] = json!(format!("{kind:?}"));
    }
    if value.is_unearthed() {
        result["unearth_departure"] = json!(true);
    }
    result
}
pub fn zone_move_outcome(value: &spellwake_engine::object_identity::ZoneMoveOutcome) -> Value {
    use spellwake_engine::object_identity::ZoneMoveOutcome;
    match value {
        ZoneMoveOutcome::Moved { movement } => {
            json!({"tag":"Moved","before":object(&movement.before),"after":object(&movement.after)})
        }
        ZoneMoveOutcome::Unchanged {
            object: value,
            reason,
        } => json!({"tag":"Unchanged","object":object(value),"reason":opaque(reason)}),
    }
}
fn card_piles(cards: &GameObjects, participants: &[SeatId]) -> Value {
    json!(participants.iter().map(|seat|json!({"player":seat.0,"library":cards.library(*seat).iter().map(opaque).collect::<Vec<_>>(),"graveyard":cards.graveyard(*seat).iter().map(opaque).collect::<Vec<_>>()})).collect::<Vec<_>>())
}
/// Observe a standalone physical owner, retaining the supplied participant order.
/// This does not construct or imply an executor, root, or accepted game candidate.
pub fn card_objects(cards: &GameObjects, participants: &[SeatId]) -> Value {
    json!({"revision":cards.revision().to_string(),"next_identity":cards.next_identity_sequence().to_string(),"objects":cards.iter().map(object).collect::<Vec<_>>(),"piles":card_piles(cards,participants)})
}
pub fn target(value: TargetRef) -> Value {
    match value {
        TargetRef::Object(value) => json!({"tag":"Object","object":opaque(value)}),
        TargetRef::Player(player) => json!({"tag":"Player","player":player.0}),
        TargetRef::Stack(value) => json!({"tag":"Stack","entry":opaque(value)}),
    }
}
pub fn basis(value: &GameBasis) -> Value {
    json!({"game":value.game.0.to_string(),"context":context(&value.context),"epoch":value.epoch.to_string(),"revision":value.revision.to_string()})
}
fn spend(value: &ManaPayment) -> Value {
    let rows = json!(
        value
            .spend
            .iter()
            .map(|row| json!({"type":opaque(row.mana_type),"amount":row.amount}))
            .collect::<Vec<_>>()
    );
    if value.from_lots.is_empty() {
        rows
    } else {
        json!({"spend":rows,"from_lots":value.from_lots})
    }
}
pub fn payment(value: &ManaPaymentRecord) -> Value {
    let mut result = json!({"entry":opaque(value.source),"payer":value.payer.0,"cost":program::cost(&value.cost),"spend":value.spend.iter().map(|row|json!({"type":opaque(row.mana_type),"amount":row.amount})).collect::<Vec<_>>()});
    if !value.from_lots.is_empty() {
        result["from_lots"] = json!(value.from_lots);
    }
    if !value.restricted_mana.is_empty() {
        result["restricted_mana"] = json!(value.restricted_mana.iter().map(|row| json!({"type":opaque(row.mana_type),"restriction":format!("{:?}",row.restriction),"amount":row.amount})).collect::<Vec<_>>());
    }
    result
}
pub fn production(value: &ManaProductionRecord) -> Value {
    let mut result = json!({"source":opaque(value.source),"ability":mana_ref::view(value.ability),"controller":value.controller.0,"produced":value.produced.iter().map(|(kind,amount)|(opaque(kind),amount)).collect::<BTreeMap<_,_>>()});
    if let Some(recipient) = value.recipient {
        result["recipient"] = json!(recipient.0);
    }
    if let Some(restriction) = value.restriction {
        result["restriction"] = json!(format!("{restriction:?}"));
    }
    result
}
fn casting_payment_view(
    mut value: Value,
    payment: &spellwake_engine::mana::CastingPayment,
) -> Value {
    if !payment.splice.is_empty() {
        value["splice"] = json!(
            payment
                .splice
                .iter()
                .map(
                    |choice| json!({"card":opaque(choice.card),"target":choice.target.map(target)})
                )
                .collect::<Vec<_>>()
        );
    }
    if payment.disturb {
        value["disturb"] = json!(true);
    }
    if let Some(half) = payment.split_half {
        value["split_half"] = json!(match half {
            spellwake_engine::program::SplitHalf::Left => "Left",
            spellwake_engine::program::SplitHalf::Right => "Right",
        });
    }
    if payment.adventure {
        value["adventure"] = json!(true);
    }
    if !payment.modes.is_empty() {
        value["modes"] = json!(
            payment
                .modes
                .iter()
                .map(|mode| {
                    let mut row = json!({"mode":mode.mode,"target":mode.target.map(target)});
                    if !mode.targets.is_empty() {
                        row["targets"] =
                            json!(mode.targets.iter().copied().map(target).collect::<Vec<_>>());
                    }
                    row
                })
                .collect::<Vec<_>>()
        );
    }
    if !payment.hybrid_choices.is_empty() {
        value["hybrid_choices"] = json!(
            payment
                .hybrid_choices
                .iter()
                .map(|side| format!("{side:?}"))
                .collect::<Vec<_>>()
        );
    }
    if !payment.fixed_targets.is_empty() {
        value["fixed_targets"] = json!(
            payment
                .fixed_targets
                .iter()
                .copied()
                .map(target)
                .collect::<Vec<_>>()
        );
    }
    if !payment.restricted_mana.is_empty() {
        value["restricted_mana"] = json!(payment.restricted_mana.iter().map(|row| json!({"type":opaque(row.mana_type),"restriction":format!("{:?}",row.restriction),"amount":row.amount})).collect::<Vec<_>>());
    }
    value
}
pub fn ability_activation(value: &AbilityActivationRecord) -> Value {
    let mut result = json!({"entry":opaque(value.entry),"ability":value.ability.0,"controller":value.controller.0,"source":object(&value.source),"source_cost":opaque(value.source_cost),"resulting_source":opaque(value.resulting_source)});
    if let Some(effect) = value.granting_effect {
        result["granting_effect"] = effect_instance(effect);
    }
    result
}
fn attack(value: &AttackDeclaration) -> Value {
    let mut result = json!({"attacker":opaque(value.attacker),"defender":value.defender.0});
    if let Some(id) = value.defending_planeswalker {
        result["defending_planeswalker"] = json!(opaque(id));
    }
    result
}
fn block(value: &BlockDeclaration) -> Value {
    json!({"blocker":opaque(value.blocker),"attacker":opaque(value.attacker)})
}
fn assignment(value: &CombatDamageAssignment) -> Value {
    json!({"source":opaque(value.source),"target":damage_trace::damage_recipient_view(value.target),"amount":value.amount})
}
pub fn reply(value: &DecisionReply) -> Value {
    match value {
        DecisionReply::CastRecovery(choice) => {
            json!({"tag":"CastRecovery","choice":format!("{choice:?}")})
        }
        DecisionReply::ZoneReplacement(reply) => match reply {
            spellwake_engine::effects::ZoneReplacementReply::Devour(id) => {
                json!({"tag":"Devour","object":id.map(opaque)})
            }
            spellwake_engine::effects::ZoneReplacementReply::DevourGraveyardOrder(id) => {
                json!({"tag":"DevourGraveyardOrder","object":opaque(id)})
            }
            spellwake_engine::effects::ZoneReplacementReply::EntryCopyFromPermanent(aura) => {
                json!({"tag":"EntryCopyFromPermanent","aura":opaque(aura)})
            }
            spellwake_engine::effects::ZoneReplacementReply::EntryCopy(donor) => {
                json!({"tag":"EntryCopy","donor":donor.map(opaque)})
            }
            spellwake_engine::effects::ZoneReplacementReply::EntryController(seat) => {
                json!({"tag":"EntryController","seat":seat.0})
            }
            spellwake_engine::effects::ZoneReplacementReply::Select(application) => {
                json!({"tag":"ZoneReplacement","reply":{"tag":"Select","application":zone_application(*application)}})
            }
            spellwake_engine::effects::ZoneReplacementReply::Optional(answer) => {
                json!({"tag":"ZoneReplacement","reply":{"tag":"Optional","answer":opaque(answer)}})
            }
        },
        DecisionReply::LibraryTop(reply) => match reply {
            spellwake_engine::effects::LibraryTopReply::Search { selected } => {
                json!({"tag":"LibraryTop","kind":"Search","selected":selected.map(opaque)})
            }
            spellwake_engine::effects::LibraryTopReply::SearchHandGraveyard { hand, graveyard } => {
                json!({"tag":"LibraryTop","kind":"SearchHandGraveyard","hand":hand.map(opaque),"graveyard":graveyard.map(opaque)})
            }
            spellwake_engine::effects::LibraryTopReply::SearchSplit { battlefield, hand } => {
                json!({"tag":"LibraryTop","kind":"SearchSplit","battlefield":battlefield.map(opaque),"hand":hand.map(opaque)})
            }
            spellwake_engine::effects::LibraryTopReply::SearchBattlefield {
                selected_in_timestamp_order,
            } => {
                json!({"tag":"LibraryTop","kind":"SearchBattlefield","selected_in_timestamp_order":selected_in_timestamp_order.iter().map(opaque).collect::<Vec<_>>()})
            }
            spellwake_engine::effects::LibraryTopReply::SearchWithEntryColors {
                selected,
                colors,
            } => {
                json!({"tag":"LibraryTop","kind":"SearchWithEntryColors","selected":selected.iter().map(opaque).collect::<Vec<_>>(),"colors":colors.iter().map(|(object,color)|json!({"object":opaque(object),"color":opaque(color)})).collect::<Vec<_>>()})
            }
            spellwake_engine::effects::LibraryTopReply::SearchWithEntryChoices {
                selected,
                choices,
            } => {
                json!({"tag":"LibraryTop","kind":"SearchWithEntryChoices","selected":selected.iter().map(opaque).collect::<Vec<_>>(),"choices":choices.iter().map(|(object,choice)|json!({"object":opaque(object),"choice":format!("{choice:?}")})).collect::<Vec<_>>()})
            }
            spellwake_engine::effects::LibraryTopReply::Scry {
                top_first,
                bottom_top_first,
            } => {
                json!({"tag":"LibraryTop","kind":"Scry","top_first":top_first.iter().map(opaque).collect::<Vec<_>>(),"bottom_top_first":bottom_top_first.iter().map(opaque).collect::<Vec<_>>()})
            }
            spellwake_engine::effects::LibraryTopReply::Surveil {
                top_first,
                to_graveyard,
            } => {
                json!({"tag":"LibraryTop","kind":"Surveil","top_first":top_first.iter().map(opaque).collect::<Vec<_>>(),"to_graveyard":to_graveyard.iter().map(opaque).collect::<Vec<_>>()})
            }
        },
        DecisionReply::SelectResolution(values) => {
            json!({"tag":"SelectResolution","objects":values.iter().map(opaque).collect::<Vec<_>>()})
        }
        DecisionReply::OrderResolutionGraveyard(values) => {
            json!({"tag":"OrderResolutionGraveyard","objects":values.iter().map(opaque).collect::<Vec<_>>()})
        }
        DecisionReply::CascadeCast {
            target: choice,
            payment,
        } => casting_payment_view(
            json!({"tag":"CascadeCast","target":choice.as_ref().map(|v|target(*v)),"payment":payment.mana.as_ref().map(spend),"additional":payment.additional,"objects":payment.objects.iter().copied().map(cast_resource).collect::<Vec<_>>() }),
            payment,
        ),
        DecisionReply::MadnessCast {
            target: choice,
            payment,
            mana_abilities,
        } => casting_payment_view(
            json!({"tag":"MadnessCast","target":choice.as_ref().map(|v|target(*v)),"spend":payment.mana.as_ref().map(spend),"additional":payment.additional,"objects":payment.objects.iter().copied().map(cast_resource).collect::<Vec<_>>(),"mana_abilities":mana_abilities.iter().map(opaque).collect::<Vec<_>>() }),
            payment,
        ),
        DecisionReply::ImpulseCast {
            target: choice,
            payment,
            mana_abilities,
        } => casting_payment_view(
            json!({"tag":"ImpulseCast","target":choice.as_ref().map(|v|target(*v)),"chosen_x":payment.chosen_x,"permanent_targets":payment.permanent_targets.iter().copied().map(target).collect::<Vec<_>>(),"modes":payment.modes.iter().map(|mode|json!({"mode":mode.mode,"target":mode.target.map(target)})).collect::<Vec<_>>(),"alternative":payment.use_alternative,"spend":payment.mana.as_ref().map(spend),"additional":payment.additional,"objects":payment.objects.iter().copied().map(cast_resource).collect::<Vec<_>>(),"mana_abilities":mana_abilities.iter().map(opaque).collect::<Vec<_>>() }),
            payment,
        ),
        DecisionReply::ReboundCast {
            target: choice,
            payment,
        } => casting_payment_view(
            json!({"tag":"ReboundCast","target":choice.as_ref().map(|v|target(*v)),"spend":payment.mana.as_ref().map(spend),"additional":payment.additional,"objects":payment.objects.iter().copied().map(cast_resource).collect::<Vec<_>>(),"alternative":payment.use_alternative}),
            payment,
        ),
        DecisionReply::SuspendCast {
            target: choice,
            payment,
        } => casting_payment_view(
            json!({"tag":"SuspendCast","target":choice.as_ref().map(|v|target(*v)),"spend":payment.mana.as_ref().map(spend),"additional":payment.additional,"objects":payment.objects.iter().copied().map(cast_resource).collect::<Vec<_>>(),"alternative":payment.use_alternative}),
            payment,
        ),
        DecisionReply::OptionalEffect(choice) => {
            json!({"tag":"OptionalEffect","choice":opaque(choice)})
        }
        DecisionReply::ResolutionPayment(reply) => {
            json!({"tag":"ResolutionPayment","reply":match reply {
                ResolutionPaymentReply::Pay(value) => json!({"tag":"Pay","payment":{"spend":spend(value)}}),
                ResolutionPaymentReply::Decline => json!({"tag":"Decline"}),
            }})
        }
        DecisionReply::Discard(values) => {
            json!({"tag":"Discard","items":values.iter().map(opaque).collect::<Vec<_>>()})
        }
        DecisionReply::KeepLegend { conflict, keep } => {
            json!({"tag":"KeepLegend","conflict":check_trace::legend_conflict_id_view(*conflict),"keep":opaque(keep)})
        }
        DecisionReply::ReturnCommanders(values) => {
            json!({"tag":"ReturnCommanders","items":values.iter().map(opaque).collect::<Vec<_>>()})
        }
        DecisionReply::OrderGraveyard(values) => {
            json!({"tag":"OrderGraveyard","items":values.iter().map(opaque).collect::<Vec<_>>()})
        }
        DecisionReply::TriggerPermanentTargets(values) => {
            json!({"tag":"TriggerPermanentTargets","targets":values.iter().copied().map(target).collect::<Vec<_>>()})
        }
        DecisionReply::ApplyCounterDoublersFirst(number) => {
            json!({"tag":"ApplyCounterDoublersFirst","number":number})
        }
        DecisionReply::TriggerModes(modes) => {
            json!({"tag":"TriggerModes","modes":modes.iter().map(|mode| {
            json!({"index":mode.mode,"target":mode.target.map(target),
                "targets":mode.targets.iter().copied().map(target).collect::<Vec<_>>()})
        }).collect::<Vec<_>>()})
        }
        DecisionReply::TriggerTarget(value) => {
            json!({"tag":"TriggerTarget","target":target(*value)})
        }
        DecisionReply::OrderTriggers(values) => {
            json!({"tag":"OrderTriggers","items":values.iter().map(|id|id.sequence().to_string()).collect::<Vec<_>>()})
        }
        DecisionReply::PayAttackCost { life_symbols, mana } => {
            json!({"tag":"PayAttackCost","life_symbols":life_symbols,"mana":mana.spend.iter().map(|s|json!([format!("{:?}",s.mana_type),s.amount])).collect::<Vec<_>>()})
        }
        DecisionReply::RedeclareReversingMana(ids) => {
            json!({"tag":"RedeclareReversingMana", "ids":ids.iter().map(|id|id.0).collect::<Vec<_>>()})
        }
        DecisionReply::RedeclareKeepingMana => json!({"tag":"RedeclareKeepingMana"}),
        DecisionReply::RedeclareAttackers => json!({"tag":"RedeclareAttackers"}),
        DecisionReply::DeclareBandedAttackers { attackers, bands } => {
            json!({"tag":"DeclareBandedAttackers","items":attackers.iter().map(attack).collect::<Vec<_>>(),
                "bands":bands.iter().map(|members|members.iter().copied().map(opaque).collect::<Vec<_>>()).collect::<Vec<_>>()})
        }
        DecisionReply::DeclareAttackers(values) => {
            json!({"tag":"DeclareAttackers","items":values.iter().map(attack).collect::<Vec<_>>()})
        }
        DecisionReply::DeclareBlockers(values) => {
            json!({"tag":"DeclareBlockers","items":values.iter().map(block).collect::<Vec<_>>()})
        }
        DecisionReply::AssignCombatDamage(values) => {
            json!({"tag":"AssignCombatDamage","items":values.iter().map(assignment).collect::<Vec<_>>()})
        }
        DecisionReply::Mulligan(choice) => json!({"tag":"Mulligan","choice":opaque(choice)}),
        DecisionReply::Bottom(values) => {
            json!({"tag":"Bottom","items":values.iter().map(opaque).collect::<Vec<_>>()})
        }
        DecisionReply::CopyTargets(choice) => {
            json!({"tag":"CopyTargets","choice":match choice { CopyTargetChoice::Retain=>json!({"tag":"Retain"}), CopyTargetChoice::Change(value)=>json!({"tag":"Change","target":target(*value)}) }})
        }
        DecisionReply::DamageOrder(value) => {
            json!({"tag":"DamageOrder","application":damage_trace::damage_application_view(*value)})
        }
        DecisionReply::AllocatePrevention(values) => {
            json!({"tag":"AllocatePrevention","items":values.iter().map(|row|json!({"attempt":opaque(row.attempt),"amount":row.amount})).collect::<Vec<_>>()})
        }
        DecisionReply::TransformOrder(objects) => {
            json!({"tag":"TransformOrder","order":objects.iter().map(opaque).collect::<Vec<_>>()})
        }
        DecisionReply::LinkedReturnOrder(objects) => {
            json!({"tag":"LinkedReturnOrder","order":objects.iter().map(opaque).collect::<Vec<_>>()})
        }
        DecisionReply::LinkedAuraChoice(value) => {
            json!({"tag":"LinkedAuraChoice","target":target(*value)})
        }
        DecisionReply::PlaneswalkerType(direction) => {
            json!({"tag":"PlaneswalkerType","subtype":direction.label()})
        }
        DecisionReply::AttackDirection(direction) => {
            json!({"tag":"AttackDirection","direction":format!("{direction:?}")})
        }
        DecisionReply::ChooseLandKind(land) => json!({"tag":"ChooseLandKind","land":land}),
        DecisionReply::MutateOnTop(value) => json!({"tag":"MutateOnTop","on_top":value}),
        DecisionReply::ChoosePile(pile) => json!({"tag":"ChoosePile","pile":pile}),
        DecisionReply::DrawReplacement(choice) => {
            json!({"tag":"DrawReplacement","choice":format!("{choice:?}")})
        }
        DecisionReply::ChooseDamageRecipient(recipient) => {
            json!({"tag":"ChooseDamageRecipient","recipient":target(*recipient)})
        }
        DecisionReply::OpponentChoice(seat) => json!({"tag":"OpponentChoice","player":seat.0}),
        DecisionReply::ReplacementChooser(seat) => {
            json!({"tag":"ReplacementChooser","player":seat.0})
        }
        DecisionReply::MyriadDefender(choice) => {
            json!({"tag":"MyriadDefender","defender":choice.map(target)})
        }
        DecisionReply::ReselectAttackDefender(choice) => {
            json!({"tag":"ReselectAttackDefender","defender":choice.map(target)})
        }
        DecisionReply::TokenDefender(chosen) => {
            json!({"tag":"TokenDefender","target":target(*chosen)})
        }
        DecisionReply::TimestampOrder(values) => {
            json!({"tag":"TimestampOrder","order":values.iter().map(|value|value.ordinal()).collect::<Vec<_>>()})
        }
        DecisionReply::RemoveCounters(counts) => {
            json!({"tag":"RemoveCounters","counts":counts.iter().map(|(kind,count)|json!({"kind":format!("{kind:?}"),"count":count})).collect::<Vec<_>>()})
        }
        DecisionReply::Proliferate(values) => {
            json!({"tag":"Proliferate","selected":values.iter().map(|value|target(*value)).collect::<Vec<_>>()})
        }
        DecisionReply::CreatureType(value) => {
            json!({"tag":"CreatureType","value":creature_type_id(*value)})
        }
        DecisionReply::ManaColor(value) => json!({"tag":"ManaColor","color":opaque(value)}),
        DecisionReply::SplitMana(amount) => json!({"tag":"SplitMana","amount":amount}),
        DecisionReply::EntryChoice(value) => match value {
            spellwake_engine::land::EntryChoice::Color(color) => {
                json!({"tag":"EntryColor","color":opaque(color)})
            }
            spellwake_engine::land::EntryChoice::Reveal(card) => {
                json!({"tag":"EntryReveal","card":card.map(opaque)})
            }
            spellwake_engine::land::EntryChoice::Players(seats) => {
                json!({"tag":"EntryPlayers","seats":seats.map(|seat|seat.0)})
            }
            spellwake_engine::land::EntryChoice::Opponent(seat) => {
                json!({"tag":"EntryOpponent","seat":seat.0})
            }
            spellwake_engine::land::EntryChoice::Chapter(n) => {
                json!({"tag":"SagaChapter","number":n})
            }
            spellwake_engine::land::EntryChoice::CardName(index) => {
                json!({"tag":"EntryCardName","index":index.0})
            }
            spellwake_engine::land::EntryChoice::CreatureType(kind) => {
                json!({"tag":"EntryCreatureType","value":creature_type_id(*kind)})
            }
            spellwake_engine::land::EntryChoice::PayLife(pay) => {
                json!({"tag":"EntryPayLife","pay":pay})
            }
        },
        DecisionReply::TokenAuraChoice(host) => {
            json!({"tag":"TokenAuraChoice","target":target(*host)})
        }
        DecisionReply::TokenEntryChoice(value) => {
            json!({"tag":"TokenEntryChoice","choice":format!("{value:?}")})
        }
    }
}
pub fn input(value: &GameInput) -> Value {
    let command = match &value.command {
        GameCommand::Concede => json!({"tag":"Concede"}),
        GameCommand::Pass => json!({"tag":"Pass"}),
        GameCommand::PlayLand { land } => json!({"tag":"PlayLand","land":opaque(land)}),
        GameCommand::PlayLandWithEntryColor { land, color } => {
            json!({"tag":"PlayLandWithEntryColor","land":opaque(land),"color":opaque(color)})
        }
        GameCommand::PlayLandWithEntryReveal { land, reveal } => {
            json!({"tag":"PlayLandWithEntryReveal","land":opaque(land),"reveal":reveal.map(opaque)})
        }
        GameCommand::ActivateMana { source, ability } => {
            json!({"tag":"ActivateMana","source":opaque(source),"ability":mana_ref::view(*ability)})
        }
        GameCommand::ActivateManaForPlayer {
            source,
            ability,
            recipient,
        } => {
            json!({"tag":"ActivateManaForPlayer","source":opaque(source),"ability":mana_ref::view(*ability),"recipient":recipient.0})
        }
        GameCommand::ActivateManaWithCostObjects {
            source,
            ability,
            objects,
        } => {
            json!({"tag":"ActivateManaWithCostObjects","source":opaque(source),"ability":mana_ref::view(*ability),"objects":objects.iter().map(opaque).collect::<Vec<_>>()})
        }
        GameCommand::ActivateManaWithRestrictedPayment {
            source,
            ability,
            payment,
            restricted_mana,
            bundle,
        } => {
            json!({"tag":"ActivateManaWithRestrictedPayment","source":opaque(source),"ability":mana_ref::view(*ability),"payment":spend(payment),"restricted_mana":format!("{restricted_mana:?}"),"bundle":format!("{bundle:?}")})
        }
        GameCommand::ActivateWithRestrictedMana {
            source,
            ability,
            granting_effect,
            origin,
            chosen_x,
            payment,
            restricted_mana,
        } => {
            json!({"tag":"ActivateWithRestrictedMana","source":opaque(source),"ability":ability.0,"granting_effect":format!("{granting_effect:?}"),"origin":format!("{origin:?}"),"chosen_x":chosen_x,"payment":payment.as_ref().map(spend),"restricted_mana":format!("{restricted_mana:?}")})
        }
        GameCommand::ActivateManaWithBundle {
            source,
            ability,
            bundle,
        } => {
            json!({"tag":"ActivateManaWithBundle","source":opaque(source),"ability":mana_ref::view(*ability),"bundle":bundle.iter().map(|(kind, amount)|(format!("{kind:?}"),amount)).collect::<BTreeMap<_,_>>()})
        }
        GameCommand::ActivateHybridMana {
            source,
            ability,
            payment,
            bundle,
        } => {
            json!({"tag":"ActivateHybridMana","source":opaque(source),"ability":mana_ref::view(*ability),"payment":spend(payment),"bundle":bundle.iter().map(|(kind, amount)|(format!("{kind:?}"),amount)).collect::<BTreeMap<_,_>>()})
        }
        GameCommand::ActivateManaWithPayment {
            source,
            ability,
            payment,
        } => {
            json!({"tag":"ActivateManaWithPayment","source":opaque(source),"ability":mana_ref::view(*ability),"payment":spend(payment)})
        }
        GameCommand::Foretell { source, payment } => {
            json!({"tag":"Foretell","source":opaque(source),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::Suspend { source, payment } => {
            json!({"tag":"Suspend","source":opaque(source),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::CastFaceDown {
            spell,
            kind,
            payment,
        } => {
            json!({"tag":"CastFaceDown","spell":opaque(spell),"kind":format!("{kind:?}"),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::TurnFaceUp {
            permanent,
            ability,
            payment,
        } => {
            json!({"tag":"TurnFaceUp","permanent":opaque(permanent),"ability":ability,"payment":payment.as_ref().map(spend)})
        }
        GameCommand::Plot { source, payment } => {
            json!({"tag":"Plot","source":opaque(source),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::CastWithResources {
            spell,
            target: choice,
            payment,
        } => {
            let mut row = json!({"tag":"CastWithResources","use_alternative":payment.use_alternative,"additional":payment.additional,"spell":opaque(spell),"target":choice.as_ref().map(|v|target(*v)),"payment":payment.mana.as_ref().map(spend),"resources":payment.objects.iter().copied().map(cast_resource).collect::<Vec<_>>() });
            if !payment.permanent_targets.is_empty() {
                row["permanent_targets"] = json!(
                    payment
                        .permanent_targets
                        .iter()
                        .copied()
                        .map(target)
                        .collect::<Vec<_>>()
                );
            }
            casting_payment_view(row, payment)
        }
        GameCommand::ActivateWithCreatures {
            source,
            ability,
            creatures,
        } => {
            json!({"tag":"ActivateWithCreatures","source":opaque(source),"ability":ability.0,"creatures":creatures.iter().map(opaque).collect::<Vec<_>>() })
        }
        GameCommand::ActivateUntargetedAbility {
            source,
            ability,
            payment,
        } => {
            json!({"tag":"ActivateUntargetedAbility","source":opaque(source),"ability":ability.0,"payment":payment.as_ref().map(spend)})
        }
        GameCommand::Ninjutsu {
            source,
            ability,
            attacker,
            payment,
        } => {
            json!({"tag":"Ninjutsu","source":opaque(source),"ability":ability.0,"attacker":opaque(attacker),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::Transmute {
            source,
            ability,
            payment,
        } => {
            json!({"tag":"Transmute","source":opaque(source),"ability":ability.0,"payment":payment.as_ref().map(spend)})
        }
        GameCommand::Cycle {
            source,
            ability,
            payment,
        } => {
            json!({"tag":"Cycle","source":opaque(source),"ability":ability.0,"payment":payment.as_ref().map(spend)})
        }
        GameCommand::ActivateEmblem {
            emblem,
            creature,
            payment,
        } => {
            json!({"tag":"ActivateEmblem","emblem":emblem.sequence(),"creature":opaque(creature),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::ActivatePermanentTargets {
            source,
            ability,
            targets,
            payment,
        } => {
            json!({"tag":"ActivatePermanentTargets","source":opaque(source),"ability":ability.0,"targets":targets.iter().copied().map(target).collect::<Vec<_>>(),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::ActivateWithCostObjects {
            source,
            ability,
            granting_effect,
            target: value,
            objects,
            payment,
        } => {
            json!({"tag":"ActivateWithCostObjects","source":opaque(source),"ability":opaque(ability),"granting_effect":granting_effect.map(effect_instance),"target":value.map(target),"objects":objects.iter().map(opaque).collect::<Vec<_>>(),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::ActivateGranted {
            source,
            effect,
            target: value,
            payment,
        } => {
            json!({"tag":"ActivateGranted","source":opaque(source),"effect":effect_instance(*effect),"target":value.map(target),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::ActivateLoyaltyX {
            source,
            ability,
            target: value,
            chosen_x,
            payment,
        } => {
            json!({"tag":"ActivateLoyaltyX","source":opaque(source),"ability":ability.0,"target":target(*value),"chosen_x":chosen_x,"payment":payment.as_ref().map(spend)})
        }
        GameCommand::ActivateAbility {
            source,
            ability,
            target: value,
            payment,
        } => {
            json!({"tag":"ActivateAbility","source":opaque(source),"ability":ability.0,"target":target(*value),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::CastUntargeted { spell, payment } => {
            json!({"tag":"CastUntargeted","spell":opaque(spell),"payment":payment.as_ref().map(spend)})
        }
        GameCommand::Cast {
            spell,
            target: value,
        } => json!({"tag":"Cast","spell":opaque(spell),"target":target(*value)}),
        GameCommand::CastPaid {
            spell,
            target: value,
            payment,
        } => {
            json!({"tag":"CastPaid","spell":opaque(spell),"target":target(*value),"payment":spend(payment)})
        }
        GameCommand::Reply { decision, choice } => {
            json!({"tag":"Reply","decision":decision.0.to_string(),"choice":reply(choice)})
        }
    };
    json!({"basis":basis(&value.basis),"actor":value.actor.0,"command":command})
}
/// Borrowed exact constructor/command inputs for qualification observation only.
/// This enum does not execute, decode or restore any native operation.
pub enum QualificationCall<'a> {
    GameInput(&'a GameInput),
    GameRelatedPlayerCauseFault {
        fault: RelatedPlayerCauseFault,
    },
    GameSupplyEntropy {
        basis: &'a GameBasis,
        words: &'a [u64],
    },
    GameInitialize {
        game: spellwake_engine::game::GameId,
        context: &'a GameContext,
        seed: &'a spellwake_engine::setup::GameSeed,
    },
    GameInitializeScheduled {
        game: spellwake_engine::game::GameId,
        context: &'a GameContext,
        seed: &'a spellwake_engine::setup::GameSeed,
        start: spellwake_engine::turn::TurnStart,
    },
    GameInitializeOpening {
        game: spellwake_engine::game::GameId,
        context: &'a GameContext,
        setup: &'a spellwake_engine::game::OpeningSetup,
        entropy_words: &'a [u64],
    },
    EffectInitialize(&'a spellwake_engine::setup::GameSeed),
    EffectCall(EffectCall<'a>),
    GameObjectsInitialize {
        revision: u64,
    },
    GameObjectsCall(GameObjectsCall<'a>),
    StackWindowCall(StackWindowCall<'a>),
    TurnCall(TurnCall),
    ProgramValidate {
        program: &'a spellwake_engine::program::Program,
        vocabulary: &'a CreatureTypeVocabulary,
    },
    PureCall {
        method: &'a str,
        input: &'a Value,
    },
}
/// Borrowed view of the cfg(test)-only component's exact native operands.
pub struct SharedDamageComponent<'a> {
    pub source: spellwake_engine::object_identity::ObjectId,
    pub recipient: spellwake_engine::damage::DamageRecipient,
    pub amount: &'a spellwake_contracts::RulesInteger,
}
pub enum EffectCall<'a> {
    BeginOpening {
        expected: u64,
    },
    OpeningShuffle {
        expected: u64,
        player: SeatId,
        entropy: &'a spellwake_engine::shuffle::EntropyTape,
        entropy_words: &'a [u64],
    },
    QualificationDiscard {
        expected: u64,
        player: SeatId,
        object: spellwake_engine::object_identity::ObjectId,
    },
    QualificationSetTapped {
        expected: u64,
        objects: &'a [spellwake_engine::object_identity::ObjectId],
        tapped: bool,
    },
    RecordTurnStart {
        expected: u64,
    },
    UntapOnTurn {
        expected: u64,
        turn: &'a TurnScheduler,
    },
    CastOnTurn {
        expected: u64,
        actor: SeatId,
        spell: spellwake_engine::object_identity::ObjectId,
        target: Option<TargetRef>,
        payment: Option<&'a ManaPayment>,
        turn: Option<&'a TurnScheduler>,
    },
    ActivateAbility {
        expected: u64,
        actor: SeatId,
        source: spellwake_engine::object_identity::ObjectId,
        ability: spellwake_engine::activated::ActivatedAbilityId,
        target: TargetRef,
        payment: Option<&'a ManaPayment>,
    },
    QualificationZoneMoves {
        expected: u64,
        moves: &'a [(
            spellwake_engine::object_identity::ObjectId,
            spellwake_engine::object_identity::ObjectDestination,
        )],
    },
    QualificationControllerSuccessor {
        object: spellwake_engine::object_identity::ObjectId,
        controller: SeatId,
    },
    BeginCleanup {
        expected: u64,
        turn: &'a TurnScheduler,
    },
    EnterCalendar {
        expected: u64,
        previous: Option<&'a TurnScheduler>,
        next_turn: &'a TurnScheduler,
        active: SeatId,
    },
    BeginManaActivation {
        expected: u64,
        actor: SeatId,
        source: spellwake_engine::object_identity::ObjectId,
        ability: spellwake_engine::mana::ManaAbilityRef,
    },
    AcceptCreatureType {
        expected: u64,
        frame: &'a CreatureTypeChoice,
        actor: SeatId,
        value: CreatureTypeId,
    },
    BeginDamageBatch {
        origin: spellwake_engine::damage::DamageOrigin,
        kind: spellwake_engine::damage::DamageKind,
        inputs: &'a [spellwake_engine::damage::DamageInput],
        damage: &'a DamageJournal,
        draws: &'a DrawJournal,
        declarations: &'a [CombatDeclarationRecord],
        assignments: &'a [CombatAssignmentRecord],
    },
    QualificationBeginSharedDamage {
        entry: spellwake_engine::stack_window::StackEntryId,
        instruction: usize,
        components: &'a [SharedDamageComponent<'a>],
    },
    QualificationPrepareResolutionCursor {
        entry: spellwake_engine::stack_window::StackEntryId,
        instruction: usize,
    },
    QualificationPrepareCreatureFields {
        counters: &'a [(
            spellwake_engine::object_identity::ObjectId,
            spellwake_engine::characteristics::PowerToughnessCounters,
        )],
        marked: &'a [(
            spellwake_engine::object_identity::ObjectId,
            spellwake_contracts::RulesInteger,
        )],
    },
    QualificationPrepareContextFields {
        life: &'a [(SeatId, spellwake_contracts::RulesInteger)],
        combat: Option<(spellwake_engine::object_identity::ObjectId, SeatId)>,
    },
    CreatureTypeChoice,
    QualificationPrepareEmptyCombat,
    QualificationPruneCreatureState,
    Pass {
        expected: u64,
        actor: SeatId,
    },
    BeginResolution {
        expected: u64,
    },
    BeginChecks {
        expected: u64,
    },
    SettleChecks {
        expected: u64,
    },
    ResolveTop {
        expected: u64,
    },
    Cast {
        expected: u64,
        actor: SeatId,
        spell: spellwake_engine::object_identity::ObjectId,
        target: TargetRef,
    },
    CastWithPayment {
        expected: u64,
        actor: SeatId,
        spell: spellwake_engine::object_identity::ObjectId,
        target: TargetRef,
        payment: Option<&'a ManaPayment>,
    },
    ActivateMana {
        expected: u64,
        actor: SeatId,
        source: spellwake_engine::object_identity::ObjectId,
        ability: spellwake_engine::mana::ManaAbilityRef,
    },
    ResumeRule {
        expected: u64,
        frame: &'a EffectDecision,
        actor: SeatId,
        reply: &'a DecisionReply,
        turn: Option<&'a TurnScheduler>,
    },
    DrawCards {
        expected: u64,
        player: SeatId,
        amount: &'a spellwake_contracts::RulesInteger,
    },
    EnterTurnStep {
        expected: u64,
        active: SeatId,
        priority: bool,
        empty_mana: bool,
    },
}
impl EffectCall<'_> {
    pub fn returns_rules_append(&self) -> bool {
        match self {
            Self::OpeningShuffle { .. }
            | Self::QualificationDiscard { .. }
            | Self::QualificationSetTapped { .. }
            | Self::UntapOnTurn { .. }
            | Self::CastOnTurn { .. }
            | Self::ActivateAbility { .. }
            | Self::QualificationZoneMoves { .. }
            | Self::BeginCleanup { .. }
            | Self::EnterCalendar { .. }
            | Self::BeginManaActivation { .. }
            | Self::AcceptCreatureType { .. }
            | Self::BeginDamageBatch { .. }
            | Self::QualificationBeginSharedDamage { .. }
            | Self::BeginResolution { .. }
            | Self::BeginChecks { .. }
            | Self::CastWithPayment { .. }
            | Self::ActivateMana { .. }
            | Self::ResumeRule { .. }
            | Self::DrawCards { .. }
            | Self::EnterTurnStep { .. } => true,
            Self::BeginOpening { .. }
            | Self::RecordTurnStart { .. }
            | Self::QualificationControllerSuccessor { .. }
            | Self::QualificationPrepareResolutionCursor { .. }
            | Self::QualificationPrepareCreatureFields { .. }
            | Self::QualificationPrepareContextFields { .. }
            | Self::CreatureTypeChoice
            | Self::QualificationPrepareEmptyCombat
            | Self::QualificationPruneCreatureState
            | Self::Pass { .. }
            | Self::SettleChecks { .. }
            | Self::ResolveTop { .. }
            | Self::Cast { .. } => false,
        }
    }
}
pub enum GameObjectsCall<'a> {
    SetTappedBatch {
        expected: u64,
        objects: &'a [spellwake_engine::object_identity::ObjectId],
        tapped: bool,
    },
    ApplyZoneMoves {
        expected: u64,
        moves: &'a [(
            spellwake_engine::object_identity::ObjectId,
            spellwake_engine::object_identity::ObjectDestination,
        )],
    },
    ShuffleLibrary {
        expected: u64,
        owner: SeatId,
        entropy: &'a spellwake_engine::shuffle::EntropyTape,
        entropy_words: &'a [u64],
    },
    RecordTurnStart {
        expected: u64,
        player: SeatId,
    },
    SeedCard {
        expected: u64,
        card: spellwake_engine::object_identity::CardId,
        owner: SeatId,
        destination: spellwake_engine::object_identity::ObjectDestination,
    },
    ApplyZoneMove {
        expected: u64,
        object: spellwake_engine::object_identity::ObjectId,
        destination: spellwake_engine::object_identity::ObjectDestination,
    },
    SetTapped {
        expected: u64,
        object: spellwake_engine::object_identity::ObjectId,
        tapped: bool,
    },
    ApplyController {
        expected: u64,
        object: spellwake_engine::object_identity::ObjectId,
        controller: SeatId,
    },
    ReorderLibrary {
        expected: u64,
        owner: SeatId,
        order: &'a [spellwake_engine::object_identity::ObjectId],
    },
    DrawCards {
        expected: u64,
        player: SeatId,
        amount: &'a spellwake_contracts::RulesInteger,
    },
}
pub enum StackWindowCall<'a> {
    RecordCardSpell {
        expected: u64,
        actor: SeatId,
        card: spellwake_engine::object_identity::ObjectId,
    },
    RecordResolutionMove {
        expected: u64,
        card: spellwake_engine::object_identity::ObjectId,
        destination: spellwake_engine::object_identity::ObjectDestination,
    },
    RecordOrderedTriggersWithReceipt {
        expected: u64,
        triggers: &'a [spellwake_engine::stack_window::OrderedTrigger],
    },
    FinishChecks {
        expected: u64,
    },
    Pass {
        expected: u64,
        actor: SeatId,
    },
    CheckResolutionTargets {
        expected: u64,
        entry: spellwake_engine::stack_window::StackEntryId,
        checks: &'a [spellwake_engine::targets::TargetCheck],
    },
    FinishResolution {
        expected: u64,
        entry: spellwake_engine::stack_window::StackEntryId,
        exit: Option<spellwake_engine::object_identity::ObjectDestination>,
    },
    RecordTapAction {
        expected: u64,
        actor: SeatId,
        source: spellwake_engine::object_identity::ObjectId,
    },
}
pub enum TurnCall {
    New {
        start: spellwake_engine::turn::TurnStart,
        skip_first_draw: bool,
    },
    CompleteWork(spellwake_engine::turn::WorkCompleted),
    CompletePriority(spellwake_engine::turn::PriorityCompleted),
}
pub fn work_completed(value: WorkCompleted) -> Value {
    match value {
        WorkCompleted::Untap => json!({"tag":"Untap"}),
        WorkCompleted::Draw => json!({"tag":"Draw"}),
        WorkCompleted::DeclareAttackers => json!({"tag":"DeclareAttackers"}),
        WorkCompleted::DeclareBlockers => json!({"tag":"DeclareBlockers"}),
        WorkCompleted::CombatDamage {
            additional_step_required,
        } => json!({"tag":"CombatDamage","additional_step_required":additional_step_required}),
        WorkCompleted::Cleanup { priority_required } => {
            json!({"tag":"Cleanup","priority_required":priority_required})
        }
    }
}
pub fn priority_completed(value: spellwake_engine::turn::PriorityCompleted) -> Value {
    use spellwake_engine::turn::PriorityCompleted;
    match value {
        PriorityCompleted::Ordinary => json!({"tag":"Ordinary"}),
        PriorityCompleted::Attackers {
            declared_or_put_attacking,
        } => json!({"tag":"Attackers","declared_or_put_attacking":declared_or_put_attacking}),
    }
}
pub fn seed_destination(value: spellwake_engine::object_identity::ObjectDestination) -> Value {
    use spellwake_engine::object_identity::ObjectDestination;
    match value {
        ObjectDestination::LibraryTop => json!({"tag":"LibraryTop"}),
        ObjectDestination::LibraryBottom => json!({"tag":"LibraryBottom"}),
        ObjectDestination::Hand => json!({"tag":"Hand"}),
        ObjectDestination::Graveyard => json!({"tag":"Graveyard"}),
        ObjectDestination::Battlefield { controller } => {
            json!({"tag":"Battlefield","controller":controller.0})
        }
        ObjectDestination::Stack { controller } => json!({"tag":"Stack","controller":controller.0}),
        ObjectDestination::Exile => json!({"tag":"Exile"}),
        ObjectDestination::Command => json!({"tag":"Command"}),
    }
}
pub fn game_seed_input(value: &spellwake_engine::setup::GameSeed) -> Value {
    json!({"seats":value.seats.iter().map(|seat|seat.0).collect::<Vec<_>>(),"active":value.active.0,"combat_rules":opaque(value.combat_rules),"cards":value.cards.iter().map(|card|json!({"card":card.card.0.to_string(),"owner":card.owner.0,"destination":seed_destination(card.destination),"program":program::view(&card.program)})).collect::<Vec<_>>(),"initial_world_cohorts":value.initial_world_cohorts.iter().map(|cohort|cohort.iter().map(|card|card.0.to_string()).collect::<Vec<_>>()).collect::<Vec<_>>(),"initial_battlefield_order":value.initial_battlefield_order.iter().map(|card|card.0.to_string()).collect::<Vec<_>>(),"mana":value.mana.iter().map(|row|json!({"player":row.player.0,"mana_type":opaque(row.mana_type),"amount":row.amount})).collect::<Vec<_>>(),"life":value.life.iter().map(|row|json!({"player":row.player.0,"total":row.total})).collect::<Vec<_>>(),"creature_vocabulary":value.creature_vocabulary.id().to_string(),"name_rules":value.name_rules.groups(),"name_rules_id":value.name_rules.id().to_string()})
}
impl QualificationCall<'_> {
    pub fn owner_method(&self) -> &str {
        match self {
            Self::GameInput(_) => "GameState::advance",
            Self::GameRelatedPlayerCauseFault { .. } => {
                "GameState::qualification_with_related_player_cause_fault"
            }
            Self::GameSupplyEntropy { .. } => "GameState::supply_entropy",
            Self::GameInitialize { .. } => "GameState::initialize",
            Self::GameInitializeScheduled { .. } => "GameState::initialize_scheduled",
            Self::GameInitializeOpening { .. } => "GameState::initialize_opening",
            Self::EffectInitialize(_) => "EffectWindow::new",
            Self::EffectCall(value) => effect_method(value),
            Self::GameObjectsInitialize { .. } => "GameObjects::new",
            Self::GameObjectsCall(value) => value.owner_method(),
            Self::StackWindowCall(value) => value.owner_method(),
            Self::TurnCall(value) => value.owner_method(),
            Self::ProgramValidate { .. } => "Program::validate",
            Self::PureCall { method, .. } => method,
        }
    }
    pub fn owner_name(&self) -> &'static str {
        match self {
            Self::GameInput(_)
            | Self::GameSupplyEntropy { .. }
            | Self::GameRelatedPlayerCauseFault { .. } => "GameState",
            Self::GameInitialize { .. }
            | Self::GameInitializeScheduled { .. }
            | Self::GameInitializeOpening { .. }
            | Self::EffectInitialize(_) => "GameSeedInput",
            Self::EffectCall(_) => "EffectWindow",
            Self::GameObjectsInitialize { .. } | Self::TurnCall(TurnCall::New { .. }) => {
                "PureValue"
            }
            Self::GameObjectsCall(_) => "GameObjects",
            Self::StackWindowCall(_) => "StackWindow",
            Self::TurnCall(_) => "TurnScheduler",
            Self::ProgramValidate { .. } | Self::PureCall { .. } => "PureValue",
        }
    }
    pub fn result_owner_name(&self) -> &'static str {
        match self {
            Self::GameObjectsInitialize { .. } => "GameObjects",
            Self::TurnCall(TurnCall::New { .. }) => "TurnScheduler",
            _ => self.owner_name(),
        }
    }
    pub fn is_pure(&self) -> bool {
        matches!(
            self,
            Self::PureCall { .. }
                | Self::ProgramValidate { .. }
                | Self::GameObjectsInitialize { .. }
                | Self::TurnCall(TurnCall::New { .. })
        )
    }
    pub fn returns_rules_append(&self) -> bool {
        match self {
            Self::GameInput(_)
            | Self::GameSupplyEntropy { .. }
            | Self::GameInitialize { .. }
            | Self::GameInitializeScheduled { .. }
            | Self::GameInitializeOpening { .. } => true,
            Self::EffectCall(value) => value.returns_rules_append(),
            Self::EffectInitialize(_)
            | Self::GameRelatedPlayerCauseFault { .. }
            | Self::GameObjectsInitialize { .. }
            | Self::GameObjectsCall(_)
            | Self::StackWindowCall(_)
            | Self::TurnCall(_)
            | Self::ProgramValidate { .. }
            | Self::PureCall { .. } => false,
        }
    }
    pub fn is_initializer(&self) -> bool {
        matches!(
            self,
            Self::GameInitialize { .. }
                | Self::GameInitializeScheduled { .. }
                | Self::GameInitializeOpening { .. }
                | Self::EffectInitialize(_)
        )
    }
}
/// Serialize exact supplied typed operands, including consuming constructor inputs.
pub fn qualification_call(
    value: &QualificationCall<'_>,
    _aliases: &AliasContext,
    execution: Option<&EffectWindow>,
) -> Result<Value, String> {
    Ok(match value {
        QualificationCall::GameSupplyEntropy {
            basis: value,
            words,
        } => {
            json!({"tag":"GameSupplyEntropy","basis":basis(value),"words":words.iter().map(u64::to_string).collect::<Vec<_>>()})
        }
        QualificationCall::GameRelatedPlayerCauseFault { fault } => {
            json!({"tag":"GameRelatedPlayerCauseFault","fault":related_player_cause_fault(*fault)})
        }
        QualificationCall::GameInput(value) => json!({"tag":"GameInput","value":input(value)}),
        QualificationCall::GameInitialize {
            game,
            context: ctx,
            seed,
        } => {
            json!({"tag":"GameInitialize","game":game.0.to_string(),"context":context(ctx),"seed":game_seed_input(seed)})
        }
        QualificationCall::GameInitializeScheduled {
            game,
            context: ctx,
            seed,
            start,
        } => {
            json!({"tag":"GameInitializeScheduled","game":game.0.to_string(),"context":context(ctx),"seed":game_seed_input(seed),"start":{"number":start.number.to_string(),"step":opaque(start.step)}})
        }
        QualificationCall::GameInitializeOpening {
            game,
            context: ctx,
            setup,
            entropy_words,
        } => {
            if setup.entropy != spellwake_engine::shuffle::EntropyTape::new(entropy_words.to_vec())
            {
                return Err("opening words differ from actual supplied entropy tape".into());
            }
            json!({"tag":"GameInitializeOpening","game":game.0.to_string(),"context":context(ctx),"seed":game_seed_input(&setup.seed),"entropy_words":entropy_words.iter().map(u64::to_string).collect::<Vec<_>>()})
        }
        QualificationCall::EffectInitialize(seed) => {
            json!({"tag":"EffectInitialize","seed":game_seed_input(seed)})
        }
        QualificationCall::EffectCall(value) => {
            let operands = match value {
                EffectCall::BeginOpening { expected } => json!({"expected":expected.to_string()}),
                EffectCall::OpeningShuffle {
                    expected,
                    player,
                    entropy,
                    entropy_words,
                } => {
                    if **entropy
                        != spellwake_engine::shuffle::EntropyTape::new(entropy_words.to_vec())
                    {
                        return Err(
                            "opening shuffle words differ from actual supplied fresh entropy tape"
                                .into(),
                        );
                    }
                    json!({"expected":expected.to_string(),"player":player.0,"entropy_words":entropy_words.iter().map(u64::to_string).collect::<Vec<_>>()})
                }
                EffectCall::QualificationDiscard {
                    expected,
                    player,
                    object,
                } => {
                    json!({"expected":expected.to_string(),"player":player.0,"object":opaque(object)})
                }
                EffectCall::QualificationSetTapped {
                    expected,
                    objects,
                    tapped,
                } => {
                    json!({"expected":expected.to_string(),"objects":objects.iter().map(opaque).collect::<Vec<_>>(),"tapped":tapped})
                }
                EffectCall::RecordTurnStart { expected } => {
                    json!({"expected":expected.to_string()})
                }
                EffectCall::UntapOnTurn { expected, turn } => {
                    json!({"expected":expected.to_string(),"turn":turn_scheduler(turn)})
                }
                EffectCall::CastOnTurn {
                    expected,
                    actor,
                    spell,
                    target: chosen,
                    payment,
                    turn,
                } => {
                    json!({"expected":expected.to_string(),"actor":actor.0,"spell":opaque(spell),"target":chosen.map(target),"payment":payment.map(spend),"turn":turn.map(turn_scheduler)})
                }
                EffectCall::ActivateAbility {
                    expected,
                    actor,
                    source,
                    ability,
                    target: chosen,
                    payment,
                } => {
                    json!({"expected":expected.to_string(),"actor":actor.0,"source":opaque(source),"ability":ability.0,"target":target(*chosen),"payment":payment.map(spend)})
                }
                EffectCall::QualificationZoneMoves { expected, moves } => {
                    json!({"expected":expected.to_string(),"moves":moves.iter().map(|(object,destination)|json!({"object":opaque(object),"destination":seed_destination(*destination)})).collect::<Vec<_>>()})
                }
                EffectCall::QualificationControllerSuccessor { object, controller } => {
                    json!({"object":opaque(object),"controller":controller.0})
                }
                EffectCall::BeginCleanup { expected, turn } => {
                    json!({"expected":expected.to_string(),"turn":turn_scheduler(turn)})
                }
                EffectCall::EnterCalendar {
                    expected,
                    previous,
                    next_turn,
                    active,
                } => {
                    json!({"expected":expected.to_string(),"previous":previous.map(turn_scheduler),"next_turn":turn_scheduler(next_turn),"active":active.0})
                }
                EffectCall::BeginManaActivation {
                    expected,
                    actor,
                    source,
                    ability,
                } => {
                    json!({"expected":expected.to_string(),"actor":actor.0,"source":opaque(source),"ability":mana_ref::view(*ability)})
                }
                EffectCall::AcceptCreatureType {
                    expected,
                    frame,
                    actor,
                    value,
                } => {
                    json!({"expected":expected.to_string(),"frame":creature_type_frame(frame),"actor":actor.0,"value":creature_type_id(*value)})
                }
                EffectCall::BeginDamageBatch {
                    origin,
                    kind,
                    inputs,
                    damage,
                    draws: drawn,
                    declarations: declared,
                    assignments: assigned,
                } => {
                    json!({"origin":damage_trace::damage_origin_view(*origin),"kind":match kind{spellwake_engine::damage::DamageKind::Combat=>"Combat",spellwake_engine::damage::DamageKind::Noncombat=>"Noncombat"},"inputs":inputs.iter().map(|input|json!({"source":damage_trace::damage_source_view(&input.source),"target":damage_trace::damage_recipient_view(input.target),"amount":input.amount})).collect::<Vec<_>>(),"damage":damage_trace::damage_journal_view(damage),"draws":draws(drawn),"declarations":declarations(declared),"assignments":assignments(assigned)})
                }
                EffectCall::QualificationBeginSharedDamage {
                    entry,
                    instruction,
                    components,
                } => {
                    json!({"entry":opaque(entry),"instruction":instruction.to_string(),"components":components.iter().map(|row|json!({"source":opaque(row.source),"recipient":damage_trace::damage_recipient_view(row.recipient),"amount":row.amount})).collect::<Vec<_>>()})
                }
                EffectCall::QualificationPrepareResolutionCursor { entry, instruction } => {
                    json!({"entry":opaque(entry),"instruction":instruction.to_string()})
                }
                EffectCall::QualificationPrepareCreatureFields { counters, marked } => {
                    json!({"counters":counters.iter().map(|(object,value)|json!({"object":opaque(object),"value":program::counters(value)})).collect::<Vec<_>>(),"marked":marked.iter().map(|(object,value)|json!({"object":opaque(object),"value":value})).collect::<Vec<_>>()})
                }
                EffectCall::QualificationPrepareContextFields { life, combat } => {
                    json!({"life":life.iter().map(|(seat,total)|json!({"player":seat.0,"total":total})).collect::<Vec<_>>(),"combat":combat.map(|(object,seat)|json!({"attacker":opaque(object),"defender":seat.0}))})
                }
                EffectCall::CreatureTypeChoice => json!({}),
                EffectCall::QualificationPrepareEmptyCombat => json!({}),
                EffectCall::QualificationPruneCreatureState => json!({}),
                EffectCall::Pass { expected, actor } => {
                    json!({"expected":expected.to_string(),"actor":actor.0})
                }
                EffectCall::BeginResolution { expected }
                | EffectCall::BeginChecks { expected }
                | EffectCall::SettleChecks { expected }
                | EffectCall::ResolveTop { expected } => json!({"expected":expected.to_string()}),
                EffectCall::Cast {
                    expected,
                    actor,
                    spell,
                    target: chosen,
                } => {
                    json!({"expected":expected.to_string(),"actor":actor.0,"spell":opaque(spell),"target":target(*chosen)})
                }
                EffectCall::CastWithPayment {
                    expected,
                    actor,
                    spell,
                    target: chosen,
                    payment,
                } => {
                    json!({"expected":expected.to_string(),"actor":actor.0,"spell":opaque(spell),"target":target(*chosen),"payment":payment.map(spend)})
                }
                EffectCall::ActivateMana {
                    expected,
                    actor,
                    source,
                    ability,
                } => {
                    json!({"expected":expected.to_string(),"actor":actor.0,"source":opaque(source),"ability":mana_ref::view(*ability)})
                }
                EffectCall::ResumeRule {
                    expected,
                    frame,
                    actor,
                    reply: chosen,
                    turn,
                } => {
                    json!({"expected":expected.to_string(),"frame":effect_frame(frame,execution.ok_or("resume_rule projection requires actual source executor")?)?,"actor":actor.0,"reply":reply(chosen),"turn":turn.map(turn_scheduler)})
                }
                EffectCall::DrawCards {
                    expected,
                    player,
                    amount,
                } => json!({"expected":expected.to_string(),"player":player.0,"amount":amount}),
                EffectCall::EnterTurnStep {
                    expected,
                    active,
                    priority,
                    empty_mana,
                } => {
                    json!({"expected":expected.to_string(),"active":active.0,"priority":priority,"empty_mana":empty_mana})
                }
            };
            json!({"tag":"EffectCall","method":effect_method(value),"operands":operands})
        }
        QualificationCall::GameObjectsInitialize { revision } => {
            json!({"tag":"GameObjectsCall","method":"GameObjects::new","operands":{"revision":revision.to_string()}})
        }
        QualificationCall::GameObjectsCall(value) => {
            let operands = match value {
                GameObjectsCall::SetTappedBatch {
                    expected,
                    objects,
                    tapped,
                } => {
                    json!({"expected":expected.to_string(),"objects":objects.iter().map(opaque).collect::<Vec<_>>(),"tapped":tapped})
                }
                GameObjectsCall::ApplyZoneMoves { expected, moves } => {
                    json!({"expected":expected.to_string(),"moves":moves.iter().map(|(object,destination)|json!({"object":opaque(object),"destination":seed_destination(*destination)})).collect::<Vec<_>>()})
                }
                GameObjectsCall::ShuffleLibrary {
                    expected,
                    owner,
                    entropy,
                    entropy_words,
                } => {
                    if **entropy
                        != spellwake_engine::shuffle::EntropyTape::new(entropy_words.to_vec())
                    {
                        return Err(
                            "shuffle words differ from actual supplied fresh entropy tape".into(),
                        );
                    }
                    json!({"expected":expected.to_string(),"owner":owner.0,"entropy_words":entropy_words.iter().map(u64::to_string).collect::<Vec<_>>()})
                }
                GameObjectsCall::RecordTurnStart { expected, player } => {
                    json!({"expected":expected.to_string(),"player":player.0})
                }
                GameObjectsCall::SeedCard {
                    expected,
                    card,
                    owner,
                    destination,
                } => {
                    json!({"expected":expected.to_string(),"card":card.0.to_string(),"owner":owner.0,"destination":seed_destination(*destination)})
                }
                GameObjectsCall::ApplyZoneMove {
                    expected,
                    object,
                    destination,
                } => {
                    json!({"expected":expected.to_string(),"object":opaque(object),"destination":seed_destination(*destination)})
                }
                GameObjectsCall::SetTapped {
                    expected,
                    object,
                    tapped,
                } => {
                    json!({"expected":expected.to_string(),"object":opaque(object),"tapped":tapped})
                }
                GameObjectsCall::ApplyController {
                    expected,
                    object,
                    controller,
                } => {
                    json!({"expected":expected.to_string(),"object":opaque(object),"controller":controller.0})
                }
                GameObjectsCall::ReorderLibrary {
                    expected,
                    owner,
                    order,
                } => {
                    json!({"expected":expected.to_string(),"owner":owner.0,"order":order.iter().map(opaque).collect::<Vec<_>>()})
                }
                GameObjectsCall::DrawCards {
                    expected,
                    player,
                    amount,
                } => json!({"expected":expected.to_string(),"player":player.0,"amount":amount}),
            };
            json!({"tag":"GameObjectsCall","method":value.owner_method(),"operands":operands})
        }
        QualificationCall::StackWindowCall(value) => {
            let operands = match value {
                StackWindowCall::RecordCardSpell {
                    expected,
                    actor,
                    card,
                } => json!({"expected":expected.to_string(),"actor":actor.0,"card":opaque(card)}),
                StackWindowCall::RecordResolutionMove {
                    expected,
                    card,
                    destination,
                } => {
                    json!({"expected":expected.to_string(),"card":opaque(card),"destination":seed_destination(*destination)})
                }
                StackWindowCall::RecordOrderedTriggersWithReceipt { expected, triggers } => {
                    json!({"expected":expected.to_string(),"triggers":triggers.iter().map(|trigger|json!({"controller":trigger.controller.0,"source":object(&trigger.source)})).collect::<Vec<_>>()})
                }
                StackWindowCall::FinishChecks { expected } => {
                    json!({"expected":expected.to_string()})
                }
                StackWindowCall::CheckResolutionTargets {
                    expected,
                    entry,
                    checks,
                } => {
                    json!({"expected":expected.to_string(),"entry":opaque(entry),"checks":checks.iter().map(|check|json!({"target":opaque(check.target),"legal":check.legal})).collect::<Vec<_>>()})
                }
                StackWindowCall::RecordTapAction {
                    expected,
                    actor,
                    source,
                } => {
                    json!({"expected":expected.to_string(),"actor":actor.0,"source":opaque(source)})
                }
                StackWindowCall::Pass { expected, actor } => {
                    json!({"expected":expected.to_string(),"actor":actor.0})
                }
                StackWindowCall::FinishResolution {
                    expected,
                    entry,
                    exit,
                } => {
                    json!({"expected":expected.to_string(),"entry":opaque(entry),"exit":exit.map(seed_destination)})
                }
            };
            json!({"tag":"StackWindowCall","method":value.owner_method(),"operands":operands})
        }
        QualificationCall::TurnCall(value) => {
            let operands = match value {
                TurnCall::New {
                    start,
                    skip_first_draw,
                } => {
                    json!({"start":{"number":start.number.to_string(),"step":opaque(start.step)},"skip_first_draw":skip_first_draw})
                }
                TurnCall::CompleteWork(result) => json!({"result":work_completed(*result)}),
                TurnCall::CompletePriority(result) => json!({"result":priority_completed(*result)}),
            };
            json!({"tag":"TurnCall","method":value.owner_method(),"operands":operands})
        }
        QualificationCall::ProgramValidate {
            program,
            vocabulary,
        } => {
            json!({"tag":"PureCall","method":"Program::validate","input":{"program":program::view(program),"creature_vocabulary":vocabulary.id().to_string()}})
        }
        QualificationCall::PureCall { method, input } => {
            if method.is_empty() {
                return Err("empty pure helper name".into());
            }
            json!({"tag":"PureCall","method":method,"input":input})
        }
    })
}
fn effect_method(value: &EffectCall<'_>) -> &'static str {
    match value {
        EffectCall::BeginOpening { .. } => "EffectWindow::begin_opening",
        EffectCall::OpeningShuffle { .. } => "EffectWindow::opening_shuffle",
        EffectCall::QualificationDiscard { .. } => "EffectWindow::qualification_discard",
        EffectCall::QualificationSetTapped { .. } => "EffectWindow::qualification_set_tapped",
        EffectCall::RecordTurnStart { .. } => "EffectWindow::record_turn_start",
        EffectCall::UntapOnTurn { .. } => "EffectWindow::untap_on_turn",
        EffectCall::CastOnTurn { .. } => "EffectWindow::cast_on_turn",
        EffectCall::ActivateAbility { .. } => "EffectWindow::activate_ability",
        EffectCall::QualificationZoneMoves { .. } => "EffectWindow::qualification_zone_moves",
        EffectCall::QualificationControllerSuccessor { .. } => {
            "EffectWindow::qualification_controller_successor"
        }
        EffectCall::BeginCleanup { .. } => "EffectWindow::begin_cleanup",
        EffectCall::EnterCalendar { .. } => "EffectWindow::enter_calendar",
        EffectCall::BeginManaActivation { .. } => "EffectWindow::begin_mana_activation",
        EffectCall::AcceptCreatureType { .. } => "EffectWindow::accept_creature_type",
        EffectCall::BeginDamageBatch { .. } => "EffectWindow::begin_damage_batch",
        EffectCall::QualificationBeginSharedDamage { .. } => {
            "EffectWindow::qualification_begin_shared_damage"
        }
        EffectCall::QualificationPrepareResolutionCursor { .. } => {
            "EffectWindow::qualification_prepare_resolution_cursor"
        }
        EffectCall::QualificationPrepareCreatureFields { .. } => {
            "EffectWindow::qualification_prepare_creature_fields"
        }
        EffectCall::QualificationPrepareContextFields { .. } => {
            "EffectWindow::qualification_prepare_context_fields"
        }
        EffectCall::CreatureTypeChoice => "EffectWindow::creature_type_choice",
        EffectCall::QualificationPrepareEmptyCombat => {
            "EffectWindow::qualification_prepare_empty_combat"
        }
        EffectCall::QualificationPruneCreatureState => {
            "EffectWindow::qualification_prune_non_battlefield_state"
        }
        EffectCall::Pass { .. } => "EffectWindow::pass",
        EffectCall::BeginResolution { .. } => "EffectWindow::begin_resolution",
        EffectCall::BeginChecks { .. } => "EffectWindow::begin_checks",
        EffectCall::SettleChecks { .. } => "EffectWindow::settle_checks",
        EffectCall::ResolveTop { .. } => "EffectWindow::resolve_top",
        EffectCall::CastWithPayment { .. } => "EffectWindow::cast_with_payment",
        EffectCall::Cast { .. } => "EffectWindow::cast",
        EffectCall::ActivateMana { .. } => "EffectWindow::activate_mana",
        EffectCall::ResumeRule { .. } => "EffectWindow::resume_rule",
        EffectCall::DrawCards { .. } => "EffectWindow::draw_cards",
        EffectCall::EnterTurnStep { .. } => "EffectWindow::enter_turn_step",
    }
}

impl GameObjectsCall<'_> {
    pub fn owner_method(&self) -> &'static str {
        match self {
            Self::SetTappedBatch { .. } => "GameObjects::set_tapped_batch",
            Self::ApplyZoneMoves { .. } => "GameObjects::apply_zone_moves",
            Self::ShuffleLibrary { .. } => "GameObjects::shuffle_library",
            Self::RecordTurnStart { .. } => "GameObjects::record_turn_start",
            Self::SeedCard { .. } => "GameObjects::seed_card",
            Self::ApplyZoneMove { .. } => "GameObjects::apply_zone_move",
            Self::SetTapped { .. } => "GameObjects::set_tapped",
            Self::ApplyController { .. } => "GameObjects::apply_controller",
            Self::ReorderLibrary { .. } => "GameObjects::reorder_library",
            Self::DrawCards { .. } => "GameObjects::draw_cards",
        }
    }
}
impl StackWindowCall<'_> {
    pub fn owner_method(&self) -> &'static str {
        match self {
            Self::RecordCardSpell { .. } => "StackWindow::record_card_spell",
            Self::RecordResolutionMove { .. } => "StackWindow::record_resolution_move",
            Self::RecordOrderedTriggersWithReceipt { .. } => {
                "StackWindow::record_ordered_triggers_with_receipt"
            }
            Self::FinishChecks { .. } => "StackWindow::finish_checks",
            Self::Pass { .. } => "StackWindow::pass",
            Self::CheckResolutionTargets { .. } => "StackWindow::check_resolution_targets",
            Self::FinishResolution { .. } => "StackWindow::finish_resolution",
            Self::RecordTapAction { .. } => "StackWindow::record_tap_action",
        }
    }
}
impl TurnCall {
    pub fn owner_method(&self) -> &'static str {
        match self {
            Self::New { .. } => "TurnScheduler::new",
            Self::CompleteWork(_) => "TurnScheduler::complete_work",
            Self::CompletePriority(_) => "TurnScheduler::complete_priority",
        }
    }
}

fn ability_roster(value: &PrintedAbilityRoster) -> Value {
    let mut result = json!({
        "source":opaque(value.source),
        "card_id":value.card.map(|card|card.0),
        "mana":value.mana.iter().map(|id|id.0).collect::<Vec<_>>(),
        "activated":value.activated.iter().map(|id|id.0).collect::<Vec<_>>(),
        "triggered":value.triggered.iter().map(|id|id.0).collect::<Vec<_>>(),
        "continuous":value.continuous.map(|id|id.0),
    });
    if !value.additional_battlefield_abilities.is_empty() {
        result["additional_battlefield_abilities"] = json!(
            value
                .additional_battlefield_abilities
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>()
        );
    }
    if value.combat_limits.one_attacker {
        result["one_attacker"] = json!(true);
    }
    if value.combat_limits.one_blocker {
        result["one_blocker"] = json!(true);
    }
    if let Some(requirement) = value.attack_requirement {
        result["attack_requirement"] = json!(format!("{requirement:?}"));
    }
    if !value.face_up.is_empty() {
        result["face_up"] = json!(value.face_up);
    }
    if !value.zone_replacements.is_empty() {
        result["zone_replacements"] = json!(
            value
                .zone_replacements
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>()
        );
    }
    result
}
fn granted_combat_keywords(
    grants: &BTreeMap<EffectInstanceKey, Vec<spellwake_engine::program::CombatTriggerKeyword>>,
) -> Value {
    json!(grants.iter().map(|(grant,keywords)|json!({"grant":effect_instance(*grant),"keywords":keywords.iter().map(program::combat_keyword).collect::<Vec<_>>()})).collect::<Vec<_>>())
}
fn granted_exalted(grants: &BTreeMap<EffectInstanceKey, u16>) -> Value {
    json!(
        grants
            .iter()
            .map(|(grant, count)| json!({"grant":effect_instance(*grant), "count":count}))
            .collect::<Vec<_>>()
    )
}
fn appearance(value: &SupportedAppearance) -> Value {
    let mut projected = json!({
        "object":object(&value.object),
        "copiable_card_types":value.copiable_card_types.iter().map(opaque).collect::<Vec<_>>(),
        "copiable_creature_subtypes":value.copiable_creature_subtypes.iter().map(|id|creature_type_id(*id)).collect::<Vec<_>>(),
        "copiable_land_subtypes":value.copiable_land_subtypes.iter().map(|kind|kind.wire_label()).collect::<Vec<_>>(),
        "copiable_names":value.copiable_names,
        "copiable_supertypes":value.copiable_supertypes.iter().map(opaque).collect::<Vec<_>>(),
        "current_names":value.current_names,
        "current_supertypes":value.current_supertypes.iter().map(opaque).collect::<Vec<_>>(),
        "types":value.types.iter().map(opaque).collect::<Vec<_>>(),
        "creature_subtypes":value.creature_subtypes.iter().map(|id|creature_type_id(*id)).collect::<Vec<_>>(),
        "land_subtypes":value.land_subtypes.iter().map(|kind|kind.wire_label()).collect::<Vec<_>>(),
        "intrinsic_mana":value.intrinsic_mana.iter().map(|kind|kind.land_type().wire_label()).collect::<Vec<_>>(),
        "colors":value.colors.iter().map(opaque).collect::<Vec<_>>(),
        "abilities":ability_roster(&value.abilities),
        "printed_text":program::printed_text(&value.printed_text),
        "keywords":value.keywords.iter().map(opaque).collect::<Vec<_>>(),
        "copiable_power_toughness":value.copiable_power_toughness.as_ref().map(program::printed_pt),
        "battlefield_counters":value.battlefield_counters.as_ref().map(program::counters),
        "battlefield_marked_damage":value.battlefield_marked_damage,
        "current_power_toughness":value.current_power_toughness.as_ref().map(program::pt),
    });
    if !value.copiable_spell_subtypes.is_empty() {
        projected["copiable_spell_subtypes"] = json!(
            value
                .copiable_spell_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.spell_subtypes.is_empty() {
        projected["spell_subtypes"] = json!(
            value
                .spell_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.copiable_artifact_subtypes.is_empty() {
        projected["copiable_artifact_subtypes"] = json!(
            value
                .copiable_artifact_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.copiable_enchantment_subtypes.is_empty() {
        projected["copiable_enchantment_subtypes"] = json!(
            value
                .copiable_enchantment_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.artifact_subtypes.is_empty() {
        projected["artifact_subtypes"] = json!(
            value
                .artifact_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.enchantment_subtypes.is_empty() {
        projected["enchantment_subtypes"] = json!(
            value
                .enchantment_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    for (label, count) in [
        ("time_counters", &value.time_counters),
        ("fade_counters", &value.fade_counters),
    ] {
        if let Some(count) = count {
            projected[label] = json!(count);
        }
    }
    if let Some(epoch) = value.previous_upkeep {
        projected["previous_upkeep"] = json!(opaque(epoch));
    }
    if value.renowned {
        projected["renowned"] = json!(true);
    }
    if let Some(amount) = &value.toxic_value {
        projected["toxic_value"] = json!(amount);
    }
    if !value.granted_triggers.is_empty() {
        projected["granted_triggers"] = json!(value.granted_triggers.iter().map(|(grant, ability)|
            json!({"grant":effect_instance(*grant),"ability":program::triggered_abilities(std::slice::from_ref(ability))[0]})).collect::<Vec<_>>());
    }
    if !value.granted_combat_keywords.is_empty() {
        projected["granted_combat_keywords"] =
            granted_combat_keywords(&value.granted_combat_keywords);
    }
    if value.saddled {
        projected["saddled"] = json!(true);
    }
    if let Some(host) = value.attached_to {
        projected["attached_to"] = target(host);
    }
    if value.granted_unblockable {
        projected["granted_unblockable"] = json!(true);
    }
    if !value.granted_exalted.is_empty() {
        projected["granted_exalted"] = granted_exalted(&value.granted_exalted);
    }
    projected
}
fn appearance_change(value: &CharacteristicAppearanceChange) -> Value {
    json!({"object":opaque(value.object),"before":appearance(&value.before),"after":appearance(&value.after)})
}
fn life_change(value: &LifeChange) -> Value {
    let mut projected = json!({"player":value.player.0,"kind":format!("{:?}",value.kind),"amount":value.amount,"before":value.before,"after":value.after});
    if let Some(origin) = &value.lifelink {
        projected["lifelink"] = json!({"batch":opaque(origin.batch),"source":damage_trace::source_id_view(origin.source),"attempts":origin.attempts.iter().map(opaque).collect::<Vec<_>>()});
    }
    projected
}
/// Shared captured cast payload for both event and related-cause projections.
pub fn cast_occurrence(value: &CastOccurrence) -> Value {
    let mut encoded = json!({"entry":opaque(value.entry()),"controller":value.controller().0,"spell":appearance(value.spell())});
    if value.spell().printed_text.triggered.iter().any(|t| {
        t.body
            .instructions
            .contains(&spellwake_engine::program::Instruction::Cascade)
    }) {
        encoded["mana_value"] = json!(value.mana_value());
    }
    if let Some(ordinal) = value.ordinal_this_turn() {
        encoded["ordinal_this_turn"] = json!(ordinal);
    }
    if let Some(spent) = value.spent_for_increment() {
        encoded["mana_spent"] = json!(spent);
    }
    encoded
}
fn related(value: &TriggerRelated) -> Value {
    match value {
        TriggerRelated::CombatDamageToPlaneswalker {
            planeswalker,
            attempt,
            ..
        } => {
            json!({"tag":"CombatDamageToPlaneswalker","planeswalker":opaque(planeswalker),"attempt":opaque(attempt)})
        }
        TriggerRelated::SubtypeCombatDamage {
            creature,
            player,
            attempt,
            ..
        } => {
            json!({"tag":"SubtypeCombatDamage","creature":opaque(creature),"player":player.0,"attempt":opaque(attempt)})
        }
        TriggerRelated::AttachedCreatureDamaged {
            creature, attempt, ..
        } => {
            json!({"tag":"AttachedCreatureDamaged","creature":opaque(creature),"attempt":opaque(attempt)})
        }
        TriggerRelated::DamageToOpponent {
            player, attempt, ..
        } => {
            json!({"tag":"DamageToOpponent","player":player.0,"attempt":opaque(attempt)})
        }
        TriggerRelated::DamageDealtToPermanent {
            recipient,
            attempt,
            amount,
        } => {
            json!({"tag":"DamageDealtToPermanent","recipient":opaque(recipient),"attempt":opaque(attempt),"amount":amount})
        }
        TriggerRelated::GraveyardReturnPaid { mana_value } => {
            json!({"tag":"GraveyardReturnPaid","mana_value":mana_value})
        }
        TriggerRelated::MiracleRevealed(object) => {
            json!({"tag":"MiracleRevealed","object":opaque(object)})
        }
        TriggerRelated::ReflexiveDiscardDamage { amount } => {
            json!({"tag":"ReflexiveDiscardDamage","amount":amount})
        }
        TriggerRelated::DamageReceived { attempt, amount } => {
            json!({"tag":"DamageReceived","attempt":opaque(attempt),"amount":amount})
        }
        TriggerRelated::CombatDamageToPlayer {
            player, attempt, ..
        } => {
            json!({"tag":"CombatDamageToPlayer","player":player.0,"attempt":opaque(attempt)})
        }
        TriggerRelated::DamageSource(source) => {
            use spellwake_engine::damage::DamageSourceId;
            let source = match source {
                DamageSourceId::Object(object) => json!({"tag":"Object","object":opaque(object)}),
                DamageSourceId::SpellCopy(entry) => {
                    json!({"tag":"SpellCopy","entry":opaque(entry)})
                }
                DamageSourceId::Emblem(emblem) => json!({"tag":"Emblem","emblem":opaque(emblem)}),
            };
            json!({"tag":"DamageSource","source":source})
        }
        TriggerRelated::TargetedPermanent(object) => {
            json!({"tag":"TargetedPermanent","object":opaque(object)})
        }
        TriggerRelated::TargetingEntry(entry) => {
            json!({"tag":"TargetingEntry","entry":opaque(entry)})
        }
        TriggerRelated::CardDrawn { player } => json!({"tag":"CardDrawn","player":player.0}),
        TriggerRelated::PlayerLost(player) => json!({"tag":"PlayerLost","player":player.0}),
        TriggerRelated::LeftGame(object) => json!({"tag":"LeftGame","object":opaque(object)}),
        TriggerRelated::FinalSagaChapter { saga, mana_value } => {
            json!({"tag":"FinalSagaChapter","saga":opaque(saga),"mana_value":mana_value})
        }
        TriggerRelated::SagaChapter { number } => json!({"tag":"SagaChapter","number":number}),
        TriggerRelated::StateTriggerSource(object) => {
            json!({"tag":"StateTriggerSource","object":opaque(object)})
        }
        TriggerRelated::Mutated(object) => json!({"tag":"Mutated","object":opaque(object)}),
        TriggerRelated::Phased(object) => json!({"tag":"Phased","object":opaque(object)}),
        TriggerRelated::CounterSource(object) => {
            json!({"tag":"CounterSource","object":opaque(object)})
        }
        TriggerRelated::AttackGroup {
            attacked,
            creatures,
        } => {
            json!({"tag":"AttackGroup","attacked":target(*attacked),"creatures":creatures.iter().map(opaque).collect::<Vec<_>>()})
        }
        TriggerRelated::ManaSource(object) => json!({"tag":"ManaSource","object":opaque(object)}),
        TriggerRelated::Combatant(object) => json!({"tag":"Combatant", "object":opaque(object)}),
        TriggerRelated::DeclaredSubtypeAttackers(attackers) => {
            json!({"tag":"DeclaredSubtypeAttackers","attackers":attackers.iter().map(|id|opaque(*id)).collect::<Vec<_>>()})
        }
        TriggerRelated::AttackedWithTotalPower(power) => {
            json!({"tag":"AttackedWithTotalPower","power":power})
        }
        TriggerRelated::AttackedAlone(attacker) => {
            json!({"tag":"AttackedAlone","attacker":opaque(attacker)})
        }
        TriggerRelated::LibraryAction(value) => {
            json!({"tag":"LibraryAction","action":library_action(value)})
        }
        TriggerRelated::AbilityForCopy(source) => {
            json!({"tag":"AbilityForCopy","original":opaque(source.original())})
        }
        TriggerRelated::SpellCopied(entry) => json!({"tag":"SpellCopied","entry":opaque(entry)}),
        TriggerRelated::SpellCast(value) => {
            json!({"tag":"SpellCast","cast":cast_occurrence(value)})
        }
        TriggerRelated::TokenCreated { creator, after } => {
            json!({"tag":"TokenCreated","creator":creator.0,"after":appearance(after)})
        }
        TriggerRelated::LifeChanged(change) => {
            json!({"tag":"LifeChanged","change":life_change(change)})
        }
        TriggerRelated::ZoneChange { before, after } => {
            json!({"tag":"ZoneChange","before":appearance(before),"after":appearance(after)})
        }
        TriggerRelated::StatusChange(change) => {
            json!({"tag":"StatusChange","before":object(&change.before),"after":object(&change.after)})
        }
        TriggerRelated::Discard {
            player,
            before,
            after,
        } => {
            json!({"tag":"Discard","player":player.0,"before":appearance(before),"after":appearance(after)})
        }
        TriggerRelated::Step { position, active } => {
            json!({"tag":"Step","position":damage_trace::turn_position_view(*position),"active":active.0})
        }
        TriggerRelated::ReflexiveTrigger => json!({"tag":"ReflexiveTrigger"}),
    }
}
fn occurrence_id(value: spellwake_engine::occurrence::OccurrenceId) -> Value {
    json!({"event":value.event.sequence().to_string(),"index":value.index.to_string()})
}
pub fn waiting(value: &WaitingInstance) -> Value {
    let mut projected = json!({"instance":value.id.sequence().to_string(),"definition":value.definition.0,"causes":trigger_causes(&value.causes),"source":appearance(&value.source),"controller":value.controller.0,"body":program::body(&value.body)});
    if let Some(grant) = value.granting_effect {
        projected["granting_effect"] = effect_instance(grant);
    }
    projected
}
fn binding(value: &TriggerBinding) -> Value {
    let mut projected = json!({"instance":value.instance.sequence().to_string(),"definition":value.definition.0,"causes":trigger_causes(&value.causes),"source":appearance(&value.source)});
    if let Some(grant) = value.granting_effect {
        projected["granting_effect"] = effect_instance(grant);
    }
    projected
}
fn trigger_causes(value: &spellwake_engine::occurrence::TriggerCauseSet) -> Value {
    use spellwake_engine::occurrence::TriggerCauseKey;
    let key = match value.key() {
        TriggerCauseKey::Occurrence(id) => {
            json!({"tag":"Occurrence","occurrence":occurrence_id(id)})
        }
        TriggerCauseKey::Together => json!({"tag":"Together"}),
        TriggerCauseKey::Player(player) => json!({"tag":"Player","player":player.0}),
    };
    json!({"event":value.event().sequence().to_string(),"key":key,"members":value.members().iter().map(|member|json!({"occurrence":occurrence_id(member.occurrence),"related_player":member.related_player.map(|player|player.0),"related":related(&member.related)})).collect::<Vec<_>>()})
}
fn copy(value: &SpellCopyObject) -> Value {
    json!({"entry":opaque(value.id),"owner":value.owner.0,"controller":value.controller.map(|seat|seat.0),"zone":opaque(value.zone)})
}
/// Observe every actual spell-copy object, including a copy awaiting cessation.
pub fn spell_copies(window: &StackWindow) -> Vec<Value> {
    window.spell_copies().values().map(copy).collect()
}
fn kind(value: &StackKind) -> Value {
    match value {
        StackKind::CardSpell(value) => json!({"tag":"CardSpell","object":opaque(value)}),
        StackKind::SpellCopy { copied_from, owner } => {
            json!({"tag":"SpellCopy","copied_from":opaque(copied_from),"owner":owner.0})
        }
        StackKind::Speed => json!({"tag":"Speed"}),
        StackKind::Monarch => json!({"tag":"Monarch"}),
        StackKind::DelayedCopy { original } => {
            json!({"tag":"DelayedCopy","original":opaque(original)})
        }
        StackKind::Emblem { emblem } => json!({"tag":"Emblem","emblem":emblem.sequence()}),
        StackKind::ActivatedAbility { source } => {
            json!({"tag":"ActivatedAbility","source":object(source)})
        }
        StackKind::TriggeredAbility { source } => {
            json!({"tag":"TriggeredAbility","source":object(source)})
        }
    }
}
fn cast_resource(resource: spellwake_engine::mana::CastResource) -> Value {
    use spellwake_engine::mana::CastResource;
    match resource {
        CastResource::Convoke { creature, color } => {
            json!({"tag":"Convoke","creature":opaque(creature),"color":color.map(opaque)})
        }
        CastResource::Improvise(artifact) => json!({"tag":"Improvise","artifact":opaque(artifact)}),
        CastResource::Retrace(card) => json!({"tag":"Retrace","card":opaque(card)}),
        CastResource::Emerge(card) => json!({"tag":"Emerge","card":opaque(card)}),
        CastResource::Sneak(card) => json!({"tag":"Sneak","card":opaque(card)}),
        CastResource::TapCreatureForSpell(card) => {
            json!({"tag":"TapCreatureForSpell","card":opaque(card)})
        }
        CastResource::TapForFlashback(card) => json!({"tag":"TapForFlashback","card":opaque(card)}),
        CastResource::ExileHandForAlternative(card) => {
            json!({"tag":"ExileHandForAlternative","card":opaque(card)})
        }
        CastResource::TapForAlternative(card) => {
            json!({"tag":"TapForAlternative","card":opaque(card)})
        }
        CastResource::WebSlinging(card) => json!({"tag":"WebSlinging","card":opaque(card)}),
        CastResource::Bargain(card) => json!({"tag":"Bargain","card":opaque(card)}),
        CastResource::RevealColorlessCreature(card) => {
            json!({"tag":"RevealColorlessCreature","card":opaque(card)})
        }
        CastResource::ExileGraveyardCardForSpell(card) => {
            json!({"tag":"ExileGraveyardCardForSpell","card":opaque(card)})
        }
        CastResource::ExileGraveyardCreatureForSpell(card) => {
            json!({"tag":"ExileGraveyardCreatureForSpell","card":opaque(card)})
        }
        CastResource::SacrificeForSpell(card) => {
            json!({"tag":"SacrificeForSpell","card":opaque(card)})
        }
        CastResource::DiscardForSpell(card) => json!({"tag":"DiscardForSpell","card":opaque(card)}),
        CastResource::JumpStart(card) => json!({"tag":"JumpStart","card":opaque(card)}),
        CastResource::Escape(card) => json!({"tag":"Escape","card":opaque(card)}),
        CastResource::ExileCreatureForReduction(card) => {
            json!({"tag":"ExileCreatureForReduction","card":opaque(card)})
        }
        CastResource::Delve(card) => json!({"tag":"Delve","card":opaque(card)}),
        CastResource::FlashbackSacrifice(object) => {
            json!({"tag":"FlashbackSacrifice","object":opaque(object)})
        }
    }
}
fn stack_entry(entry: &StackEntry) -> Value {
    let mut row = json!({"entry":opaque(entry.id()),"controller":entry.controller().0,"kind":kind(entry.kind()),"targets":entry.targets().targets().iter().map(|bound|json!({"id":opaque(bound.choice().id),"clause":bound.choice().clause.0,"target":target(bound.choice().target)})).collect::<Vec<_>>()});
    if let Some(seat) = entry.attacking_defender() {
        row["attacking_defender"] = json!(seat.0);
    }
    if let Some(original) = entry.ability_copy_of() {
        row["ability_copy_of"] = json!(opaque(original));
    }
    if !entry.power_contributors().is_empty() {
        row["power_contributors"] = json!(
            entry
                .power_contributors()
                .iter()
                .map(opaque)
                .collect::<Vec<_>>()
        );
    }
    if entry.bought_back() {
        row["buyback"] = json!(true);
    }
    if let Some(x) = entry.chosen_x() {
        row["chosen_x"] = json!(x);
    }
    if !entry.kicker_choices().is_empty() {
        row["kicker"] = json!(entry.kicker_choices());
    }
    if entry.paid_conditional_cost() {
        row["paid_conditional_cost"] = json!(true);
    }
    if entry.cast_with_jump_start() {
        row["jump_start"] = json!(true);
    }
    if entry.cast_with_flashback() {
        row["flashback"] = json!(true);
    }
    row
}
fn window_phase(value: WindowPhase) -> Value {
    match value {
        WindowPhase::Opening => json!({"tag":"Opening"}),
        WindowPhase::TurnWork => json!({"tag":"TurnWork"}),
        WindowPhase::Priority(value) => {
            let phase = match value {
                PriorityPhase::RequiresChecks { candidate } => {
                    json!({"tag":"RequiresChecks","candidate":candidate.0})
                }
                PriorityPhase::Ready { holder } => json!({"tag":"Ready","holder":holder.0}),
                PriorityPhase::AllPassed => json!({"tag":"AllPassed"}),
            };
            json!({"tag":"Priority","phase":phase})
        }
        WindowPhase::Resolving(entry) => json!({"tag":"Resolving","entry":opaque(entry)}),
        WindowPhase::CheckingTargets(entry) => {
            json!({"tag":"CheckingTargets","entry":opaque(entry)})
        }
        WindowPhase::AllTargetsIllegal(entry) => {
            json!({"tag":"AllTargetsIllegal","entry":opaque(entry)})
        }
        WindowPhase::StepEnded => json!({"tag":"StepEnded"}),
    }
}
/// Observe the actual standalone window, without executor bodies or root metadata.
pub fn stack_window(window: &StackWindow) -> Value {
    json!({"revision":window.revision().to_string(),"next_entry":window.next_entry_sequence().to_string(),"priority_revision":window.priority_revision().map(|value|value.to_string()),"phase":window_phase(window.phase()),"participants":window.participants().iter().map(|seat|seat.0).collect::<Vec<_>>(),"active":window.active_player().0,"cards":card_objects(window.cards(),window.participants()),"entries":window.entries().iter().map(stack_entry).collect::<Vec<_>>(),"copies":window.spell_copies().values().map(copy).collect::<Vec<_>>(),"resolving":window.resolving().map(stack_entry)})
}
pub(crate) fn continuous_effect(value: &ResolvedContinuousEffect) -> Value {
    json!({
        "id":opaque(value.id),
        "timestamp":value.timestamp.sequence().to_string(),
        "creator":opaque(value.creator),
        "affected":value.affected.iter().map(opaque).collect::<Vec<_>>(),
        "parts":program::resolved_parts(&value.parts),
        "duration":program::duration(value.duration),
    })
}
fn effect_instance(value: EffectInstanceKey) -> Value {
    match value {
        EffectInstanceKey::Emblem(id) => json!({"tag":"Emblem","id":id.sequence()}),
        EffectInstanceKey::Inherited {
            source,
            ability,
            donor,
            ordinal,
        } => {
            json!({"tag":"Inherited", "source":opaque(source), "ability":ability.0, "donor":opaque(donor), "ordinal":ordinal})
        }
        EffectInstanceKey::PrintedKeyword { source, keyword } => {
            json!({"tag": "PrintedKeyword", "source": opaque(source), "keyword": opaque(keyword)})
        }
        EffectInstanceKey::Resolved(id) => json!({"tag":"Resolved","id":opaque(id)}),
        EffectInstanceKey::Static { source, ability } => {
            json!({"tag":"Static","source":opaque(source),"ability":ability.0})
        }
    }
}
fn characteristic_bucket(value: CharacteristicBucket) -> &'static str {
    match value {
        CharacteristicBucket::Copy => "Copy",
        CharacteristicBucket::Control => "Control",
        CharacteristicBucket::Text => "Text",
        CharacteristicBucket::Type => "Type",
        CharacteristicBucket::Color => "Color",
        CharacteristicBucket::Abilities => "Abilities",
        CharacteristicBucket::DefinePT => "DefinePT",
        CharacteristicBucket::SetPT => "SetPT",
        CharacteristicBucket::ModifyPT => "ModifyPT",
        CharacteristicBucket::SwitchPT => "SwitchPT",
    }
}
fn pt_evaluation(value: &PowerToughnessEvaluation) -> Value {
    json!({
        "object":opaque(value.object),
        "copiable":value.copiable.as_ref().map(program::printed_pt),
        "base":program::pt(&value.base),
        "derived":program::pt(&value.derived),
        "applied_effects":value.applied_effects.iter().map(|key|effect_instance(*key)).collect::<Vec<_>>(),
    })
}
pub(crate) fn current_characteristics(value: &ObjectCharacteristics) -> Value {
    let mut projected = json!({
        "object":opaque(value.object),
        "zone":opaque(value.zone),
        "defined":{
            "names":value.defined.names,
            "supertypes":value.defined.supertypes.iter().map(opaque).collect::<Vec<_>>(),
            "types":value.defined.types.iter().map(opaque).collect::<Vec<_>>(),
            "creature_subtypes":value.defined.creature_subtypes.iter().map(|id|creature_type_id(*id)).collect::<Vec<_>>(),
            "land_subtypes":value.defined.land_subtypes.iter().map(|kind|kind.wire_label()).collect::<Vec<_>>(),
            "colors":value.defined.colors.iter().map(opaque).collect::<Vec<_>>(),
            "printed_pt":value.defined.printed_pt.as_ref().map(program::printed_pt),
        },
        "current_names":value.current_names,
        "current_supertypes":value.current_supertypes.iter().map(opaque).collect::<Vec<_>>(),
        "current_types":value.current_types.iter().map(opaque).collect::<Vec<_>>(),
        "current_creature_subtypes":value.current_creature_subtypes.iter().map(|id|creature_type_id(*id)).collect::<Vec<_>>(),
        "mana_abilities":value.mana_abilities().into_iter().map(mana_ref::view).collect::<Vec<_>>(),
        "current_land_subtypes":value.current_land_subtypes.iter().map(|kind|kind.wire_label()).collect::<Vec<_>>(),
        "intrinsic_mana":value.intrinsic_mana.iter().map(|kind|kind.land_type().wire_label()).collect::<Vec<_>>(),
        "current_colors":value.current_colors.iter().map(opaque).collect::<Vec<_>>(),
        "controller":value.controller.map(|seat|seat.0),
        "abilities":ability_roster(&value.abilities),
        "printed_text":program::printed_text(&value.printed_text),
        "keywords":value.keywords.iter().map(opaque).collect::<Vec<_>>(),
        "power_toughness":value.power_toughness.as_ref().map(pt_evaluation),
    });
    if !value.defined.spell_subtypes.is_empty() {
        projected["defined"]["spell_subtypes"] = json!(
            value
                .defined
                .spell_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.current_spell_subtypes.is_empty() {
        projected["current_spell_subtypes"] = json!(
            value
                .current_spell_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.defined.artifact_subtypes.is_empty() {
        projected["defined"]["artifact_subtypes"] = json!(
            value
                .defined
                .artifact_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.defined.enchantment_subtypes.is_empty() {
        projected["defined"]["enchantment_subtypes"] = json!(
            value
                .defined
                .enchantment_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.current_artifact_subtypes.is_empty() {
        projected["current_artifact_subtypes"] = json!(
            value
                .current_artifact_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if !value.current_enchantment_subtypes.is_empty() {
        projected["current_enchantment_subtypes"] = json!(
            value
                .current_enchantment_subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if value.saddled {
        projected["saddled"] = json!(true);
    }
    if let Some(host) = value.attached_to {
        projected["attached_to"] = target(host);
    }
    if let Some(amount) = &value.toxic_value {
        projected["toxic_value"] = json!(amount);
    }
    if !value.granted_triggers.is_empty() {
        projected["granted_triggers"] = json!(value.granted_triggers.iter().map(|(grant, ability)|
            json!({"grant":effect_instance(*grant),"ability":program::triggered_abilities(std::slice::from_ref(ability))[0]})).collect::<Vec<_>>());
    }
    if !value.granted_combat_keywords.is_empty() {
        projected["granted_combat_keywords"] =
            granted_combat_keywords(&value.granted_combat_keywords);
    }
    if value.granted_unblockable {
        projected["granted_unblockable"] = json!(true);
    }
    if !value.granted_exalted.is_empty() {
        projected["granted_exalted"] = granted_exalted(&value.granted_exalted);
    }
    projected
}
fn generated_operations(values: &BTreeMap<ObjectId, ResolvedPart>) -> Value {
    json!(
        values
            .iter()
            .map(|(object, part)| json!({
                "object":opaque(object),
                "parts":program::resolved_parts(&ResolvedParts(vec![part.clone()])),
            }))
            .collect::<Vec<_>>()
    )
}
fn generated_component(value: &GeneratedComponent) -> Value {
    json!({
        "key":effect_instance(value.key),
        "bucket":characteristic_bucket(value.bucket),
        "present":value.present,
        "affected":value.affected.iter().map(opaque).collect::<Vec<_>>(),
        "generated":generated_operations(&value.generated),
    })
}
fn component_wait(value: &ComponentWait) -> Value {
    json!({"waiting":effect_instance(value.waiting),"prerequisite":effect_instance(value.prerequisite)})
}
pub(crate) fn evaluation_trace(value: &EvaluationTrace) -> Value {
    json!({
        "selections":value.selections.iter().map(|selection|json!({
            "bucket":characteristic_bucket(selection.bucket),
            "generated":selection.generated.iter().map(generated_component).collect::<Vec<_>>(),
            "original_waits":selection.original_waits.iter().map(component_wait).collect::<Vec<_>>(),
            "strongly_connected":selection.strongly_connected.iter().map(|group|group.iter().map(|key|effect_instance(*key)).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "derived_timestamp_waits":selection.derived_timestamp_waits.iter().map(component_wait).collect::<Vec<_>>(),
            "retained_outside_waits":selection.retained_outside_waits.iter().map(component_wait).collect::<Vec<_>>(),
            "eligible":selection.eligible.iter().map(|key|effect_instance(*key)).collect::<Vec<_>>(),
            "chosen":effect_instance(selection.chosen),
        })).collect::<Vec<_>>(),
        "applied":value.applied.iter().map(|application|json!({
            "key":effect_instance(application.key),
            "bucket":characteristic_bucket(application.bucket),
            "affected":application.affected.iter().map(opaque).collect::<Vec<_>>(),
            "generated":generated_operations(&application.generated),
        })).collect::<Vec<_>>(),
        "counter_applications":value.counter_applications.iter().map(|application|json!({
            "object":opaque(application.object),
            "after_components":application.after_components,
            "counters":program::counters(&application.counters),
            "before":program::pt(&application.before),
            "after":program::pt(&application.after),
        })).collect::<Vec<_>>(),
        "bindings":value.bindings.iter().map(|(key,binding)|json!({
            "key":effect_instance(*key),
            "binding":match binding {
                PassBinding::NotProcessed => json!({"tag":"NotProcessed"}),
                PassBinding::Processed(affected) => json!({"tag":"Processed","affected":affected.iter().map(opaque).collect::<Vec<_>>()}),
            },
        })).collect::<Vec<_>>(),
    })
}
fn combatant(value: &Combatant) -> Value {
    json!({"object":opaque(value.object()),"controller":value.controller().0,"control_epoch":opaque(value.control_epoch())})
}
fn attacking_creature(row: &spellwake_engine::combat::AttackingCreature) -> Value {
    let mut value = json!({"object":opaque(row.combatant().object()),"controller":row.combatant().controller().0,"control_epoch":opaque(row.combatant().control_epoch()),"defender":row.defender().0,"block_status":opaque(row.block_status())});
    if let Some(id) = row.defending_planeswalker() {
        value["defending_planeswalker"] = json!(opaque(id));
    }
    value
}
fn blocking_creature(row: &spellwake_engine::combat::BlockingCreature) -> Value {
    let mut value = json!({"object":opaque(row.combatant().object()),"controller":row.combatant().controller().0,"control_epoch":opaque(row.combatant().control_epoch()),"attacker":row.attacker().map(opaque)});
    if row.attackers().len() > 1 {
        value["attackers"] = json!(row.attackers().iter().map(opaque).collect::<Vec<_>>());
    }
    value
}
fn combat(value: &CombatState) -> Value {
    let mut out = json!({"had_attackers":value.had_attackers(),"attackers":value.attackers().map(attacking_creature).collect::<Vec<_>>(),"blockers":value.blockers().map(blocking_creature).collect::<Vec<_>>()});
    let visible_defenders = value
        .attackers()
        .map(|row| row.defender())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    if value.attacked_player_count() > visible_defenders.max(1) {
        out["attacked_player_count"] = json!(value.attacked_player_count());
    }
    if !value.first_strike_participants().is_empty() {
        out["first_strike_participants"] = json!(value.first_strike_participants().values().map(|row| json!({"object":opaque(row.object()),"controller":row.controller().0,"control_epoch":opaque(row.control_epoch())})).collect::<Vec<_>>());
    }
    out
}

pub fn frame(value: &RuleFrame, state: &GameState) -> Result<Value, String> {
    frame_with_cards(value, state.execution().window().cards())
}
/// Reuse the native common-frame conversion without constructing a root or token.
pub fn effect_frame(value: &EffectDecision, execution: &EffectWindow) -> Result<Value, String> {
    frame_with_cards(&RuleFrame::from(value.clone()), execution.window().cards())
}
fn frame_with_cards(value: &RuleFrame, cards: &GameObjects) -> Result<Value, String> {
    Ok(match value {
        RuleFrame::ZoneReplacement(value) => {
            json!({"tag":"ZoneReplacement","schema":"zone-replacement-native-frame-v2","revision":value.revision().to_string(),"cause":cause(value.cause()),"stage":value.stage().to_string(),"chooser":value.chooser().0,"request":match value.request(){
                spellwake_engine::effects::ZoneReplacementRequest::Devour { entrant, choices } => json!({"tag":"Devour","entrant":opaque(entrant),"choices":choices.iter().map(opaque).collect::<Vec<_>>()}),
                spellwake_engine::effects::ZoneReplacementRequest::DevourGraveyardOrder{choices}=>json!({"tag":"DevourGraveyardOrder","choices":choices.iter().map(opaque).collect::<Vec<_>>()}),
                spellwake_engine::effects::ZoneReplacementRequest::EntryCopy {object,donors,battlefield_donors} => {
                    let mut value = json!({"tag":"EntryCopy","object":opaque(object),"donors":donors.iter().map(opaque).collect::<Vec<_>>() });
                    if !battlefield_donors.is_empty() {
                        value["auras"] = json!(battlefield_donors.iter().map(|(aura, host)| json!({"aura":opaque(aura),"host":opaque(host)})).collect::<Vec<_>>());
                    }
                    value
                },
                spellwake_engine::effects::ZoneReplacementRequest::EntryController {object,opponents}=>json!({"tag":"EntryController","object":opaque(object),"opponents":opponents.iter().map(|seat|seat.0).collect::<Vec<_>>()}),
                spellwake_engine::effects::ZoneReplacementRequest::Select{applications}=>json!({"tag":"Select","applications":applications.iter().map(|a|zone_application(*a)).collect::<Vec<_>>()}),
                spellwake_engine::effects::ZoneReplacementRequest::ConfirmOptional{application}=>json!({"tag":"ConfirmOptional","application":zone_application(*application)})
            }})
        }
        RuleFrame::LibraryTop(value) => {
            let mut frame = json!({"tag":"LibraryTop","kind":opaque(value.kind()),"revision":value.revision().to_string(),"entry":opaque(value.entry()),"instruction":value.instruction().to_string(),"chooser":value.chooser().0,"domain_top_first":value.domain_top_first().iter().map(opaque).collect::<Vec<_>>()});
            if value.kind() == spellwake_engine::effects::LibraryChoiceKind::Search {
                frame["eligible"] = json!(
                    value
                        .eligible_results()
                        .iter()
                        .map(opaque)
                        .collect::<Vec<_>>()
                );
            }
            frame
        }
        RuleFrame::ResolutionSelect(value) => json!({
            "tag":"ResolutionSelect","revision":value.revision().to_string(),
            "entry":opaque(value.entry()),"instruction":value.instruction().to_string(),
            "chooser":value.chooser().0,"operation":opaque(value.operation()),
            "domain":value.domain().iter().map(opaque).collect::<Vec<_>>(),
            "required":value.required().to_string()
        }),
        RuleFrame::ResolutionGraveyardOrder(value) => {
            let mut order = json!({
                "tag":"ResolutionGraveyardOrder","revision":value.revision().to_string(),
                "entry":opaque(value.entry()),"instruction":value.instruction().to_string(),
                "chooser":value.chooser().0,"domain":value.domain().iter().map(opaque).collect::<Vec<_>>()
            });
            if value.destination() == spellwake_engine::program::PublicZoneDestination::Library {
                order["destination"] = json!("Library");
            }
            order
        }
        RuleFrame::OptionalEffect(value) => {
            let mut encoded = json!({"tag":"OptionalEffect","revision":value.revision().to_string(),"entry":opaque(value.entry()),"instruction":value.instruction().to_string(),"chooser":value.chooser().0,"options":value.options().iter().map(opaque).collect::<Vec<_>>()});
            if value.rebound_or_buyback() {
                encoded["rebound_or_buyback"] = json!(true);
            }
            if let Some(offer) = value.exile_cast() {
                let key = match offer.ability {
                    spellwake_engine::effects::ExileCastAbility::Suspend => "suspended_cast",
                    spellwake_engine::effects::ExileCastAbility::Rebound => "rebound_cast",
                    spellwake_engine::effects::ExileCastAbility::Cascade => "cascade_cast",
                    spellwake_engine::effects::ExileCastAbility::Miracle => "impulse_cast",
                    spellwake_engine::effects::ExileCastAbility::Madness => "madness_cast",
                    spellwake_engine::effects::ExileCastAbility::Siege => "siege",
                    spellwake_engine::effects::ExileCastAbility::FoundSpell => "found_spell",
                    spellwake_engine::effects::ExileCastAbility::Commander => "commander",
                    spellwake_engine::effects::ExileCastAbility::TargetedGraveyard => {
                        "targeted_graveyard"
                    }
                    spellwake_engine::effects::ExileCastAbility::RevealedLibrary => {
                        "revealed_library"
                    }
                    spellwake_engine::effects::ExileCastAbility::Impulse
                    | spellwake_engine::effects::ExileCastAbility::MatchingHand => "impulse_cast",
                };
                fn face(offer: &spellwake_engine::effects::ExileCastOffer) -> Value {
                    let mut value = json!({"spell":opaque(offer.spell),"targets":offer.targets.as_ref().map(|choices|choices.iter().copied().map(target).collect::<Vec<_>>())});
                    if let Some(cost) = &offer.normal_price {
                        value["cost"] = program::cost(cost);
                    }
                    if matches!(
                        offer.ability,
                        spellwake_engine::effects::ExileCastAbility::Miracle
                            | spellwake_engine::effects::ExileCastAbility::Impulse
                            | spellwake_engine::effects::ExileCastAbility::RevealedLibrary
                            | spellwake_engine::effects::ExileCastAbility::Commander
                            | spellwake_engine::effects::ExileCastAbility::Siege
                            | spellwake_engine::effects::ExileCastAbility::FoundSpell
                            | spellwake_engine::effects::ExileCastAbility::TargetedGraveyard
                    ) {
                        value["allows_x"] = json!(offer.allows_x);
                        value["allows_alternative"] = json!(offer.allows_alternative);
                        if offer.alternative_targetless {
                            value["alternative_targetless"] = json!(true);
                        }
                        if let Some(price) = &offer.alternative_price {
                            value["alternative_price"] = program::cost(price);
                        }
                        if let Some(spec) = &offer.modes {
                            value["modes"] = json!({"minimum":spec.minimum,"maximum":spec.maximum,"allow_repeat":spec.allow_repeat,"modes":spec.modes.iter().map(|mode|json!({"index":mode.index,"targets":mode.targets.as_ref().map(|items|items.iter().copied().map(target).collect::<Vec<_>>())})).collect::<Vec<_>>()});
                        }
                        if !offer.hybrid_symbols.is_empty() {
                            value["hybrid_symbols"] = json!(
                                offer
                                    .hybrid_symbols
                                    .iter()
                                    .map(|s| [
                                        format!("{:?}", s.first),
                                        format!("{:?}", s.alternative)
                                    ])
                                    .collect::<Vec<_>>()
                            );
                        }
                        if let Some(spec) = &offer.fixed_targets {
                            value["fixed_targets"] = json!(
                                spec.iter()
                                    .map(|values| values
                                        .iter()
                                        .copied()
                                        .map(target)
                                        .collect::<Vec<_>>())
                                    .collect::<Vec<_>>()
                            );
                        }
                        if let Some(spec) = &offer.permanent_targets {
                            value["permanent_targets"] = json!({"minimum":spec.minimum,"maximum":spec.maximum,"targets":spec.targets.iter().copied().map(target).collect::<Vec<_>>()});
                        }
                    }
                    if let Some(cost) = &offer.madness_price {
                        value["cost"] = program::cost(cost);
                    }
                    value
                }
                encoded[key] = face(offer);
                if let Some(right) = &offer.split_right {
                    encoded[key]["normal_available"] = json!(offer.normal_available);
                    encoded[key]["split_right"] = face(right);
                }
                if let Some(alternate) = &offer.adventure {
                    encoded[key]["normal_available"] = json!(offer.normal_available);
                    encoded[key]["adventure"] = face(alternate);
                }
            }
            encoded
        }
        RuleFrame::ResolutionPayment(value) => {
            let mut encoded = json!({"tag":"ResolutionPayment","revision":value.revision().to_string(),"entry":opaque(value.entry()),"instruction":value.instruction().to_string(),"payer":value.payer().0,"cost":program::cost(value.cost())});
            if value.variable() {
                encoded["variable"] = json!(true);
            }
            if let Some(colors) = value.restricted_colors() {
                encoded["restricted_colors"] = json!(colors.iter().map(opaque).collect::<Vec<_>>());
            }
            encoded
        }
        RuleFrame::Checks(value) => {
            json!({"tag":"Checks","frame":check_trace::check_decision_view(value,cards,|card|card.0.to_string())?})
        }
        RuleFrame::TriggerTarget(value) => {
            json!({"tag":"TriggerTarget","frame":trigger_target_view(value)})
        }
        RuleFrame::Triggers(value) => {
            json!({"tag":"Triggers","frame":check_trace::trigger_order_view(value)})
        }
        RuleFrame::Opening(value) => match value {
            OpeningDecision::Declare {
                player,
                taken,
                can_mulligan,
            } => {
                json!({"tag":"Mulligan","chooser":player.0,"taken":taken,"can_mulligan":can_mulligan})
            }
            OpeningDecision::Bottom {
                player,
                count,
                hand,
            } => {
                json!({"tag":"Bottom","chooser":player.0,"count":count,"hand":hand.iter().map(opaque).collect::<Vec<_>>()})
            }
        },
        RuleFrame::CounterReplacement(frame) => json!({
            "tag":"CounterReplacement","chooser":frame.chooser().0,"object":opaque(frame.object()),
            "doublers":frame.doublers(),"plus_one":frame.original().plus_one,"minus_one":frame.original().minus_one,
        }),
        RuleFrame::TransformOrder(f) => {
            json!({"tag":"TransformOrder","chooser":f.chooser().0,"order":f.objects().iter().map(opaque).collect::<Vec<_>>()})
        }
        RuleFrame::LinkedReturnOrder(f) => {
            json!({"tag":"LinkedReturnOrder","chooser":f.chooser().0,"destination":opaque(f.destination()),"order":f.objects().iter().map(opaque).collect::<Vec<_>>()})
        }
        RuleFrame::LinkedAuraChoice(f) => {
            json!({"tag":"LinkedAuraChoice","chooser":f.chooser().0,"object":opaque(f.object()),"domain":f.domain().iter().copied().map(target).collect::<Vec<_>>()})
        }
        RuleFrame::PlaneswalkerType(value) => {
            json!({"tag":"PlaneswalkerType","chooser":value.chooser().0})
        }
        RuleFrame::AttackDirection(value) => {
            json!({"tag":"AttackDirection","chooser":value.chooser().0})
        }
        RuleFrame::MyriadDefender(value) => {
            json!({"tag":"MyriadDefender","chooser":value.chooser().0,"opponent":value.opponent().0,"may_decline":value.may_decline(),"defenders":value.defenders().map(target).collect::<Vec<_>>()})
        }
        RuleFrame::ReselectAttackDefender(value) => {
            json!({"tag":"ReselectAttackDefender","chooser":value.chooser().0,"attacker":opaque(value.attacker()),"defenders":value.defenders().map(target).collect::<Vec<_>>()})
        }
        RuleFrame::TokenDefender(value) => {
            json!({"tag":"TokenDefender","chooser":value.chooser().0,"member":value.member().ordinal(),"defenders":value.defenders().map(target).collect::<Vec<_>>()})
        }
        RuleFrame::TimestampOrder(value) => {
            json!({"tag":"TimestampOrder","revision":value.revision().to_string(),"entry":opaque(value.entry()),"instruction":value.instruction(),"chooser":value.chooser().0,"members":value.members().iter().map(|member|json!({"ordinal":member.member.ordinal(),"creator":member.creator.0,"definition":program::view(&member.definition)})).collect::<Vec<_>>()})
        }
        RuleFrame::CounterRemoval(frame) => {
            json!({"tag":"CounterRemoval","chooser":frame.chooser().0,"object":opaque(frame.object()),"maximum":frame.maximum(),"domain":frame.domain().iter().map(|(kind,count)|json!({"kind":format!("{kind:?}"),"count":count})).collect::<Vec<_>>()})
        }
        RuleFrame::Proliferate(frame) => {
            json!({"tag":"Proliferate","chooser":frame.chooser().0,"revision":frame.revision().to_string(),"domain":frame.domain().iter().map(|(who,kinds)|json!({"recipient":target(*who),"counters":kinds.iter().map(opaque).collect::<Vec<_>>()})).collect::<Vec<_>>()})
        }
        RuleFrame::DrawReplacement(value) => {
            json!({"tag":"DrawReplacement","revision":value.revision,"chooser":value.chooser.0,"domain":format!("{:?}",value.domain)})
        }
        RuleFrame::DamageRecipientChoice(value) => {
            json!({"tag":"DamageRecipientChoice","chooser":value.chooser().0,"recipients":value.recipients().iter().copied().map(target).collect::<Vec<_>>()})
        }
        RuleFrame::OpponentChoice(value) => {
            json!({"tag":"OpponentChoice","chooser":value.chooser().0,"opponents":value.opponents().iter().map(|s|s.0).collect::<Vec<_>>()})
        }
        RuleFrame::ReplacementChooser(value) => {
            json!({"tag":"ReplacementChooser","entry":opaque(value.entry()),"instruction":value.instruction(),"departed":value.departed().0,"chooser":value.chooser().0,"candidates":value.candidates().iter().map(|seat|seat.0).collect::<Vec<_>>()})
        }
        RuleFrame::Mutation(value) => {
            json!({"tag":"Mutation","chooser":value.chooser().0,"host":opaque(value.host()),"spell":opaque(value.spell())})
        }
        RuleFrame::RevealedPiles(value) => {
            use spellwake_engine::effects::PileChoice;
            let choice = match value.choice() {
                PileChoice::DividePermanents(cards) => {
                    json!({"kind":"DividePermanents","cards":cards.iter().map(opaque).collect::<Vec<_>>()})
                }
                PileChoice::Sacrifice { first, second } => {
                    json!({"kind":"Sacrifice","first":first.iter().map(opaque).collect::<Vec<_>>(),"second":second.iter().map(opaque).collect::<Vec<_>>()})
                }
                PileChoice::Divide(cards) => {
                    json!({"kind":"Divide","cards":cards.iter().map(opaque).collect::<Vec<_>>()})
                }
                PileChoice::Take { first, second } => {
                    json!({"kind":"Take","first":first.iter().map(opaque).collect::<Vec<_>>(),"second":second.iter().map(opaque).collect::<Vec<_>>()})
                }
                PileChoice::OneToHandRestBottom(cards) => {
                    json!({"kind":"OneToHandRestBottom","cards":cards.iter().map(opaque).collect::<Vec<_>>()})
                }
                PileChoice::OptionalCreatureToBattlefield { looked, eligible } => {
                    json!({"kind":"OptionalCreatureToBattlefield","looked":looked.iter().map(opaque).collect::<Vec<_>>(),"eligible":eligible.iter().map(opaque).collect::<Vec<_>>()})
                }
                PileChoice::OptionalUpTo {
                    looked,
                    eligible,
                    max,
                } => {
                    json!({"kind":"OptionalUpTo","looked":looked.iter().map(opaque).collect::<Vec<_>>(),"eligible":eligible.iter().map(opaque).collect::<Vec<_>>(),"max":max})
                }
                PileChoice::OneToHand(cards) => {
                    json!({"kind":"OneToHand","cards":cards.iter().map(opaque).collect::<Vec<_>>()})
                }
                PileChoice::Creature(cards) => {
                    json!({"kind":"Creature","cards":cards.iter().map(opaque).collect::<Vec<_>>()})
                }
                PileChoice::BottomOrder(cards) => {
                    json!({"kind":"BottomOrder","cards":cards.iter().map(opaque).collect::<Vec<_>>()})
                }
                PileChoice::GraveyardOrder(cards) => {
                    json!({"kind":"GraveyardOrder","cards":cards.iter().map(opaque).collect::<Vec<_>>()})
                }
            };
            json!({"tag":"RevealedPiles","chooser":value.chooser().0,"choice":choice})
        }
        RuleFrame::LoyaltyRemoval(value) => {
            json!({"tag":"LoyaltyRemoval","revision":value.revision().to_string(),"entry":opaque(value.entry()),"instruction":value.instruction(),"chooser":value.chooser().0,"objects":value.objects().iter().map(opaque).collect::<Vec<_>>()})
        }
        RuleFrame::ResolutionManaColor(value) if value.split().is_some() => {
            let (first, second, total) = value.split().expect("split checked by pattern guard");
            json!({"tag":"SplitMana","revision":value.revision().to_string(),"entry":opaque(value.entry()),"instruction":value.instruction(),"chooser":value.chooser().0,"first":opaque(first),"second":opaque(second),"total":total})
        }
        RuleFrame::ResolutionManaColor(value) => {
            json!({"tag":"ResolutionManaColor","revision":value.revision().to_string(),"entry":opaque(value.entry()),"instruction":value.instruction(),"chooser":value.chooser().0,"colors":value.colors().iter().map(opaque).collect::<Vec<_>>()})
        }
        RuleFrame::CreatureType(value) => creature_type_frame(value),
        RuleFrame::CastRecovery(value) => {
            json!({"tag":"CastRecovery","revision":value.revision().to_string(),"chooser":value.chooser().0,"spell":opaque(value.spell()),"life_cost":value.life_cost(),"mana_cost":program::cost(value.mana_cost())})
        }
        RuleFrame::EntryChoice(value) => {
            json!({"tag":"EntryChoice","revision":value.revision().to_string(),"chooser":value.chooser().0,"object":opaque(value.object()),"rule":format!("{:?}",value.rule()),"eligible_reveals":value.eligible_reveals().iter().map(opaque).collect::<Vec<_>>() })
        }
        RuleFrame::TokenAuraChoice(f) => {
            json!({"tag":"TokenAuraChoice","chooser":f.chooser().0,"member":f.preview().member.ordinal(),"domain":f.domain().iter().copied().map(target).collect::<Vec<_>>()})
        }
        RuleFrame::TokenEntryChoice(value) => {
            json!({"tag":"TokenEntryChoice","revision":value.revision().to_string(),"chooser":value.chooser().0,"member":value.member().ordinal(),"rule":format!("{:?}",value.rule()),"eligible_reveals":value.eligible_reveals().iter().map(opaque).collect::<Vec<_>>() })
        }
        RuleFrame::CopyTargets(value) => {
            json!({"tag":"CopyTargets","entry":opaque(value.entry()),"original":opaque(value.original()),"instruction":value.instruction_index(),"chooser":value.chooser().0})
        }
        RuleFrame::ManaColor(value) => {
            json!({"tag":"ManaColor","revision":value.revision().to_string(),"source":opaque(value.source()),"ability":mana_ref::view(value.ability()),"chooser":value.chooser().0,"amount":value.amount(),"offered":value.colors().iter().map(opaque).collect::<Vec<_>>()})
        }
        RuleFrame::Combat(value) => match value {
            CombatDecision::Attackers(value) => {
                let mut view = json!({"tag":"DeclareAttackers","revision":value.revision().to_string(),"chooser":value.chooser().0,"attackers":value.eligible_attackers().iter().map(opaque).collect::<Vec<_>>(),"defenders":value.defenders().iter().map(|seat|seat.0).collect::<Vec<_>>()});
                if !value.capped_defenders().is_empty() {
                    view["capped_defenders"] = json!(
                        value
                            .capped_defenders()
                            .iter()
                            .map(|p| p.0)
                            .collect::<Vec<_>>()
                    );
                }
                if let Some(cost) = value.payment() {
                    view["payment"] =
                        json!({"generic":cost.generic,"white_phyrexian":cost.white_phyrexian});
                }
                if !value.defender_costs().is_empty() {
                    view["prices"]=json!(value.defender_costs().iter().map(|(&defender,cost)|json!({"defender":target(defender),"generic":cost.generic,"white_phyrexian":cost.white_phyrexian})).collect::<Vec<_>>());
                }
                if value.single_attacker() {
                    view["single_attacker"] = json!(true);
                }
                if !value.forbidden_defenders().is_empty() {
                    view["forbidden_defenders"]=json!(value.forbidden_defenders().iter().map(|(id,set)|json!({"attacker":opaque(id),"defenders":set.iter().map(|v|target(*v)).collect::<Vec<_>>()})).collect::<Vec<_>>());
                }
                if !value.required_attacks().is_empty() {
                    view["required_attacks"] = json!(value.required_attacks().iter().map(|(id, defenders)| json!({"attacker":opaque(id),"defenders":defenders.iter().map(|v| target(*v)).collect::<Vec<_>>()})).collect::<Vec<_>>());
                }
                if !value.battles().is_empty() {
                    view["battles"] = json!(value.battles().iter().map(opaque).collect::<Vec<_>>());
                }
                if !value.planeswalkers().is_empty() {
                    view["planeswalkers"] =
                        json!(value.planeswalkers().iter().map(opaque).collect::<Vec<_>>());
                }
                view
            }
            CombatDecision::Blockers(value) => {
                let mut projected = json!({"tag":"DeclareBlockers","revision":value.revision().to_string(),"chooser":value.chooser().0,"blockers":value.eligible_blockers().iter().map(opaque).collect::<Vec<_>>(),"attackers":value.attackers().iter().map(opaque).collect::<Vec<_>>()});
                if value.defending_player() != value.chooser() {
                    projected["defending_player"] = json!(value.defending_player().0);
                }
                if value.single_blocker() {
                    projected["single_blocker"] = json!(true);
                }
                let pairs: Vec<_> = value.restricted_blockers().map(|(blocker, attackers)| json!({"blocker":opaque(blocker),"attackers": attackers.iter().map(opaque).collect::<Vec<_>>() })).collect();
                if !pairs.is_empty() {
                    projected["restricted_blockers"] = json!(pairs);
                }
                let groups: Vec<_> = value.group_minimums().map(|(attacker, minimum)| json!({"attacker":opaque(attacker),"minimum":minimum})).collect();
                if !groups.is_empty() {
                    projected["minimum_if_blocked"] = json!(groups);
                }
                let extra: Vec<_> = value
                    .eligible_blockers()
                    .iter()
                    .filter(|id| value.maximum_blocks(**id) > 1)
                    .map(|id| json!({"blocker":opaque(id),"maximum":value.maximum_blocks(*id)}))
                    .collect();
                if !extra.is_empty() {
                    projected["maximum_blocks"] = json!(extra);
                }
                projected
            }
            CombatDecision::DamageAssignments(value) => {
                json!({"tag":"AssignCombatDamage","revision":value.revision().to_string(),"chooser":value.chooser().0,"position":damage_trace::turn_position_view(value.position()),"domains":value.domains().iter().map(|domain| {
                    let mut row = json!({"source":opaque(domain.source()),"power":domain.power(),"recipients":domain.recipients().iter().map(|target|damage_trace::damage_recipient_view(*target)).collect::<Vec<_>>()});
                    if !domain.trample_lethal().is_empty() {
                        row["trample_lethal"] = json!(domain.trample_lethal().iter().map(|(blocker,minimum)| json!({"blocker":opaque(blocker),"minimum":minimum})).collect::<Vec<_>>());
                    }
                    row
                }).collect::<Vec<_>>()})
            }
        },
        RuleFrame::Damage(value) => match value {
            DamageDecision::Order(value) => {
                json!({"tag":"DamageOrder","revision":value.revision().to_string(),"batch":opaque(value.batch()),"stage":value.stage().to_string(),"origin":damage_trace::damage_origin_view(*value.origin()),"chooser":value.chooser().0,"applications":value.applications().iter().map(|application|damage_trace::damage_application_view(*application)).collect::<Vec<_>>()})
            }
            DamageDecision::Allocation(value) => {
                json!({"tag":"AllocatePrevention","revision":value.revision().to_string(),"batch":opaque(value.batch()),"stage":value.stage().to_string(),"origin":damage_trace::damage_origin_view(*value.origin()),"chooser":value.chooser().0,"application":damage_trace::damage_application_view(value.application()),"capacity":value.capacity(),"required":value.required(),"eligible":value.eligible().iter().map(|domain|json!({"attempt":opaque(domain.attempt()),"source":damage_trace::damage_source_view(domain.source()),"amount":domain.amount()})).collect::<Vec<_>>()})
            }
        },
    })
}

/// Project each actual stack entry with its owned body, copiable values and bindings.
/// This observes the stack only; it does not evaluate the battlefield or execute rules.
fn stack_payload(execution: &EffectWindow, entry: &StackEntry) -> Value {
    let mut value = stack_entry(entry);
    if entry.sunburst_colors() > 0 {
        value["sunburst_colors"] = json!(entry.sunburst_colors());
    }
    value["body"] = json!(execution.stack_body(entry.id()).map(program::body));
    value["copiable_card_types"] = json!(
        execution
            .stack_copiable_card_types(entry.id())
            .map(|types| types.iter().map(opaque).collect::<Vec<_>>())
    );
    value["copiable_creature_subtypes"] = json!(
        execution
            .stack_copiable_creature_subtypes(entry.id())
            .map(|types| types
                .iter()
                .map(|id| creature_type_id(*id))
                .collect::<Vec<_>>())
    );
    if let Some(subtypes) = execution.stack_copiable_spell_subtypes(entry.id())
        && !subtypes.is_empty()
    {
        value["copiable_spell_subtypes"] = json!(
            subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if let Some(subtypes) = execution.stack_copiable_artifact_subtypes(entry.id())
        && !subtypes.is_empty()
    {
        value["copiable_artifact_subtypes"] = json!(
            subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if let Some(subtypes) = execution.stack_copiable_enchantment_subtypes(entry.id())
        && !subtypes.is_empty()
    {
        value["copiable_enchantment_subtypes"] = json!(
            subtypes
                .iter()
                .map(|kind| kind.wire_label())
                .collect::<Vec<_>>()
        );
    }
    if let Some(keywords) = execution.stack_copiable_keywords(entry.id())
        && !keywords.is_empty()
    {
        value["copiable_keywords"] = json!(keywords.iter().map(opaque).collect::<Vec<_>>());
    }
    value["copiable_names"] = json!(execution.stack_copiable_names(entry.id()));
    value["copiable_supertypes"] = json!(
        execution
            .stack_copiable_supertypes(entry.id())
            .map(|types| types.iter().map(opaque).collect::<Vec<_>>())
    );
    value["copiable_triggered"] = json!(
        execution
            .stack_copiable_triggered_abilities(entry.id())
            .map(program::triggered_abilities)
    );
    value["bindings"] = json!(
        execution
            .execution_bindings(entry.id())
            .map(|bindings| json!({"trigger":bindings.trigger.as_ref().map(binding)}))
    );
    value
}
/// Observe the queued stack payloads, retaining the predecessor wire shape.
pub fn stack_payloads(execution: &EffectWindow) -> Vec<Value> {
    execution
        .window()
        .entries()
        .iter()
        .map(|entry| stack_payload(execution, entry))
        .collect()
}
/// Observe the complete actual resolving entry, including its owned body,
/// targets, captured source/causes and copy traits during a suspended decision.
pub fn resolving_payload(execution: &EffectWindow) -> Option<Value> {
    execution
        .window()
        .resolving()
        .map(|entry| stack_payload(execution, entry))
}

/// Observe the actual executor without inventing root basis, calendar, or decision state.
pub fn effect(execution: &EffectWindow, aliases: &AliasContext) -> Result<Value, String> {
    let window = execution.window();
    let counters = execution.operational_counters();
    let evaluated = execution
        .evaluated_characteristics()
        .map_err(|error| format!("characteristic projection rejected: {error:?}"))?;
    let programs: BTreeMap<CardId, _> = execution.programs().collect();
    let objects = window
        .cards()
        .iter()
        .map(|card| {
            let current = evaluated
                .get(card.id())
                .ok_or_else(|| format!("characteristic evaluation omitted {:?}", card.id()))?;
            let definition = execution.base_program(card.id())
                .ok_or_else(|| format!("baseline definition omitted {:?}", card.id()))?;
            Ok(json!({
                "object":object(card),
                "alias":card.card_id().and_then(|id|aliases.alias(id)),
                "program":program::view(definition),
                "characteristics":current_characteristics(current),
                "battlefield_counters":execution.power_toughness_counters(card.id()).as_ref().map(program::counters),
                "battlefield_marked_damage":execution.battlefield_marked_damage(card.id()),
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let stack = stack_payloads(execution);
    let resources = json!({"next_object":window.cards().next_identity_sequence().to_string(),"next_stack_entry":window.next_entry_sequence().to_string(),"priority_revision":window.priority_revision().map(|revision|revision.to_string()),"next_event":counters.next_event.to_string(),"next_instance":counters.next_instance.to_string(),"next_wave":counters.next_wave.to_string(),"next_effect":counters.next_effect.to_string(),"next_characteristic_timestamp":counters.next_characteristic_timestamp.to_string(),"next_world_tenure":counters.next_world_tenure.to_string(),"next_shield":counters.next_shield.to_string(),"next_multiplier":counters.next_multiplier.to_string(),"next_batch":counters.next_batch.to_string(),"next_attempt":counters.next_attempt.to_string()});
    let mut result = json!({"effect_revision":execution.revision().to_string(),"window_revision":window.revision().to_string(),"objects_revision":window.cards().revision().to_string(),"resources":resources,"instruction_cursor":execution.instruction_index(),"window_phase":opaque(window.phase()),"participants":window.participants().iter().map(|seat|seat.0).collect::<Vec<_>>(),"active":window.active_player().0,"objects":objects,
        "programs":programs.iter().map(|(card,definition)|json!({"card_id":card.0,"definition":program::view(definition)})).collect::<Vec<_>>(),
        "characteristic_timestamps":execution.characteristic_timestamps().map(|(object,timestamp)|json!({"object":opaque(object),"timestamp":timestamp.sequence().to_string()})).collect::<Vec<_>>(),
        "evaluation_trace":evaluation_trace(&evaluated.trace),
        "piles":card_piles(window.cards(),window.participants()),
        "mana":window.participants().iter().map(|seat|json!({"player":seat.0,"pool":ManaType::ALL.iter().map(|kind|(opaque(kind),execution.mana(*seat,*kind))).collect::<BTreeMap<_,_>>()})).collect::<Vec<_>>(),
        "life":window.participants().iter().map(|seat|json!({"player":seat.0,"total":execution.life_total(*seat)})).collect::<Vec<_>>(),
        "stack":stack,"copies":window.spell_copies().values().map(copy).collect::<Vec<_>>(),"waiting":execution.waiting_triggers().map(waiting).collect::<Vec<_>>(),"combat":execution.combat().map(combat),
        "state_based_losses":execution.state_based_losses().iter().map(|seat|seat.0).collect::<Vec<_>>(),
        "continuous_effects":execution.continuous_effects().map(continuous_effect).collect::<Vec<_>>(),"prevention_shields":execution.prevention_shields().map(|shield|json!({"id":opaque(shield.id),"creator":opaque(shield.creator),"target":damage_trace::damage_effect_target_view(shield.target),"remaining":shield.remaining})).collect::<Vec<_>>(),"damage_multipliers":execution.damage_multipliers().map(|value|json!({"id":opaque(value.id),"creator":opaque(value.creator),"target":damage_trace::damage_effect_target_view(value.target),"factor":value.factor,"filter":damage_trace::filter_view(value.filter)})).collect::<Vec<_>>(),
        "creature_vocabulary":execution.creature_vocabulary().id().to_string(),"name_rules":execution.name_rules().groups(),"name_rules_id":execution.name_rules().id().to_string(),"resolution_state":execution.resolution_state().map(resolution_state),
        "pending_damage":execution.pending_damage().map(|batch|json!({"parent":execution.damage_parent().map(damage_parent),"id":opaque(batch.id()),"origin":damage_trace::damage_origin_view(batch.origin()),"kind":opaque(batch.kind()),"stage":batch.stage().to_string(),"phase":damage_trace::damage_batch_phase_view(batch.phase()),"consumed_shields":batch.consumed_shields().iter().map(|application|damage_trace::damage_application_view(*application)).collect::<Vec<_>>(),"components":batch.components().map(|component|json!({"attempt":damage_trace::damage_attempt_view(component.attempt()),"remaining":component.remaining(),"used_multipliers":component.used_multipliers().iter().map(opaque).collect::<Vec<_>>()})).collect::<Vec<_>>()}))});
    if !execution.deathtouch_damage().is_empty() {
        result["deathtouch_damage"] = json!(
            execution
                .deathtouch_damage()
                .iter()
                .map(opaque)
                .collect::<Vec<_>>()
        );
    }
    let plotted: Vec<_> = execution
        .window()
        .cards()
        .iter()
        .filter_map(|card| {
            execution
                .plotted_on(card.id())
                .map(|turn| json!({"object":opaque(card.id()), "turn":turn.to_string()}))
        })
        .collect();
    if !plotted.is_empty() {
        result["plotted"] = json!(plotted);
    }
    if let Some(damage) = execution.damage_to_players_this_turn() {
        result["damage_to_players_this_turn"] = json!(
            damage
                .iter()
                .map(|(p, n)| json!({"player":p.0,"amount":n}))
                .collect::<Vec<_>>()
        );
    }
    if !execution.charge_counters().is_empty() {
        result["charge_counters"] = json!(
            execution
                .charge_counters()
                .iter()
                .map(|(o, n)| json!({"object":opaque(o),"amount":n}))
                .collect::<Vec<_>>()
        );
    }
    if !execution.suspect_counters().is_empty() {
        result["suspect_counters"] = json!(
            execution
                .suspect_counters()
                .iter()
                .map(|(object, amount)| json!({"object":opaque(object),"amount":amount}))
                .collect::<Vec<_>>()
        );
    }
    if !execution.storage_counters().is_empty() {
        result["storage_counters"] = json!(
            execution
                .storage_counters()
                .iter()
                .map(|(object, amount)| json!({"object":opaque(object),"amount":amount}))
                .collect::<Vec<_>>()
        );
    }
    if let Some(history) = execution.cast_turn_history() {
        result["cast_turn_history"] = json!({"casters":history.casters.iter().map(|s|s.0).collect::<Vec<_>>(),"lost_life":history.lost_life.iter().map(|s|s.0).collect::<Vec<_>>()});
        let facts = &mut result["cast_turn_history"];
        if !history.discarded.is_empty() {
            facts["discarded"] = json!(
                history
                    .discarded
                    .iter()
                    .map(|(card, seat)| json!({"object":opaque(card),"player":seat.0}))
                    .collect::<Vec<_>>()
            );
        }
        if !history.combat_types.is_empty() {
            facts["combat_types"] = json!(
                history
                    .combat_types
                    .iter()
                    .map(|(seat, types)| json!({"player":seat.0,"types":types}))
                    .collect::<Vec<_>>()
            );
        }
        if !history.freerunning.is_empty() {
            facts["freerunning"] = json!(
                history
                    .freerunning
                    .iter()
                    .map(|seat| seat.0)
                    .collect::<Vec<_>>()
            );
        }
    }
    for (label, counters) in [
        ("time_counters", execution.time_counters()),
        ("lore_counters", execution.lore_counters()),
        ("fade_counters", execution.fade_counters()),
    ] {
        if !counters.is_empty() {
            result[label] = json!(
                counters
                    .iter()
                    .map(|(id, n)| json!({"object":opaque(id),"count":n}))
                    .collect::<Vec<_>>()
            );
        }
    }
    if !execution.growth_last_known().is_empty() {
        result["growth_last_known"] = json!(
            execution
                .growth_last_known()
                .values()
                .map(appearance)
                .collect::<Vec<_>>()
        );
    }
    if let Some(cuts) = execution.upkeep_cuts() {
        result["upkeep_cuts"] = json!(
            cuts.iter()
                .map(|(p, epoch)| json!({"player":p.0,"epoch":opaque(epoch)}))
                .collect::<Vec<_>>()
        );
    }
    if !execution.age_counters().is_empty() {
        result["age_counters"] = json!(
            execution
                .age_counters()
                .iter()
                .map(|(id, count)| json!({"object":opaque(id),"count":count}))
                .collect::<Vec<_>>()
        );
    }
    if !execution.renowned_objects().is_empty() {
        result["renowned_objects"] = json!(
            execution
                .renowned_objects()
                .iter()
                .map(opaque)
                .collect::<Vec<_>>()
        );
    }
    if !execution.poisoned_players().is_empty() {
        result["poison"] = json!(
            execution
                .poisoned_players()
                .iter()
                .map(|(player, total)| json!({"player":player.0,"total":total}))
                .collect::<Vec<_>>()
        );
    }
    if !execution.regeneration_shields().is_empty() {
        result["regeneration_shields"] = json!(execution.regeneration_shields().iter().flat_map(|(permanent, origins)| origins.iter()
            .map(move |(entry, instruction)| json!({"object":opaque(permanent),"entry":opaque(entry),"instruction":instruction}))).collect::<Vec<_>>());
    }
    if !execution.departed_damage_sources().is_empty() {
        result["departed_damage_sources"] = json!(execution.departed_damage_sources().values().map(|source|{let mut row=json!({"object":object(&source.object),"keywords":source.keywords.iter().map(opaque).collect::<Vec<_>>() }); if !source.colors.is_empty() || source.keywords.is_empty() {row["colors"]=json!(source.colors.iter().map(opaque).collect::<Vec<_>>());} row}).collect::<Vec<_>>());
    }
    result["pending_trigger_placement"] = execution
        .pending_trigger_placement()
        .map_err(|error| format!("pending trigger projection rejected: {error:?}"))?
        .as_ref()
        .map(trigger_placement)
        .unwrap_or(Value::Null);
    result["world_tenures"] = json!(execution.world_tenures().map(|(object,tenure)|json!({"object":opaque(object),"tenure":tenure.sequence().to_string()})).collect::<Vec<_>>());
    result["pending_work"] = pending_work(execution)?;
    result["resolving"] = json!(resolving_payload(execution));
    Ok(result)
}

/// Project the actual process discriminator and its existing borrowed payload.
pub fn pending_work(execution: &EffectWindow) -> Result<Value, String> {
    let choice = execution.resolution_choice_state().map(|state| match state {
        ResolutionChoiceState::Optional { cursor, chooser, options } => json!({"cursor":resolution_state(cursor),"chooser":chooser.0,"options":options.iter().map(opaque).collect::<Vec<_>>()}),
        ResolutionChoiceState::Payment { cursor, payer, cost, restricted_colors, paid_destination, declined_destination, .. } => {
            let mut encoded=json!({"cursor":resolution_state(cursor),"payer":payer.0,"cost":program::cost(cost),"paid_destination":paid_destination.to_string(),"declined_destination":declined_destination.to_string()});
            if let Some(colors)=restricted_colors {encoded["restricted_colors"]=json!(colors.iter().map(opaque).collect::<Vec<_>>());}
            encoded
        },
    });
    let cursor = || {
        execution
            .resolution_state()
            .map(resolution_state)
            .ok_or_else(|| "pending process omitted its owned resolution cursor".to_owned())
    };
    Ok(match execution.pending_work_kind() {
        None => Value::Null,
        Some(PendingWorkKind::LibraryTop) => {
            return Err("historical pending-work projection excludes native library work".into());
        }
        Some(PendingWorkKind::ActivationCost) => {
            return Err(
                "historical pending-work projection excludes activation cost replacement work"
                    .into(),
            );
        }
        Some(PendingWorkKind::LookTop) => {
            return Err("historical pending-work projection excludes private top-look work".into());
        }
        Some(PendingWorkKind::ImpulseReplacement) => {
            return Err(
                "historical pending-work projection excludes impulse replacement work".into(),
            );
        }
        Some(
            PendingWorkKind::BattlefieldEntry
            | PendingWorkKind::PermanentEntry
            | PendingWorkKind::SearchEntry
            | PendingWorkKind::LandPlay
            | PendingWorkKind::ZoneMove,
        ) => {
            return Err(
                "historical pending-work projection excludes battlefield entry replacement work"
                    .into(),
            );
        }
        Some(PendingWorkKind::ResolutionSelection) => json!({
            "tag":"ResolutionSelection",
            "work":resolution_selection_state(execution.resolution_selection_state().ok_or("selection work missing")?)
        }),
        Some(PendingWorkKind::OptionalEffect) => {
            json!({"tag":"OptionalEffect","work":choice.ok_or("optional work missing")?})
        }
        Some(PendingWorkKind::ResolutionPayment) => {
            json!({"tag":"ResolutionPayment","work":choice.ok_or("payment work missing")?})
        }
        Some(PendingWorkKind::ManaColor) => {
            let frame = execution.pending_mana_color().ok_or("color work missing")?;
            let view = frame_with_cards(
                &RuleFrame::ManaColor(frame.clone()),
                execution.window().cards(),
            )?;
            let parent = match choice {
                Some(work) => json!({"tag":"Payment","work":work}),
                None => json!({"tag":"Priority"}),
            };
            json!({"tag":"ManaColor","frame":view,"parent":parent})
        }
        Some(PendingWorkKind::Resolution) => json!({"tag":"Resolution","cursor":cursor()?}),
        Some(PendingWorkKind::TokenEntry) => json!({"tag":"TokenEntry","cursor":cursor()?}),
        Some(PendingWorkKind::Damage) => {
            json!({"tag":"Damage","parent":execution.damage_parent().map(damage_parent).ok_or("damage parent missing")?})
        }
        Some(PendingWorkKind::Checks) => {
            if execution.has_check_replacement_work() {
                return Err(
                    "historical pending-work projection excludes replacement-bearing Checks".into(),
                );
            }
            json!({"tag":"Checks"})
        }
        Some(PendingWorkKind::Combat) => json!({"tag":"Combat"}),
        Some(PendingWorkKind::CastRecovery) => json!({"tag":"CastRecovery"}),
    })
}

fn zone_event(value: spellwake_engine::effects::ZoneEventKey) -> Value {
    match value {
        spellwake_engine::effects::ZoneEventKey::DrawReveal { before } => {
            json!({"tag":"DrawReveal","before":opaque(before)})
        }
        spellwake_engine::effects::ZoneEventKey::DamageFollowup { batch, before } => {
            json!({"tag":"DamageFollowup","batch":opaque(batch),"before":opaque(before)})
        }
        spellwake_engine::effects::ZoneEventKey::LandPlay { before } => {
            json!({"tag":"LandPlay","before":opaque(before)})
        }
        spellwake_engine::effects::ZoneEventKey::LinkedReturn { before } => {
            json!({"tag":"LinkedReturn","before":opaque(before)})
        }
        spellwake_engine::effects::ZoneEventKey::Cost { entry, before } => {
            json!({"tag":"Cost","entry":opaque(entry),"before":opaque(before)})
        }
        spellwake_engine::effects::ZoneEventKey::ManaCost {
            source,
            ability,
            before,
        } => {
            json!({"tag":"ManaCost","source":opaque(source),"ability":mana_ref::view(ability),"before":opaque(before)})
        }
        spellwake_engine::effects::ZoneEventKey::PermanentEntry { entry, before } => {
            json!({"tag":"PermanentEntry","entry":opaque(entry),"before":opaque(before)})
        }
        spellwake_engine::effects::ZoneEventKey::Instruction {
            entry,
            instruction,
            before,
        } => {
            json!({"tag":"Instruction","entry":opaque(entry),"instruction":instruction.to_string(),"before":opaque(before)})
        }
        spellwake_engine::effects::ZoneEventKey::Check { event, before } => {
            json!({"tag":"Check","event":check_trace::check_event_view(event),"before":opaque(before)})
        }
    }
}
fn zone_instance(value: spellwake_engine::effects::ZoneEffectInstance) -> Value {
    if let Some(created) = value.created {
        use spellwake_engine::effects::CreatedZoneEffect;
        let (kind, entry, instruction) = match created {
            CreatedZoneEffect::CorpseReturn => {
                return json!({"source":opaque(value.source),"effect":"CorpseReturn"});
            }
            CreatedZoneEffect::Commander => {
                return json!({"source":opaque(value.source),"effect":"Commander"});
            }
            CreatedZoneEffect::Regenerate { entry, instruction } => {
                ("Regenerate", entry, instruction)
            }
            CreatedZoneEffect::ExileIfDies { entry, instruction } => {
                ("ExileIfDies", entry, instruction)
            }
            CreatedZoneEffect::ShieldCounter => {
                return json!({"source":opaque(value.source),"effect":"ShieldCounter"});
            }
            CreatedZoneEffect::FinalityCounter => {
                return json!({"source":opaque(value.source),"effect":"FinalityCounter"});
            }
        };
        return json!({"source":opaque(value.source),"effect":kind,"entry":opaque(entry),"instruction":instruction});
    }
    if value.madness {
        return json!({"source":opaque(value.source),"effect":"Madness"});
    }
    if value.unearth {
        return json!({"source":opaque(value.source),"effect":"Unearth"});
    }
    json!({"source":opaque(value.source),"ability":value.ability.0})
}
fn zone_application(value: spellwake_engine::effects::ZoneApplication) -> Value {
    json!({"event":zone_event(value.event),"effect":zone_instance(value.effect)})
}
/// Complete generic replacement continuation in its new privileged epoch.
/// This is neither a complete Checks archive nor public transport authority.
pub fn zone_replacement_work(value: &spellwake_engine::effects::ZoneReplacementBatch) -> Value {
    json!({"schema":"zone-replacement-native-work-v2","cause":cause(value.cause()),"apnap":value.apnap().iter().map(|seat|seat.0).collect::<Vec<_>>(),"window_revision":value.window_revision().to_string(),"stage":value.stage().to_string(),"phase":match value.phase(){spellwake_engine::effects::ZoneReplacementPhase::Selecting=>json!({"tag":"Selecting"}),spellwake_engine::effects::ZoneReplacementPhase::Settled=>json!({"tag":"Settled"}),spellwake_engine::effects::ZoneReplacementPhase::Confirming{application}=>json!({"tag":"Confirming","application":zone_application(*application)})},
    "intents":value.intents().iter().map(|i|json!({"before":opaque(i.before),"reason":opaque(i.reason)})).collect::<Vec<_>>(),
    "candidates":value.candidates().values().map(|c|{let mut row=json!({"key":zone_event(c.key),"before":object(&c.before),"action":opaque(c.action),"reason":opaque(c.reason),"original":opaque(c.original),"current":opaque(c.current)});if let Some(controller)=c.entering_controller {row["entering_controller"]=json!(controller.0);}row}).collect::<Vec<_>>(),
    "sources":value.sources().values().map(|s|json!({"instance":zone_instance(s.instance),"before":object(&s.before),"definition":program::zone_replacements(std::slice::from_ref(&s.definition))})).collect::<Vec<_>>(),
    "applied":value.applied().iter().map(|(event,effects)|json!({"event":zone_event(*event),"effects":effects.iter().map(|e|zone_instance(*e)).collect::<Vec<_>>()})).collect::<Vec<_>>(),
    "optional":value.optional().iter().map(|(application,answer)|json!({"application":zone_application(*application),"answer":opaque(answer)})).collect::<Vec<_>>(),
    "history":value.history().iter().map(|step|json!({"application":zone_application(step.application),"optional":step.optional.map(opaque)})).collect::<Vec<_>>(),
    "evaluated":{"objects":value.evaluated().objects.values().map(current_characteristics).collect::<Vec<_>>(),"trace":evaluation_trace(&value.evaluated().trace)}
    })
}
/// Complete privileged selected-object continuation; every accepted prefix stays explicit.
pub fn resolution_selection_state(value: ResolutionSelectionState<'_>) -> Value {
    let operation = match value.operation {
        ResolutionSelectionOperation::MillEachPlayer { amount, segments } => {
            json!({"tag":"MillEachPlayer","amount":amount,"segments":segments.iter().map(|(seat,cards)|json!({"player":seat.0,"cards":cards.iter().map(opaque).collect::<Vec<_>>()})).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::DestroyAnnouncedTargets { objects } => {
            json!({"tag":"DestroyAnnouncedTargets","objects":objects.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::SacrificeContributor { eligible } => {
            json!({"tag":"SacrificeContributor","eligible":eligible.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::OtherControlledCreature {
            source, sacrifice, ..
        } => json!({"tag":"OtherControlledCreature","source":opaque(source),"sacrifice":sacrifice}),
        ResolutionSelectionOperation::PutHandCardOnBottom => json!({"tag":"PutHandCardOnBottom"}),
        ResolutionSelectionOperation::PutInLibrary { objects, placement } => {
            json!({"tag":"PutInLibrary","placement":opaque(placement),"objects":objects.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::ShuffleAnnouncedTargets { objects } => {
            json!({"tag":"ShuffleAnnouncedTargets","objects":objects.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::ExileAnnouncedTargets { objects } => {
            json!({"tag":"ExileAnnouncedTargets","objects":objects.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::ExileLibrarySegment { player, cards } => {
            json!({"tag":"ExileLibrarySegment","player":player.0,"cards":cards.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::ReturnAnnouncedTargets { objects } => {
            json!({"tag":"ReturnAnnouncedTargets","objects":objects.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::ReturnSingle { object } => {
            json!({"tag":"ReturnSingle","object":opaque(object)})
        }
        ResolutionSelectionOperation::DestroySingle { object } => {
            json!({"tag":"DestroySingle","object":opaque(object)})
        }
        ResolutionSelectionOperation::WholeGroup { group, destination } => {
            json!({"tag":"WholeGroup","group":opaque(group),"destination":opaque(destination)})
        }
        ResolutionSelectionOperation::ExileSingle {
            object,
            source,
            monarch_controller,
        } => {
            json!({"tag":"ExileSingle","object":opaque(object),"source":source.map(opaque),"monarch_controller":monarch_controller.map(|seat|seat.0)})
        }
        ResolutionSelectionOperation::SacrificeMatching { filter, .. } => match filter {
            spellwake_engine::program::SacrificeSelection::Subtype(kind) => {
                json!({"tag":"SacrificeMatching","subtype":creature_type_id(*kind)})
            }
            spellwake_engine::program::SacrificeSelection::NontokenCreature => {
                json!({"tag":"SacrificeMatching","subtype":null})
            }
            other => json!({"tag":"SacrificeMatching","kind":format!("{other:?}")}),
        },
        ResolutionSelectionOperation::Sacrifice { filter } => json!({
            "tag":"Sacrifice",
            "filter":{"required_types":filter.required_types.iter().map(opaque).collect::<Vec<_>>(),
                      "forbidden_types":filter.forbidden_types.iter().map(opaque).collect::<Vec<_>>()}
        }),
        ResolutionSelectionOperation::ExileLinkedTargets { objects, source } => {
            json!({"tag":"ExileLinkedTargets","objects":objects.iter().map(opaque).collect::<Vec<_>>(),"source":opaque(source)})
        }
        ResolutionSelectionOperation::AttachToSource {
            source,
            eligible,
            optional,
        } => {
            json!({"tag":"AttachToSource","source":opaque(source),"eligible":eligible.iter().map(opaque).collect::<Vec<_>>(),"optional":optional})
        }
        ResolutionSelectionOperation::CopyDifferentlyNamedTokens => {
            json!({"tag":"CopyDifferentlyNamedTokens"})
        }
        ResolutionSelectionOperation::Amass {
            amount,
            army,
            subtype,
            eligible,
        } => {
            json!({"tag":"Amass","amount":amount,"army":creature_type_id(*army),"subtype":creature_type_id(*subtype),"eligible":eligible.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::Bolster { amount, eligible } => {
            json!({"tag":"Bolster","amount":amount,"eligible":eligible.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::Untap { kind } => json!({"tag":"Untap","kind":opaque(kind)}),
        ResolutionSelectionOperation::EnterAurasAttachedToSource { source, eligible } => {
            json!({"tag":"EnterAurasAttachedToSource","source":opaque(source),"eligible":eligible.iter().map(opaque).collect::<Vec<_>>() })
        }
        ResolutionSelectionOperation::ChooseCreaturesToKeep => {
            json!({"tag":"ChooseCreaturesToKeep"})
        }
        ResolutionSelectionOperation::LibraryCohortToGraveyard { player, cards } => {
            json!({"tag":"LibraryCohortToGraveyard","player":player.0,"count":cards.len()})
        }
        ResolutionSelectionOperation::ChooseVow => json!({"tag":"ChooseVow"}),
        ResolutionSelectionOperation::ExileFromHand {
            allow_battlefield: true,
        } => json!({"tag":"ExileFromHandOrBattlefield"}),
        ResolutionSelectionOperation::ExileFromHand {
            allow_battlefield: false,
        } => json!({"tag":"ExileFromHand"}),
        ResolutionSelectionOperation::PayLifeToSaveCreatures { cost, saved } => json!({
            "tag":"PayLifeToSaveCreatures","cost":cost,"saved":saved.iter()
                .map(|(player, objects)| json!({"player":player.0,"objects":objects.iter().map(opaque).collect::<Vec<_>>() }))
                .collect::<Vec<_>>(),
        }),
        ResolutionSelectionOperation::SacrificeExceptVowed { kept } => {
            json!({"tag":"SacrificeExceptVowed","kept":kept.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::SacrificeColored => json!({"tag":"SacrificeColored"}),
        ResolutionSelectionOperation::ExileChosen { filter } => {
            json!({"tag":"ExileChosen","required_types":filter.required_types.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"forbidden_types":filter.forbidden_types.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::ReturnToHand { filter } => {
            json!({"tag":"ReturnToHand","required_types":filter.required_types.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"forbidden_types":filter.forbidden_types.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::ExileChosenGraveyardCards { player, domain } => {
            json!({"tag":"ExileChosenGraveyardCards","player":player.0,"domain":domain.iter().map(opaque).collect::<Vec<_>>() })
        }
        ResolutionSelectionOperation::ExileGraveyardTargets { objects } => {
            json!({"tag":"ExileGraveyardTargets","objects":objects.iter().map(opaque).collect::<Vec<_>>()})
        }
        ResolutionSelectionOperation::ReturnGraveyardCardsToHand { objects } => {
            json!({"tag":"ReturnTargetGraveyardCreatureToHand","objects":objects.iter().map(opaque).collect::<Vec<_>>() })
        }
        ResolutionSelectionOperation::RevealedHand {
            chooser,
            minimum_mana_value,
            nonland_only,
            count,
            action,
        } => {
            let mut row = json!({"tag":"RevealedHand","chooser":chooser.0,"minimum_mana_value":minimum_mana_value,"action":opaque(action)});
            if *nonland_only {
                row["nonland_only"] = json!(true);
            }
            if count.get() != 1 {
                row["count"] = json!(count.get());
            }
            row
        }
        ResolutionSelectionOperation::GraveyardCreatureToHand => {
            json!({"tag":"GraveyardCreatureToHand"})
        }
        ResolutionSelectionOperation::DiscardUpTo => json!({"tag":"DiscardUpTo"}),
        ResolutionSelectionOperation::OwnedCommandersToHand => {
            json!({"tag":"OwnedCommandersToHand"})
        }
        ResolutionSelectionOperation::RandomDiscard { object } => {
            json!({"tag":"RandomDiscard","object":opaque(object)})
        }
        ResolutionSelectionOperation::RandomSacrifice { object } => {
            json!({"tag":"RandomSacrifice","object":opaque(object)})
        }
        ResolutionSelectionOperation::Discard => json!({"tag":"Discard"}),
        ResolutionSelectionOperation::DiscardFiltered { filter } => {
            json!({"tag":"DiscardFiltered","filter":opaque(filter)})
        }
    };
    let selected = |rows: &[spellwake_engine::effects::ResolutionAcceptedSelection]| {
        rows.iter().map(|row|json!({"player":row.player.0,"objects":row.objects.iter().map(opaque).collect::<Vec<_>>()})).collect::<Vec<_>>()
    };
    let orders = |rows: &[spellwake_engine::effects::ResolutionAcceptedGraveyardOrder]| {
        rows.iter().map(|row|json!({"owner":row.owner.0,"top_to_bottom":row.top_to_bottom.iter().map(opaque).collect::<Vec<_>>()})).collect::<Vec<_>>()
    };
    let stage = match value.stage {
        ResolutionSelectionStage::Choosing {
            requirements,
            next_player,
            accepted,
        } => json!({
            "tag":"Choosing","next_player":next_player.to_string(),
            "requirements":requirements.iter().map(|row|json!({"player":row.player.0,"domain":row.domain.iter().map(opaque).collect::<Vec<_>>(),"required":row.required.to_string()})).collect::<Vec<_>>(),
            "accepted":selected(accepted)
        }),
        ResolutionSelectionStage::Replacing {
            batch: work,
            selections,
        } => {
            json!({"tag":"Replacing","selections":selected(selections),"work":zone_replacement_work(work)})
        }
        ResolutionSelectionStage::Ordering {
            selections,
            replacements,
            groups,
            next_owner,
            orders: accepted,
        } => {
            let mut result = json!({
                "tag":"Ordering","selections":selected(selections),
                "groups":groups.iter().map(|row|json!({"owner":row.owner.0,"entrants":row.entrants.iter().map(opaque).collect::<Vec<_>>()})).collect::<Vec<_>>(),
                "next_owner":next_owner.to_string(),"orders":orders(accepted)
            });
            if let Some(work) = replacements {
                result["replacements"] = zone_replacement_work(work);
            }
            result
        }
        ResolutionSelectionStage::Ready(batch) => {
            let objects = |rows: &[spellwake_engine::effects::ResolutionSelectedObject]| {
                rows.iter()
                    .map(|row| json!({"player":row.player.0,"object":opaque(row.object)}))
                    .collect::<Vec<_>>()
            };
            let mut result = json!({"tag":"Ready","batch":{"selections":selected(&batch.selections),"logical":objects(&batch.logical),"application":objects(&batch.application),
                "owner_orders":orders(&batch.owner_orders),"token_departures":batch.token_departures.iter().map(opaque).collect::<Vec<_>>()}});
            if let Some(work) = &batch.replacements {
                result["batch"]["replacements"] = zone_replacement_work(work);
            }
            result
        }
    };
    json!({"cursor":resolution_state(value.cursor),"operation":operation,"stage":stage})
}

/// Full current root projection, adding only fields actually owned by GameState.
pub fn state(value: &GameState, aliases: &AliasContext) -> Result<Value, String> {
    if value.conclusion().is_some_and(|c| {
        matches!(
            c.cause(),
            spellwake_engine::game::GameConclusionCause::Conceded { .. }
        )
    }) {
        return Err("historical root projection excludes concession".into());
    }
    let mut result = effect(value.execution(), aliases)?;
    let decision=value.pending_decision().map(|decision|Ok::<Value,String>(json!({"id":decision.id().0.to_string(),"basis":basis(decision.basis()),"chooser":decision.chooser().0,"frame":frame(decision.frame(),value)?}))).transpose()?;
    result["basis"] = basis(value.basis());
    result["resources"]["next_decision"] = json!(value.next_decision_sequence().to_string());
    result["decision"] = json!(decision);
    result["turn"]=json!(value.turn().map(|turn|json!({"position":damage_trace::turn_position_view(turn.position()),"stage":opaque(turn.stage()),"lands_played":turn.lands_played()})));
    result["opening"]=json!(value.opening().map(|opening|json!({"complete":opening.complete(),"players":value.execution().window().participants().iter().map(|seat|json!({"player":seat.0,"kept":opening.kept(*seat),"taken":opening.mulligans_taken(*seat),"charged":opening.charged(*seat),"can_mulligan":opening.can_mulligan(*seat)})).collect::<Vec<_>>()})));
    result["entropy"] = json!(
        value
            .entropy()
            .map(|entropy| json!({"position":entropy.position(),"remaining":entropy.remaining()}))
    );
    result["outcome"] = json!(value.outcome().map(opaque));
    result["eliminations"] = json!(
        value
            .pending_eliminations()
            .iter()
            .map(|seat| seat.0)
            .collect::<Vec<_>>()
    );
    Ok(result)
}

pub(crate) fn draw_origin(value: &DrawOrigin) -> Value {
    match value {
        DrawOrigin::Opening => json!({"tag":"Opening"}),
        DrawOrigin::Turn { position } => {
            json!({"tag":"Turn","position":damage_trace::turn_position_view(*position)})
        }
        DrawOrigin::Instruction { entry, instruction } => {
            json!({"tag":"Instruction","entry":opaque(entry),"instruction":instruction})
        }
        DrawOrigin::MechanismUnit => json!({"tag":"MechanismUnit"}),
    }
}
fn zone_reason(value: ZoneReason) -> Value {
    match value {
        ZoneReason::Surveil { player } => json!({"tag":"Surveil","player":player.0}),
        ZoneReason::Mill { player } => json!({"tag":"Mill","player":player.0}),
        ZoneReason::Opening(phase) => json!({"tag":"Opening","phase":opaque(phase)}),
        ZoneReason::LandPlay => json!({"tag":"LandPlay"}),
        ZoneReason::Manifest => json!({"tag":"Manifest"}),
        ZoneReason::ReturnFromSacrifice => json!({"tag":"ReturnFromSacrifice"}),
        ZoneReason::ExileToBattlefield => json!({"tag":"ExileToBattlefield"}),
        ZoneReason::HandToBattlefield => json!({"tag":"HandToBattlefield"}),
        ZoneReason::LookTopToBattlefield => json!({"tag":"LookTopToBattlefield"}),
        ZoneReason::SearchToBattlefield => json!({"tag":"SearchToBattlefield"}),
        ZoneReason::PutIntoOwnersLibrary(placement) => {
            json!({"tag":"PutIntoOwnersLibrary","placement":opaque(placement)})
        }
        ZoneReason::ShufflePermanentIntoLibrary => json!({"tag":"ShufflePermanentIntoLibrary"}),
        ZoneReason::ExiledCardsToLibraryBottom => json!({"tag":"ExiledCardsToLibraryBottom"}),
        ZoneReason::ShuffleHandAndGraveyardIntoLibrary => {
            json!({"tag":"ShuffleHandAndGraveyardIntoLibrary"})
        }
        ZoneReason::ShuffleGraveyardIntoLibrary => json!({"tag":"ShuffleGraveyardIntoLibrary"}),
        ZoneReason::PutLibraryCohortIntoGraveyard => json!({"tag":"PutLibraryCohortIntoGraveyard"}),
        ZoneReason::RevealedToGraveyard => json!({"tag":"RevealedToGraveyard"}),
        ZoneReason::SearchToGraveyard => json!({"tag":"SearchToGraveyard"}),
        ZoneReason::ReturnFromGraveyardToBattlefield => {
            json!({"tag":"ReturnFromGraveyardToBattlefield"})
        }
        ZoneReason::ReturnFromLinkedExile => json!({"tag":"ReturnFromLinkedExile"}),
        ZoneReason::CastAnnouncement => json!({"tag":"CastAnnouncement"}),
        ZoneReason::PermanentResolution => json!({"tag":"PermanentResolution"}),
        ZoneReason::ResolutionExit => json!({"tag":"ResolutionExit"}),
        ZoneReason::AllTargetsIllegalExit => json!({"tag":"AllTargetsIllegalExit"}),
        ZoneReason::Foretell => json!({"tag":"Foretell"}),
        ZoneReason::Suspend => json!({"tag":"Suspend"}),
        ZoneReason::Plot => json!({"tag":"Plot"}),
        ZoneReason::AlternativeHandExileCost => json!({"tag":"AlternativeHandExileCost"}),
        ZoneReason::EscapeCost => json!({"tag":"EscapeCost"}),
        ZoneReason::DelveCost => json!({"tag":"DelveCost"}),
        ZoneReason::ExiledByEffect => json!({"tag":"ExiledByEffect"}),
        ZoneReason::Destroyed => json!({"tag":"Destroyed"}),
        ZoneReason::ReturnToOwnersHand => json!({"tag":"ReturnToOwnersHand"}),
        ZoneReason::Countered => json!({"tag":"Countered"}),
        ZoneReason::ExileCost => json!({"tag":"ExileCost"}),
        ZoneReason::SacrificeCost => json!({"tag":"SacrificeCost"}),
        ZoneReason::Sacrifice { player } => json!({"tag":"Sacrifice","player":player.0}),
        ZoneReason::Draw { player } => json!({"tag":"Draw","player":player.0}),
        ZoneReason::CascadeExile => json!({"tag":"CascadeExile"}),
        ZoneReason::CascadeBottom => json!({"tag":"CascadeBottom"}),
        ZoneReason::MadnessDeclined => json!({"tag":"MadnessDeclined"}),
        ZoneReason::CommanderReturn => json!({"tag":"CommanderReturn"}),
        ZoneReason::PlayerDeparture => json!({"tag":"PlayerDeparture"}),
        ZoneReason::PersistReturn => json!({"tag":"PersistReturn"}),
        ZoneReason::UndyingReturn => json!({"tag":"UndyingReturn"}),
        ZoneReason::NinjutsuEntry => json!({"tag":"NinjutsuEntry"}),
        ZoneReason::UnearthReturn => json!({"tag":"UnearthReturn"}),
        ZoneReason::DashReturn => json!({"tag":"DashReturn"}),
        ZoneReason::WarpExile => json!({"tag":"WarpExile"}),
        ZoneReason::Ingest { player } => json!({"tag":"Ingest","player":player.0}),
        ZoneReason::UnearthExile => json!({"tag":"UnearthExile"}),
        ZoneReason::SearchToExile => json!({"tag":"SearchToExile"}),
        ZoneReason::TransfigureEntry => json!({"tag":"TransfigureEntry"}),
        ZoneReason::SearchToHand => json!({"tag":"SearchToHand"}),
        ZoneReason::LookTopToHand => json!({"tag":"LookTopToHand"}),
        ZoneReason::TransmuteDiscard { player } => {
            json!({"tag":"TransmuteDiscard","player":player.0})
        }
        ZoneReason::CycleDiscard { player } => json!({"tag":"CycleDiscard","player":player.0}),
        ZoneReason::Discard { player } => json!({"tag":"Discard","player":player.0}),
        ZoneReason::StateBased(causes) => {
            json!({"tag":"StateBased","causes":causes.iter().map(opaque).collect::<Vec<_>>()})
        }
        ZoneReason::MechanismUnit => json!({"tag":"MechanismUnit"}),
    }
}
fn cause(value: &EventCause) -> Value {
    macro_rules! tagged { ($tag:literal, $($key:literal:$value:expr),* $(,)?)=>{json!({"tag":$tag,$($key:$value),*})}; }
    match value {
        EventCause::OpeningShuffle { player, phase } => {
            tagged!("OpeningShuffle","player":player.0,"phase":opaque(phase))
        }
        EventCause::Draw { player, origin } => {
            tagged!("Draw","player":player.0,"origin":draw_origin(origin))
        }
        EventCause::OpeningCardsMoved { phase, cohort } => {
            tagged!("OpeningCardsMoved","phase":opaque(phase),"cohort":cohort.iter().map(|seat|seat.0).collect::<Vec<_>>())
        }
        EventCause::Foretell {
            actor,
            source,
            payment,
        } => {
            json!({"tag":"Foretell","actor":actor.0,"source":opaque(source),"payment":spend(payment)})
        }
        EventCause::Suspend {
            actor,
            source,
            payment,
        } => {
            json!({"tag":"Suspend","actor":actor.0,"source":opaque(source),"payment":spend(payment)})
        }
        EventCause::TurnFaceUp {
            actor,
            source,
            payment,
        } => {
            json!({"tag":"TurnFaceUp","actor":actor.0,"source":opaque(source),"payment":spend(payment)})
        }
        EventCause::Plot {
            actor,
            source,
            payment,
        } => json!({"tag":"Plot","actor":actor.0,"source":opaque(source),"payment":spend(payment)}),
        EventCause::LandPlay { actor } => tagged!("LandPlay","actor":actor.0),
        EventCause::CastAnnouncement { entry } => tagged!("CastAnnouncement","entry":opaque(entry)),
        EventCause::CastLifePayment { entry } => {
            json!({"tag":"CastLifePayment","entry":opaque(entry)})
        }
        EventCause::EscapePayment { entry } => json!({"tag":"EscapePayment","entry":opaque(entry)}),
        EventCause::CastObjectPayment { entry, resource } => {
            json!({"tag":"CastObjectPayment","entry":opaque(entry),"resource":cast_resource(*resource)})
        }
        EventCause::CastCompleted { entry } => tagged!("CastCompleted","entry":opaque(entry)),
        EventCause::ActivatedAbilityCost { entry, ability } => {
            tagged!("ActivatedAbilityCost","entry":opaque(entry),"ability":ability.0)
        }
        EventCause::ActivatedAbilityCompleted { entry, ability } => {
            tagged!("ActivatedAbilityCompleted","entry":opaque(entry),"ability":ability.0)
        }
        EventCause::ManaAbilityCost { source, ability } => {
            tagged!("ManaAbilityCost","source":opaque(source),"ability":mana_ref::view(*ability))
        }
        EventCause::ManaAbilityActivated { source, ability } => {
            tagged!("ManaAbilityActivated","source":opaque(source),"ability":mana_ref::view(*ability))
        }
        EventCause::ManaProduced { source, ability } => {
            tagged!("ManaProduced","source":opaque(source),"ability":mana_ref::view(*ability))
        }
        EventCause::ManaDebited { entry } => tagged!("ManaDebited","entry":opaque(entry)),
        EventCause::TriggerTargetsBound { entry } => {
            json!({"tag":"TriggerTargetsBound","entry":opaque(entry)})
        }
        EventCause::Instruction { entry, instruction } => {
            tagged!("Instruction","entry":opaque(entry),"instruction":instruction)
        }
        EventCause::CounterSpell {
            entry,
            instruction,
            target,
        } => {
            tagged!("CounterSpell","entry":opaque(entry),"instruction":instruction,"target":opaque(target))
        }
        EventCause::ResolutionCompleted { entry, completion } => {
            tagged!("ResolutionCompleted","entry":opaque(entry),"completion":opaque(completion))
        }
        EventCause::UntapTurn { active } => tagged!("UntapTurn","active":active.0),
        EventCause::CleanupDiscard { event, boundary } => {
            tagged!("CleanupDiscard","event":check_trace::check_event_view(*event),"boundary":check_trace::check_boundary_view(*boundary))
        }
        EventCause::CleanupCharacteristics { position } => {
            tagged!("CleanupCharacteristics","position":damage_trace::turn_position_view(*position))
        }
        EventCause::StateBasedActions { event, boundary } => {
            tagged!("StateBasedActions","event":check_trace::check_event_view(*event),"boundary":check_trace::check_boundary_view(*boundary))
        }
        EventCause::PlayerDeparture => tagged!("PlayerDeparture",),
        EventCause::DamageAdjusted { batch } => tagged!("DamageAdjusted","batch":opaque(batch)),
        EventCause::DamageDealt { batch } => tagged!("DamageDealt","batch":opaque(batch)),
        EventCause::DamageResults { batch } => tagged!("DamageResults","batch":opaque(batch)),
        EventCause::AttackCostPaid { position, player } => {
            tagged!("AttackCostPaid","position":damage_trace::turn_position_view(*position),"player":player.0)
        }
        EventCause::AttackTaps { position, actor } => {
            tagged!("AttackTaps","position":damage_trace::turn_position_view(*position),"actor":actor.0)
        }
        EventCause::AttackersDeclared { position, actor } => {
            tagged!("AttackersDeclared","position":damage_trace::turn_position_view(*position),"actor":actor.0)
        }
        EventCause::BlockersDeclared { position, actor } => {
            tagged!("BlockersDeclared","position":damage_trace::turn_position_view(*position),"actor":actor.0)
        }
        EventCause::CombatDamageAssigned {
            position,
            actor,
            origin,
        } => {
            tagged!("CombatDamageAssigned","position":damage_trace::turn_position_view(*position),"actor":actor.0,"origin":opaque(origin))
        }
        EventCause::CombatEnded { position } => {
            tagged!("CombatEnded","position":damage_trace::turn_position_view(*position))
        }
        EventCause::ManaPoolsEmptied { ended } => {
            tagged!("ManaPoolsEmptied","ended":damage_trace::turn_position_view(*ended))
        }
        EventCause::TurnOrderAdvanced { position } => {
            tagged!("TurnOrderAdvanced","position":damage_trace::turn_position_view(*position))
        }
        EventCause::StepBegan { position } => {
            tagged!("StepBegan","position":damage_trace::turn_position_view(*position))
        }
        EventCause::MechanismUnit => json!({"tag":"MechanismUnit"}),
    }
}
pub fn shuffle(value: &spellwake_engine::shuffle::ShuffleRecord) -> Value {
    json!({"player":value.player.0,"algorithm":value.algorithm,"before":value.before.iter().map(opaque).collect::<Vec<_>>(),"after":value.after.iter().map(opaque).collect::<Vec<_>>(),"entropy_start":value.entropy_start,"entropy_end":value.entropy_end})
}
/// Complete privileged action metadata, including actual movement receipts.
/// Native identities here carry no public transport or reply authority.
pub fn library_action(value: &spellwake_engine::effects::LibraryAction) -> Value {
    json!({"kind":opaque(value.kind),"player":value.player.0,"requested":value.requested,"looked_top_first":value.looked_top_first.iter().map(opaque).collect::<Vec<_>>(),"top_first":value.top_first.iter().map(opaque).collect::<Vec<_>>(),"bottom_top_first":value.bottom_top_first.iter().map(opaque).collect::<Vec<_>>(),"selected_before_top_first":value.selected_before_top_first.iter().map(opaque).collect::<Vec<_>>(),"public_zone_moves":value.public_zone_moves.iter().map(|movement|json!({"before":opaque(movement.before),"after":opaque(movement.after),"destination":opaque(movement.destination)})).collect::<Vec<_>>(),"graveyard_before_top_first":value.graveyard_before_top_first.iter().map(opaque).collect::<Vec<_>>()})
}
pub fn occurrence(value: &RulesOccurrence) -> Value {
    match value {
        RulesOccurrence::LibraryAction(value) => {
            json!({"tag":"LibraryAction","action":library_action(value)})
        }
        RulesOccurrence::TokenCreated { creator, after } => {
            json!({"tag":"TokenCreated","creator":creator.0,"after":appearance(after)})
        }
        RulesOccurrence::TokenCeased { object: value } => {
            json!({"tag":"TokenCeased","object":object(value)})
        }
        RulesOccurrence::ControllerChanged(change) => {
            json!({"tag":"ControllerChanged","before":object(&change.before),"after":object(&change.after)})
        }
        RulesOccurrence::LifeChanged(change) => {
            json!({"tag":"LifeChanged","change":life_change(change)})
        }
        RulesOccurrence::CreatureTypeChosen {
            entry,
            instruction,
            slot,
            chooser,
            value,
        } => {
            json!({"tag":"CreatureTypeChosen","entry":opaque(entry),"instruction":instruction.to_string(),"slot":slot.ordinal().to_string(),"chooser":chooser.0,"value":creature_type_id(*value)})
        }
        RulesOccurrence::ZoneChange(value) => {
            let mut row = json!({"tag":"ZoneChange","before":appearance(&value.before),"after":appearance(&value.after),"reason":zone_reason(value.reason)});
            if !value.additional_arrivals.is_empty() {
                row["additionalArrivals"] = json!(
                    value
                        .additional_arrivals
                        .iter()
                        .map(appearance)
                        .collect::<Vec<_>>()
                );
            }
            if value.madness {
                row["madness"] = json!(true);
            }
            row
        }
        RulesOccurrence::StatusChange { change, reason } => {
            json!({"tag":"StatusChange","before":object(&change.before),"after":object(&change.after),"reason":opaque(reason)})
        }
        RulesOccurrence::StackExiledToEndTurn { entries } => {
            json!({"tag":"StackExiledToEndTurn","entries":entries.iter().map(opaque).collect::<Vec<_>>()})
        }
        RulesOccurrence::GraveyardReturnPaid {
            controller,
            mana_value,
            source,
        } => {
            json!({"tag":"GraveyardReturnPaid","controller":controller.0,"mana_value":mana_value,"source":opaque(source.object.id())})
        }
        RulesOccurrence::CardVoteCast { player, object } => {
            json!({"tag":"CardVoteCast","player":player.0,"object":opaque(object)})
        }
        RulesOccurrence::CoinFlipped { player, won } => {
            json!({"tag":"CoinFlipped","player":player.0,"won":won})
        }
        RulesOccurrence::Shuffled(value) => json!({"tag":"Shuffled","value":shuffle(value)}),
        RulesOccurrence::FailedDraw { player, attempts } => {
            json!({"tag":"FailedDraw","player":player.0,"attempts":attempts})
        }
        RulesOccurrence::PermanentTargeted {
            object,
            entry,
            controller,
            ..
        } => {
            json!({"tag":"WardTargeted","object":opaque(object),"entry":opaque(entry),"controller":controller.0})
        }
        RulesOccurrence::RegenerationShieldCreated {
            object,
            entry,
            instruction,
        } => {
            json!({"tag":"RegenerationShieldCreated","object":opaque(object),"entry":opaque(entry),"instruction":instruction})
        }
        RulesOccurrence::PermanentRegenerated {
            object,
            entry,
            instruction,
            removed_damage,
            removed_from_combat,
        } => {
            json!({"tag":"PermanentRegenerated","object":opaque(object),"entry":opaque(entry),"instruction":instruction,"removed_damage":removed_damage,"removed_from_combat":removed_from_combat})
        }
        RulesOccurrence::CreaturesGoaded { player, objects } => {
            json!({"tag":"CreaturesGoaded","player":player.0,"objects":objects.iter().map(opaque).collect::<Vec<_>>()})
        }
        RulesOccurrence::AdditionalCombatCreated => json!({"tag":"AdditionalCombatCreated"}),
        RulesOccurrence::ExtraTurnCreated { player } => {
            json!({"tag":"ExtraTurnCreated","player":player.0})
        }
        RulesOccurrence::CardDrawOrdinal { player, number } => {
            json!({"tag":"CardDrawOrdinal","player":player.0,"number":number})
        }
        RulesOccurrence::CityBlessingGained { player } => {
            json!({"tag":"CityBlessingGained","player":player.0})
        }
        RulesOccurrence::MonarchChanged { before, after } => {
            json!({"tag":"MonarchChanged","before":before.map(|seat|seat.0),"after":after.map(|seat|seat.0)})
        }
        RulesOccurrence::TurnPreventionCreated {
            creator,
            instruction,
            controller,
            filter,
        } => {
            json!({"tag":"TurnPreventionCreated","creator":opaque(creator),"instruction":instruction,"controller":controller.0,"filter":opaque(filter)})
        }
        RulesOccurrence::CommanderTypeManaSpent {
            source,
            player,
            units,
        } => {
            json!({"tag":"CommanderTypeManaSpent","source":appearance(source),"player":player.0,"units":units})
        }
        RulesOccurrence::ReflexiveDiscardDamage {
            source,
            controller,
            amount,
        } => {
            json!({"tag":"ReflexiveDiscardDamage","source":appearance(source),"controller":controller.0,"amount":amount})
        }
        RulesOccurrence::ManaAddedByEffect {
            entry,
            player,
            produced,
            before,
            after,
        } => {
            json!({"tag":"ManaAddedByEffect","entry":opaque(entry),"player":player.0,"produced":program::mana_values(produced),"before":program::mana_values(before),"after":program::mana_values(after)})
        }
        RulesOccurrence::AttackCostPaid {
            player,
            cost,
            white_symbols_paid_with_life,
            mana,
        } => {
            json!({"tag":"AttackCostPaid","player":player.0,"generic":cost.generic,"white_phyrexian":cost.white_phyrexian,"life_symbols":white_symbols_paid_with_life,"mana":mana.iter().map(|s|json!([format!("{:?}",s.mana_type),s.amount])).collect::<Vec<_>>()})
        }
        RulesOccurrence::AttackDefenderChanged { attacker, from, to } => {
            json!({"tag":"AttackDefenderChanged","attacker":opaque(attacker),"from":target(*from),"to":target(*to)})
        }
        RulesOccurrence::Mutated { permanent } => {
            json!({"tag":"Mutated","object":opaque(permanent)})
        }
        RulesOccurrence::Phased { objects, out } => {
            json!({"tag":"Phased","objects":objects.iter().map(opaque).collect::<Vec<_>>(),"out":out})
        }
        RulesOccurrence::EmblemActivated {
            entry,
            emblem,
            controller,
        } => {
            json!({"tag":"EmblemActivated","entry":opaque(entry),"emblem":emblem.sequence(),"controller":controller.0})
        }
        RulesOccurrence::EmblemCreated { emblem } => {
            json!({"tag":"EmblemCreated","id":emblem.id.sequence(),"controller":emblem.controller.0,"definition":format!("{:?}", emblem.definition)})
        }
        RulesOccurrence::EmblemDamagePrevented {
            emblem,
            source,
            batch,
            stage,
            recipient,
            allocations,
        } => {
            json!({"tag":"EmblemDamagePrevented","emblem":opaque(emblem),"source":opaque(source),"batch":opaque(batch),"stage":stage,"recipient":opaque(recipient),"allocations":allocations.iter().map(|a|json!({"attempt":opaque(a.attempt),"amount":a.amount})).collect::<Vec<_>>()})
        }
        RulesOccurrence::StaticDamageMultiplied {
            source,
            batch,
            stage,
            attempt,
            before,
            after,
        } => json!({
            "tag":"StaticDamageMultiplied","source":opaque(source),"batch":opaque(batch),
            "stage":stage,"attempt":opaque(attempt),"before":before,"after":after,
        }),
        RulesOccurrence::DamageReplacedWithMill {
            source,
            batch,
            stage,
            player,
            allocations,
        } => {
            json!({"tag":"DamageReplacedWithMill","source":opaque(source),"batch":opaque(batch),"stage":stage,
                "player":player.0,"allocations":allocations.iter().map(|a|json!({"attempt":opaque(a.attempt),"amount":a.amount})).collect::<Vec<_>>()})
        }
        RulesOccurrence::StaticDamagePrevented {
            source,
            batch,
            stage,
            recipient,
            allocations,
        } => {
            json!({"tag":"StaticDamagePrevented","source":opaque(source),"batch":opaque(batch),"stage":stage,"recipient":opaque(recipient),"allocations":allocations.iter().map(|a|json!({"attempt":opaque(a.attempt),"amount":a.amount})).collect::<Vec<_>>()})
        }
        RulesOccurrence::TurnDamagePrevented {
            creator,
            instruction,
            batch,
            stage,
            recipient,
            allocations,
        } => {
            json!({"tag":"TurnDamagePrevented","creator":opaque(creator),"instruction":instruction,"batch":opaque(batch),"stage":stage,"recipient":opaque(recipient),"allocations":allocations.iter().map(|a|json!({"attempt":opaque(a.attempt),"amount":a.amount})).collect::<Vec<_>>()})
        }
        RulesOccurrence::SpellControllerChanged {
            entry,
            before,
            after,
        } => {
            json!({"tag":"SpellControllerChanged","entry":opaque(entry),"before":before.0,"after":after.0})
        }
        RulesOccurrence::StackTargetsChanged {
            entry,
            before,
            after,
        } => {
            let choices = |bounds: &[spellwake_engine::targets::TargetChoice]| {
                bounds.iter().map(|choice| json!({"id":choice.id.0,"clause":choice.clause.0,"target":target(choice.target)})).collect::<Vec<_>>()
            };
            json!({"tag":"StackTargetsChanged","entry":opaque(entry),"before":choices(before),"after":choices(after)})
        }
        RulesOccurrence::AbilityCountered { entry } => {
            json!({"tag":"AbilityCountered","entry":opaque(entry)})
        }
        RulesOccurrence::SpeedChanged {
            player,
            before,
            after,
        } => json!({"tag":"SpeedChanged","player":player.0,"before":before,"after":after}),
        RulesOccurrence::PowerAbilityResolved {
            source,
            kind,
            contributors,
            charge_placed,
        } => {
            let mut value = json!({"tag":"PowerAbilityResolved","source":opaque(source),"kind":format!("{kind:?}"),"contributors":contributors.iter().map(opaque).collect::<Vec<_>>()});
            if let Some(amount) = charge_placed {
                value["charge_placed"] = json!(amount);
            }
            value
        }
        RulesOccurrence::DayNightChanged { before, after } => {
            json!({"tag":"DayNightChanged","before":format!("{before:?}"),"after":format!("{after:?}")})
        }
        RulesOccurrence::Transformed { before, after } => {
            json!({"tag":"Transformed","before":appearance(before),"after":appearance(after)})
        }
        RulesOccurrence::TurnedFaceUp {
            before,
            after,
            ability,
        } => {
            json!({"tag":"TurnedFaceUp","before":appearance(before),"after":appearance(after),"ability":ability})
        }
        RulesOccurrence::FaceDownRevealed { object: value } => {
            json!({"tag":"FaceDownRevealed","object":object(value)})
        }
        RulesOccurrence::Explored { source, controller } => {
            json!({"tag":"Explored","source":object(source),"controller":controller.0})
        }
        RulesOccurrence::TurnedFaceUpByEffect { before, after } => {
            json!({"tag":"TurnedFaceUpByEffect","before":appearance(before),"after":appearance(after)})
        }
        RulesOccurrence::LibraryCardRevealed { object: value } => {
            json!({"tag":"LibraryCardRevealed","object":object(value)})
        }
        RulesOccurrence::MiracleRevealed { object: value } => {
            json!({"tag":"MiracleRevealed","object":object(&value.object)})
        }
        RulesOccurrence::HandCardRevealed { object: value } => {
            json!({"tag":"HandCardRevealed","object":object(value)})
        }
        RulesOccurrence::AttachmentChanged {
            source,
            before,
            after,
        } => {
            json!({"tag":"AttachmentChanged","source":opaque(source),"before":before.map(target),"after":after.map(target)})
        }
        RulesOccurrence::CastCompleted(value) => {
            json!({"tag":"CastCompleted","cast":cast_occurrence(value)})
        }
        RulesOccurrence::ActivatedAbility {
            entry,
            source,
            ability,
        } => {
            json!({"tag":"ActivatedAbility","entry":opaque(entry),"source":object(source),"ability":ability.0})
        }
        RulesOccurrence::ManaAbility {
            source,
            ability,
            controller,
        } => {
            json!({"tag":"ManaAbility","source":object(source),"ability":mana_ref::view(*ability),"controller":controller.0})
        }
        RulesOccurrence::ManaProduced {
            production: value,
            before,
            after,
        } => {
            json!({"tag":"ManaProduced","production":production(value),"before":before.iter().map(|(kind,amount)|(opaque(kind),amount)).collect::<BTreeMap<_,_>>(),"after":after.iter().map(|(kind,amount)|(opaque(kind),amount)).collect::<BTreeMap<_,_>>()})
        }
        RulesOccurrence::ManaDebited {
            payment: value,
            mana_type,
            amount,
            before,
            after,
        } => {
            json!({"tag":"ManaDebited","payment":payment(value),"type":opaque(mana_type),"amount":amount,"before":before,"after":after})
        }
        RulesOccurrence::ManaExpired {
            player,
            expired,
            remaining,
        } => {
            json!({"tag":"ManaExpired","player":player.0,"expired":program::mana_values(expired),"remaining":program::mana_values(remaining)})
        }
        RulesOccurrence::ArtifactTappedForMana { source, actor } => {
            json!({"tag":"ArtifactTappedForMana","source":opaque(source),"actor":actor.0})
        }
        RulesOccurrence::ManaPoolEmptied { player, previous } => {
            json!({"tag":"ManaPoolEmptied","player":player.0,"previous":previous.iter().map(|(kind,amount)|(opaque(kind),amount)).collect::<BTreeMap<_,_>>()})
        }
        RulesOccurrence::AbilityCopied {
            entry,
            original,
            controller,
            targets,
        } => {
            json!({"tag":"AbilityCopied","entry":opaque(entry),"original":opaque(original),"controller":controller.0,"targets":targets.iter().map(|t|json!({"id":t.id.0,"clause":t.clause.0,"target":target(t.target)})).collect::<Vec<_>>()})
        }
        RulesOccurrence::CopyCreated {
            copiable_keywords,
            copiable_card_types,
            copiable_creature_subtypes,
            copiable_spell_subtypes,
            copiable_artifact_subtypes,
            copiable_enchantment_subtypes,
            copiable_names,
            copiable_supertypes,
            copiable_triggered,
            entry,
            original,
            controller,
            targets,
        } => {
            let mut value = json!({"tag":"CopyCreated","copiable_card_types":copiable_card_types.iter().map(opaque).collect::<Vec<_>>(),"copiable_creature_subtypes":copiable_creature_subtypes.iter().map(|id|creature_type_id(*id)).collect::<Vec<_>>(),"copiable_names":copiable_names,"copiable_supertypes":copiable_supertypes.iter().map(opaque).collect::<Vec<_>>(),"copiable_triggered":program::triggered_abilities(copiable_triggered),"entry":opaque(entry),"original":opaque(original),"controller":controller.0,"targets":targets.iter().map(|choice|json!({"clause":choice.clause.0,"target":target(choice.target)})).collect::<Vec<_>>()});
            if !copiable_spell_subtypes.is_empty() {
                value["copiable_spell_subtypes"] = json!(
                    copiable_spell_subtypes
                        .iter()
                        .map(|kind| kind.wire_label())
                        .collect::<Vec<_>>()
                );
            }
            if !copiable_artifact_subtypes.is_empty() {
                value["copiable_artifact_subtypes"] = json!(
                    copiable_artifact_subtypes
                        .iter()
                        .map(|kind| kind.wire_label())
                        .collect::<Vec<_>>()
                );
            }
            if !copiable_enchantment_subtypes.is_empty() {
                value["copiable_enchantment_subtypes"] = json!(
                    copiable_enchantment_subtypes
                        .iter()
                        .map(|kind| kind.wire_label())
                        .collect::<Vec<_>>()
                );
            }
            if !copiable_keywords.is_empty() {
                value["copiable_keywords"] =
                    json!(copiable_keywords.iter().map(opaque).collect::<Vec<_>>());
            }
            value
        }
        RulesOccurrence::WorldTenureChanged(change) => {
            let (kind, object, tenure) = match change {
                spellwake_engine::characteristics::WorldTenureChange::Gained { object, tenure } => {
                    ("Gained", object, tenure)
                }
                spellwake_engine::characteristics::WorldTenureChange::Lost { object, tenure } => {
                    ("Lost", object, tenure)
                }
            };
            json!({"tag":"WorldTenureChanged","change":{"tag":kind,"object":opaque(object),"tenure":tenure.sequence().to_string()}})
        }
        RulesOccurrence::CopyMoved { before, after } => {
            json!({"tag":"CopyMoved","before":copy(before),"after":copy(after)})
        }
        RulesOccurrence::CopyCeased { copy: value } => {
            json!({"tag":"CopyCeased","copy":copy(value)})
        }
        RulesOccurrence::PlayerProtectionGranted { player, color } => {
            json!({"tag":"PlayerProtectionGranted","player":player.0,"color":format!("{color:?}")})
        }
        RulesOccurrence::ContinuousEffectCreated {
            effect,
            appearances,
        } => {
            json!({"tag":"ContinuousEffectCreated","effect":continuous_effect(effect),"appearances":appearances.iter().map(appearance_change).collect::<Vec<_>>()})
        }
        RulesOccurrence::SourceControlEffectsEnded { effects } => {
            json!({"tag":"SourceControlEffectsEnded","effects":effects.iter().map(opaque).collect::<Vec<_>>()})
        }
        RulesOccurrence::CleanupCharacteristics {
            expired_effects,
            marks_removed,
            appearances,
        } => {
            json!({"tag":"CleanupCharacteristics","expired_effects":expired_effects.iter().map(opaque).collect::<Vec<_>>(),"marks_removed":marks_removed.iter().map(|removal|json!({"object":opaque(removal.object),"previous":removal.previous})).collect::<Vec<_>>(),"appearances":appearances.iter().map(appearance_change).collect::<Vec<_>>()})
        }
        RulesOccurrence::CombatantsRemoved { participants } => {
            json!({"tag":"CombatantsRemoved","participants":participants.iter().map(combatant).collect::<Vec<_>>()})
        }
        RulesOccurrence::TimeCounterRemoved {
            object,
            before,
            after,
        } => {
            json!({"tag":"TimeCounterRemoved","object":opaque(object),"before":before,"after":after})
        }
        RulesOccurrence::FadeCounterRemoved {
            object,
            before,
            after,
        } => {
            json!({"tag":"FadeCounterRemoved","object":opaque(object),"before":before,"after":after})
        }
        RulesOccurrence::PermanentCountersDoubled { selected } => {
            json!({"tag":"PermanentCountersDoubled","selected":selected.iter().map(|(who,kinds)|json!({"recipient":target(*who),"counters":kinds.iter().map(opaque).collect::<Vec<_>>()})).collect::<Vec<_>>()})
        }
        RulesOccurrence::Proliferated { player, selected } => {
            json!({"tag":"Proliferated","player":player.0,"selected":selected.iter().map(|(who,kinds)|json!({"recipient":target(*who),"counters":kinds.iter().map(opaque).collect::<Vec<_>>()})).collect::<Vec<_>>()})
        }
        RulesOccurrence::AgeCounterPlaced {
            object,
            before,
            after,
        } => {
            json!({"tag":"AgeCounterPlaced","object":opaque(object),"before":before,"after":after})
        }
        RulesOccurrence::LoreCountersChanged {
            object,
            before,
            after,
            exact_chapter_only,
        } => {
            json!({"tag":"LoreCountersChanged","object":opaque(object),"before":before,"after":after,"exact_chapter_only":exact_chapter_only})
        }
        RulesOccurrence::SubtypeThresholdMet {
            object,
            definition,
            grant,
        } => {
            json!({"tag":"SubtypeThresholdMet","object":opaque(object),"definition":opaque(definition),"grant":format!("{grant:?}")})
        }
        RulesOccurrence::CounterThresholdMet {
            object,
            definition,
            grant,
            kind,
        } => {
            if *kind == spellwake_engine::effects::CounterKind::Page {
                json!({"tag":"PageThresholdMet","object":opaque(object),"definition":opaque(definition),"grant":format!("{grant:?}")})
            } else {
                json!({"tag":"CounterThresholdMet","object":opaque(object),"definition":opaque(definition),"grant":format!("{grant:?}"),"kind":format!("{kind:?}")})
            }
        }
        RulesOccurrence::PermanentCounterChanged {
            object,
            kind,
            before,
            after,
        } => {
            json!({"tag":"PermanentCounterChanged","object":opaque(object),"kind":format!("{kind:?}"),"before":before,"after":after})
        }
        RulesOccurrence::SuspectCountersChanged {
            object,
            before,
            after,
        } => {
            json!({"tag":"SuspectCountersChanged","object":opaque(object),"before":before,"after":after})
        }
        RulesOccurrence::ChargeCountersChanged {
            object,
            before,
            after,
        } => {
            json!({"tag":"ChargeCountersChanged","object":opaque(object),"before":before,"after":after})
        }
        RulesOccurrence::StorageCountersChanged {
            object,
            before,
            after,
        } => {
            json!({"tag":"StorageCountersChanged","object":opaque(object),"before":before,"after":after})
        }
        RulesOccurrence::LoyaltyCountersChanged {
            object,
            before,
            after,
        } => {
            json!({"tag":"LoyaltyCountersChanged","object":opaque(object),"before":before,"after":after})
        }
        RulesOccurrence::CountersPlaced {
            object,
            before,
            added,
            after,
            before_appearance,
            after_appearance,
        } => {
            json!({"tag":"CountersPlaced","object":opaque(object),"before":program::counters(before),"added":program::counters(added),"after":program::counters(after),"before_appearance":appearance(before_appearance),"after_appearance":appearance(after_appearance)})
        }
        RulesOccurrence::CountersCancelled {
            object,
            before,
            removed,
            after,
        } => {
            json!({"tag":"CountersCancelled","object":opaque(object),"before":program::counters(before),"removed":program::counters(removed),"after":program::counters(after)})
        }
        RulesOccurrence::DamageEffectCreated(value) => {
            json!({"tag":"DamageEffectCreated","value":damage_trace::damage_journal_view(&DamageJournal{created:vec![value.clone()],..Default::default()}).created})
        }
        RulesOccurrence::DamagePrevented {
            application,
            damage_before,
            shield_before,
        } => {
            json!({"tag":"DamagePrevented","application":damage_trace::damage_journal_view(&DamageJournal{preventions:vec![application.clone()],..Default::default()}).preventions,"damage_before":damage_before.iter().map(|(attempt,amount)|(opaque(attempt),amount)).collect::<BTreeMap<_,_>>(),"shield_before":shield_before})
        }
        RulesOccurrence::ProtectionPrevented(value) => {
            json!({"tag":"ProtectionPrevented","value":damage_trace::protection_view(value)})
        }
        RulesOccurrence::DamageMultiplied(value) => {
            json!({"tag":"DamageMultiplied","value":damage_trace::damage_journal_view(&DamageJournal{multiplications:vec![value.clone()],..Default::default()}).multiplications})
        }
        RulesOccurrence::DamageDealt {
            damage,
            kind,
            origin,
            ..
        } => {
            json!({"tag":"DamageDealt","damage":damage_trace::damage_journal_view(&DamageJournal{applied:vec![damage.clone()],..Default::default()}).applied,"kind":opaque(kind),"origin":damage_trace::damage_origin_view(*origin)})
        }
        RulesOccurrence::DamageResult {
            recipient,
            before,
            after,
            components,
        } => {
            json!({"tag":"DamageResult","recipient":damage_trace::damage_recipient_view(*recipient),"before":before,"after":after,"components":components.iter().map(opaque).collect::<Vec<_>>()})
        }
        RulesOccurrence::PowerToughnessCountersRemoved {
            object,
            before,
            after,
        } => {
            json!({"tag":"PowerToughnessCountersRemoved","object":opaque(object),"before":program::counters(before),"after":program::counters(after)})
        }
        RulesOccurrence::CounterRemovedForGraft {
            object,
            before,
            after,
        } => {
            json!({"tag":"CounterRemovedForGraft","object":opaque(object),"before":program::counters(before),"after":program::counters(after)})
        }
        RulesOccurrence::CreatureEvolved { object } => {
            json!({"tag":"CreatureEvolved","object":opaque(object)})
        }
        RulesOccurrence::CreatureMentored { source, target } => {
            json!({"tag":"CreatureMentored","source":opaque(source),"target":opaque(target)})
        }
        RulesOccurrence::BecameRenowned { object } => {
            json!({"tag":"BecameRenowned","object":opaque(object)})
        }
        RulesOccurrence::ExperienceCountersChanged {
            player,
            before,
            after,
        } => {
            json!({"tag":"ExperienceCountersChanged","player":player.0,"before":before,"after":after})
        }
        RulesOccurrence::PlayerCounterChanged {
            player,
            kind,
            before,
            after,
        } => {
            json!({"tag":"PlayerCounterChanged","player":player.0,"kind":opaque(kind),"before":before,"after":after})
        }
        RulesOccurrence::PlayerPoisoned {
            player,
            before,
            after,
            contributions,
        } => {
            json!({"tag":"PlayerPoisoned","player":player.0,"before":before,"after":after,"contributions":contributions.iter().map(|c|json!({"origin":opaque(c.origin),"source":opaque(c.source),"controller":c.controller.0,"amount":c.amount})).collect::<Vec<_>>()})
        }
        RulesOccurrence::PlayerLost { player, reasons } => {
            json!({"tag":"PlayerLost","player":player.0,"reasons":reasons.iter().map(opaque).collect::<Vec<_>>()})
        }
        RulesOccurrence::TurnsSkipped {
            players,
            expired_effects,
        } => {
            json!({"tag":"TurnsSkipped","players":players.iter().map(|seat|seat.0).collect::<Vec<_>>(),"expired_effects":expired_effects.iter().map(opaque).collect::<Vec<_>>()})
        }
        RulesOccurrence::PermanentLeftGame { before } => {
            json!({"tag":"PermanentLeftGame","object":opaque(before.object.id())})
        }
        RulesOccurrence::AttackersDeclared {
            origin,
            selected,
            membership,
        } => {
            json!({"tag":"AttackersDeclared","origin":opaque(origin),"selected":selected.iter().map(attack).collect::<Vec<_>>(),"membership":membership.iter().map(attacking_creature).collect::<Vec<_>>()})
        }
        RulesOccurrence::BlockersDeclared {
            origin,
            selected,
            membership,
            statuses,
        } => {
            json!({"tag":"BlockersDeclared","origin":opaque(origin),"selected":selected.iter().map(block).collect::<Vec<_>>(),"membership":membership.iter().map(blocking_creature).collect::<Vec<_>>(),"statuses":statuses.iter().map(|(object,before,after)|json!({"object":opaque(object),"before":opaque(before),"after":opaque(after)})).collect::<Vec<_>>()})
        }
        RulesOccurrence::CombatDamageAssigned(value) => {
            json!({"tag":"CombatDamageAssigned","value":assignment(value)})
        }
        RulesOccurrence::CombatEnded { previous } => {
            json!({"tag":"CombatEnded","previous":previous.as_ref().map(combat)})
        }
        RulesOccurrence::FinalSagaChapterCompleted {
            saga,
            controller,
            mana_value,
        } => {
            json!({"tag":"FinalSagaChapterCompleted","saga":opaque(saga),"controller":controller.0,"mana_value":mana_value})
        }
        RulesOccurrence::EntryCompleted {
            entry,
            controller,
            kind: value,
            completion,
        } => {
            json!({"tag":"EntryCompleted","entry":opaque(entry),"controller":controller.0,"kind":kind(value),"completion":opaque(completion)})
        }
        RulesOccurrence::StepBegan { position, active } => {
            json!({"tag":"StepBegan","position":damage_trace::turn_position_view(*position),"active":active.0})
        }
        RulesOccurrence::ReflexiveTrigger {
            source,
            controller,
            body,
        } => {
            json!({"tag":"ReflexiveTrigger","source":appearance(source),"controller":controller.0,"body":program::body(body)})
        }
    }
}
/// Common exact placement fields for journals and pending executor snapshots.
pub fn trigger_placement(value: &spellwake_engine::occurrence::TriggerPlacement) -> Value {
    json!({"instance":value.instance.sequence().to_string(),"wave":value.wave.sequence().to_string(),
        "controller":value.controller.0,"position":value.position,"entry":opaque(value.entry)})
}
/// Current native required target frame, using the shared TargetRef encoder.
pub fn trigger_target_view(value: &spellwake_engine::occurrence::TriggerTargetChoice) -> Value {
    let mut result = json!({"revision":value.revision().to_string(),"boundary":check_trace::check_boundary_view(value.boundary()),
        "event":check_trace::check_event_view(value.event()),"wave":value.wave().sequence().to_string(),
        "chooser":value.chooser().0,"instance":value.instance().sequence().to_string(),"entry":opaque(value.entry()),
        "domain":value.domain().iter().copied().map(target).collect::<Vec<_>>()});
    if let Some(modal) = value.modal() {
        result["modes"] = json!({
            "minimum": modal.minimum, "maximum": modal.maximum, "distinct_targets": modal.distinct_targets,
            "options": modal.modes.iter().map(|mode| json!({
                "index": mode.index, "target_required": mode.target_required, "multiple": mode.multiple,
                "targets": mode.targets.iter().copied().map(target).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
    }
    if let Some((min, max)) = value.permanent_target_bounds() {
        result["minimum"] = json!(min);
        result["maximum"] = json!(max);
    }
    if let Some((min, max)) = value.ordered_any_target_bounds() {
        result["ordered_any_minimum"] = json!(min);
        result["ordered_any_maximum"] = json!(max);
    }
    result
}
/// One actual journal record; pending placement metadata shares only its fields.
pub fn rules_record(record: &RulesRecord) -> Value {
    match record {
        RulesRecord::AttackDeclarationReversed {
            player,
            attackers,
            kept_mana_abilities,
            reversed_mana,
            retracted_tap_triggers,
        } => {
            json!({"tag":"AttackDeclarationReversed","player":player.0,"attackers":attackers.iter().map(opaque).collect::<Vec<_>>(),"kept_mana_abilities":kept_mana_abilities,"reversed_mana":reversed_mana.iter().map(|id|id.0).collect::<Vec<_>>(),"retracted_tap_triggers":retracted_tap_triggers.iter().map(|id|id.sequence()).collect::<Vec<_>>()})
        }
        RulesRecord::CastAnnouncementReversed {
            entry,
            stack_object,
            restored,
        } => {
            json!({"tag":"CastAnnouncementReversed","entry":opaque(entry),"stack_object":opaque(stack_object),"restored":opaque(restored)})
        }
        RulesRecord::GameConcluded { conclusion } => {
            let mut encoded = json!({"tag":"GameConcluded","outcome":opaque(conclusion.outcome()),"cause":match conclusion.cause() {spellwake_engine::game::GameConclusionCause::Conceded { player } => json!({"tag":"Conceded","player":player.0}),spellwake_engine::game::GameConclusionCause::CheckedLosses {players} => json!({"tag":"CheckedLosses","players":players.iter().map(|p|p.0).collect::<Vec<_>>()}),
                    spellwake_engine::game::GameConclusionCause::WonByEffect {winner,source,other_players} =>
                        json!({"tag":"WonByEffect","winner":winner.0,"source":opaque(source),"other_players":other_players.iter().map(|p|p.0).collect::<Vec<_>>()})}});
            if !conclusion.face_down_reveals().is_empty() {
                encoded["face_down_reveals"] = json!(
                    conclusion
                        .face_down_reveals()
                        .iter()
                        .map(object)
                        .collect::<Vec<_>>()
                );
            }
            encoded
        }
        RulesRecord::OptionalEffectOutcome {
            entry,
            instruction,
            chooser,
            outcome,
            next,
        } => {
            json!({"tag":"OptionalEffectOutcome","entry":opaque(entry),"instruction":instruction.to_string(),"chooser":chooser.0,"outcome":opaque(outcome),"next":next.to_string()})
        }
        RulesRecord::ChooserReplaced {
            entry,
            instruction,
            departed,
            chooser,
            selected,
        } => {
            json!({"tag":"ChooserReplaced","entry":opaque(entry),"instruction":instruction.to_string(),"departed":departed.0,"chooser":chooser.0,"selected":selected.0})
        }
        RulesRecord::ResolutionPaymentOutcome {
            entry,
            instruction,
            payer,
            cost,
            restricted_colors,
            outcome,
            next,
        } => {
            let mut encoded = json!({"tag":"ResolutionPaymentOutcome","entry":opaque(entry),"instruction":instruction.to_string(),"payer":payer.0,"cost":program::cost(cost),"outcome":match outcome {
            ResolutionPaymentOutcome::Paid(record) => json!({"tag":"Paid","payment":payment(record)}),
            ResolutionPaymentOutcome::Declined => json!({"tag":"Declined"}),
            ResolutionPaymentOutcome::UnpaidDeparted => json!({"tag":"UnpaidDeparted"}),
        },"next":next.to_string()});
            if let Some(colors) = restricted_colors {
                encoded["restricted_colors"] = json!(colors.iter().map(opaque).collect::<Vec<_>>());
            }
            encoded
        }
        RulesRecord::InstructionBranch {
            entry,
            instruction,
            condition,
            left,
            right,
            result,
            next,
        } => {
            json!({"tag":"InstructionBranch","entry":opaque(*entry),"instruction":instruction.to_string(),"condition":super::program::resolution_condition(condition),"left":left,"right":right,"result":result,"next":next.to_string()})
        }
        RulesRecord::InstructionJump {
            entry,
            instruction,
            next,
        } => {
            json!({"tag":"InstructionJump","entry":opaque(*entry),"instruction":instruction.to_string(),"next":next.to_string()})
        }
        RulesRecord::Event(value) => {
            json!({"tag":"Event","event":value.id.sequence().to_string(),"cause":cause(&value.cause),"occurrences":value.occurrences.iter().map(|row|json!({"id":occurrence_id(row.id),"value":occurrence(&row.value)})).collect::<Vec<_>>()})
        }
        RulesRecord::SpeedTriggerDiscovered {
            instance,
            controller,
            causes,
        } => {
            json!({"tag":"SpeedTriggerDiscovered","instance":opaque(instance),"controller":controller.0,"causes":causes.iter().map(opaque).collect::<Vec<_>>()})
        }
        RulesRecord::EmblemTriggerDiscovered {
            instance,
            controller,
            emblem,
            cause,
        } => {
            json!({"tag":"EmblemTriggerDiscovered","instance":opaque(instance),"controller":controller.0,"emblem":emblem.sequence(),"cause":opaque(cause)})
        }
        RulesRecord::DelayedCopyTriggerDiscovered {
            instance,
            controller,
            original,
            cause,
        } => {
            json!({"tag":"DelayedCopyTriggerDiscovered","instance":opaque(instance),"controller":controller.0,"original":opaque(original),"cause":opaque(cause)})
        }
        RulesRecord::MonarchTriggerDiscovered {
            instance,
            controller,
            beneficiary,
            causes,
        } => {
            json!({"tag":"MonarchTriggerDiscovered","instance":opaque(instance),"controller":controller.0,"beneficiary":beneficiary.map(|seat|seat.0),"causes":causes.iter().map(opaque).collect::<Vec<_>>() })
        }
        RulesRecord::TriggerDiscovered(value) => {
            json!({"tag":"TriggerDiscovered","value":waiting(value)})
        }
        RulesRecord::TriggerPlacementStarted(value) | RulesRecord::TriggerPlaced(value) => {
            let mut row = trigger_placement(value);
            row["tag"] = json!(if matches!(record, RulesRecord::TriggerPlaced(_)) {
                "TriggerPlaced"
            } else {
                "TriggerPlacementStarted"
            });
            row
        }
        RulesRecord::TriggerTargetSelected {
            placement,
            target: selected,
        } => {
            let mut row = trigger_placement(placement);
            row["tag"] = json!("TriggerTargetSelected");
            row["target"] = json!({"id":opaque(selected.id),"clause":selected.clause.0,"target":target(selected.target)});
            row
        }
        RulesRecord::TriggerRemoved { placement, reason } => {
            let mut row = trigger_placement(placement);
            row["tag"] = json!("TriggerRemoved");
            row["reason"] = json!(match reason {
                spellwake_engine::occurrence::TriggerRemovalReason::NoLegalTargets =>
                    "NoLegalTargets",
            });
            row
        }
    }
}
pub fn journal(value: &RulesJournal) -> Value {
    json!(value.records.iter().map(rules_record).collect::<Vec<_>>())
}
/// Project only the actual operation's drawn prefix and empty remainder.
pub fn draws(value: &DrawJournal) -> Value {
    json!(value.events.iter().map(|row|match row {DrawEvent::Drawn{player,before,after}=>json!({"tag":"Drawn","player":player.0,"before":object(before),"after":object(after)}),DrawEvent::EmptyLibrary{player,attempts}=>json!({"tag":"EmptyLibrary","player":player.0,"attempts":attempts})}).collect::<Vec<_>>())
}
fn declarations(value: &[CombatDeclarationRecord]) -> Value {
    json!(value.iter().map(|row|match row {
        CombatDeclarationRecord::Attackers{player,origin,attackers,taps}=>json!({"tag":"Attackers","player":player.0,"origin":opaque(origin),"attackers":attackers.iter().map(attack).collect::<Vec<_>>(),"taps":taps.iter().map(|tap|json!({"before":object(&tap.before),"after":object(&tap.after)})).collect::<Vec<_>>()}),
        CombatDeclarationRecord::Blockers{player,origin,blockers}=>json!({"tag":"Blockers","player":player.0,"origin":opaque(origin),"blockers":blockers.iter().map(block).collect::<Vec<_>>()})
    }).collect::<Vec<_>>())
}
fn assignments(value: &[CombatAssignmentRecord]) -> Value {
    json!(value.iter().map(|row|json!({"player":row.player.0,"origin":opaque(row.origin),"assignments":row.assignments.iter().map(assignment).collect::<Vec<_>>()})).collect::<Vec<_>>())
}
fn permanent_entry(value: &PermanentEntryRecord) -> Value {
    json!({"entry":opaque(value.entry),"before":object(&value.before),"after":object(&value.after)})
}
fn completion(value: &RuleCompletion) -> Value {
    match value {
        RuleCompletion::Checks(value) => {
            json!({"tag":"Checks","boundary":check_trace::check_boundary_view(value.boundary()),"performed":value.performed(),"waiting_triggers":value.waiting_triggers()})
        }
        RuleCompletion::Resolution => json!({"tag":"Resolution"}),
        RuleCompletion::GameEnded => json!({"tag":"GameEnded"}),
        RuleCompletion::ManaAbility => json!({"tag":"ManaAbility"}),
        RuleCompletion::LinkedReturn => json!({"tag":"LinkedReturn"}),
        RuleCompletion::LandPlay => json!({"tag":"LandPlay"}),
        RuleCompletion::ActivationCost => json!({"tag":"ActivationCost"}),
        RuleCompletion::TurnWork(value) => {
            let work = match value {
                WorkCompleted::Untap => json!({"tag":"Untap"}),
                WorkCompleted::Draw => json!({"tag":"Draw"}),
                WorkCompleted::DeclareAttackers => json!({"tag":"DeclareAttackers"}),
                WorkCompleted::DeclareBlockers => json!({"tag":"DeclareBlockers"}),
                WorkCompleted::CombatDamage {
                    additional_step_required,
                } => {
                    json!({"tag":"CombatDamage","additional_step_required":additional_step_required})
                }
                WorkCompleted::Cleanup { priority_required } => {
                    json!({"tag":"Cleanup","priority_required":priority_required})
                }
            };
            json!({"tag":"TurnWork","work":work})
        }
    }
}
/// Observe an actual executor result and local append without root metadata.
/// Check-choice references resolve from current cards or their real move-before receipts.
pub fn rule_progress(value: &RuleProgress, aliases: &AliasContext) -> Result<Value, String> {
    let execution = value.state();
    let result = match value.result() {
        RuleResult::Complete {
            checks,
            draws: drawn,
            completion: completed,
            journal,
            declarations: declared,
            assignments: assigned,
            permanent_entry: entered,
        } => {
            json!({"tag":"Complete","checks":check_trace::check_journal_view(checks,execution.window().cards(),|card|card.0.to_string())?,"draws":draws(drawn),"completion":completion(completed),"damage":damage_trace::damage_journal_view(journal),"declarations":declarations(declared),"assignments":assignments(assigned),"permanent_entry":entered.as_ref().map(permanent_entry)})
        }
        RuleResult::Pending {
            checks,
            draws: drawn,
            frame,
            journal,
            declarations: declared,
            assignments: assigned,
        } => {
            json!({"tag":"Pending","checks":check_trace::check_journal_view(checks,execution.window().cards(),|card|card.0.to_string())?,"draws":draws(drawn),"frame":effect_frame(frame,execution)?,"damage":damage_trace::damage_journal_view(journal),"declarations":declarations(declared),"assignments":assignments(assigned)})
        }
    };
    Ok(json!({"state":effect(execution,aliases)?,"result":result,"append":journal(value.append())}))
}
pub fn candidate_base(value: &CandidateBase) -> Value {
    match value {
        CandidateBase::Initialize {
            game,
            context: value,
        } => json!({"tag":"Initialize","game":game.0.to_string(),"context":context(value)}),
        CandidateBase::Advance(value) => json!({"tag":"Advance","basis":basis(value)}),
    }
}
pub fn candidate(
    value: &NativeCandidate,
    source: Option<&GameState>,
    aliases: &AliasContext,
) -> Result<Value, String> {
    if value
        .rules_journal()
        .records
        .iter()
        .any(|record| matches!(record, RulesRecord::GameConcluded { .. }))
    {
        return Err("historical candidate projection excludes terminal records".into());
    }
    let checks = check_trace::check_journal_view(
        value.check_journal(),
        source.unwrap_or(value.state()).execution().window().cards(),
        |card| card.0.to_string(),
    )?;
    let declared = declarations(value.combat_declarations());
    Ok(
        json!({"base":candidate_base(value.base()),"state":state(value.state(),aliases)?,"append":journal(value.rules_journal()),"checks":checks,"damage":damage_trace::damage_journal_view(value.damage_journal()),"declarations":declared,"assignments":assignments(value.combat_assignments()),"draws":draws(value.draw_journal()),"shuffles":value.shuffle_journal().iter().map(shuffle).collect::<Vec<_>>(),"turn_transitions":value.turn_transitions().iter().map(|row|json!({"from":row.from.map(damage_trace::turn_position_view),"to":damage_trace::turn_position_view(row.to),"active_before":row.active_before.0,"active_after":row.active_after.0})).collect::<Vec<_>>(),
        "mana_payment":value.mana_payment().map(payment),"mana_production":value.mana_production().map(production),"ability_activation":value.ability_activation().map(ability_activation),"land_play":value.land_play().map(|row|json!({"player":row.player.0,"before":object(&row.before),"after":object(&row.after)})),"permanent_entry":value.permanent_entry().map(permanent_entry)}),
    )
}

#[path = "error.rs"]
mod error;
pub use error::{
    definition_error, effect_error, game_error, object_identity_error, stack_window_error,
    trigger_error, turn_error,
};

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;

/// Serialize the actual closed input without a supplied seat, member vector or state.
pub fn related_player_cause_fault(fault: RelatedPlayerCauseFault) -> Value {
    json!({"kind":match fault {
        RelatedPlayerCauseFault::EmptyCauseSet => "EmptyCauseSet",
        RelatedPlayerCauseFault::MissingRelatedPlayer => "MissingRelatedPlayer",
        RelatedPlayerCauseFault::ForeignParticipant => "ForeignParticipant",
        RelatedPlayerCauseFault::AmbiguousParticipants => "AmbiguousParticipants",
    }})
}
/// Project the actual typed setup failure, distinct from a gameplay GameError.
pub fn related_player_fault_setup_error(error: &RelatedPlayerFaultSetupError) -> Value {
    json!({"tag":"Err","error":{"domain":"RelatedPlayerFaultSetupError","variant":match error {
        RelatedPlayerFaultSetupError::WrongGate => "WrongGate",
        RelatedPlayerFaultSetupError::WrongBinding => "WrongBinding",
        RelatedPlayerFaultSetupError::InsufficientCauseMembers => "InsufficientCauseMembers",
        RelatedPlayerFaultSetupError::InvalidOriginalCauseSet => "InvalidOriginalCauseSet",
        RelatedPlayerFaultSetupError::ControllerNotDistinctParticipant => "ControllerNotDistinctParticipant",
        RelatedPlayerFaultSetupError::NoForeignSeat => "NoForeignSeat",
    }}})
}
