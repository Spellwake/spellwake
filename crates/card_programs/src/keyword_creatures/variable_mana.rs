//! Compile the printed multiplicity of a variable mana cost independently of its fixed symbols.
use super::{InventoryCard, Profile, without_reminder};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::program::{EntryCounterAbility, Program};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let mana = face.mana_cost.as_deref()?;
    let count = u8::try_from(mana.matches("{X}").count()).ok()?;
    let full = without_reminder(face.oracle_text.as_deref().unwrap_or_default())?;
    let mut life_price = None;
    let mut clauses = Vec::new();
    for line in full.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(amount) = line
            .strip_prefix("As an additional cost to cast this spell, pay ")
            .and_then(|s| s.strip_suffix(" life."))
        {
            let price = if amount == "X" {
                spellwake_engine::program::ValueExpr::chosen_x()
            } else {
                spellwake_engine::program::ValueExpr::Literal(super::number(Some(amount))?.into())
            };
            if life_price.replace(price).is_some() {
                return None;
            }
        } else {
            clauses.push(line);
        }
    }
    if count == 0 && life_price.is_none() {
        return None;
    }
    let text = clauses.join("\n");
    let fixed = mana.replace("{X}", "");
    let mut reduced = card.clone();
    reduced.faces[0].mana_cost = Some(if fixed.is_empty() {
        "{0}".to_owned()
    } else {
        fixed
    });
    if matches!(face.type_line.as_deref(), Some("Instant" | "Sorcery"))
        && let Some(body) = spell_body(&text, &face.name, vocabulary)
    {
        if card.layout != "normal" || card.name != face.name {
            return None;
        }
        let cost = super::fixed_cost(reduced.faces[0].mana_cost.as_deref()?)?;
        let mut program = if face.type_line.as_deref() == Some("Instant") {
            Program::instant(cost, body)
        } else {
            Program::sorcery(cost, body)
        };
        program.additional_life_cost = life_price;
        program.x_mana_symbols = count;
        return Some(program);
    }
    let mut entry_counters = 0;
    let mut remaining = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line
            .strip_suffix(" enters with X +1/+1 counters on it.")
            .is_some_and(|subject| {
                subject == "This creature"
                    || subject == face.name
                    || face
                        .name
                        .split_once(',')
                        .is_some_and(|(short, _)| subject == short)
            })
        {
            entry_counters += 1;
        } else {
            remaining.push(line);
        }
    }
    reduced.faces[0].oracle_text = Some(remaining.join("\n"));
    let mut base = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
    base.additional_life_cost = life_price;
    base.x_mana_symbols = count;
    base.entry_counter_abilities.extend(std::iter::repeat_n(
        EntryCounterAbility::ChosenXPlusOne,
        entry_counters,
    ));
    Some(base)
}

fn spell_body(
    text: &str,
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::program::ExecutableBody> {
    use spellwake_engine::program::{BodyTarget, ExecutableBody, Instruction, ValueExpr};
    if let Some(mastery) = text.trim().strip_prefix(name).and_then(|line| {
        line.strip_prefix(" deals X damage to target creature.\nSpell mastery — If there are ")
    }) && let Some((minimum, consequence)) =
        mastery.split_once(" or more instant and/or sorcery cards in your graveyard, ")
        && consequence.strip_prefix(name)
            == Some(" also deals X damage to that creature's controller.")
    {
        let minimum = u8::try_from(super::ordinary_text::amount(minimum)?).ok()?;
        return Some(ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::DamageCreatureAtSpellMastery {
                amount: ValueExpr::chosen_x(),
                minimum,
            }],
        ));
    }
    if text.trim()
        == "Search your library and/or graveyard for a creature card with mana value X or less and put it onto the battlefield. If you search your library this way, shuffle. If X is 10 or more, creatures you control get +X/+X and gain haste until end of turn."
    {
        use spellwake_engine::characteristics::KeywordAbility;
        use spellwake_engine::program::{
            CardType, Comparison, EffectSubject, LibrarySearch, ObjectFilter, ResolutionCondition,
            ResolvingPlayer, SearchCardMatcher, SearchManaValue, SearchSource,
        };
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 2,
                },
                Instruction::SearchLibrary {
                    filter: Box::new(
                        LibrarySearch::to_battlefield(
                            SearchCardMatcher::CardTypes([CardType::Creature].into()),
                            false,
                        )
                        .with_source(SearchSource::OwnLibraryOrGraveyard)
                        .with_mana_value(SearchManaValue::AtMostAnnouncedX),
                    ),
                },
                Instruction::Jump { destination: 4 },
                Instruction::PutGraveyardCreatureAtMostX,
                Instruction::BranchIfFalse {
                    condition: ResolutionCondition {
                        left: ValueExpr::chosen_x(),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::Literal(10.into()),
                    },
                    destination: 6,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                    EffectSubject::Matching(ObjectFilter::creature().controlled()),
                    ValueExpr::chosen_x(),
                    ValueExpr::chosen_x(),
                    [KeywordAbility::Haste].into(),
                    std::collections::BTreeSet::new(),
                ),
            ],
        ));
    }
    let (target, instruction) = match text.trim() {
        "Cast this spell only during combat on an opponent's turn.\nReturn X target creature cards from your graveyard to the battlefield. Sacrifice those creatures at the beginning of the next end step." => {
            (
                BodyTarget::FilteredOwnGraveyardCard(
                    spellwake_engine::program::ObjectFilter::creature(),
                ),
                Instruction::ReturnChosenXGraveyardCreatures,
            )
        }
        "Each opponent loses X life. You gain life equal to the life lost this way." => (
            BodyTarget::None,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::drain_life(
                spellwake_engine::toolbox::PlayerSelector::Group(
                    spellwake_engine::program::AffectedPlayers::EachOpponent,
                ),
                ValueExpr::chosen_x(),
                spellwake_engine::toolbox::PlayerSelector::Resolving(
                    spellwake_engine::program::ResolvingPlayer::Controller,
                ),
                true,
            ),
        ),
        "All creatures get -X/-X until end of turn." => (
            BodyTarget::None,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                spellwake_engine::program::EffectSubject::Matching(
                    spellwake_engine::program::ObjectFilter::creature(),
                ),
                ValueExpr::chosen_x().scaled(-1),
                ValueExpr::chosen_x().scaled(-1),
                Default::default(),
                Default::default(),
            ),
        ),
        "Draw X cards." => (
            BodyTarget::None,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                ValueExpr::chosen_x(),
            ),
        ),
        "Target player draws X cards." => (
            BodyTarget::SinglePlayer,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_target_player(
                ValueExpr::chosen_x(),
            ),
        ),
        other => {
            if let Some(recipient) = other
                .strip_prefix(name)
                .and_then(|s| s.strip_prefix(" deals X damage to "))
                .and_then(|s| s.strip_suffix('.'))
            {
                let target = match recipient {
                    "any target" => BodyTarget::SingleAnyTarget,
                    "target creature" => BodyTarget::SingleCreature,
                    "target player" => BodyTarget::SinglePlayer,
                    "target player or planeswalker" => BodyTarget::SinglePlayerOrPlaneswalker,
                    _ => return None,
                };
                (
                    target,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        ValueExpr::chosen_x(),
                        false,
                    ),
                )
            } else {
                let template = other.strip_prefix("Create X ")?.strip_suffix('.')?;
                let Instruction::CreateTokens { mut creations } =
                    super::ordinary_text::create_tokens(&format!("one {template}"), vocabulary)?
                else {
                    return None;
                };
                let [creation] = creations.as_mut_slice() else {
                    return None;
                };
                creation.amount = ValueExpr::chosen_x();
                (BodyTarget::None, Instruction::CreateTokens { creations })
            }
        }
    };
    Some(ExecutableBody::new(target, vec![instruction]))
}
