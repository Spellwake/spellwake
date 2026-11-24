use super::*;
use crate::{
    characteristics::{
        Color, DefiningPart, DefiningParts, PermanentSelector, PrintedText, StaticAbilityId,
        StaticPart, StaticParts,
    },
    test_support::{creature_types::selected_vocabulary, program as codec},
};
use serde_json::json;

fn source() -> Program {
    let mut p = Program::artifact(None, vec![]);
    p.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(7),
        parts: DefiningParts(vec![DefiningPart::Colors(BTreeSet::new())]),
    });
    p
}

#[test]
fn color_definition_roundtrips_program_and_printed_text_including_colorless() {
    let v = selected_vocabulary().unwrap();
    for colors in [BTreeSet::new(), Color::ALL.into_iter().collect()] {
        let mut p = source();
        p.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
            id: StaticAbilityId(7),
            parts: DefiningParts(vec![DefiningPart::Colors(colors)]),
        });
        let encoded = codec::view(&p);
        assert_eq!(encoded["continuous"]["tag"], "CharacteristicDefining");
        assert_eq!(codec::decode(encoded.clone(), &v).unwrap(), p);
        let text = PrintedText {
            cast_reductions: vec![],
            entry_counter_abilities: vec![],
            conditional_cost: None,
            plot: None,
            madness: None,
            miracle: None,
            exile_action: None,
            flashback: None,
            flashback_x_mana_symbols: 0,
            sacrifice_creature_cast_cost: false,
            sacrifice_cast_cost_filter: None,
            tap_creatures_cast_cost: false,
            reveal_colorless_creature_cast_cost: false,
            discard_cast_cost: false,
            discard_cast_cost_filter: None,
            graveyard_creature_exile_cost: 0,
            graveyard_exile_x_cost: false,
            graveyard_self_cast: None,
            flashback_sacrifice_creatures: 0,
            flashback_tap_white_creatures: 0,
            flashback_life: 0,
            additional_cast_costs: vec![],
            activation_restrictions: Default::default(),
            activation_modes: Default::default(),
            activation_hybrid_symbols: Default::default(),
            extra_activation_costs: Default::default(),
            extra_cast_costs: Vec::new(),
            loyalty_costs: Default::default(),
            variable_loyalty_costs: Default::default(),
            face_up_abilities: Vec::new(),
            printed_keywords: Default::default(),
            printed_toxic: None,
            zone_replacements: vec![],
            mana: vec![],
            activated: vec![],
            triggered: vec![],
            continuous: p.continuous.clone(),
            additional_battlefield_abilities: p.additional_battlefield_abilities.clone(),
            static_conditions: p.static_conditions.clone(),
        };
        assert_eq!(
            codec::printed_text(&text)["continuous"],
            encoded["continuous"]
        );
        assert_eq!(p.continuous.as_ref().unwrap().id(), StaticAbilityId(7));
    }
}

#[test]
fn color_definitions_admit_on_spells_but_ordinary_battlefield_statics_do_not() {
    let v = selected_vocabulary().unwrap();
    let body = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::DrawController {
            amount: crate::program::ValueExpr::Literal(1.into()),
        }],
    );
    for mut p in [
        Program::instant(ManaCost::free(), body.clone()),
        Program::sorcery(ManaCost::free(), body),
    ] {
        p.continuous = source().continuous;
        assert_eq!(p.validate(&v), Ok(()));
        assert_eq!(codec::decode(codec::view(&p), &v).unwrap(), p);
        p.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(7),
            selector: PermanentSelector::default(),
            parts: StaticParts(vec![StaticPart::SwitchPT]),
        });
        assert_eq!(p.validate(&v), Err(DefinitionError::ContinuousEffect));
        assert!(codec::decode(codec::view(&p), &v).is_err());
    }
}

#[test]
fn tagged_static_codec_rejects_old_mixed_missing_and_unknown_forms() {
    let v = selected_vocabulary().unwrap();
    let encoded = codec::view(&source());
    for invalid in [
        json!({"id":7,"colors":[]}),
        json!({"tag":"Unknown","id":7,"colors":[]}),
        json!({"tag":"DefineColors","id":7}),
        json!({"tag":"CharacteristicDefining","id":7,"parts":[{"tag":"Colors","colors":[]}],"selector":{}}),
        json!({"tag":"DefineColors","id":7,"colors":[],"parts":[]}),
        json!({"tag":"DefineColors","id":7,"colors":[],"condition":"Always"}),
        json!({"tag":"DefineColors","id":7,"colors":[],"source_colors":true}),
        json!({"tag":"DefineColors","id":7,"colors":[],"operation":"Add"}),
        json!({"tag":"Battlefield","id":7,"colors":[]}),
    ] {
        let mut value = encoded.clone();
        value["continuous"] = invalid;
        assert!(codec::decode(value, &v).is_err());
    }
    let ordinary = StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(2),
        selector: PermanentSelector::default(),
        parts: StaticParts(vec![StaticPart::SwitchPT]),
    };
    let mut p = Program::artifact(None, vec![]);
    p.continuous = Some(ordinary);
    let mut old = codec::view(&p);
    old["continuous"].as_object_mut().unwrap().remove("tag");
    assert!(codec::decode(old, &v).is_err());
    let mut mixed = codec::view(&p);
    mixed["continuous"]["colors"] = json!([]);
    assert!(codec::decode(mixed, &v).is_err());
}

#[test]
fn color_definition_codec_rejects_duplicate_unknown_and_malformed_colors() {
    let v = selected_vocabulary().unwrap();
    for colors in [
        json!(["Blue", "Blue"]),
        json!(["Colorless"]),
        json!(["Purple"]),
        json!([1]),
        json!(null),
        json!({}),
    ] {
        let mut value = codec::view(&source());
        value["continuous"]["parts"][0]["colors"] = colors;
        assert!(codec::decode(value, &v).is_err());
    }
}
