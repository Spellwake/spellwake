//! Complete source definitions that can be cast face down and turned face up for a fixed mana cost.
use super::*;
use spellwake_engine::{
    characteristics::{
        AbilityChange, PowerToughness, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    },
    face_down::FaceUpAbility,
    mana::{ManaAbility, ManaAbilityId, ManaOutput},
    program::TriggerGrouping,
};

pub(crate) fn compile(
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
    if !["Morph ", "Megamorph ", "Disguise "]
        .iter()
        .any(|prefix| raw.lines().any(|line| line.starts_with(prefix)))
    {
        return None;
    }
    let text = without_reminder(raw)?;
    let mut abilities = Vec::new();
    let mut triggers = Vec::new();
    let mut remaining = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(cost) = line.strip_prefix("Morph ") {
            abilities.push(FaceUpAbility::Morph(fixed_cost(cost)?));
        } else if let Some(cost) = line.strip_prefix("Megamorph ") {
            abilities.push(FaceUpAbility::Megamorph(fixed_cost(cost)?));
        } else if let Some(cost) = line.strip_prefix("Disguise ") {
            abilities.push(FaceUpAbility::Disguise(fixed_cost(cost)?));
        } else if let Some(effect) = line.strip_prefix("When this creature is turned face up, ") {
            triggers.push(turn_up_effect(effect)?);
        } else {
            remaining.push(line);
        }
    }
    let mut program = if face.type_line.as_deref() == Some("Land") && remaining == ["{T}: Add {C}."]
    {
        Program::land(
            BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::Fixed([(ManaType::Colourless, 1.into())].into()),
            }],
        )
    } else {
        ordinary_creature(
            card,
            vocabulary,
            Profile::GameplayV4,
            Some(&remaining.join("\n")),
        )
        .ok()?
    };
    for body in triggers {
        let id = program
            .triggered
            .iter()
            .map(|t| t.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)?;
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition: spellwake_engine::program::TriggerSpec::status_changed(
                spellwake_engine::program::ObjectFilter::self_object(),
                spellwake_engine::program::StatusTriggerKind::TurnsFaceUp,
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body,
        });
    }
    program.face_up_abilities = abilities;
    Some(program)
}

fn turn_up_effect(text: &str) -> Option<ExecutableBody> {
    let (target, instruction) = match text {
        "return target creature to its owner's hand." => (
            BodyTarget::SINGLE_CREATURE,
            crate::keyword_creatures::toolbox_emit::return_to_owners_hand(),
        ),
        "target creature gains haste until end of turn."
        | "target creature gains trample until end of turn."
        | "target creature gains first strike until end of turn." => {
            let keyword = match text {
                "target creature gains haste until end of turn." => KeywordAbility::Haste,
                "target creature gains trample until end of turn." => KeywordAbility::Trample,
                _ => KeywordAbility::FirstStrike,
            };
            (
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                        AbilityChange::grant_keywords([keyword]),
                    )]),
                    ResolvedDuration::UntilEndOfTurn,
                ),
            )
        }
        "counter target spell." => (
            BodyTarget::single_spell(spellwake_engine::program::SpellFilter::any()),
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                ),
            )),
        ),
        "target creature gets +3/+3 until end of turn."
        | "target creature gets -3/-3 until end of turn." => {
            let delta = if text.contains("+3/+3") { 3 } else { -3 };
            (
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(PowerToughness {
                        power: delta.into(),
                        toughness: delta.into(),
                    })]),
                    ResolvedDuration::UntilEndOfTurn,
                ),
            )
        }
        _ => return None,
    };
    Some(ExecutableBody::new(target, vec![instruction]))
}
