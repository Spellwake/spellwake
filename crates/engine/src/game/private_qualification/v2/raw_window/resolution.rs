//! Independent original-seed raw continuation; no executor body is installed.
use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::game::private_qualification::v2) struct RawResolutionWitness {
    initialization: Box<Result<EffectWindow, EffectError>>,
    checks: Option<Box<Result<RuleProgress, EffectError>>>,
    calls: Vec<(StackWindow, Result<StackWindow, StackWindowError>)>,
    final_window: Option<StackWindow>,
}

pub(in crate::game::private_qualification::v2) fn resolution_branch(
    seed: &GameSeed,
    fixture: &Value,
    aliases: &packet::AliasContext,
    action: &Value,
) -> Result<(Value, RawResolutionWitness), String> {
    if action["source_snapshot"] != "fresh_initial_fixture_construction"
        || action["scope"]
            != "independent_raw_window_branch_from_original_Hand_O_not_the_consumed_first_branch"
    {
        return Err("unsupported independent raw source contract".into());
    }
    let [cast, passes, movement, finish] = rows(action, "sequence")? else {
        return Err("raw branch requires four exact operations".into());
    };
    if cast["op"] != "record_card_spell"
        || passes["op"] != "real_pass_and_target_gate"
        || movement["op"] != "record_resolution_move"
        || movement["destination"] != "Exile"
        || finish["op"] != "finish_resolution"
        || !finish["exit"].is_null()
    {
        return Err("unsupported raw branch operation".into());
    }
    let initialization = crate::test_support::native_calls::effect_initialize(seed.clone());
    let mut witness = RawResolutionWitness {
        initialization: Box::new(initialization.clone()),
        checks: None,
        calls: vec![],
        final_window: None,
    };
    let seeded = match initialization {
        Ok(value) => value,
        Err(error) => {
            return Ok((
                json!({"status":"failed","initialization":projection::effect_error(&error)}),
                witness,
            ));
        }
    };
    let expected = seeded.revision();
    let checks = capture::progress(
        &seeded,
        aliases,
        projection::EffectCall::BeginChecks { expected },
        |source| source.begin_checks(expected),
        true,
    )?;
    let check_view = match &checks {
        Ok(value) => json!({"tag":"Ok","progress":projection::rule_progress(value,aliases)?}),
        Err(error) => projection::effect_error(error),
    };
    witness.checks = Some(Box::new(checks.clone()));
    let checked_source = checks.as_ref().ok().map(|value| value.state().clone());
    let mut window = match checks {
        Ok(value) if matches!(value.result(), RuleResult::Complete { .. }) => {
            value.into_state().window().clone()
        }
        _ => {
            return Ok((
                json!({"status":"failed","checks":check_view,"failure":"fresh actual checks did not complete; no Ready reconstruction"}),
                witness,
            ));
        }
    };
    let mut records = vec![capture::effect_point(
        json!({"operation":"fresh-EffectWindow.begin_checks","scope":"actual-EF-checks-before-independent-raw-window","result":check_view}),
        checked_source
            .as_ref()
            .ok_or("completed raw branch lacks actual check successor")?,
        aliases,
        "raw resolution actual completed checks",
    )?];
    macro_rules! invoke {
        ($label:expr,$input:expr,$call:expr) => {{
            let source=window.clone();
            let result=capture::window(&window,$input,$call,|result|Ok(match result{Ok(value)=>json!({"tag":"Ok","state":projection::stack_window(value)}),Err(error)=>projection::stack_window_error(error)}),|value|Some(value.clone()),true)?;
            records.push(capture::window_point(json!({"operation":$label,"source":projection::stack_window(&source),"result":match &result {Ok(value)=>json!({"tag":"Ok","state":projection::stack_window(value)}),Err(error)=>projection::stack_window_error(error)},"retained_source_full_state_eq":source==window,"raw_primitive_no_rules_append":true}),&source,"raw resolution actual source",None)?);
            witness.calls.push((source,result.clone()));
            match result {Ok(value)=>window=value,Err(error)=>{
                witness.final_window=Some(window.clone());
                return Ok((json!({"status":"failed","records":records,"failure":format!("actual raw call rejected: {error:?}"),"final_window":projection::stack_window(&window)}),witness));
            }}
        }};
    }
    let alias = string(cast, "source_current_alias")?;
    let cards = rows(&fixture["seed"], "cards")?
        .iter()
        .filter(|card| card["alias"] == alias)
        .collect::<Vec<_>>();
    let [card] = cards.as_slice() else {
        return Err("raw source alias not unique".into());
    };
    let physical = CardId(
        card["card_id"]
            .as_u64()
            .ok_or("raw source physical ID missing")?,
    );
    let object = window
        .cards()
        .current_identity(physical)
        .ok_or("raw original Hand source missing")?;
    let actor = packet::seat(string(cast, "actor")?)?;
    let expected = window.revision();
    invoke!(
        cast,
        projection::StackWindowCall::RecordCardSpell {
            expected,
            actor,
            card: object
        },
        |source| source.record_card_spell(expected, actor, object)
    );
    let own_entry = window
        .entries()
        .last()
        .ok_or("actual raw cast has no entry")?
        .clone();
    let crate::stack_window::StackKind::CardSpell(original_object) = own_entry.kind() else {
        return Err("actual raw entry is not card spell".into());
    };
    let original_object = *original_object;
    for actor in rows(passes, "pass_order")? {
        if matches!(
            window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            let expected = window.revision();
            invoke!(
                json!({"owner":"StackWindow","method":"finish_checks","scope":"trusted-raw-readiness-callback-no-EF-SBA-or-trigger-execution"}),
                projection::StackWindowCall::FinishChecks { expected },
                |source| source.finish_checks(expected)
            );
        }
        let expected = window.revision();
        let player = packet::seat(actor.as_str().ok_or("invalid raw pass actor")?)?;
        invoke!(
            json!({"owner":"StackWindow","method":"pass","actor":actor}),
            projection::StackWindowCall::Pass {
                expected,
                actor: player
            },
            |source| source.pass(expected, player)
        );
    }
    records.push(capture::window_point(json!({"operation":"actual-raw-target-gate","entry":projection::opaque(own_entry.id()),"assessment":format!("{:?}",window.target_assessment()),"phase":projection::stack_window(&window)["phase"]}),&window,"raw resolution actual target assessment",None)?);
    if window.resolving().map(|entry| entry.id()) != Some(own_entry.id()) {
        witness.final_window = Some(window.clone());
        return Ok((
            json!({"status":"failed","records":records,"failure":"actual raw passes did not admit selected entry resolution"}),
            witness,
        ));
    }
    let expected = window.revision();
    invoke!(
        movement,
        projection::StackWindowCall::RecordResolutionMove {
            expected,
            card: original_object,
            destination: ObjectDestination::Exile
        },
        |source| source.record_resolution_move(expected, original_object, ObjectDestination::Exile)
    );
    let expected = window.revision();
    let entry = own_entry.id();
    invoke!(
        finish,
        projection::StackWindowCall::FinishResolution {
            expected,
            entry,
            exit: None
        },
        |source| source.finish_resolution(expected, entry, None)
    );
    witness.final_window = Some(window.clone());
    Ok((
        capture::window_point(
            json!({"status":"completed_recipe","records":records,"final_window":projection::stack_window(&window),"scope":action["scope"],"executor_transplantation":false,"authored_body_execution":false}),
            &window,
            "raw resolution actual final window",
            None,
        )?,
        witness,
    ))
}
