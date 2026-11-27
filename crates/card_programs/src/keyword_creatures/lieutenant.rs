//! Controller-relative continuous and intervening-if lieutenant abilities.
use super::*;
use spellwake_engine::{
    characteristics::{
        AbilityChange, ControllerSelector, NumericExpression, NumericPair, PermanentSelector,
        StaticPart, StaticParts,
    },
    program::{
        AffectedPlayers, BodyTarget, EffectSubject, ExecutableBody, Instruction, ObjectFilter,
        ResolvingPlayer, TriggerGrouping,
    },
};
pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let rules = without_reminder(face.oracle_text.as_deref()?)?;
    let (keywords, ability) = rules.trim().split_once("\nLieutenant — ")?;
    let mut program =
        ordinary_creature(card, vocabulary, Profile::Current, Some(keywords.trim())).ok()?;
    if let Some(clause) =
        ability.strip_prefix("As long as you control your commander, this creature gets +2/+2 and ")
    {
        let source = PermanentSelector {
            controller: ControllerSelector::SourceItself,
            while_controlling_own_commander: true,
            ..Default::default()
        };
        let two = NumericExpression::Literal(2.into());
        let mut parts = vec![StaticPart::ModifyPT(NumericPair {
            power: two.clone(),
            toughness: two,
        })];
        match clause {
            "creatures you control have vigilance." => {
                let mut creatures = PermanentSelector {
                    controller: ControllerSelector::SourceController,
                    while_controlling_own_commander: true,
                    ..Default::default()
                };
                creatures.required_types.insert(CardType::Creature);
                army_grants::add_battlefield_ability(
                    &mut program,
                    creatures,
                    StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
                        [KeywordAbility::Vigilance],
                    ))]),
                )?;
            }
            "has \"Whenever this creature becomes blocked, you may draw two cards.\"" => {
                parts.push(StaticPart::GrantTrigger(Box::new(TriggeredAbility {
                    id: TriggeredAbilityId(0),
                    condition: spellwake_engine::program::TriggerSpec::blocks_or_blocked(
                        spellwake_engine::program::ObjectFilter::self_object(),
                        spellwake_engine::program::BlockTriggerKind::BecomesBlocked {
                            capture_defender: false,
                        },
                    ),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![
                            Instruction::OptionalSequence {
                                chooser: ResolvingPlayer::Controller,
                                instructions: 1,
                            },
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                                spellwake_engine::program::ValueExpr::Literal(2.into()),
                            ),
                        ],
                    ),
                })));
            }
            _ => return None,
        }
        army_grants::add_battlefield_ability(&mut program, source, StaticParts(parts))?;
    } else {
        let effect = ability.strip_prefix(
            "At the beginning of combat on your turn, if you control your commander, ",
        )?;
        let mut body = vec![Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                spellwake_engine::program::InterveningCondition::OwnCommander,
            ),
        ))];
        match effect {
            "draw a card." => body.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            ),
            "each opponent loses 3 life." => body.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                    spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::EachOpponent),
                    spellwake_engine::program::ValueExpr::literal(3),
                ),
            ),
            "put a +1/+1 counter on each creature you control." => body.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                    spellwake_engine::toolbox::ObjectQuery::zone(
                        spellwake_engine::toolbox::QueryZone::Battlefield,
                        ObjectFilter::creature().controlled(),
                    ),
                    spellwake_engine::effects::CounterKind::PlusOne,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ),
            "prevent all combat damage that would be dealt to creatures you control this turn. Other creatures you control gain vigilance until end of turn." =>
            {
                body.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                        spellwake_engine::damage::TurnPreventionSpec {
                            combat_only: true,
                            source: spellwake_engine::damage::PreventionSourceSpec::Any,
                            recipient:
                                spellwake_engine::damage::PreventionRecipientSpec::ControlledCreatures,
                            both_directions: false,
                        },
                    ),
                );
                body.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        EffectSubject::Matching(
                            ObjectFilter::creature()
                                .controlled()
                                .other_than_ability_source(),
                        ),
                        spellwake_engine::program::ValueExpr::literal(0),
                        spellwake_engine::program::ValueExpr::literal(0),
                        [KeywordAbility::Vigilance].into(),
                        std::collections::BTreeSet::new(),
                    ),
                );
            }
            _ => {
                let token = effect
                    .strip_prefix("create ")?
                    .strip_suffix(". That token gains haste until end of turn.")?;
                body.push(ordinary_text::create_tokens(token, vocabulary)?);
                body.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        EffectSubject::CreatedTokens,
                        spellwake_engine::program::ValueExpr::literal(0),
                        spellwake_engine::program::ValueExpr::literal(0),
                        [KeywordAbility::Haste].into(),
                        std::collections::BTreeSet::new(),
                    ),
                );
            }
        }
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::step_began(
                spellwake_engine::turn::Step::BeginningCombat,
                spellwake_engine::program::PlayerRelation::Controller,
            )
            .only_if(spellwake_engine::program::TriggerCondition::ControlsOwnCommander),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, body),
        });
    }
    Some(program)
}
