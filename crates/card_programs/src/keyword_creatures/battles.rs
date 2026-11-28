//! Transforming Sieges with a supported entry trigger and keyword creature reverse face.
use super::*;
use spellwake_engine::characteristics::Color;
use spellwake_engine::program::{
    BodyTarget, CardType, ExecutableBody, Instruction, ProgramKind, TriggerGrouping,
    TriggeredAbility, TriggeredAbilityId,
};

pub(super) fn compile(
    card: &InventoryCard,
    inventory: &Inventory,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [front, back] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "transform"
        || front.type_line.as_deref() != Some("Battle — Siege")
        || card.name != format!("{} // {}", front.name, back.name)
        || !back.mana_cost.as_deref().unwrap_or("").is_empty()
    {
        return None;
    }
    let id = card.oracle_id.as_deref()?;
    let printed = inventory.source_defense(id, 0)?.parse::<u16>().ok()?;
    let text = without_reminder(front.oracle_text.as_deref()?)?;
    let life = text
        .trim()
        .strip_prefix("When this Siege enters, you gain ")?
        .strip_suffix(" life and draw a card.")?
        .parse::<u16>()
        .ok()?;
    let mut reverse_card = card.clone();
    reverse_card.layout = "normal".into();
    reverse_card.name = back.name.clone();
    reverse_card.faces = vec![back.clone()];
    let mut reverse =
        creature_face(&reverse_card, vocabulary, Profile::Current, None, true).ok()?;
    reverse.names = ObjectNames::single(EnglishCardName::new(back.name.clone()).ok()?);
    reverse.color_indicator = Some(
        inventory
            .back_color_indicator(id)?
            .iter()
            .map(|color| match color.as_str() {
                "W" => Some(Color::White),
                "U" => Some(Color::Blue),
                "B" => Some(Color::Black),
                "R" => Some(Color::Red),
                "G" => Some(Color::Green),
                _ => None,
            })
            .collect::<Option<BTreeSet<_>>>()?,
    );
    let mut battle = Program::enchantment(Some(fixed_cost(front.mana_cost.as_deref()?)?));
    battle.card_types = [CardType::Battle].into();
    battle.printed_defense = Some(i64::from(printed).into());
    battle.siege_back = Some(Box::new(reverse));
    let ProgramKind::Permanent(permanent) = &mut battle.kind else {
        return None;
    };
    permanent.entry_tap = Some(spellwake_engine::land::EntryTap::SiegeProtector);
    battle.triggered = vec![
        TriggeredAbility {
            id: TriggeredAbilityId(0),
            grouping: TriggerGrouping::EachOccurrence,
            condition: spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::literal(i64::from(life)),
                    ),
                    crate::keyword_creatures::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                ],
            ),
        },
        TriggeredAbility {
            id: TriggeredAbilityId(1),
            grouping: TriggerGrouping::EachOccurrence,
            condition: spellwake_engine::program::TriggerSpec::counters_changed(
                spellwake_engine::program::ObjectFilter::self_object(),
                spellwake_engine::program::CounterTriggerKind::LastDefenseRemoved,
            ),
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::exile_defeated_siege(),
                    )),
                    Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::cast_defeated_siege(),
                    )),
                ],
            ),
        },
    ];
    Some(battle)
}
