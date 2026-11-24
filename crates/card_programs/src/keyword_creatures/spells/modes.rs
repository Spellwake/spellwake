//! Fixed-count modal spells. Each bullet must compile as its own complete body.
use super::*;
use spellwake_engine::{
    characteristics::PowerToughnessCounters,
    life::LifeAdjustment,
    program::{ProgramKind, ResolvingPlayer, SpellModes},
};

pub(super) fn compile(
    rules: &str,
    name: &str,
    kind: &str,
    cost: ManaCost,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let (rules, combat_only) = rules
        .trim()
        .strip_prefix("Cast this spell only during combat on your turn.\n")
        .map_or((rules, false), |body| (body, true));
    let (rules, entwine) = if let Some((rules, price)) = rules.trim().rsplit_once("\nEntwine ") {
        (rules, Some(fixed_cost(price.trim())?))
    } else {
        (rules, None)
    };
    let mut lines = rules.trim().lines();
    let (minimum, maximum, allow_repeat) = match lines.next()?.trim() {
        "Choose one —" => (1, 1, false),
        "Choose two —" => (2, 2, false),
        "Choose one or both —" => (1, 2, false),
        "Choose three. You may choose the same mode more than once." => (3, 3, true),
        _ => return None,
    };
    let mut bodies = lines.map(|line| mode_body(line.trim().strip_prefix("• ")?, name, vocabulary));
    let first = bodies.next()??;
    let additional = bodies.collect::<Option<Vec<_>>>()?;
    if additional.is_empty() {
        return None;
    }
    let mut program = match kind {
        "Instant" => Program::instant(cost, first),
        "Sorcery" => Program::sorcery(cost, first),
        _ => return None,
    };
    program.cast_during_own_combat = combat_only;
    let ProgramKind::Spell(spell) = &mut program.kind else {
        return None;
    };
    spell.modes = Some(SpellModes {
        minimum,
        maximum,
        allow_repeat,
        distinct_targets: false,
        uniqueness: spellwake_engine::program::ModalUniqueness::None,
        mode_costs: Vec::new(),
        escalate_mana: None,
        prefix: None,
        suffix: None,
        additional,
    });
    if let Some(price) = entwine {
        program
            .additional_cast_costs
            .push(AdditionalCastCost::Entwine(price));
    }
    Some(program)
}

fn mode_body(
    text: &str,
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ExecutableBody> {
    if text
        == "Untap all creatures you control. After this phase, there is an additional combat phase."
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdditionalCombat {
                untap: spellwake_engine::program::CombatUntapGroup::ControlledCreatures,
            }],
        ));
    }
    if let Some(body) = ordinary_effects::body(text, name)
        .or_else(|| super::super::basic_land_search::body(text))
        .or_else(|| {
            let body = super::super::innistrad::spell(text, name, vocabulary)?;
            (!body
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FixedTargetClause(_))))
            .then_some(body)
        })
    {
        return Some(body);
    }
    if let Some(literal) = text.strip_prefix("Create ").and_then(|text| {
        text.strip_suffix('.')
            .or_else(|| text.ends_with(".\"").then_some(text))
    }) {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![ordinary_text::create_tokens(literal, vocabulary)?],
        ));
    }
    if let Some(price) = text
        .strip_prefix("Counter target spell unless its controller pays ")
        .and_then(|s| s.strip_suffix('.'))
    {
        return Some(ExecutableBody::new(
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
            vec![
                Instruction::OfferManaPayment {
                    payer: ResolvingPlayer::TargetSpellController,
                    cost: fixed_cost(price)?,
                    paid_destination: 2,
                    declined_destination: 1,
                },
                Instruction::CounterSpell,
            ],
        ));
    }
    if let Some(count) = text
        .strip_prefix("Put ")
        .and_then(|s| s.strip_suffix(" +1/+1 counters on target creature."))
    {
        return Some(ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::PutPowerToughnessCounters {
                counters: PowerToughnessCounters {
                    plus_one: ordinary_text::amount(count)?.into(),
                    minus_one: 0.into(),
                },
            }],
        ));
    }
    if text == "Target player draws a card and loses 1 life." {
        return Some(ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![
                Instruction::DrawTargetPlayer {
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                },
                Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Lose(1.into()),
                },
            ],
        ));
    }
    None
}
