//! Source-qualified compiled recipe admission and the real privileged Fixture factory.
use super::{data::*, prepared::materialize_prepared, source::SelectionSource};
use crate::{
    ordinary_game::{
        catalogue::{DefinitionInput, PhysicalRow},
        fixture::{A, B, CaseId, ContextInputs, Fixture, FixtureInitialization},
        mandatory::data::*,
    },
    packet,
};
use sha2::{Digest, Sha256};
use spellwake_card_programs::RESOLUTION_SELECTION_PROFILE;
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    combat::CombatRules,
    game::{GameContext, OpeningSetup},
    object_identity::CardId,
    setup::GameSeed,
    shuffle::EntropyTape,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const INPUTS: &[u8] = include_bytes!("selection-inputs-v3.json");
const INPUT_SHA256: &str = "c8135acffe65288655d0678ccaf4a27c921bf5ad704fdba3965fc2f5a617bad4";
fn name(value: &str) -> bool {
    !value.is_empty() && value.trim() == value
}

/// Scope-specific presence checks operate on the same duplicate-safe raw value.
/// The shared generic field table remains the sole typed decoder.
pub(super) fn parse(bytes: &[u8]) -> Result<SelectionInputs, String> {
    let raw = packet::unique_json(bytes).map_err(|e| format!("selection inputs JSON: {e}"))?;
    let cases = raw
        .get("cases")
        .and_then(serde_json::Value::as_array)
        .ok_or("selection inputs cases required")?;
    for case in cases {
        if !case
            .get("decision_plans")
            .is_some_and(serde_json::Value::is_array)
            || !case
                .get("reached_probe_limit")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|n| n > 0)
        {
            return Err(
                "selection requires explicit decision_plans and positive reached_probe_limit"
                    .into(),
            );
        }
    }
    let inputs: SelectionInputs =
        serde_json::from_value(raw).map_err(|e| format!("selection inputs schema: {e}"))?;
    let e = &inputs.epochs;
    if inputs.schema_version != 1
        || inputs.profile != RESOLUTION_SELECTION_PROFILE
        || (
            e.registry,
            e.report,
            e.aggregate,
            e.current,
            e.witness,
            e.record_codec,
            e.vocabulary,
        ) != (9, 5, 1, 11, 9, 2, 1)
        || inputs.seats.len() != 2
        || inputs.seats[0].role != Role::P0
        || inputs.seats[0].ordinary_role != "A"
        || inputs.seats[0].native_seat != u64::from(A.0)
        || inputs.seats[1].role != Role::P1
        || inputs.seats[1].ordinary_role != "B"
        || inputs.seats[1].native_seat != u64::from(B.0)
        || inputs.definitions.len() != 9
        || inputs.authorities.len() != 5
        || inputs
            .cases
            .iter()
            .map(|case| case.id)
            .ne(SelectionCase::ALL)
    {
        return Err("selection closed input header/case inventory mismatch".into());
    }
    for case in &inputs.cases {
        validate_case(case)?;
    }
    Ok(inputs)
}
pub(super) fn inputs(definitions: &[DefinitionInput]) -> Result<SelectionInputs, String> {
    if format!("{:x}", Sha256::digest(INPUTS)) != INPUT_SHA256 {
        return Err("compiled selection positive-input authority mismatch".into());
    }
    let inputs = parse(INPUTS)?;
    validate_definitions(&inputs, definitions)?;
    Ok(inputs)
}
fn validate_definitions(
    inputs: &SelectionInputs,
    definitions: &[DefinitionInput],
) -> Result<(), String> {
    if definitions.len() != 9 || inputs.definitions.len() != 9 {
        return Err("selection complete definition count mismatch".into());
    }
    for (source, input) in definitions.iter().zip(&inputs.definitions) {
        if source.name != input.name
            || source.oracle_id != input.oracle_id
            || source.scryfall_id != input.printing_id
            || source.source_line_one_based != input.source_line
            || source.raw_line_sha256_including_lf != input.raw_line_sha256
            || source.registry_signature_v1_sha256 != input.semantic_sha256
        {
            return Err("selection complete definition authority mismatch".into());
        }
    }
    Ok(())
}

pub(in crate::ordinary_game) fn recipe(case: SelectionCase) -> Result<SelectionInputCase, String> {
    if format!("{:x}", Sha256::digest(INPUTS)) != INPUT_SHA256 {
        return Err("compiled selection authority mismatch".into());
    }
    parse(INPUTS)?
        .cases
        .into_iter()
        .find(|r| r.id == case)
        .ok_or_else(|| "selection case absent".into())
}

/// Static reference membership only. Native frames, legality and receipts are
/// deliberately not predicted by this fixture admission boundary.
struct References<'a> {
    case: &'a SelectionInputCase,
    aliases: BTreeSet<&'a str>,
    instructions: BTreeMap<&'a str, &'a Instruction<super::prepared::PreparedManaType>>,
    triggers: BTreeSet<&'a str>,
    causes: BTreeSet<&'a str>,
    saved: BTreeSet<&'a str>,
}
impl References<'_> {
    fn entry(&self, entry: &EntryRef) -> Result<(), String> {
        let valid = match entry {
            EntryRef::CastEntry { operation } => matches!(
                self.instructions.get(operation.as_str()),
                Some(Instruction::CastUntargeted { .. } | Instruction::CastTargeted { .. })
            ),
            EntryRef::TriggerEntry { trigger } => self.triggers.contains(trigger.as_str()),
            EntryRef::CopyEntry { .. } => false,
        };
        if valid {
            Ok(())
        } else {
            Err("selection unknown entry reference".into())
        }
    }
    fn object(&self, object: &ObjectRef) -> Result<(), String> {
        match object {
            ObjectRef::PhysicalCurrent { alias } => self.alias(alias),
            ObjectRef::PhysicalAtCause { alias, cause, .. } => {
                self.alias(alias)?;
                self.cause(cause)
            }
            ObjectRef::SavedObject { binding } => {
                if self.saved.contains(binding.as_str()) {
                    Ok(())
                } else {
                    Err("selection unknown saved binding".into())
                }
            }
            ObjectRef::CreatedToken { creation_entry, .. } => self.entry(creation_entry),
            ObjectRef::TokenAtCause { token, cause, .. } => {
                self.entry(&token.creation_entry)?;
                self.cause(cause)
            }
        }
    }
    fn alias(&self, alias: &str) -> Result<(), String> {
        if self.aliases.contains(alias) {
            Ok(())
        } else {
            Err("selection unknown physical alias".into())
        }
    }
    fn cause(&self, cause: &str) -> Result<(), String> {
        if self.causes.contains(cause) {
            Ok(())
        } else {
            Err("selection unknown cause reference".into())
        }
    }
    fn objects(&self, refs: &[ObjectRef]) -> Result<(), String> {
        for value in refs {
            self.object(value)?;
        }
        Ok(())
    }
    fn target(&self, target: &Target) -> Result<(), String> {
        match target {
            Target::Object { object } => self.object(object),
            Target::Stack { entry } => self.entry(entry),
            Target::Player { .. } => Ok(()),
        }
    }
    fn entries(&self, entries: &[EntryRef]) -> Result<(), String> {
        for e in entries {
            self.entry(e)?;
        }
        Ok(())
    }
    fn point(&self, p: &Point) -> Result<(), String> {
        self.entries(&p.stack_bottom_first)
    }
    fn settlement(&self, s: &Settlement) -> Result<(), String> {
        let groups = match s {
            Settlement::RetainPriority {} => return Ok(()),
            Settlement::Drain { groups } => groups,
            Settlement::YieldAtStack {
                groups,
                stack_bottom_first,
            } => {
                self.entries(stack_bottom_first)?;
                groups
            }
        };
        if groups
            .iter()
            .any(|name| !self.case.trigger_groups.iter().any(|g| &g.name == name))
        {
            return Err("selection unknown trigger group".into());
        }
        Ok(())
    }
}
fn distinct_names<'a>(values: impl Iterator<Item = &'a str>) -> Result<BTreeSet<&'a str>, String> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !name(value) || !seen.insert(value) {
            return Err("selection blank or duplicate identity".into());
        }
    }
    Ok(seen)
}
pub(super) fn validate_case(case: &SelectionInputCase) -> Result<(), String> {
    if case.reached_probe_limit.is_none_or(|n| n == 0)
        || case.budget.maximum_callbacks == 0
        || case.budget.maximum_accepted_commands == 0
        || case.budget.maximum_individual_turn == 0
    {
        return Err("selection input budget mismatch".into());
    }
    let aliases = distinct_names(case.inventory.iter().map(|r| r.alias.as_str()))?;
    distinct_names(case.instructions.iter().map(Instruction::id))?;
    distinct_names(case.trigger_groups.iter().map(|g| g.name.as_str()))?;
    let triggers = distinct_names(
        case.trigger_groups
            .iter()
            .flat_map(|g| g.triggers.iter().map(|t| t.name.as_str())),
    )?;
    let causes = distinct_names(case.causal_operations.iter().map(|c| match c {
        CausalOperation::CastCompleted { name, .. }
        | CausalOperation::CastResolution { name, .. }
        | CausalOperation::CopyResolution { name, .. }
        | CausalOperation::TriggerResolution { name, .. }
        | CausalOperation::CombatDamage { name, .. } => name.as_str(),
    }))?;
    let saved = distinct_names(case.instructions.iter().filter_map(|i| match i {
        Instruction::SaveObject { binding, .. } => Some(binding.as_str()),
        _ => None,
    }))?;
    let refs = References {
        case,
        aliases,
        instructions: case.instructions.iter().map(|i| (i.id(), i)).collect(),
        triggers,
        causes,
        saved,
    };
    for i in &case.instructions {
        match i {
            Instruction::PlayLand { point, source, .. }
            | Instruction::ActivateMana { point, source, .. }
            | Instruction::SaveObject { point, source, .. }
            | Instruction::ProbeMana { point, source, .. } => {
                refs.point(point)?;
                refs.object(source)?;
            }
            Instruction::CastUntargeted {
                point,
                source,
                settlement,
                ..
            } => {
                refs.point(point)?;
                refs.object(source)?;
                refs.settlement(settlement)?;
            }
            Instruction::CastTargeted {
                point,
                source,
                target,
                settlement,
                ..
            } => {
                refs.point(point)?;
                refs.object(source)?;
                refs.target(target)?;
                refs.settlement(settlement)?;
            }
            Instruction::PassTo {
                stack_bottom_first, ..
            } => refs.entries(stack_bottom_first)?,
            Instruction::AdvanceReady { .. } => {}
            Instruction::Combat {
                attackers,
                blockers,
                settlement,
                ..
            } => {
                refs.objects(attackers)?;
                for b in blockers {
                    refs.object(&b.blocker)?;
                    refs.object(&b.attacker)?;
                }
                refs.settlement(settlement)?;
            }
        }
    }
    for c in &case.causal_operations {
        match c {
            CausalOperation::CastCompleted { entry, .. }
            | CausalOperation::CastResolution { entry, .. }
            | CausalOperation::CopyResolution { entry, .. }
            | CausalOperation::TriggerResolution { entry, .. } => refs.entry(entry)?,
            CausalOperation::CombatDamage {
                combat_instruction, ..
            } => {
                if !matches!(
                    refs.instructions.get(combat_instruction.as_str()),
                    Some(Instruction::Combat { .. })
                ) {
                    return Err("selection unknown combat cause".into());
                }
            }
        }
    }
    for group in &case.trigger_groups {
        let members = distinct_names(group.triggers.iter().map(|t| t.name.as_str()))?;
        if distinct_names(group.order_bottom_first.iter().map(String::as_str))? != members {
            return Err("selection trigger order is not complete".into());
        }
        for t in &group.triggers {
            refs.object(&t.source)?;
            refs.object(&t.subject)?;
            refs.cause(&t.cause_operation)?;
        }
    }
    for c in &case.player_choices {
        if !refs.triggers.contains(c.trigger.as_str()) {
            return Err("selection unknown player-choice trigger".into());
        }
    }
    for c in &case.timestamp_choices {
        refs.entry(&c.creation_entry)?;
    }
    for c in &case.graveyard_choices {
        refs.cause(&c.cause)?;
        refs.objects(&c.domain)?;
        refs.objects(&c.entrants_top_first)?;
    }
    if let Stop::BoundedAtStack {
        stack_bottom_first, ..
    } = &case.stop
    {
        refs.entries(stack_bottom_first)?;
    }
    let mut plan_ids = BTreeSet::new();
    for plan in &case.decision_plans {
        let (id, entry, domain, count, selected, probes, order) = match plan {
            SelectionDecisionPlan::ResolutionSelect {
                id,
                entry,
                domain,
                required_count,
                selected,
                probes,
                ..
            } => (id, entry, domain, required_count, selected, probes, false),
            SelectionDecisionPlan::ResolutionGraveyardOrder {
                id,
                entry,
                domain,
                entrant_count,
                entrants_top_first,
                probes,
                ..
            } => (
                id,
                entry,
                domain,
                entrant_count,
                entrants_top_first,
                probes,
                true,
            ),
        };
        if !name(id) || !plan_ids.insert(id) {
            return Err("selection blank or duplicate plan identity".into());
        }
        refs.entry(entry)?;
        refs.objects(domain)?;
        refs.objects(selected)?;
        let Some(n) = count.to_usize() else {
            return Err("selection count outside finite domain".into());
        };
        if n == 0
            || n > domain.len()
            || selected.len() != n
            || domain
                .iter()
                .enumerate()
                .any(|(i, x)| domain[..i].contains(x))
            || selected
                .iter()
                .enumerate()
                .any(|(i, x)| selected[..i].contains(x) || !domain.contains(x))
            || (order
                && (domain.len() != n
                    || domain.iter().any(|x| {
                        !matches!(
                            x,
                            ObjectRef::PhysicalCurrent { .. } | ObjectRef::PhysicalAtCause { .. }
                        )
                    })))
        {
            return Err("selection positive plan domain/count mismatch".into());
        }
        super::probes::validate_attachment(case.id, id, probes)?;
    }
    Ok(())
}

/// Bind authored plan coordinates to the actual guarded Program instruction.
/// This inspects source structure only; it does not execute or predict a frame.
fn validate_source_plans(
    recipe: &SelectionInputCase,
    materialized: &crate::ordinary_game::catalogue::Materialized,
) -> Result<(), String> {
    use spellwake_engine::program::{
        CardType, ExecutableBody, Instruction as NativeInstruction, Program, ProgramKind,
    };
    let physical_program = |object: &ObjectRef| -> Result<&Program, String> {
        let alias = match object {
            ObjectRef::PhysicalCurrent { alias } | ObjectRef::PhysicalAtCause { alias, .. } => {
                alias
            }
            _ => return Err("selection source reference requires a declared physical card".into()),
        };
        let card = materialized
            .aliases
            .iter()
            .find(|(_, r)| &r.alias == alias)
            .map(|(id, _)| *id)
            .ok_or("selection source alias missing")?;
        materialized
            .cards
            .iter()
            .find(|r| r.card == card)
            .map(|r| &r.program)
            .ok_or_else(|| "selection source Program missing".into())
    };
    let body = |entry: &EntryRef| -> Result<&ExecutableBody, String> {
        match entry {
            EntryRef::CastEntry { operation } => {
                let source = recipe
                    .instructions
                    .iter()
                    .find_map(|i| match i {
                        Instruction::CastTargeted { id, source, .. }
                        | Instruction::CastUntargeted { id, source, .. }
                            if id == operation =>
                        {
                            Some(source)
                        }
                        _ => None,
                    })
                    .ok_or("selection cast source absent")?;
                match &physical_program(source)?.kind {
                    ProgramKind::Spell(spell) => Ok(&spell.body),
                    _ => Err("selection plan cast source has no spell body".into()),
                }
            }
            EntryRef::TriggerEntry { trigger } => {
                let declaration = recipe
                    .trigger_groups
                    .iter()
                    .flat_map(|g| &g.triggers)
                    .find(|t| &t.name == trigger)
                    .ok_or("selection trigger source absent")?;
                physical_program(&declaration.source)?
                    .triggered
                    .iter()
                    .find(|a| a.id.0.to_string() == declaration.definition_id)
                    .map(|a| &a.body)
                    .ok_or_else(|| "selection trigger definition absent from guarded source".into())
            }
            EntryRef::CopyEntry { .. } => {
                Err("selection positive authority excludes copy entries".into())
            }
        }
    };
    for group in &recipe.trigger_groups {
        for trigger in &group.triggers {
            body(&EntryRef::TriggerEntry {
                trigger: trigger.name.clone(),
            })?;
        }
    }
    for plan in &recipe.decision_plans {
        let (entry, instruction, operation) = match plan {
            SelectionDecisionPlan::ResolutionSelect {
                entry,
                instruction,
                operation,
                ..
            }
            | SelectionDecisionPlan::ResolutionGraveyardOrder {
                entry,
                instruction,
                operation,
                ..
            } => (entry, instruction, operation),
        };
        let instruction = usize::try_from(*instruction)
            .ok()
            .and_then(|i| body(entry).ok()?.instructions.get(i))
            .ok_or("selection plan instruction absent from guarded source")?;
        let matches = match (operation, instruction) {
            (
                SelectionOperation::SacrificeCreature,
                NativeInstruction::Sacrifice { filter, .. },
            ) => {
                filter.required_types == BTreeSet::from([CardType::Creature])
                    && filter.forbidden_types.is_empty()
            }
            (SelectionOperation::DiscardCards, NativeInstruction::Discard { .. }) => true,
            _ => false,
        };
        if !matches {
            return Err("selection plan operation differs from guarded source instruction".into());
        }
    }
    Ok(())
}

impl SelectionSource {
    /// Build a genuine prepared fixture from the immutable full source9 owner.
    /// This privileged API admits initial state only, with no selected policy,
    /// Report5 projection, negative execution or natural complete-game claim.
    /// # Errors
    /// Rejects a foreign vocabulary or any compiled source, physical identity,
    /// resource, partition or reference mismatch without changing this owner.
    pub fn fixture(
        &self,
        case: SelectionCase,
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<Fixture, String> {
        self.validate_witness(&vocabulary.id().to_string())?;
        let inputs = inputs(self.definitions())?;
        let recipe = inputs
            .cases
            .iter()
            .find(|r| r.id == case)
            .ok_or("selection case absent")?;
        self.fixture_from_recipe(recipe, vocabulary)
    }
    fn fixture_from_recipe(
        &self,
        recipe: &SelectionInputCase,
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<Fixture, String> {
        self.validate_witness(&vocabulary.id().to_string())?;
        validate_case(recipe)?;
        let rows = recipe
            .inventory
            .iter()
            .enumerate()
            .map(|(i, r)| {
                Ok(PhysicalRow {
                    alias: r.alias.clone(),
                    card: CardId(
                        u64::try_from(i)
                            .map_err(|_| "selection physical ordinal overflow")?
                            .checked_add(1)
                            .ok_or("selection physical ordinal overflow")?,
                    ),
                    owner: r.owner.seat(),
                    oracle_id: r.oracle_id.clone(),
                    copy: u32::try_from(r.copy).map_err(|_| "selection copy ordinal overflow")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let prepared = materialize_prepared(
            self.inventory(),
            self.registry(),
            &vocabulary,
            self.definitions(),
            &rows,
            &recipe.initialization,
        )?;
        validate_source_plans(recipe, &prepared.materialized)?;
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context_inputs = ContextInputs {
            reference: format!("selection-context-v1-{}", recipe.id.as_str()),
            release: RESOLUTION_SELECTION_PROFILE.into(),
            rules: vocabulary.rules_revision().into(),
            creature_vocabulary: vocabulary.id(),
            name_rules_id: names.id(),
            catalogue: format!("oracle-sha256:{}", self.inventory().compressed_sha256),
            programs: "resolution-sacrifice-discard-v1:registry9".into(),
            profile: format!("resolution-sacrifice-discard-v1:{}", recipe.id.as_str()),
        };
        let context = GameContext::new(
            context_inputs.release.clone(),
            context_inputs.rules.clone(),
            context_inputs.catalogue.clone(),
            context_inputs.programs.clone(),
            context_inputs.profile.clone(),
            vocabulary.id(),
            names.id(),
        )
        .map_err(|e| format!("selection fixture context: {e:?}"))?;
        Ok(Fixture {
            case: CaseId::Selection(recipe.id),
            initialization: FixtureInitialization::Scheduled(prepared.start),
            game: CaseId::Selection(recipe.id).game_id(),
            context,
            context_inputs,
            setup: OpeningSetup {
                seed: GameSeed {
                    seats: vec![A, B],
                    active: prepared.active,
                    cards: prepared.materialized.cards,
                    initial_battlefield_order: prepared.initial_battlefield_order,
                    initial_world_cohorts: vec![],
                    mana: prepared.mana,
                    life: prepared.life,
                    combat_rules: CombatRules::UnrestrictedFreeForAll,
                    creature_vocabulary: vocabulary,
                    name_rules: names,
                },
                entropy: EntropyTape::new(vec![]),
            },
            aliases: prepared.materialized.aliases,
            definitions: prepared.materialized.definitions,
            shuffles: vec![],
        })
    }
}

#[cfg(test)]
mod tests;
