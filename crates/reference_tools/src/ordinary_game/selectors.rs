//! Source-owned selectors attach names to actual exposed candidates, without
//! advancing play or manufacturing intermediate root states.
use super::fixture::{A, B, CaseId, Fixture};
use serde::Serialize;
use spellwake_engine::{
    checks::CheckDecision,
    combat::{CombatDecision, CombatDeclarationRecord, DeclarationOrigin},
    damage::DamageRecipient,
    draw::DrawEvent,
    game::{DecisionReply, GameCommand, GameInput, GameState, NativeCandidate, RuleFrame},
    opening::OpeningDecision,
    priority::PriorityPhase,
    stack_window::WindowPhase,
    turn::Step,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Scope {
    CandidateEvidence,
    FullCheckpoint,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct Observation {
    pub name: String,
    pub semantic_selector_ref: String,
    pub candidate_ordinal: usize,
    pub scope: Scope,
}
#[derive(Clone, Default)]
pub(super) struct Selectors {
    seen: BTreeSet<String>,
}
impl Selectors {
    pub fn observe(
        &mut self,
        fixture: &Fixture,
        source: Option<&GameState>,
        input: Option<&GameInput>,
        candidate: &NativeCandidate,
        ordinal: usize,
    ) -> Result<Vec<Observation>, String> {
        if matches!(
            fixture.case,
            CaseId::Interaction(_) | CaseId::Mandatory(_) | CaseId::Optional(_)
        ) {
            return self.observe_each_candidate(fixture, candidate, ordinal);
        }
        if matches!(fixture.case, CaseId::Teaching(_)) {
            return self.observe_teaching(fixture, source, input, candidate, ordinal);
        }
        let state = candidate.state();
        let mut found = vec![];
        let mut add = |name: String, scope: Scope| -> Result<(), String> {
            if !self.seen.insert(name.clone()) {
                return Err(format!("selector matched twice: {name}"));
            }
            found.push(Observation {
                semantic_selector_ref: name.clone(),
                name,
                candidate_ordinal: ordinal,
                scope,
            });
            Ok(())
        };
        let full = Scope::FullCheckpoint;
        let evidence = Scope::CandidateEvidence;
        if ordinal == 0 {
            add("initialization".into(), full)?;
        }
        if state.opening().is_some_and(|v| v.complete())
            && source.is_some_and(|v| v.opening().is_some_and(|o| !o.complete()))
        {
            add("opening.complete".into(), full)?;
        }
        let position = state.turn().map(|v| v.position());
        let turn = position.map(|v| v.number);
        let personal = turn.map(|v| v.div_ceil(2));
        let seat = if state.execution().window().active_player() == A {
            "A"
        } else if state.execution().window().active_player() == B {
            "B"
        } else {
            return Err("unknown active selector seat".into());
        };
        let active_key = || -> Result<String, String> {
            Ok(format!(
                "{seat}.{}",
                personal.ok_or("selector has no actual turn")?
            ))
        };
        if let Some(decision) = state.pending_decision() {
            match decision.frame() {
                RuleFrame::Opening(OpeningDecision::Bottom { .. }) => {
                    add("mulligan.bottom.before".into(), full)?
                }
                RuleFrame::Combat(CombatDecision::Attackers(_))
                    if fixture.case.is_trade() && turn == Some(5) =>
                {
                    add("young.live-domain".into(), full)?
                }
                RuleFrame::Combat(CombatDecision::Blockers(_))
                    if fixture.case.is_trade() && turn == Some(9) =>
                {
                    add("trade.before".into(), full)?
                }
                RuleFrame::Checks(CheckDecision::GraveyardOrder(_))
                    if fixture.case.is_trade() && turn == Some(9) =>
                {
                    add(
                        if decision.chooser() == A {
                            "trade.owner.A"
                        } else if decision.chooser() == B {
                            "trade.owner.B"
                        } else {
                            return Err("unknown trade order chooser".into());
                        }
                        .into(),
                        full,
                    )?;
                }
                RuleFrame::Checks(CheckDecision::CleanupDiscard(_)) if turn == Some(2) => {
                    add("cleanup.first.before".into(), full)?
                }
                _ => {}
            }
        }
        if let Some(input) = input {
            match &input.command {
                GameCommand::Reply {
                    choice: DecisionReply::Bottom(_),
                    ..
                } => add("mulligan.bottom.after".into(), full)?,
                GameCommand::Reply {
                    choice: DecisionReply::Discard(_),
                    ..
                } => {
                    let prior = source.ok_or("discard selector lacks real source")?;
                    let p = prior
                        .turn()
                        .ok_or("discard source has no calendar")?
                        .position();
                    if p.step != Step::Cleanup {
                        return Err("discard source not cleanup".into());
                    }
                    let player = prior.execution().window().active_player();
                    let n = p.number.div_ceil(2);
                    let label = if player == A {
                        "A"
                    } else if player == B {
                        "B"
                    } else {
                        return Err("discard owner unknown".into());
                    };
                    let tail = fixture.case == CaseId::G03
                        && ((player == B && n == 53) || (player == A && n == 54));
                    add(
                        format!("cleanup.{label}.{n}"),
                        if p.number == 2 || tail {
                            full
                        } else {
                            evidence
                        },
                    )?;
                    if tail {
                        add(format!("cleanup.tail.{label}"), full)?;
                    }
                }
                GameCommand::Reply {
                    choice: DecisionReply::OrderGraveyard(_),
                    ..
                } if fixture.case.is_trade() && input.actor == B => {
                    add("trade.settled".into(), full)?
                }
                GameCommand::CastUntargeted { .. } => add(format!("cast.{}", active_key()?), full)?,
                _ => {}
            }
        }
        if candidate.land_play().is_some() {
            add(format!("land.{}", active_key()?), full)?;
        }
        if let Some(produced) = candidate.mana_production() {
            let physical = state
                .execution()
                .window()
                .cards()
                .get(produced.source)
                .ok_or("mana selector source missing")?;
            let label = &fixture
                .aliases
                .get(
                    &physical
                        .card_id()
                        .ok_or("named ordinary fixture mana source is not physical")?,
                )
                .ok_or("mana selector alias missing")?
                .alias;
            let suffix = if label.ends_with(".P1") {
                1
            } else if label.ends_with(".P2") {
                2
            } else {
                return Err("unplanned named mana selector source".into());
            };
            add(format!("mana.{}.{suffix}", active_key()?), full)?;
        }
        if candidate.permanent_entry().is_some() {
            add(format!("resolve.{}", active_key()?), full)?;
        }
        for row in candidate.combat_declarations() {
            if let CombatDeclarationRecord::Attackers {
                origin, attackers, ..
            } = row
            {
                if fixture.case == CaseId::G01
                    && turn == Some(3)
                    && *origin == DeclarationOrigin::ForcedEmpty
                {
                    add("young.forced-empty".into(), full)?;
                }
                if !attackers.is_empty() {
                    add(format!("attack.{}", active_key()?), evidence)?;
                }
            }
        }
        if candidate
            .damage_journal()
            .applied
            .iter()
            .any(|v| matches!(v.target, DamageRecipient::Player(_)))
        {
            add(
                format!(
                    "damage.{}",
                    personal.ok_or("damage selector calendar absent")?
                ),
                full,
            )?;
        }
        if position.is_some_and(|v| v.number == 3 && v.step == Step::Upkeep)
            && state.execution().window().phase()
                == WindowPhase::Priority(PriorityPhase::Ready { holder: A })
            && source.is_some_and(|v| v.turn().is_some_and(|v| v.position().number == 2))
        {
            add("rotation.first".into(), full)?;
        }
        if fixture.case == CaseId::G03 {
            for row in &candidate.draw_journal().events {
                if let DrawEvent::Drawn { player, after, .. } = row {
                    let label = &fixture
                        .aliases
                        .get(&after.card_id().ok_or("draw receipt is not physical")?)
                        .ok_or("draw selector alias missing")?
                        .alias;
                    if label.ends_with(".P60") {
                        add(
                            format!("draw.last.{}", if *player == A { "A" } else { "B" }),
                            full,
                        )?;
                    }
                }
            }
            if position.is_some_and(|v| v.number == 108 && v.step == Step::Upkeep) {
                match state.execution().window().phase() {
                    WindowPhase::Priority(PriorityPhase::Ready { holder: B }) => {
                        add("failed-draw.before".into(), full)?
                    }
                    WindowPhase::Priority(PriorityPhase::Ready { holder: A }) => {
                        add("failed-draw.after-first-pass".into(), full)?
                    }
                    _ => {}
                }
            }
        }
        if state.outcome().is_some() {
            add("terminal".into(), full)?;
        }
        Ok(found)
    }
    fn observe_each_candidate(
        &mut self,
        fixture: &Fixture,
        candidate: &NativeCandidate,
        ordinal: usize,
    ) -> Result<Vec<Observation>, String> {
        let name = format!("{}.candidate.{ordinal}", fixture.case.as_str());
        if !self.seen.insert(name.clone()) {
            return Err("duplicate interaction candidate selector".into());
        }
        if ordinal == 0 {
            self.seen.insert("initialization".into());
        }
        if candidate.state().outcome().is_some() {
            self.seen.insert("terminal".into());
        }
        Ok(vec![Observation {
            semantic_selector_ref: name.clone(),
            name,
            candidate_ordinal: ordinal,
            scope: Scope::FullCheckpoint,
        }])
    }
    pub fn finish(&self, case: CaseId) -> Result<(), String> {
        if let CaseId::Optional(case) = case {
            if !self.seen.contains("initialization")
                || (case.is_full_game() && !self.seen.contains("terminal"))
            {
                return Err(
                    "optional/payment initialization or full-game terminal selector missing".into(),
                );
            }
            return Ok(());
        }
        if matches!(case, CaseId::Mandatory(_)) {
            // Policy::finish separately enforces the source-declared natural
            // outcome or bounded stop. Every actual exposed candidate has its
            // own checkpoint here, including that final state.
            if !self.seen.contains("initialization") {
                return Err("mandatory initialization selector missing".into());
            }
            return Ok(());
        }
        if let CaseId::Interaction(case) = case {
            if !self.seen.contains("initialization") {
                return Err("interaction initialization selector missing".into());
            }
            if matches!(
                case,
                super::interaction::InteractionCase::Bi01
                    | super::interaction::InteractionCase::Bi02
                    | super::interaction::InteractionCase::If11
                    | super::interaction::InteractionCase::If12
            ) && !self.seen.contains("terminal")
            {
                return Err("interaction terminal selector missing".into());
            }
            return Ok(());
        }
        if matches!(case, CaseId::Teaching(_)) {
            if !self.seen.contains("initialization")
                || !self.seen.contains("opening.complete")
                || !self.seen.contains("terminal")
            {
                return Err("teaching initialization/opening/terminal selectors missing".into());
            }
            return Ok(());
        }
        let mut expected = BTreeSet::from([
            "initialization".into(),
            "opening.complete".into(),
            "cleanup.first.before".into(),
            "rotation.first".into(),
            "terminal".into(),
        ]);
        let mut add = |key: String| {
            expected.insert(key);
        };
        match case {
            CaseId::Selection(_) => {
                return Err(
                    "selection old-selector profile is unsupported in fixture-only admission"
                        .into(),
                );
            }
            CaseId::G01 => {
                for n in 1..=12 {
                    add(format!("land.A.{n}"));
                }
                for n in 1..=2 {
                    add(format!("mana.A.2.{n}"));
                }
                add("cast.A.2".into());
                add("resolve.A.2".into());
                add("young.forced-empty".into());
                for n in 3..=12 {
                    add(format!("attack.A.{n}"));
                    add(format!("damage.{n}"));
                }
                for n in 1..=11 {
                    add(format!("cleanup.B.{n}"));
                }
            }
            CaseId::G02a | CaseId::G02b => {
                for key in [
                    "mulligan.bottom.before",
                    "mulligan.bottom.after",
                    "young.live-domain",
                    "trade.before",
                    "trade.owner.A",
                    "trade.owner.B",
                    "trade.settled",
                ] {
                    add(key.into());
                }
                for n in 1..=15 {
                    add(format!("land.A.{n}"));
                }
                for n in 2..=14 {
                    add(format!("land.B.{n}"));
                }
                for (seat, n) in [("A", 2), ("A", 3), ("A", 5), ("B", 3), ("B", 4)] {
                    for j in 1..=2 {
                        add(format!("mana.{seat}.{n}.{j}"));
                    }
                    add(format!("cast.{seat}.{n}"));
                    add(format!("resolve.{seat}.{n}"));
                }
                for n in 5..=15 {
                    add(format!("attack.A.{n}"));
                    if n > 5 {
                        add(format!("damage.{n}"));
                    }
                }
                add("cleanup.B.1".into());
            }
            CaseId::Teaching(_)
            | CaseId::Interaction(_)
            | CaseId::Mandatory(_)
            | CaseId::Optional(_) => {
                return Err("catalogue selectors use explicit profile".into());
            }
            CaseId::G03 => {
                for n in 1..=53 {
                    add(format!("cleanup.B.{n}"));
                }
                for n in 2..=54 {
                    add(format!("cleanup.A.{n}"));
                }
                for key in [
                    "draw.last.A",
                    "draw.last.B",
                    "cleanup.tail.A",
                    "cleanup.tail.B",
                    "failed-draw.before",
                    "failed-draw.after-first-pass",
                ] {
                    add(key.into());
                }
            }
        }
        if self.seen != expected {
            return Err(format!(
                "selector set mismatch: missing {:?}; extra {:?}",
                expected.difference(&self.seen).collect::<Vec<_>>(),
                self.seen.difference(&expected).collect::<Vec<_>>()
            ));
        }
        Ok(())
    }
}

impl Selectors {
    fn observe_teaching(
        &mut self,
        _fixture: &Fixture,
        source: Option<&GameState>,
        input: Option<&GameInput>,
        candidate: &NativeCandidate,
        ordinal: usize,
    ) -> Result<Vec<Observation>, String> {
        let state = candidate.state();
        let mut names = vec![];
        if ordinal == 0 {
            names.push("initialization".to_owned());
        }
        if state.opening().is_some_and(|v| v.complete())
            && source.is_some_and(|v| v.opening().is_some_and(|o| !o.complete()))
        {
            names.push("opening.complete".to_owned());
        }
        if state.outcome().is_some() {
            names.push("terminal".to_owned());
        }
        if input.is_some_and(|i| !matches!(i.command, GameCommand::Pass))
            || source.is_some_and(|s| {
                s.turn().map(|t| t.position()) != state.turn().map(|t| t.position())
                    || s.pending_decision() != state.pending_decision()
            })
        {
            names.push(format!("actual.{ordinal}"));
        }
        names
            .into_iter()
            .map(|name| {
                if !self.seen.insert(name.clone()) {
                    return Err(format!("teaching selector matched twice: {name}"));
                }
                Ok(Observation {
                    semantic_selector_ref: name.clone(),
                    name,
                    candidate_ordinal: ordinal,
                    scope: Scope::FullCheckpoint,
                })
            })
            .collect()
    }
}
