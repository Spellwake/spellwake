//! Strict compiled finite recipe grammar from the reviewed schema.
use super::MandatoryCase;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub(in crate::ordinary_game) enum Role {
    #[serde(rename = "P0")]
    P0,
    #[serde(rename = "P1")]
    P1,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct TokenBirth {
    pub kind: TokenBirthKind,
    pub creation_entry: EntryRef,
    pub instruction: u64,
    pub creator: Role,
    pub member: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum ObjectRef {
    PhysicalCurrent {
        alias: String,
    },
    PhysicalAtCause {
        alias: String,
        cause: String,
        appearance: Appearance,
    },
    SavedObject {
        binding: String,
    },
    CreatedToken {
        creation_entry: EntryRef,
        instruction: u64,
        creator: Role,
        member: u64,
    },
    TokenAtCause {
        token: TokenBirth,
        cause: String,
        appearance: Appearance,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub(in crate::ordinary_game) enum Appearance {
    #[serde(rename = "before")]
    Before,
    #[serde(rename = "after")]
    After,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
// Preserve the retained recipe wire tags and Debug variant names.
#[allow(clippy::enum_variant_names)]
pub(in crate::ordinary_game) enum EntryRef {
    CastEntry { operation: String },
    TriggerEntry { trigger: String },
    CopyEntry { copy: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum Target {
    Object { object: ObjectRef },
    Stack { entry: EntryRef },
    Player { role: Role },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct Boundary {
    pub turn: u64,
    pub step: RecipeStep,
    pub active: Role,
    pub holder: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub(in crate::ordinary_game) enum RecipeStep {
    #[serde(rename = "untap")]
    Untap,
    #[serde(rename = "upkeep")]
    Upkeep,
    #[serde(rename = "draw")]
    Draw,
    #[serde(rename = "precombat-main")]
    PrecombatMain,
    #[serde(rename = "beginning-combat")]
    BeginningCombat,
    #[serde(rename = "declare-attackers")]
    DeclareAttackers,
    #[serde(rename = "declare-blockers")]
    DeclareBlockers,
    #[serde(rename = "combat-damage")]
    CombatDamage,
    #[serde(rename = "end-combat")]
    EndCombat,
    #[serde(rename = "postcombat-main")]
    PostcombatMain,
    #[serde(rename = "end")]
    End,
    #[serde(rename = "cleanup")]
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct Point {
    pub boundary: Boundary,
    pub actor: Role,
    pub stack_bottom_first: Vec<EntryRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct PaymentItem<ScriptMana = Mana> {
    pub mana_type: ScriptMana,
    pub amount: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub(in crate::ordinary_game) enum Mana {
    #[serde(rename = "White")]
    White,
    #[serde(rename = "Blue")]
    Blue,
    #[serde(rename = "Black")]
    Black,
    #[serde(rename = "Red")]
    Red,
    #[serde(rename = "Green")]
    Green,
}

pub(in crate::ordinary_game) type Payment<ScriptMana = Mana> = Vec<PaymentItem<ScriptMana>>;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum Ability {
    BasicLand { land_type: Land },
    Printed { ability_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub(in crate::ordinary_game) enum Land {
    #[serde(rename = "Plains")]
    Plains,
    #[serde(rename = "Island")]
    Island,
    #[serde(rename = "Swamp")]
    Swamp,
    #[serde(rename = "Forest")]
    Forest,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct Trigger {
    pub name: String,
    pub source: ObjectRef,
    pub definition_id: String,
    pub captured_controller: Role,
    pub cause_operation: String,
    pub event: TriggerEvent,
    pub subject: ObjectRef,
    #[serde(default)]
    pub caster: Option<Role>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub(in crate::ordinary_game) enum TriggerEvent {
    #[serde(rename = "entry")]
    Entry,
    #[serde(rename = "dies")]
    Dies,
    #[serde(rename = "cast")]
    Cast,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct TriggerGroup {
    pub name: String,
    pub triggers: Vec<Trigger>,
    pub order_bottom_first: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct PlayerChoice {
    pub trigger: String,
    pub chooser: Role,
    pub domain_roles: Vec<Role>,
    pub target: Role,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct TimestampChoice {
    pub creation_entry: EntryRef,
    pub instruction: u64,
    pub chooser: Role,
    pub members_earliest_first: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct GraveyardChoice {
    pub cause: String,
    pub chooser: Role,
    pub domain: Vec<ObjectRef>,
    pub entrants_top_first: Vec<ObjectRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum Settlement {
    RetainPriority {},
    Drain {
        groups: Vec<String>,
    },
    YieldAtStack {
        stack_bottom_first: Vec<EntryRef>,
        groups: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum Instruction<ScriptMana = Mana> {
    PlayLand {
        id: String,
        point: Point,
        source: ObjectRef,
    },
    ActivateMana {
        id: String,
        point: Point,
        source: ObjectRef,
        ability: Ability,
    },
    CastUntargeted {
        id: String,
        point: Point,
        source: ObjectRef,
        payment: Payment<ScriptMana>,
        settlement: Settlement,
    },
    CastTargeted {
        id: String,
        point: Point,
        source: ObjectRef,
        payment: Payment<ScriptMana>,
        target: Target,
        settlement: Settlement,
    },
    PassTo {
        id: String,
        boundary: Boundary,
        stack_bottom_first: Vec<EntryRef>,
        receiver: Role,
    },
    AdvanceReady {
        id: String,
        boundary: Boundary,
    },
    Combat {
        id: String,
        turn: u64,
        active: Role,
        cause: String,
        attackers: Vec<ObjectRef>,
        blockers: Vec<InstructionCombatBlockersItem>,
        settlement: Settlement,
    },
    SaveObject {
        id: String,
        point: Point,
        source: ObjectRef,
        binding: String,
    },
    ProbeMana {
        id: String,
        point: Point,
        source: ObjectRef,
        ability: Ability,
        mutation: ProbeMutation,
    },
}

impl<ScriptMana> Instruction<ScriptMana> {
    /// Move the same instruction while mapping only cast-payment mana kinds.
    /// Amount lexemes are neither parsed nor validated by this structural map.
    pub(in crate::ordinary_game) fn map_mana<NextMana>(
        self,
        mut map: impl FnMut(ScriptMana) -> NextMana,
    ) -> Instruction<NextMana> {
        let mut payment = |rows: Payment<ScriptMana>| {
            rows.into_iter()
                .map(|row| PaymentItem {
                    mana_type: map(row.mana_type),
                    amount: row.amount,
                })
                .collect()
        };
        match self {
            Self::PlayLand { id, point, source } => Instruction::PlayLand { id, point, source },
            Self::ActivateMana {
                id,
                point,
                source,
                ability,
            } => Instruction::ActivateMana {
                id,
                point,
                source,
                ability,
            },
            Self::CastUntargeted {
                id,
                point,
                source,
                payment: rows,
                settlement,
            } => Instruction::CastUntargeted {
                id,
                point,
                source,
                payment: payment(rows),
                settlement,
            },
            Self::CastTargeted {
                id,
                point,
                source,
                payment: rows,
                target,
                settlement,
            } => Instruction::CastTargeted {
                id,
                point,
                source,
                payment: payment(rows),
                target,
                settlement,
            },
            Self::PassTo {
                id,
                boundary,
                stack_bottom_first,
                receiver,
            } => Instruction::PassTo {
                id,
                boundary,
                stack_bottom_first,
                receiver,
            },
            Self::AdvanceReady { id, boundary } => Instruction::AdvanceReady { id, boundary },
            Self::Combat {
                id,
                turn,
                active,
                cause,
                attackers,
                blockers,
                settlement,
            } => Instruction::Combat {
                id,
                turn,
                active,
                cause,
                attackers,
                blockers,
                settlement,
            },
            Self::SaveObject {
                id,
                point,
                source,
                binding,
            } => Instruction::SaveObject {
                id,
                point,
                source,
                binding,
            },
            Self::ProbeMana {
                id,
                point,
                source,
                ability,
                mutation,
            } => Instruction::ProbeMana {
                id,
                point,
                source,
                ability,
                mutation,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InstructionCombatBlockersItem {
    pub blocker: ObjectRef,
    pub attacker: ObjectRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub(in crate::ordinary_game) enum ProbeMutation {
    #[serde(rename = "young")]
    Young,
    #[serde(rename = "tapped")]
    Tapped,
    #[serde(rename = "wrong_actor")]
    WrongActor,
    #[serde(rename = "stale_incarnation")]
    StaleIncarnation,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum Initialization {
    Opening {
        starter: Role,
        life: InitializationOpeningLife,
        decks: Vec<InitializationOpeningDecksItem>,
        shuffle_orders: Vec<InitializationOpeningShuffleOrdersItem>,
        keep_choices: Vec<InitializationOpeningKeepChoicesItem>,
    },
    Scheduled {
        turn: u64,
        step: StartStep,
        active: Role,
        life: InitializationScheduledLife,
        zones: Vec<InitializationScheduledZonesItem>,
        battlefield_oldest_first: Vec<InitializationScheduledBattlefieldOldestFirstItem>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InitializationOpeningLife {
    #[serde(rename = "P0")]
    pub p0: String,
    #[serde(rename = "P1")]
    pub p1: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InitializationOpeningDecksItem {
    pub role: Role,
    pub entries: Vec<InitializationOpeningDecksItemEntriesItem>,
    pub card_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InitializationOpeningDecksItemEntriesItem {
    pub oracle_id: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InitializationOpeningShuffleOrdersItem {
    pub role: Role,
    pub top_first: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InitializationOpeningKeepChoicesItem {
    pub role: Role,
    pub taken: u64,
    pub choice: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub(in crate::ordinary_game) enum StartStep {
    #[serde(rename = "untap")]
    Untap,
    #[serde(rename = "precombat-main")]
    PrecombatMain,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InitializationScheduledLife {
    #[serde(rename = "P0")]
    pub p0: String,
    #[serde(rename = "P1")]
    pub p1: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InitializationScheduledZonesItem {
    pub role: Role,
    pub hand: Vec<String>,
    pub library_top_first: Vec<String>,
    pub graveyard_top_first: Vec<String>,
    pub exile: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InitializationScheduledBattlefieldOldestFirstItem {
    pub alias: String,
    pub controller: Role,
    pub tapped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum Stop {
    NaturalOutcome {
        turn: u64,
        step: RecipeStep,
        active: Role,
    },
    BoundedReady {
        boundary: Boundary,
    },
    BoundedAtStack {
        boundary: Boundary,
        stack_bottom_first: Vec<EntryRef>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum CausalOperation {
    CastCompleted {
        name: String,
        entry: EntryRef,
    },
    CopyResolution {
        name: String,
        entry: EntryRef,
    },
    CastResolution {
        name: String,
        entry: EntryRef,
    },
    TriggerResolution {
        name: String,
        entry: EntryRef,
    },
    CombatDamage {
        name: String,
        combat_instruction: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(
    deny_unknown_fields,
    bound(
        deserialize = "Id: Deserialize<'de>, Init: Deserialize<'de>, Plan: Deserialize<'de>, ScriptMana: Deserialize<'de>"
    )
)]
pub(in crate::ordinary_game) struct Case<
    Id = MandatoryCase,
    Init = Initialization,
    Plan = DecisionPlan,
    ScriptMana = Mana,
> {
    pub id: Id,
    pub inventory: Vec<CaseInventoryItem>,
    pub initialization: Init,
    pub instructions: Vec<Instruction<ScriptMana>>,
    pub trigger_groups: Vec<TriggerGroup>,
    pub causal_operations: Vec<CausalOperation>,
    pub player_choices: Vec<PlayerChoice>,
    pub timestamp_choices: Vec<TimestampChoice>,
    pub graveyard_choices: Vec<GraveyardChoice>,
    pub stop: Stop,
    pub budget: CaseBudget,
    #[serde(default)]
    pub decision_plans: Vec<Plan>,
    #[serde(default)]
    pub reached_probe_limit: Option<u64>,
}

impl<Id, Init, Plan, ScriptMana> Case<Id, Init, Plan, ScriptMana> {
    /// Change only the static recipe identity, preserving every authored input.
    pub fn map_id<Next>(self, map: impl FnOnce(Id) -> Next) -> Case<Next, Init, Plan, ScriptMana> {
        Case {
            id: map(self.id),
            inventory: self.inventory,
            initialization: self.initialization,
            instructions: self.instructions,
            trigger_groups: self.trigger_groups,
            causal_operations: self.causal_operations,
            player_choices: self.player_choices,
            timestamp_choices: self.timestamp_choices,
            graveyard_choices: self.graveyard_choices,
            stop: self.stop,
            budget: self.budget,
            decision_plans: self.decision_plans,
            reached_probe_limit: self.reached_probe_limit,
        }
    }
    /// Move the same case into a controller representation. This performs no
    /// scalar validation, command issue, source lookup or wire reserialization.
    pub(in crate::ordinary_game) fn map_execution<NextInit, NextPlan, NextMana>(
        self,
        initialize: impl FnOnce(Init) -> NextInit,
        mut plan: impl FnMut(Plan) -> NextPlan,
        mut mana: impl FnMut(ScriptMana) -> NextMana,
    ) -> Case<Id, NextInit, NextPlan, NextMana> {
        Case {
            id: self.id,
            inventory: self.inventory,
            initialization: initialize(self.initialization),
            instructions: self
                .instructions
                .into_iter()
                .map(|instruction| instruction.map_mana(&mut mana))
                .collect(),
            trigger_groups: self.trigger_groups,
            causal_operations: self.causal_operations,
            player_choices: self.player_choices,
            timestamp_choices: self.timestamp_choices,
            graveyard_choices: self.graveyard_choices,
            stop: self.stop,
            budget: self.budget,
            decision_plans: self.decision_plans.into_iter().map(&mut plan).collect(),
            reached_probe_limit: self.reached_probe_limit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::ordinary_game) enum OptionalChoice {
    Take,
    Decline,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct Quote {
    pub generic: String,
    pub required: Payment,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum Funding {
    Fixed {
        source: ObjectRef,
        ability: Ability,
        produced: Payment,
    },
    Color {
        source: ObjectRef,
        ability: Ability,
        amount: String,
        colors: Vec<Mana>,
        choice: Mana,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum PaymentOutcome {
    Pay { allocation: Payment },
    Decline {},
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum CopyChoice {
    Retain {},
    Change { target: Target },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum DecisionPlan {
    Optional {
        id: String,
        entry: EntryRef,
        instruction: u64,
        chooser: Role,
        options: Vec<OptionalChoice>,
        choice: OptionalChoice,
    },
    Payment {
        id: String,
        entry: EntryRef,
        instruction: u64,
        payer: Role,
        cost: Quote,
        funding: Vec<Funding>,
        outcome: PaymentOutcome,
    },
    CopyTargets {
        id: String,
        entry: EntryRef,
        instruction: u64,
        original: EntryRef,
        chooser: Role,
        choice: CopyChoice,
        bind_copy: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct CaseInventoryItem {
    pub alias: String,
    pub owner: Role,
    pub oracle_id: String,
    pub copy: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct CaseBudget {
    pub maximum_individual_turn: u64,
    pub maximum_accepted_commands: u64,
    pub maximum_callbacks: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(
    deny_unknown_fields,
    bound(
        deserialize = "Id: Deserialize<'de>, Init: Deserialize<'de>, Plan: Deserialize<'de>, ScriptMana: Deserialize<'de>"
    )
)]
pub(in crate::ordinary_game) struct Inputs<
    Id = MandatoryCase,
    Init = Initialization,
    Plan = DecisionPlan,
    ScriptMana = Mana,
> {
    pub schema_version: u64,
    pub profile: String,
    pub epochs: InputsEpochs,
    pub seats: Vec<Seat>,
    pub authorities: Vec<InputsAuthoritiesItem>,
    pub definitions: Vec<InputsDefinitionsItem>,
    pub cases: Vec<Case<Id, Init, Plan, ScriptMana>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InputsEpochs {
    pub registry: u64,
    pub report: u64,
    pub aggregate: u64,
    pub current: u64,
    pub witness: u64,
    pub record_codec: u64,
    pub vocabulary: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InputsAuthoritiesItem {
    pub name: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct InputsDefinitionsItem {
    pub name: String,
    pub oracle_id: String,
    pub printing_id: String,
    pub source_line: u64,
    pub raw_line_sha256: String,
    pub semantic_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(in crate::ordinary_game) enum TokenBirthKind {
    #[serde(rename = "created_token")]
    CreatedToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct Seat {
    pub role: Role,
    pub ordinary_role: String,
    pub native_seat: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_compiled_cases_default_extensions_and_map_identity_losslessly() {
        let inputs: Inputs =
            serde_json::from_slice(include_bytes!("../mandatory-inputs-v1.json")).unwrap();
        assert_eq!(inputs.cases.len(), MandatoryCase::ALL.len());
        for case in inputs.cases {
            assert!(case.decision_plans.is_empty());
            assert!(case.reached_probe_limit.is_none());
            assert!(
                case.trigger_groups
                    .iter()
                    .flat_map(|group| &group.triggers)
                    .all(|trigger| trigger.caster.is_none())
            );
            let mapped = case
                .clone()
                .map_id(crate::ordinary_game::fixture::CaseId::Mandatory);
            let restored = mapped.map_id(|id| match id {
                crate::ordinary_game::fixture::CaseId::Mandatory(id) => id,
                _ => panic!("mapping changed the source profile"),
            });
            assert_eq!(restored, case);
        }
    }

    #[test]
    fn payment_plan_schema_keeps_quote_funding_and_reply_strict() {
        // Pure grammar fixture. This is not a source-qualified executable case.
        let value = serde_json::json!({"kind":"payment","id":"synthetic-offer",
            "entry":{"kind":"cast_entry","operation":"synthetic-cast"},
            "instruction":0,"payer":"P0","cost":{"generic":"3","required":[]},
            "funding":[{"kind":"color","source":{"kind":"physical_current","alias":"synthetic-mana-source"},
                "ability":{"kind":"printed","ability_id":"1"},"amount":"1", "colors":["White","Blue","Black","Red","Green"],"choice":"Red"}],
            "outcome":{"kind":"pay","allocation":[{"mana_type":"Red","amount":"1"},{"mana_type":"Blue","amount":"2"}]}});
        assert!(serde_json::from_value::<DecisionPlan>(value.clone()).is_ok());
        for pointer in ["/payer", "/cost", "/funding/0/choice", "/outcome"] {
            let mut wrong = value.clone();
            *wrong.pointer_mut(pointer).unwrap() = serde_json::Value::Null;
            assert!(
                serde_json::from_value::<DecisionPlan>(wrong).is_err(),
                "{pointer}"
            );
        }
        let mut wrong = value;
        wrong["cost"]["implicit_payer"] = serde_json::json!("P0");
        assert!(serde_json::from_value::<DecisionPlan>(wrong).is_err());
        assert!(
            serde_json::from_str::<Quote>(r#"{"generic":"0","generic":"0","required":[]}"#)
                .is_err()
        );
    }
}

#[cfg(test)]
mod normalization_tests;
