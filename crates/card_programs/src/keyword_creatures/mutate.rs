//! Full-text binding for represented mutate costs and triggered effects.
use super::*;
use spellwake_engine::program::{BodyTarget, ConditionalCastCost, ExecutableBody, TriggerGrouping};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if !matches!(card.layout.as_str(), "mutate" | "normal") {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut cost = None;
    let mut mutate_trigger = None;
    let mut ordinary = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(price) = line.strip_prefix("Mutate ") {
            if cost.replace(fixed_cost(price.trim())?).is_some() {
                return None;
            }
        } else if let Some(effect) = line.strip_prefix("Whenever this creature mutates, ") {
            let instruction = match effect {
                "draw a card." => crate::keyword_creatures::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
                "put a +1/+1 counter on it." => {
                    crate::keyword_creatures::toolbox_emit::put_counters_on_source(
                        spellwake_engine::effects::CounterKind::PlusOne,
                        spellwake_engine::program::ValueExpr::literal(1),
                    )
                }
                "put two +1/+1 counters on it." => {
                    crate::keyword_creatures::toolbox_emit::put_counters_on_source(
                        spellwake_engine::effects::CounterKind::PlusOne,
                        spellwake_engine::program::ValueExpr::literal(2),
                    )
                }
                _ => {
                    let life = effect
                        .strip_prefix("you gain ")?
                        .strip_suffix(" life.")?
                        .parse::<u16>()
                        .ok()?;
                    crate::keyword_creatures::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::literal(i64::from(life)),
                    )
                }
            };
            if mutate_trigger.replace(instruction).is_some() {
                return None;
            }
        } else {
            ordinary.push(line);
        }
    }
    let cost = cost?;
    let mut remaining = card.clone();
    // Scryfall marks this single printed creature face with a "mutate" layout.
    // The ordinary face compiler binds everything left after the ability is extracted.
    remaining.layout = "normal".to_owned();
    remaining.faces[0].oracle_text = Some(ordinary.join("\n"));
    let mut program = super::compile(&remaining, vocabulary, Profile::Current).ok()?;
    if !program.has_type(spellwake_engine::program::CardType::Creature)
        || program.conditional_cost.is_some()
    {
        return None;
    }
    program.conditional_cost = Some(ConditionalCastCost::Mutate(cost));
    if let Some(instruction) = mutate_trigger {
        let id = program
            .triggered
            .iter()
            .map(|t| t.id.0)
            .max()
            .map_or(Some(0), |id| id.checked_add(1))?;
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition: spellwake_engine::program::TriggerSpec::status_changed(
                spellwake_engine::program::ObjectFilter::self_object(),
                spellwake_engine::program::StatusTriggerKind::Mutated,
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, vec![instruction]),
        });
    }
    Some(program)
}
