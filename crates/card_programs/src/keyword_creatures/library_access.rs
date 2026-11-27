//! Compose printed library-top visibility and play clauses with the remaining abilities.
use super::*;
use spellwake_engine::{
    characteristics::{LibraryTopPermission, StaticAbilityId, StaticContinuousAbility},
    land::EntryTap,
    program::ProgramKind,
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || face.name != card.name {
        return None;
    }
    let raw = face.oracle_text.as_deref()?;
    if let Some(rest) = raw.strip_prefix("Skip your draw step.\n") {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(rest.to_owned());
        let mut result = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        let id = result
            .continuous
            .iter()
            .chain(&result.additional_battlefield_abilities)
            .map(|rule| rule.id().0)
            .max()
            .map_or(Some(0), |n| n.checked_add(1))?;
        let rule = spellwake_engine::characteristics::StaticContinuousAbility::DrawRule {
            id: StaticAbilityId(id),
            rule: spellwake_engine::characteristics::StaticDrawRule::SkipControllerDrawStep,
        };
        if result.continuous.is_none() {
            result.continuous = Some(rule);
        } else {
            result.additional_battlefield_abilities.push(rule);
        }
        return Some(result);
    }
    if let Some(program) = draw_replacement(card, vocabulary, raw) {
        return Some(program);
    }
    let mut visibility = None;
    let mut permission = None;
    let mut choose_type = false;
    let mut exile_top_cost = None;
    let mut other = Vec::new();
    for line in raw.lines() {
        match line.trim() {
            line if line
                .strip_prefix("{T}, Pay ")
                .and_then(|s| s.strip_suffix(" life: Exile the top card of your library."))
                .is_some() =>
            {
                let cost = line
                    .strip_prefix("{T}, Pay ")?
                    .strip_suffix(" life: Exile the top card of your library.")?
                    .parse::<u16>()
                    .ok()?;
                if cost == 0 || exile_top_cost.replace(cost).is_some() {
                    return None;
                }
            }
            "Play with the top card of your library revealed." => {
                if visibility.replace(true).is_some() {
                    return None;
                }
            }
            "You may look at the top card of your library any time." => {
                if visibility.replace(false).is_some() {
                    return None;
                }
            }
            "As this creature enters, choose a creature type." => {
                if choose_type {
                    return None;
                }
                choose_type = true;
            }
            line if line.starts_with("You may ")
                && line.ends_with(" from the top of your library.") =>
            {
                let matching = match line
                    .strip_prefix("You may ")?
                    .strip_suffix(" from the top of your library.")?
                {
                    "play lands" => LibraryTopPermission::Land,
                    "cast instant and sorcery spells" => LibraryTopPermission::InstantOrSorcery,
                    "cast artifact spells and colorless spells" => {
                        LibraryTopPermission::ArtifactOrColorlessSpell
                    }
                    "cast creature spells of the chosen type" => {
                        LibraryTopPermission::CreatureOfSourceEntryType
                    }
                    _ => return None,
                };
                if permission.replace(matching).is_some() {
                    return None;
                }
            }
            _ => other.push(line),
        }
    }
    let (public, matching) = (visibility?, permission?);
    if choose_type != (matching == LibraryTopPermission::CreatureOfSourceEntryType) {
        return None;
    }
    let mut reduced = card.clone();
    reduced.faces[0].oracle_text = Some(other.join("\n"));
    let mut program = if other.is_empty() && face.type_line.as_deref() == Some("Artifact") {
        Program::artifact(Some(fixed_cost(face.mana_cost.as_deref()?)?), Vec::new())
    } else {
        super::compile(&reduced, vocabulary, Profile::Current).ok()?
    };
    let ProgramKind::Permanent(permanent) = &mut program.kind else {
        return None;
    };
    if choose_type
        && permanent
            .entry_tap
            .replace(EntryTap::ChooseCreatureType)
            .is_some()
    {
        return None;
    }
    let next = program
        .continuous
        .iter()
        .chain(&program.additional_battlefield_abilities)
        .map(|ability| ability.id().0)
        .max()
        .map_or(Some(0), |id| id.checked_add(1))?;
    program.additional_battlefield_abilities.extend([
        spellwake_engine::characteristics::StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(next),
            permission: spellwake_engine::characteristics::CastPermissionRule::SeeLibraryTop {
                public,
            },
        },
        spellwake_engine::characteristics::StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(next.checked_add(1)?),
            permission: spellwake_engine::characteristics::CastPermissionRule::PlayLibraryTop(
                matching,
            ),
        },
    ]);
    if let Some(life) = exile_top_cost {
        use spellwake_engine::program::{BodyTarget, ExecutableBody, ResolvingPlayer};
        let id = program
            .activated
            .iter()
            .map(|ability| ability.id.0)
            .max()
            .map_or(Some(0), |id| id.checked_add(1))?;
        program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(id),
            mana_cost: ManaCost::free(),
            source_cost: spellwake_engine::activated::SourceCost::PayLife {
                amount: spellwake_engine::activated::LifeCostAmount::Fixed(life),
                tap_source: true,
                sacrifice: None,
            },
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_library_top(
                        ResolvingPlayer::Controller,
                        spellwake_engine::program::ValueExpr::literal(1),
                        false,
                    ),
                ],
            ),
        });
    }
    program.validate(vocabulary).ok()?;
    Some(program)
}

fn draw_replacement(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    text: &str,
) -> Option<Program> {
    use spellwake_engine::characteristics::StaticDrawRule;
    let (rest, ability) = match text
        .rsplit_once('\n')
        .map_or(("", text), |(other, last)| (other, last))
    {
        (
            rest,
            "If an opponent would draw two or more cards, instead you and that player each draw a card.",
        ) => (
            rest,
            StaticContinuousAbility::DrawRule {
                id: StaticAbilityId(0),
                rule: StaticDrawRule::SplitOpponentsMultipleDraw,
            },
        ),
        (
            rest,
            "If you would draw a card, you may instead choose land or nonland and reveal cards from the top of your library until you reveal a card of the chosen kind. Put that card into your hand and put all other cards revealed this way on the bottom of your library in any order.",
        ) => (
            rest,
            StaticContinuousAbility::DrawRule {
                id: StaticAbilityId(0),
                rule: StaticDrawRule::ReplaceWithLandKind,
            },
        ),
        _ => return None,
    };
    let face = card.faces.first()?;
    let mut result = if rest.is_empty() && face.type_line.as_deref() == Some("Enchantment") {
        Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?))
    } else {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(rest.into());
        super::compile(&reduced, vocabulary, Profile::Current).ok()?
    };
    if result.continuous.is_some() {
        return None;
    }
    result.continuous = Some(ability);
    result.validate(vocabulary).ok()?;
    Some(result)
}

/// Printed impulse permissions granted by an end-step or a creature damage trigger.
pub(super) fn impulse(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::characteristics::{
        ControllerSelector, PermanentSelector, StaticPart, StaticParts,
    };
    use spellwake_engine::program::{
        BodyTarget, CardType, Comparison, ExecutableBody, Instruction, ResolutionCondition,
        TriggerGrouping,
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || face.name != card.name {
        return None;
    }
    let text = face.oracle_text.as_deref()?;
    let trigger = |condition, ops| TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, ops),
    };
    let mut program;
    if text.strip_prefix("Whenever an opponent attacks you and/or one or more planeswalkers you control, exile the top card of that player's library. ")==Some("You may play that card for as long as it remains exiled, and you may spend mana as though it were mana of any color to cast it.") && face.type_line.as_deref()==Some("Enchantment") {
        program=Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
        program.triggered.push(trigger(
            spellwake_engine::program::TriggerSpec::attacks(
                spellwake_engine::program::ObjectFilter::any(),
                spellwake_engine::program::AttackTriggerMode::OpponentsAtController { minimum: 1 },
            ),
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_library_top(
                    spellwake_engine::program::ResolvingPlayer::UniqueRelatedPlayer,
                    spellwake_engine::program::ValueExpr::literal(1),
                    false,
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::grant_play_permission(
                    spellwake_engine::toolbox::ObjectQuery::LastMoved,
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration,
                    false,
                    false,
                    true,
                ),
            ],
        ));
    } else if let Some(effect) = text
        .strip_prefix("At the beginning of your end step, if you control a creature with power ")
        .and_then(|rest| rest.split_once(" or greater, "))
    {
        if face.type_line.as_deref() != Some("Enchantment")
            || effect
                .1
                .strip_prefix("exile the top card of your library. ")
                != Some(
                    "You may play that card until you exile another card with this enchantment.",
                )
        {
            return None;
        }
        let minimum = effect.0.parse::<u16>().ok()?;
        if minimum == 0 {
            return None;
        }
        program = Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
        let gate = ResolutionCondition {
            left: spellwake_engine::program::ValueExpr::aggregate(spellwake_engine::program::ObjectFilter::creature().controlled(), spellwake_engine::program::ObjectStatKind::Power, spellwake_engine::program::StatAggregate::Max),
            comparison: Comparison::GreaterOrEqual,
            right: spellwake_engine::program::ValueExpr::Literal(i64::from(minimum).into()),
        };
        program.triggered.push(trigger(
            spellwake_engine::program::TriggerSpec::step_began(spellwake_engine::turn::Step::End, spellwake_engine::program::PlayerRelation::Controller).only_if(spellwake_engine::program::TriggerCondition::ControlledCreaturePowerAtLeast(minimum)),
            vec![
                Instruction::BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(gate))), destination: 2 },
                Instruction::ExileTopPlay {
                    count: spellwake_engine::program::ValueExpr::Literal(1.into()),
                    library_owner: spellwake_engine::program::AffectedPlayers::One(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    duration: spellwake_engine::program::ImpulsePlayDuration::UntilNextFromSource,
                    cast_only: false,
                    free: false,
                    any_color: false,
                    face_down: false,
                    grant_play: true,
                },
            ],
        ));
    } else if let Some((kind, rule)) = text.split_once(" creatures you control have \"") {
        if rule.strip_prefix("Whenever this creature deals combat damage to a player, ")
            != Some("exile the top card of your library. You may play that card this turn.\"")
        {
            return None;
        }
        let subtype = vocabulary.resolve_label(kind)?;
        program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
        program.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: PermanentSelector {
                controller: ControllerSelector::SourceController,
                required_types: [CardType::Creature].into(),
                creature_subtype: Some(subtype),
                ..PermanentSelector::default()
            },
            parts: StaticParts(vec![StaticPart::GrantTrigger(Box::new(trigger(
                spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_library_top(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                        spellwake_engine::program::ValueExpr::literal(1),
                        false,
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::grant_play_permission(
                        spellwake_engine::toolbox::ObjectQuery::LastMoved,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                        false,
                        false,
                        false,
                    ),
                ],
            )))]),
        });
    } else {
        return None;
    }
    program.validate(vocabulary).ok()?;
    Some(program)
}
