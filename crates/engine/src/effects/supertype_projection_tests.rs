//! Populated current privileged projections from actual root transitions.
use crate::test_support::{
    packet::AliasContext,
    projection::{candidate, state},
};
use crate::{
    game::{DecisionReply, GameCommand},
    object_identity::CardId,
    program::Program,
};
use serde_json::{Value, json};
#[test]
fn populated_supertype_views_preserve_root_receipts_stack_traits_and_ability_none() {
    use crate::test_support::characteristic_trajectories as tr;
    use spellwake_engine::{
        activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
        characteristics::{
            ResolvedDuration, ResolvedPartSpec, SubtypeOperation, TypeChangeSpec, TypeOperation,
        },
        mana::{ManaCost, ManaPayment},
        program::{BodyTarget, ExecutableBody, Instruction, Supertype},
        targets::TargetRef,
    };
    use std::collections::BTreeSet;
    fn event<'a>(view: &'a Value, tag: &str) -> &'a Value {
        view["append"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|record| {
                record
                    .get("occurrences")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .map(|row| &row["value"])
            .find(|row| row["tag"] == tag)
            .unwrap()
    }
    let mut source = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::ZoneOp(Box::new(
                    crate::toolbox::ZoneOpSpec::move_all(
                        crate::toolbox::ObjectQuery::Targets,
                        crate::toolbox::ZoneDest::Hand,
                    ),
                ))],
            ),
        }],
    );
    source.supertypes = BTreeSet::from([Supertype::Basic]);
    let mut original = tr::spell(
        3,
        BodyTarget::SinglePermanent,
        vec![Instruction::ZoneOp(Box::new(
            crate::toolbox::ZoneOpSpec::move_all(
                crate::toolbox::ObjectQuery::Targets,
                crate::toolbox::ZoneDest::Hand,
            ),
        ))],
    );
    original.program.supertypes = BTreeSet::from([Supertype::Snow]);
    let game = tr::world(
        tr::vocabulary(),
        vec![
            tr::card(1, source, true),
            tr::spell(
                2,
                BodyTarget::SinglePermanent,
                vec![tr::effect(
                    vec![ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Add(BTreeSet::from([Supertype::Snow])),
                        types: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Keep,
                    })],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            original,
            tr::spell(
                4,
                BodyTarget::SingleSpell(crate::program::SpellFilter::copyable_instruction()),
                vec![Instruction::CopySpell],
            ),
        ],
        vec![CardId(1)],
    );
    let target = tr::object(&game, 1);
    let aliases = AliasContext::default();
    let (changed, _) = tr::resolve(&game, 2, target);
    let view = candidate(&changed, Some(&game), &aliases).unwrap();
    let card = view["state"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|card| card["object"]["physical_card"] == 1)
        .unwrap();
    assert_eq!(card["program"]["supertypes"], json!(["Basic"]));
    assert_eq!(
        card["characteristics"]["defined"]["supertypes"],
        json!(["Basic"])
    );
    assert_eq!(
        card["characteristics"]["current_supertypes"],
        json!(["Basic", "Snow"])
    );
    let change = &event(&view, "ContinuousEffectCreated")["appearances"][0];
    assert_eq!(change["before"]["current_supertypes"], json!(["Basic"]));
    assert_eq!(
        change["after"]["current_supertypes"],
        json!(["Basic", "Snow"])
    );
    assert_eq!(change["after"]["copiable_supertypes"], json!(["Basic"]));
    let cast = tr::submit(
        changed.state(),
        tr::P0,
        GameCommand::CastPaid {
            spell: tr::object(changed.state(), 3),
            target: TargetRef::Object(target),
            payment: ManaPayment::default(),
        },
    );
    let original_entry = cast.state().execution().window().entries()[0].id();
    let copier = tr::submit(
        cast.state(),
        tr::P0,
        GameCommand::CastPaid {
            spell: tr::object(cast.state(), 4),
            target: TargetRef::Stack(original_entry),
            payment: ManaPayment::default(),
        },
    );
    let view = state(copier.state(), &aliases).unwrap();
    assert_eq!(view["stack"][0]["copiable_supertypes"], json!(["Snow"]));
    assert_eq!(view["stack"][1]["copiable_supertypes"], json!([]));
    let pending = tr::pass_pair(copier.state());
    let decision = pending.state().pending_decision().unwrap();
    let copied = tr::submit(
        pending.state(),
        tr::P0,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::CopyTargets(spellwake_engine::effects::CopyTargetChoice::Retain),
        },
    );
    let view = candidate(&copied, Some(pending.state()), &aliases).unwrap();
    assert_eq!(
        event(&view, "CopyCreated")["copiable_supertypes"],
        json!(["Snow"])
    );
    assert_eq!(
        view["state"]["stack"].as_array().unwrap().last().unwrap()["copiable_supertypes"],
        json!(["Snow"])
    );
    let ability = tr::activate(copied.state(), target, target, false);
    let view = state(ability.state(), &aliases).unwrap();
    assert_eq!(
        view["stack"].as_array().unwrap().last().unwrap()["copiable_supertypes"],
        Value::Null
    );
}
