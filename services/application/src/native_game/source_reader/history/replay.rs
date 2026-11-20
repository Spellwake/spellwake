//! Actual native cut/evidence joining with strict private-byte read mocks.
use super::super::history_replay::{HistoryReplay, NativeHistoryEvidence};
use super::*;
use crate::native_game::initial_source::tests::{
    checkpoint_read_steps, checkpoint_store, ids, source, spec,
};
use crate::test_http::Mock;
use serde_json::Value;
fn sources(checkpoint: &NativeCheckpoint) -> Value {
    let basis = checkpoint.basis();
    let creator = Uuid::from_u128(0x91);
    let id = Uuid::from_u128(0x92);
    json!({"anchor":{"gameId":basis.game_id,"contextId":basis.context_id,"anchorCheckpointId":id,
        "anchorStateRef":checkpoint.reference(),"anchorCreatorReceiptId":creator,"throughAcceptedRevision":"0","throughEventSequence":"0"},
        "groups":[{"checkpointId":id,"acceptedRevision":"0","stateRef":checkpoint.reference(),
        "factsRef":checkpoint.prepare_results().unwrap().reference(),"creatorReceiptId":creator,"eventSchemaRevision":"native-transition-v1",
        "events":[{"eventId":Uuid::from_u128(0x93),"sequence":"0","acceptedRevision":"0",
        "privatePayloadRef":checkpoint.prepare_event().unwrap().reference(),"acceptedAt":"2026-11-06T00:00:00.000000Z","creatorReceiptId":creator,"facts":[]}]}],
        "nextAfterRevision":null})
}
fn decoded(value: Value) -> NativeTransitionSources {
    let page: Page = crate::decode_wire_object(&serde_json::to_vec(&value).unwrap()).unwrap();
    NativeTransitionSources {
        groups: page.groups,
        cast_accounts: crate::native_game::cast_activity::NativeCastAccounts::new(
            page.anchor.game_id,
            page.anchor.context_id,
            [
                (Uuid::from_u128(0x1ab5), Some(Uuid::from_u128(0xa1))),
                (Uuid::from_u128(0x1ab6), Some(Uuid::from_u128(0xa2))),
            ],
        )
        .unwrap(),
        anchor: page.anchor,
    }
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; history evidence join only"]
fn actual_native_cut_matches_event_and_results_and_rejects_substitutions_before_io() {
    let checkpoint = NativeCheckpoint::initial(
        prepare_initial(
            ids(),
            source(NativeProgramPool::ResolutionSelectionNaturalMana),
            spec(),
        )
        .unwrap(),
    )
    .unwrap();
    let event = checkpoint.prepare_event().unwrap();
    let results = checkpoint.prepare_results().unwrap();
    let selected = decoded(sources(&checkpoint));
    let mut steps = checkpoint_read_steps(event.reference(), event.original_bytes());
    steps.extend(checkpoint_read_steps(
        results.reference(),
        results.original_bytes(),
    ));
    let mock = Mock::new(steps);
    let store = checkpoint_store(&mock);
    let total = event.reference().size() + results.reference().size();
    let mut budget = NativeRecoveryBudget::new(2, total).unwrap();
    let mut verifier = HistoryReplay::new(&selected);
    verifier.observe(&store, &checkpoint, &mut budget).unwrap();
    assert_eq!(
        verifier.finish().unwrap(),
        NativeHistoryEvidence::TransitionV1
    );
    assert_eq!(budget.remaining_bytes(), 0);
    assert_eq!(mock.finish().len(), 4);
    // Different accepted cut, foreign Event, false Facts and unqualified feature
    // facts must all fail before any private artifact request.
    let mut negatives = vec![];
    for field in ["stateRef", "factsRef"] {
        let mut value = sources(&checkpoint);
        value["groups"][0][field] = json!(checkpoint.initial_reference());
        negatives.push(value);
    }
    let mut value = sources(&checkpoint);
    value["groups"][0]["events"][0]["privatePayloadRef"] = json!(checkpoint.reference());
    negatives.push(value);
    let mut value = sources(&checkpoint);
    value["groups"][0]["events"][0]["facts"] =
        json!([{"accountId":Uuid::from_u128(0x94),"kind":"cast","factRevision":"v1","value":"1"}]);
    negatives.push(value);
    let mut value = sources(&checkpoint);
    value["groups"][0]["acceptedRevision"] = json!("1");
    negatives.push(value);
    for value in negatives {
        let selected = decoded(value);
        let mock = Mock::new(vec![]);
        assert!(
            HistoryReplay::new(&selected)
                .observe(
                    &checkpoint_store(&mock),
                    &checkpoint,
                    &mut NativeRecoveryBudget::new(10, 100_000).unwrap()
                )
                .is_err()
        );
        assert!(mock.finish().is_empty());
    }
    let mock = Mock::new(vec![]);
    assert!(matches!(
        HistoryReplay::new(&selected).observe(
            &checkpoint_store(&mock),
            &checkpoint,
            &mut NativeRecoveryBudget::new(2, total - 1).unwrap()
        ),
        Err(NativeCheckpointError::Budget)
    ));
    assert!(mock.finish().is_empty());
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; historical evidence remains unqualified"]
fn historical_cut_is_not_promoted_to_verified_and_missing_or_extra_cuts_fail() {
    let checkpoint = NativeCheckpoint::initial(
        prepare_initial(
            ids(),
            source(NativeProgramPool::ResolutionSelectionNaturalMana),
            spec(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut value = sources(&checkpoint);
    value["groups"][0]["eventSchemaRevision"] = Value::Null;
    let selected = decoded(value);
    let mock = Mock::new(vec![]);
    let store = checkpoint_store(&mock);
    let mut budget = NativeRecoveryBudget::new(1, 1).unwrap();
    assert!(HistoryReplay::new(&selected).finish().is_err());
    let mut verifier = HistoryReplay::new(&selected);
    verifier.observe(&store, &checkpoint, &mut budget).unwrap();
    assert!(verifier.observe(&store, &checkpoint, &mut budget).is_err());
    assert_eq!(
        verifier.finish().unwrap(),
        NativeHistoryEvidence::HistoricalUnqualified
    );
    let mut value = sources(&checkpoint);
    value["groups"][0]["eventSchemaRevision"] = Value::Null;
    let extra = value["groups"][0].clone();
    value["groups"].as_array_mut().unwrap().push(extra);
    let selected = decoded(value);
    let mut verifier = HistoryReplay::new(&selected);
    verifier.observe(&store, &checkpoint, &mut budget).unwrap();
    assert!(verifier.finish().is_err());
    assert!(mock.finish().is_empty());
}

#[test]
#[ignore = "requires genuine catalogue/vocabulary; all-cut single-walk mechanism"]
fn every_accepted_cut_is_joined_during_one_actual_cold_input_replay() {
    use crate::native_game::checkpoint::load_checkpoint_observed;
    use crate::native_game::initial_source::tests::checkpoint_fixture;
    let programs = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let fixture = checkpoint_fixture(None);
    let opening = || prepare_initial(ids(), programs, spec()).unwrap();
    let initial = NativeInitialExpected::new(
        opening().reference().clone(),
        ids().release_id,
        ids().profile_id,
        vec![Uuid::from_u128(0x1ab5), Uuid::from_u128(0x1ab6)],
    )
    .unwrap();
    let expected =
        NativeCheckpointExpected::new(fixture.tip.reference().clone(), fixture.tip.basis().clone())
            .unwrap();
    let state_steps = || {
        let mut steps = vec![];
        for (reference, bytes) in fixture.states.iter().rev() {
            steps.extend(checkpoint_read_steps(reference, bytes));
        }
        steps
    };
    let mut steps = state_steps();
    for (reference, bytes) in &fixture.inputs {
        steps.extend(checkpoint_read_steps(reference, bytes));
    }
    let mock = Mock::new(steps);
    let mut groups = vec![];
    let mut evidence = vec![];
    load_checkpoint_observed(
        &checkpoint_store(&mock),
        &expected,
        &initial,
        opening(),
        &mut NativeRecoveryBudget::new(100, 10_000_000).unwrap(),
        &mut |checkpoint, _| {
            let revision = checkpoint.basis().revision.get();
            let mut value = sources(checkpoint)["groups"][0].clone();
            value["acceptedRevision"] = json!(revision.to_string());
            value["events"][0]["acceptedRevision"] = json!(revision.to_string());
            value["events"][0]["sequence"] = json!(revision.to_string());
            groups.push(value);
            evidence.push((checkpoint.prepare_event()?, checkpoint.prepare_results()?));
            Ok(())
        },
    )
    .unwrap();
    mock.finish();
    assert_eq!(groups.len(), fixture.states.len());
    let mut value = sources(&fixture.tip);
    value["groups"] = json!(groups);
    value["anchor"]["throughAcceptedRevision"] = json!(fixture.tip.basis().revision);
    value["anchor"]["throughEventSequence"] = json!(fixture.tip.basis().revision);
    let selected = decoded(value);
    let mut steps = state_steps();
    for (index, (event, results)) in evidence.iter().enumerate() {
        if index > 0 {
            let (reference, bytes) = &fixture.inputs[index - 1];
            steps.extend(checkpoint_read_steps(reference, bytes));
        }
        steps.extend(checkpoint_read_steps(
            event.reference(),
            event.original_bytes(),
        ));
        steps.extend(checkpoint_read_steps(
            results.reference(),
            results.original_bytes(),
        ));
    }
    let total = fixture
        .states
        .iter()
        .chain(&fixture.inputs)
        .map(|(reference, _)| reference.size())
        .sum::<u64>()
        + evidence
            .iter()
            .map(|(event, results)| event.reference().size() + results.reference().size())
            .sum::<u64>();
    let records = fixture.states.len() + 2 * evidence.len();
    let mock = Mock::new(steps);
    let store = checkpoint_store(&mock);
    let mut budget = NativeRecoveryBudget::new(records, total).unwrap();
    let mut verifier = HistoryReplay::new(&selected);
    let recovered = load_checkpoint_observed(
        &store,
        &expected,
        &initial,
        opening(),
        &mut budget,
        &mut |checkpoint, budget| verifier.observe(&store, checkpoint, budget),
    )
    .unwrap();
    assert_eq!(
        verifier.finish().unwrap(),
        NativeHistoryEvidence::TransitionV1
    );
    assert_eq!(recovered.state(), fixture.tip.state());
    assert_eq!(recovered.original_bytes(), fixture.tip.original_bytes());
    assert_eq!(budget.remaining_bytes(), 0);
    assert!(budget.charge_work(1, 0).is_err());
    assert_eq!(
        mock.finish().len(),
        2 * (fixture.states.len() + fixture.inputs.len() + 2 * evidence.len())
    );
}

#[test]
#[ignore = "requires genuine catalogue/vocabulary; all-cut single-walk mechanism"]
fn mixed_v1_activity_cuts_recompute_genuine_cast_facts_and_refuse_tampering() {
    use crate::admission_ports::NativeEventSchema;
    use crate::native_game::cast_activity::NativeCastAccounts;
    use crate::native_game::checkpoint::load_checkpoint_observed;
    use crate::native_game::initial_source::tests::{cast_activity::cast_fixture, read_root};
    let programs = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let fixture = cast_fixture(
        "14c8f55d-d177-4c25-a931-ebeb9e6062a0",
        "b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6",
        spellwake_engine::land::BasicLandType::Forest,
        crate::player_wire::ManaColor::Green,
        false,
    );
    let initial = NativeInitialExpected::new(
        fixture.tip.initial_reference().clone(),
        ids().release_id,
        ids().profile_id,
        vec![Uuid::from_u128(0x1ab5), Uuid::from_u128(0x1ab6)],
    )
    .unwrap();
    let opening = || read_root(&fixture.original, &initial, programs).unwrap();
    let accounts = || {
        NativeCastAccounts::new(
            ids().game_id,
            ids().context_id,
            [
                (Uuid::from_u128(0x1ab5), Some(Uuid::from_u128(0xa1))),
                (Uuid::from_u128(0x1ab6), Some(Uuid::from_u128(0xa2))),
            ],
        )
        .unwrap()
    };
    let expected =
        NativeCheckpointExpected::new(fixture.tip.reference().clone(), fixture.tip.basis().clone())
            .unwrap();
    let state_steps = || {
        let mut steps = vec![];
        for (reference, bytes) in fixture.states.iter().rev() {
            steps.extend(checkpoint_read_steps(reference, bytes));
        }
        steps
    };
    let mut steps = state_steps();
    for (reference, bytes) in &fixture.inputs {
        steps.extend(checkpoint_read_steps(reference, bytes));
    }
    let mock = Mock::new(steps);
    let mut groups = vec![];
    let mut evidence = vec![];
    load_checkpoint_observed(
        &checkpoint_store(&mock),
        &expected,
        &initial,
        opening(),
        &mut NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
        &mut |checkpoint, _| {
            let revision = checkpoint.basis().revision.get();
            let mut value = sources(checkpoint)["groups"][0].clone();
            value["acceptedRevision"] = json!(revision.to_string());
            value["events"][0]["acceptedRevision"] = json!(revision.to_string());
            value["events"][0]["sequence"] = json!(revision.to_string());
            let schema=if revision==0{NativeEventSchema::TransitionV1}else{NativeEventSchema::ActivityV1};
            let event=checkpoint.prepare_event_schema(schema)?;let results=checkpoint.prepare_results_schema(schema)?;
            value["eventSchemaRevision"]=json!(schema.as_str());value["factsRef"]=json!(results.reference());value["events"][0]["privatePayloadRef"]=json!(event.reference());
            value["checkpointId"]=json!(Uuid::from_u128(0x10000+u128::from(revision)));value["creatorReceiptId"]=json!(Uuid::from_u128(0x20000+u128::from(revision)));value["events"][0]["creatorReceiptId"]=value["creatorReceiptId"].clone();value["events"][0]["eventId"]=json!(Uuid::from_u128(0x30000+u128::from(revision)));
            value["events"][0]["facts"]=json!(if revision==0{vec![]}else{accounts().facts(checkpoint)?.iter().map(|fact|json!({"accountId":fact.account_id,"kind":fact.kind,"factRevision":fact.revision,"value":fact.value})).collect::<Vec<_>>()});
            groups.push(value);
            evidence.push((event,results));
            Ok(())
        },
    )
    .unwrap();
    mock.finish();
    assert_eq!(groups.len(), fixture.states.len());
    let mut value = sources(&fixture.tip);
    value["groups"] = json!(groups);
    value["anchor"]["throughAcceptedRevision"] = json!(fixture.tip.basis().revision);
    value["anchor"]["throughEventSequence"] = json!(fixture.tip.basis().revision);
    value["anchor"]["anchorCheckpointId"] =
        value["groups"].as_array().unwrap().last().unwrap()["checkpointId"].clone();
    value["anchor"]["anchorCreatorReceiptId"] =
        value["groups"].as_array().unwrap().last().unwrap()["creatorReceiptId"].clone();
    let selected = decoded(value.clone());
    let mut steps = state_steps();
    for (index, (event, results)) in evidence.iter().enumerate() {
        if index > 0 {
            let (reference, bytes) = &fixture.inputs[index - 1];
            steps.extend(checkpoint_read_steps(reference, bytes));
        }
        steps.extend(checkpoint_read_steps(
            event.reference(),
            event.original_bytes(),
        ));
        steps.extend(checkpoint_read_steps(
            results.reference(),
            results.original_bytes(),
        ));
    }
    let total = fixture
        .states
        .iter()
        .chain(&fixture.inputs)
        .map(|(reference, _)| reference.size())
        .sum::<u64>()
        + evidence
            .iter()
            .map(|(event, results)| event.reference().size() + results.reference().size())
            .sum::<u64>();
    let records = fixture.states.len() + 2 * evidence.len();
    let mock = Mock::new(steps);
    let store = checkpoint_store(&mock);
    let mut budget = NativeRecoveryBudget::new(records, total).unwrap();
    let mut verifier = HistoryReplay::new(&selected);
    let recovered = load_checkpoint_observed(
        &store,
        &expected,
        &initial,
        opening(),
        &mut budget,
        &mut |checkpoint, budget| verifier.observe(&store, checkpoint, budget),
    )
    .unwrap();
    assert_eq!(
        verifier.finish().unwrap(),
        NativeHistoryEvidence::ActivityTransitionsV1
    );
    assert_eq!(recovered.state(), fixture.tip.state());
    assert_eq!(recovered.original_bytes(), fixture.tip.original_bytes());
    assert_eq!(budget.remaining_bytes(), 0);
    assert!(budget.charge_work(1, 0).is_err());
    assert_eq!(
        mock.finish().len(),
        2 * (fixture.states.len() + fixture.inputs.len() + 2 * evidence.len())
    );
    // Cut-local negative controls use the last already reconstructed candidate.
    // Complete-prefix SQL source validation is a separate boundary; above replay
    // positively checks EVERY actual cut, including the original V1 opening.
    let last = value["groups"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["events"][0]["facts"].as_array().unwrap().len(), 2);
    let mut bad_cuts = Vec::new();
    for (field, bad) in [
        ("accountId", json!(Uuid::from_u128(0xa1))),
        ("kind", json!("native:cast-other")),
        ("factRevision", json!("native-cast-colors-v2")),
        ("value", json!("2")),
    ] {
        let mut cut = last.clone();
        cut["events"][0]["facts"][0][field] = bad;
        bad_cuts.push(cut);
    }
    let mut missing = last.clone();
    missing["events"][0]["facts"].as_array_mut().unwrap().pop();
    bad_cuts.push(missing);
    let mut extra = last.clone();
    extra["events"][0]["facts"]
        .as_array_mut()
        .unwrap()
        .push(last["events"][0]["facts"][0].clone());
    bad_cuts.push(extra);
    let mut legacy = last.clone();
    legacy["eventSchemaRevision"] = json!("native-transition-v1");
    bad_cuts.push(legacy);
    let mut altered_event = last.clone();
    altered_event["events"][0]["privatePayloadRef"] =
        json!(fixture.tip.prepare_event().unwrap().reference());
    bad_cuts.push(altered_event);
    for cut in bad_cuts {
        let mut bad = value.clone();
        bad["groups"] = json!([cut]);
        let selected = decoded(bad);
        let mock = Mock::new(vec![]);
        let store = checkpoint_store(&mock);
        let mut verifier = HistoryReplay::new(&selected);
        assert!(
            verifier
                .observe(
                    &store,
                    &fixture.tip,
                    &mut NativeRecoveryBudget::new(10, 1_000_000).unwrap()
                )
                .is_err()
        );
        assert!(mock.finish().is_empty());
    }
}
