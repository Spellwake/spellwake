//! Finite wire adapters over shared observations, never additional rules owners.
use super::*;
use serde::{Serializer, ser::SerializeStruct};
use spellwake_engine::program::{ExecutableBody, Instruction, ProgramKind};

#[derive(Debug, Clone, Serialize)]
pub(super) struct ExecutionObservation {
    pub stack: Vec<serde_json::Value>,
    pub resolution: Option<serde_json::Value>,
    pub waiting: Vec<serde_json::Value>,
    pub pending_trigger_placement: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct OptionalExecutionObservation {
    #[serde(flatten)]
    pub execution: ExecutionObservation,
    pub pending_work: serde_json::Value,
    pub copies: Vec<serde_json::Value>,
    pub resolving: Option<serde_json::Value>,
}
#[derive(Debug, Clone)]
pub(super) enum HeaderEpoch {
    Legacy(Vec<StackView>),
    Interaction {
        legacy_stack: Vec<StackView>,
        execution: ExecutionObservation,
    },
    Mandatory(ExecutionObservation),
    OptionalPayment(OptionalExecutionObservation),
}
#[derive(Debug, Clone)]
pub(super) enum JournalEpoch {
    Absent,
    Interaction(serde_json::Value),
    Mandatory(serde_json::Value),
    OptionalPayment(serde_json::Value),
}
impl Serialize for Header {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut row = serializer.serialize_struct(
            "Header",
            match &self.epoch {
                HeaderEpoch::Interaction { .. } => 25,
                HeaderEpoch::Legacy(_)
                | HeaderEpoch::Mandatory(_)
                | HeaderEpoch::OptionalPayment(_) => 24,
            },
        )?;
        macro_rules! fields { ($($name:ident),* $(,)?) => { $(row.serialize_field(stringify!($name), &self.$name)?;)* }; }
        fields!(
            basis,
            executor_revision,
            window_revision,
            registry_revision,
            participants,
            active,
            combat_rules,
            turn,
            pending_turn_work,
            phase,
            priority_holder
        );
        match &self.epoch {
            HeaderEpoch::Legacy(stack)
            | HeaderEpoch::Interaction {
                legacy_stack: stack,
                ..
            } => row.serialize_field("stack", stack)?,
            HeaderEpoch::Mandatory(_) | HeaderEpoch::OptionalPayment(_) => {}
        }
        fields!(
            pending,
            instruction_index,
            life,
            mana,
            counts_by_owner,
            battlefield_counts_by_controller,
            losses,
            pending_eliminations,
            outcome,
            opening,
            entropy,
            combat
        );
        match &self.epoch {
            HeaderEpoch::Legacy(_) => {}
            HeaderEpoch::Interaction { execution, .. } => {
                #[derive(Serialize)]
                struct HistoricalExecution<'a> {
                    stack: &'a [serde_json::Value],
                    resolution: &'a Option<serde_json::Value>,
                }
                row.serialize_field(
                    "interaction_execution",
                    &HistoricalExecution {
                        stack: &execution.stack,
                        resolution: &execution.resolution,
                    },
                )?;
            }
            HeaderEpoch::Mandatory(execution) => row.serialize_field("execution", execution)?,
            HeaderEpoch::OptionalPayment(execution) => {
                row.serialize_field("execution", execution)?
            }
        }
        row.end()
    }
}
impl Serialize for Facts {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut row = serializer.serialize_struct(
            "Facts",
            match &self.epoch {
                JournalEpoch::Absent => 12,
                JournalEpoch::Interaction(_)
                | JournalEpoch::Mandatory(_)
                | JournalEpoch::OptionalPayment(_) => 13,
            },
        )?;
        macro_rules! fields { ($($name:ident),* $(,)?) => { $(row.serialize_field(stringify!($name), &self.$name)?;)* }; }
        fields!(
            shuffle_journal,
            draw_journal,
            turn_transitions,
            land_play,
            mana_production,
            mana_payment,
            ability_activation,
            permanent_entry,
            combat_declarations,
            combat_assignments,
            damage_journal,
            check_journal
        );
        match &self.epoch {
            JournalEpoch::Absent => {}
            JournalEpoch::Interaction(journal) => {
                row.serialize_field("interaction_journal", journal)?
            }
            JournalEpoch::Mandatory(journal) => row.serialize_field("rules_journal", journal)?,
            JournalEpoch::OptionalPayment(journal) => {
                row.serialize_field("rules_journal", journal)?
            }
        }
        row.end()
    }
}
/// Fixture equality is provenance only. The calling ordinary header/facts owner
/// first applies the shared borrowed semantic-capability check; mutable matching
/// fixtures do not authorize a new capability or extend a predecessor epoch.
fn check_fixture_programs(
    fixture: &Fixture,
    candidate: &NativeCandidate,
    profile: &str,
) -> Result<(), String> {
    let execution = candidate.state().execution();
    for (card, program) in execution.programs() {
        if !fixture
            .setup
            .seed
            .cards
            .iter()
            .any(|seed| seed.card == card && &seed.program == program)
        {
            return Err(format!(
                "{profile} projection requires exact source-guarded fixture Programs"
            ));
        }
    }
    // A token's definition must come from an actual admitted creation body.
    // Compare whole materialized Programs, including names, colors and subtypes.
    let mut templates = Vec::new();
    let mut observe_body = |body: &ExecutableBody| -> Result<(), String> {
        for instruction in &body.instructions {
            if let Instruction::CreateTokens { creations } = instruction {
                for creation in creations {
                    templates.push(
                        creation
                            .definition
                            .materialize(execution.creature_vocabulary())
                            .map_err(|error| format!("{profile} token template: {error:?}"))?,
                    );
                }
            }
        }
        Ok(())
    };
    for definition in &fixture.definitions {
        if let ProgramKind::Spell(spell) = &definition.program.kind {
            observe_body(&spell.body)?;
        }
        for trigger in &definition.program.triggered {
            observe_body(&trigger.body)?;
        }
    }
    for object in execution
        .window()
        .cards()
        .iter()
        .filter(|object| object.card_id().is_none())
    {
        if !templates
            .iter()
            .any(|program| Some(program) == execution.base_program(object.id()))
        {
            return Err(format!(
                "{profile} projection excludes unrepresented token definitions"
            ));
        }
    }
    Ok(())
}
pub(super) fn check_mandatory_scope(
    fixture: &Fixture,
    candidate: &NativeCandidate,
) -> Result<(), String> {
    check_fixture_programs(fixture, candidate, "mandatory")?;
    let execution = candidate.state().execution();
    if execution
        .resolution_state()
        .is_some_and(|cursor| !cursor.locals().is_empty())
        || matches!(
            candidate
                .state()
                .pending_decision()
                .map(|pending| pending.frame()),
            Some(
                RuleFrame::CreatureType(_)
                    | RuleFrame::CopyTargets(_)
                    | RuleFrame::ManaColor(_)
                    | RuleFrame::Damage(_)
                    | RuleFrame::OptionalEffect(_)
                    | RuleFrame::ResolutionPayment(_)
            )
        )
    {
        return Err("mandatory projection excludes unrepresented resolution choices".into());
    }
    if candidate.rules_journal().records.iter().any(|record| {
        matches!(
            record,
            RulesRecord::OptionalEffectOutcome { .. }
                | RulesRecord::ResolutionPaymentOutcome { .. }
        )
    }) {
        return Err("ordinary Report1–3 exclude resolution choice controls".into());
    }
    check_predecessor_instruction_journal(candidate.rules_journal())
}
/// Admit the selected source Programs and their complete retained bodies. Native
/// execution remains responsible for legality, role resolution and payment.
pub(super) fn check_optional_scope(
    fixture: &Fixture,
    candidate: &NativeCandidate,
) -> Result<(), String> {
    check_fixture_programs(fixture, candidate, "optional/payment")?;
    let execution = candidate.state().execution();
    if execution
        .resolution_state()
        .is_some_and(|cursor| !cursor.locals().is_empty())
        || matches!(
            candidate
                .state()
                .pending_decision()
                .map(|pending| pending.frame()),
            Some(RuleFrame::CreatureType(_) | RuleFrame::Damage(_))
        )
    {
        return Err("optional/payment projection excludes unrepresented resolution choices".into());
    }
    let represented_body = |body: &ExecutableBody| {
        fixture.definitions.iter().any(|definition| {
            matches!(&definition.program.kind, ProgramKind::Spell(spell) if &spell.body == body)
                || definition
                    .program
                    .triggered
                    .iter()
                    .any(|trigger| &trigger.body == body)
        })
    };
    let represented_trigger = |source: &GameObject, definition, body: &ExecutableBody| {
        source.card_id().is_some_and(|card| {
            fixture.setup.seed.cards.iter().any(|seed| {
                seed.card == card
                    && seed
                        .program
                        .triggered
                        .iter()
                        .any(|trigger| trigger.id == definition && &trigger.body == body)
            })
        })
    };
    for waiting in execution.waiting_triggers() {
        if !represented_trigger(&waiting.source.object, waiting.definition, &waiting.body) {
            return Err("optional/payment projection excludes foreign waiting bodies".into());
        }
    }
    for entry in execution
        .window()
        .entries()
        .iter()
        .chain(execution.window().resolving())
    {
        if let Some(body) = execution.stack_body(entry.id()) {
            if !represented_body(body) {
                return Err("optional/payment projection excludes foreign stack bodies".into());
            }
            if let Some(binding) = execution
                .execution_bindings(entry.id())
                .and_then(|bindings| bindings.trigger.as_ref())
                && !represented_trigger(&binding.source.object, binding.definition, body)
            {
                return Err(
                    "optional/payment projection excludes foreign captured trigger bodies".into(),
                );
            }
        }
        if matches!(entry.kind(), StackKind::SpellCopy { .. }) {
            let matched = fixture.definitions.iter().any(|definition| {
                let program = &definition.program;
                let ProgramKind::Spell(spell) = &program.kind else {
                    return false;
                };
                execution.stack_body(entry.id()) == Some(&spell.body)
                    && execution.stack_copiable_card_types(entry.id()) == Some(&program.card_types)
                    && execution.stack_copiable_creature_subtypes(entry.id())
                        == Some(&program.creature_subtypes)
                    && execution.stack_copiable_names(entry.id()) == Some(&program.names)
                    && execution.stack_copiable_supertypes(entry.id()) == Some(&program.supertypes)
                    && execution.stack_copiable_triggered_abilities(entry.id())
                        == Some(program.triggered.as_slice())
            });
            if !matched {
                return Err("optional/payment projection excludes foreign copied payloads".into());
            }
        }
    }
    check_predecessor_instruction_journal(candidate.rules_journal())
}
fn check_predecessor_instruction_journal(journal: &RulesJournal) -> Result<(), String> {
    for record in &journal.records {
        if matches!(
            record,
            RulesRecord::InstructionBranch { .. }
                | RulesRecord::InstructionJump { .. }
                | RulesRecord::ChooserReplaced { .. }
        ) || matches!(record, RulesRecord::Event(event) if event.occurrences.iter().any(|occurrence|
                matches!(occurrence.value, RulesOccurrence::CreatureTypeChosen { .. })))
        {
            return Err("mandatory projection excludes unrepresented instruction choices".into());
        }
    }
    Ok(())
}
