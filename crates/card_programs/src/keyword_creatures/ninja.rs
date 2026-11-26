//! Complete fixed-mana ninjutsu and sneak creature faces.
use super::*;
use spellwake_engine::program::{
    AffectedPlayers, ConditionalCastCost, ResolvingPlayer, TriggerGrouping,
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let raw = face.oracle_text.as_deref()?;
    if !raw.lines().any(|line| {
        line.starts_with("Commander ninjutsu {")
            || line.starts_with("Ninjutsu {")
            || line.starts_with("Sneak {")
    }) {
        return None;
    }
    let text = without_reminder(raw)?;
    let short_name = card.name.split_once(", ").map(|(name, _)| name);
    let mut price = None;
    let mut remaining = Vec::new();
    let mut damage = None;
    for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((keyword, amount)) = line
            .strip_prefix("Commander ninjutsu ")
            .map(|amount| ("Commander ninjutsu", amount))
            .or_else(|| line.split_once(' '))
            && matches!(keyword, "Commander ninjutsu" | "Ninjutsu" | "Sneak")
        {
            if price.replace((keyword, fixed_cost(amount)?)).is_some() {
                return None;
            }
        } else if let Some(body) = line
            .strip_prefix("Whenever this creature deals combat damage to a player, ")
            .or_else(|| {
                line.strip_prefix("Whenever ")
                    .and_then(|rule| {
                        rule.strip_prefix(&card.name)
                            .or_else(|| short_name.and_then(|name| rule.strip_prefix(name)))
                    })
                    .and_then(|rule| rule.strip_prefix(" deals combat damage to a player, "))
            })
        {
            let op = match body {
                "that player discards two cards." => vec![Instruction::Discard {
                    players: AffectedPlayers::One(ResolvingPlayer::UniqueRelatedPlayer),
                    amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                }],
                "you draw a card and lose 1 life." => vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                    Instruction::AdjustControllerLife {
                        adjustment: spellwake_engine::life::LifeAdjustment::Lose(1.into()),
                    },
                ],
                _ => return None,
            };
            if damage.replace(op).is_some() {
                return None;
            }
        } else {
            remaining.push(line);
        }
    }
    let mut reduced = card.clone();
    reduced.faces[0].oracle_text = Some(remaining.join("\n"));
    let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
    let (keyword, price) = price?;
    if matches!(keyword, "Ninjutsu" | "Commander ninjutsu") {
        let id = program
            .activated
            .iter()
            .map(|x| x.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)?;
        let mut ability = ActivatedAbility::ninjutsu(ActivatedAbilityId(id), price);
        if keyword == "Commander ninjutsu" {
            ability.source_cost =
                spellwake_engine::activated::SourceCost::Ninjutsu { from_command: true };
        }
        program.activated.push(ability);
    } else if program
        .conditional_cost
        .replace(ConditionalCastCost::Sneak(price))
        .is_some()
    {
        return None;
    }
    if let Some(body) = damage {
        let id = program
            .triggered
            .iter()
            .map(|x| x.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)?;
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition: spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, body),
        });
    }
    Some(program)
}
