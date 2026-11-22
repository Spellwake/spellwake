//! Bind each printed chapter symbol to an independently triggered chapter ability.
use super::*;
use spellwake_engine::program::{
    BodyTarget, EnchantmentSubtype, ExecutableBody, Instruction, ObjectFilter, TriggerGrouping,
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "saga"
        || face.name != card.name
        || face.type_line.as_deref()? != "Enchantment — Saga"
    {
        return None;
    }
    let mut program = Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
    program
        .enchantment_subtypes
        .insert(EnchantmentSubtype::Saga);
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let rules = text.trim();
    let chapters = rules.strip_prefix("Read ahead").map(str::trim_start);
    let mut last = 0;
    for line in chapters
        .unwrap_or(rules)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let (symbols, rules) = line.split_once(" — ")?;
        let body = chapter_body(rules, &face.name, vocabulary)?;
        for symbol in symbols.split(", ") {
            let number = match symbol {
                "I" => 1,
                "II" => 2,
                "III" => 3,
                "IV" => 4,
                "V" => 5,
                "VI" => 6,
                _ => return None,
            };
            last = last.max(number);
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(program.triggered.len().try_into().ok()?),
                condition: spellwake_engine::program::TriggerSpec::counters_changed(
                    spellwake_engine::program::ObjectFilter::self_object(),
                    spellwake_engine::program::CounterTriggerKind::SagaChapter(number),
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: body.clone(),
            });
        }
    }
    if chapters.is_some()
        && let spellwake_engine::program::ProgramKind::Permanent(permanent) = &mut program.kind
    {
        permanent.entry_tap = Some(spellwake_engine::land::EntryTap::ReadAhead {
            final_chapter: last,
        });
    }
    (!program.triggered.is_empty()).then_some(program)
}

fn chapter_body(
    text: &str,
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ExecutableBody> {
    if let Some(creation) = text
        .strip_prefix("Create ")
        .and_then(|s| s.strip_suffix('.'))
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![ordinary_text::create_tokens(creation, vocabulary)?],
        ));
    }
    match text {
        "Exile up to two target cards from graveyards. For each creature card exiled this way, you gain 1 life." => {
            Some(ExecutableBody::new(
                BodyTarget::AnyGraveyardCard,
                vec![Instruction::ExileUpToTwoGraveyardTargetsGainLife],
            ))
        }
        "For each player, exile up to one target non-Saga, nonland permanent that player controls until this Saga leaves the battlefield." => {
            Some(ExecutableBody::new(
                BodyTarget::FilteredPermanent(ObjectFilter::nonland().non_saga()),
                vec![Instruction::ExileOnePerPlayerUntilSourceLeaves],
            ))
        }
        "Choose any number of artifact tokens and/or creature tokens you control with different names. For each of them, create a token that's a copy of it." => {
            Some(ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CopyDifferentlyNamedTokens],
            ))
        }
        "Mill two cards, then you may return a creature card from your graveyard to your hand." => {
            Some(ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::Mill {
                        player: spellwake_engine::program::ResolvingPlayer::Controller,
                        amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                    },
                    Instruction::ReturnUpToOneOwnGraveyardCreatureToHand,
                ],
            ))
        }
        "Return all land cards from your graveyard to the battlefield, then shuffle your graveyard into your library." => {
            Some(ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::ReturnGraveyardCohort(
                        spellwake_engine::program::GraveyardCohortReturn::own(
                            spellwake_engine::program::GraveyardReturnFilter::CardType(
                                CardType::Land,
                            ),
                        ),
                    ),
                    Instruction::ShuffleOwnGraveyardIntoLibrary,
                ],
            ))
        }
        "Put target creature or planeswalker card from a graveyard onto the battlefield under your control." => {
            Some(graveyard_return::eldest_reborn_third_chapter())
        }
        "Destroy target nonland permanent an opponent controls." => Some(ExecutableBody::new(
            BodyTarget::FilteredPermanent(ObjectFilter::nonland().opposing()),
            vec![Instruction::DestroyTarget],
        )),
        _ => basic_land_search::body(text)
            .or_else(|| spells::ordinary_effects::body(text, name))
            .or_else(|| ordinary_triggers::body(text, name, vocabulary)),
    }
}
