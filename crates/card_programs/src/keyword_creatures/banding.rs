//! Continuous and token grants with a qualified "bands with other" ability.
use super::*;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, Color, ColorFilter, ControllerSelector, PermanentSelector, StaticPart,
        StaticParts,
    },
    combat::BandQuality,
    program::{BodyTarget, ExecutableBody, Instruction},
    token::{TokenDefinition, TokenName},
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
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let rules = text.trim();
    if let Some(color) = rules.strip_suffix(
        " legendary creatures you control have \"bands with other legendary creatures.\"",
    ) {
        let color = match color {
            "White" => Color::White,
            "Blue" => Color::Blue,
            "Black" => Color::Black,
            "Red" => Color::Red,
            "Green" => Color::Green,
            _ => return None,
        };
        if face.type_line.as_deref() != Some("Land") {
            return None;
        }
        let mut program = Program::land(Default::default(), vec![]);
        let selector = PermanentSelector {
            controller: ControllerSelector::SourceController,
            required_supertypes: [Supertype::Legendary].into(),
            required_types: [CardType::Creature].into(),
            color: ColorFilter::Has(color),
            ..Default::default()
        };
        let grant = AbilityChange::grant_keywords([KeywordAbility::BandsWithOther(
            BandQuality::LegendaryCreatures,
        )]);
        army_grants::add_battlefield_ability(
            &mut program,
            selector,
            StaticParts(vec![StaticPart::Abilities(grant)]),
        )?;
        return Some(program);
    }
    if let Some(description) = rules
        .strip_prefix("Create ")
        .and_then(|text| text.strip_suffix(" with banding."))
    {
        let Instruction::CreateTokens { mut creations } =
            ordinary_text::create_tokens(description, vocabulary)?
        else {
            return None;
        };
        let [creation] = creations.as_mut_slice() else {
            return None;
        };
        let mut token = creation.definition.program().clone();
        token.printed_keywords.insert(KeywordAbility::Banding);
        creation.definition =
            TokenDefinition::new(token, creation.definition.name().clone(), vocabulary).ok()?;
        let body = ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens { creations }],
        );
        let cost = fixed_cost(face.mana_cost.as_deref()?)?;
        return match face.type_line.as_deref()? {
            "Instant" => Some(Program::instant(cost, body)),
            "Sorcery" => Some(Program::sorcery(cost, body)),
            _ => None,
        };
    }
    let remove_others =
        "{T}: Target creature loses all \"bands with other\" abilities until end of turn.";
    let remove_banding = "{T}: Target creature loses banding and all \"bands with other\" abilities until end of turn. Activate only during any upkeep step.";
    let during_upkeep = rules.strip_prefix("{T}: Add {U}.\n") == Some(remove_banding);
    if rules == remove_others || during_upkeep {
        use spellwake_engine::characteristics::{
            AbilityRemoval, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        };
        use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaType};
        let (mut program, removal) = if during_upkeep {
            if face.type_line.as_deref() != Some("Legendary Land") {
                return None;
            }
            let mut land = Program::land(
                Default::default(),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed([(ManaType::Blue, 1.into())].into()),
                }],
            );
            land.supertypes.insert(Supertype::Legendary);
            land.activation_restrictions.insert(
                ActivatedAbilityId(0),
                spellwake_engine::activated::ActivationRestriction::DuringUpkeep,
            );
            (land, AbilityRemoval::Banding)
        } else {
            (
                ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?,
                AbilityRemoval::BandsWithOther,
            )
        };
        let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
            removal,
        ))]);
        program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: spellwake_engine::mana::ManaCost::free(),
            source_cost: SourceCost::Tap,
            body: ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        parts,
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                ],
            ),
        });
        return Some(program);
    }
    let (price, description) = rules.split_once(": Create ")?;
    let (token, names) = description.split_once(" named ")?;
    let name =
        names.strip_suffix(". It has \"bands with other creatures named Wolves of the Hunt.\"")?;
    if name != "Wolves of the Hunt" {
        return None;
    }
    let Instruction::CreateTokens { mut creations } =
        ordinary_text::create_tokens(token, vocabulary)?
    else {
        return None;
    };
    let [creation] = creations.as_mut_slice() else {
        return None;
    };
    let mut token = creation.definition.program().clone();
    token
        .printed_keywords
        .insert(KeywordAbility::BandsWithOther(
            BandQuality::CreaturesNamedWolvesOfTheHunt,
        ));
    creation.definition = TokenDefinition::new(
        token,
        TokenName::Specified(EnglishCardName::new(name.to_owned()).ok()?),
        vocabulary,
    )
    .ok()?;
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: fixed_cost(price)?,
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens { creations }],
        ),
    });
    Some(program)
}
