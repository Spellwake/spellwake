//! Finite command lowering and receipt annotations. GameState alone executes rules.
use super::{data::*, inputs};
use crate::ordinary_game::selection::probes::{
    BirthAuthority, CausalAppearanceLink, CausalObjectAuthority, ObjectOrigin, ReachedSelection,
    ResolvedEntry, ResolvedObject, ResolvedTarget, SavedObjectAuthority,
};
use crate::ordinary_game::{
    fixture::{A, B, CaseId, Fixture},
    policy::StopKind,
    probes::RejectionProbe,
};
use spellwake_engine::{
    checks::CheckDecision,
    combat::{AttackDeclaration, BlockDeclaration, CombatDamageAssignment, CombatDecision},
    effects::{
        CopyTargetChoice, EffectError, OptionalEffectChoice, ResolutionChoiceState,
        ResolutionPaymentOutcome, ResolutionPaymentReply,
    },
    game::{
        CandidateBase, DecisionReply, GameCommand, GameError, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    land::BasicLandType,
    mana::{ManaAbilityId, ManaAbilityRef, ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectId, ObjectZone},
    occurrence::{
        EventCause, EventId, RulesEvent, RulesOccurrence, RulesRecord, TriggerInstanceId,
        TriggerRelated, WaitingInstance,
    },
    opening::{MulliganChoice, OpeningDecision},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackWindowError, WindowPhase},
    targets::TargetRef,
    token::TokenBatchMember,
    turn::Step,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
mod selection;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    Idle,
    Settlement,
    Combat { attacked: bool, blocked: bool },
}
/// Policy uses only starter-first Keep declarations, never seed/deck lowering.
/// This private runtime metadata has no wire deserializer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeInitialization {
    Opening {
        keep_choices: Vec<InitializationOpeningKeepChoicesItem>,
    },
    Prepared,
}
impl From<Initialization> for RuntimeInitialization {
    fn from(value: Initialization) -> Self {
        match value {
            Initialization::Opening { keep_choices, .. } => Self::Opening { keep_choices },
            Initialization::Scheduled { .. } => Self::Prepared,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeDecisionPlan {
    Legacy(DecisionPlan),
    Selection(crate::ordinary_game::selection::data::SelectionDecisionPlan),
}
impl RuntimeDecisionPlan {
    fn legacy(&self) -> Option<&DecisionPlan> {
        match self {
            Self::Legacy(plan) => Some(plan),
            Self::Selection(_) => None,
        }
    }
}
type RuntimeCase = Case<CaseId, RuntimeInitialization, RuntimeDecisionPlan, ManaType>;

/// The two already admitted kind types share the existing late numeric parsers.
trait NativeManaKind: Copy {
    fn native(self) -> ManaType;
}
impl NativeManaKind for Mana {
    fn native(self) -> ManaType {
        FinitePolicy::mana(self)
    }
}
impl NativeManaKind for ManaType {
    fn native(self) -> ManaType {
        self
    }
}
/// Annotation state only: cursors, actual journals and handles, never native state.
#[derive(Debug, Clone)]
struct Progress {
    cursor: usize,
    stage: Stage,
    accepted: u64,
    callbacks: u64,
    opening: usize,
    casts: BTreeMap<String, StackEntryId>,
    triggers: BTreeMap<String, WaitingInstance>,
    entries: BTreeMap<String, StackEntryId>,
    events: BTreeMap<EventId, RulesEvent>,
    causes: BTreeMap<String, BTreeSet<EventId>>,
    saved: BTreeMap<String, SavedObjectAuthority>,
    births: BTreeMap<(StackEntryId, usize, SeatId, usize), BirthAuthority>,
    lineage: BTreeMap<ObjectId, ObjectId>,
    allowed: BTreeSet<String>,
    players: BTreeSet<usize>,
    timestamps: BTreeSet<usize>,
    graveyards: BTreeSet<usize>,
    orders: BTreeSet<(String, SeatId)>,
    last_player_target_revision: Option<u64>,
    probes: Vec<RejectionProbe>,
    decisions: BTreeSet<usize>,
    funding: BTreeMap<usize, FundingProgress>,
    copies: BTreeMap<String, StackEntryId>,
    selection_consumed: BTreeMap<String, crate::ordinary_game::selection::probes::ConsumedPlan>,
    selection_receipts: Vec<crate::ordinary_game::selection::probes::ProbeAuthorityReceipt>,
    selection_pending: Vec<crate::ordinary_game::selection::probes::PendingObligation>,
}
#[derive(Debug, Clone, Default)]
struct FundingProgress {
    next: usize,
    awaiting_color: bool,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            cursor: 0,
            stage: Stage::Idle,
            accepted: 0,
            callbacks: 0,
            opening: 0,
            casts: BTreeMap::new(),
            triggers: BTreeMap::new(),
            entries: BTreeMap::new(),
            events: BTreeMap::new(),
            causes: BTreeMap::new(),
            saved: BTreeMap::new(),
            births: BTreeMap::new(),
            lineage: BTreeMap::new(),
            allowed: BTreeSet::new(),
            players: BTreeSet::new(),
            timestamps: BTreeSet::new(),
            graveyards: BTreeSet::new(),
            orders: BTreeSet::new(),
            last_player_target_revision: None,
            probes: vec![],
            decisions: BTreeSet::new(),
            funding: BTreeMap::new(),
            copies: BTreeMap::new(),
            selection_consumed: BTreeMap::new(),
            selection_receipts: vec![],
            selection_pending: vec![],
        }
    }
}
#[derive(Debug, Clone)]
enum AcceptedKind {
    Simple,
    Cast(String),
    Opening,
    Choice,
    Attack,
    Block,
    Pass,
    Decision(usize),
    Selection {
        reached: Arc<ReachedSelection>,
        receipt_start: usize,
        pending_start: usize,
    },
    Funding {
        plan: usize,
        step: usize,
        color: bool,
        source: ObjectId,
        ability: ManaAbilityRef,
    },
    Color {
        plan: usize,
        step: usize,
        source: ObjectId,
        ability: ManaAbilityRef,
    },
    Copy(usize),
}
/// Owned prepared continuation. It is consumed only after candidate ancestry checks.
#[derive(Debug, Clone)]
pub(crate) struct FiniteIntent {
    next: Progress,
    kind: AcceptedKind,
    input: GameInput,
}
impl FiniteIntent {
    pub(crate) fn probes(&self) -> &[RejectionProbe] {
        &self.next.probes
    }
    pub(in crate::ordinary_game) fn selection_metadata(
        &self,
    ) -> Option<crate::ordinary_game::selection::probes::PreparedSelectionMetadata<'_>> {
        match &self.kind {
            AcceptedKind::Selection {
                reached,
                receipt_start,
                pending_start,
            } => Some(
                crate::ordinary_game::selection::probes::PreparedSelectionMetadata {
                    reached,
                    receipts: &self.next.selection_receipts[*receipt_start..],
                    pending: &self.next.selection_pending[*pending_start..],
                },
            ),
            _ => None,
        }
    }
    /// Only the original prepared ActivateMana input can use payment authority.
    pub(crate) fn permits_payment_funding(&self) -> bool {
        matches!(self.kind, AcceptedKind::Funding { .. })
    }
}
#[derive(Debug, Clone)]
pub(crate) struct FinitePolicy {
    fixture: Fixture,
    recipe: RuntimeCase,
    progress: Progress,
    initializer: Option<crate::ordinary_game::selection::authority::InitializerAuthority>,
}
impl FinitePolicy {
    fn normalize_recipe(recipe: Case<CaseId>) -> RuntimeCase {
        recipe.map_execution(
            RuntimeInitialization::from,
            RuntimeDecisionPlan::Legacy,
            Self::mana,
        )
    }

    pub(crate) fn new(fixture: &Fixture) -> Result<Self, String> {
        let CaseId::Mandatory(case) = fixture.case else {
            return Err("mandatory strategy requires mandatory case".into());
        };
        let recipe = inputs()?
            .cases
            .into_iter()
            .find(|r| r.id == case)
            .ok_or("mandatory recipe missing")?;
        Ok(Self {
            fixture: fixture.clone(),
            recipe: Self::normalize_recipe(recipe.map_id(CaseId::Mandatory)),
            progress: Progress::default(),
            initializer: None,
        })
    }
    /// Lowered optional recipes reuse this controller after source admission.
    pub(in crate::ordinary_game) fn from_optional(
        fixture: &Fixture,
        recipe: Case<CaseId>,
    ) -> Result<Self, String> {
        if !matches!(fixture.case, CaseId::Optional(_)) || fixture.case != recipe.id {
            return Err("optional finite recipe/fixture identity mismatch".into());
        }
        let names = recipe
            .decision_plans
            .iter()
            .map(|plan| match plan {
                DecisionPlan::Optional { id, .. }
                | DecisionPlan::Payment { id, .. }
                | DecisionPlan::CopyTargets { id, .. } => id.as_str(),
            })
            .collect::<BTreeSet<_>>();
        if names.len() != recipe.decision_plans.len()
            || names.contains("")
            || recipe.budget.maximum_accepted_commands == 0
            || recipe.budget.maximum_callbacks == 0
            || recipe.reached_probe_limit.is_none_or(|limit| limit == 0)
        {
            return Err("optional finite plan identity/budget mismatch".into());
        }
        if recipe.instructions.is_empty()
            || recipe
                .instructions
                .iter()
                .map(Instruction::id)
                .collect::<BTreeSet<_>>()
                .len()
                != recipe.instructions.len()
            || matches!(&recipe.stop, Stop::BoundedAtStack { stack_bottom_first, .. } if stack_bottom_first.is_empty())
        {
            return Err("optional instruction identity/stop mismatch".into());
        }
        let mut copy_names = BTreeSet::new();
        for plan in &recipe.decision_plans {
            if let DecisionPlan::CopyTargets { bind_copy, .. } = plan
                && (bind_copy.is_empty() || !copy_names.insert(bind_copy))
            {
                return Err("duplicate or empty copy binding".into());
            }
        }
        for group in &recipe.trigger_groups {
            let members = group
                .triggers
                .iter()
                .map(|v| &v.name)
                .collect::<BTreeSet<_>>();
            if members.len() != group.triggers.len()
                || group.order_bottom_first.len() != members.len()
                || group.order_bottom_first.iter().collect::<BTreeSet<_>>() != members
            {
                return Err("optional trigger group is not a complete permutation".into());
            }
            for trigger in &group.triggers {
                if (trigger.event == TriggerEvent::Cast) != trigger.caster.is_some() {
                    return Err("cast trigger requires exactly its declared caster".into());
                }
            }
        }
        for plan in &recipe.decision_plans {
            if let DecisionPlan::Optional {
                options, choice, ..
            } = plan
                && (options.is_empty()
                    || options
                        .iter()
                        .map(|value| Self::optional(*value))
                        .collect::<BTreeSet<_>>()
                        .len()
                        != options.len()
                    || !options.contains(choice))
            {
                return Err("invalid declared optional domain/choice".into());
            }
            if let DecisionPlan::Payment {
                cost,
                funding,
                outcome,
                ..
            } = plan
            {
                Self::quote(cost)?;
                for step in funding {
                    match step {
                        Funding::Fixed { produced, .. } => {
                            Self::amounts(produced, true)?;
                        }
                        Funding::Color {
                            amount,
                            colors,
                            choice,
                            ..
                        } => {
                            if super::integer(amount)? <= 0.into()
                                || colors.is_empty()
                                || colors.iter().collect::<BTreeSet<_>>().len() != colors.len()
                                || !colors.contains(choice)
                            {
                                return Err("invalid declared mana-color plan".into());
                            }
                        }
                    }
                }
                if let PaymentOutcome::Pay { allocation } = outcome {
                    Self::payment(allocation)?;
                }
            }
        }
        Ok(Self {
            fixture: fixture.clone(),
            recipe: Self::normalize_recipe(recipe),
            progress: Progress::default(),
            initializer: None,
        })
    }
    pub(in crate::ordinary_game) fn from_selection(
        fixture: &Fixture,
        recipe: crate::ordinary_game::selection::data::SelectionInputCase,
    ) -> Result<Self, String> {
        if fixture.case != CaseId::Selection(recipe.id)
            || recipe.reached_probe_limit.is_none_or(|cap| cap == 0)
        {
            return Err("selected finite recipe/fixture identity or cap mismatch".into());
        }
        Ok(Self {
            fixture: fixture.clone(),
            recipe: recipe.map_id(CaseId::Selection).map_execution(
                |_| RuntimeInitialization::Prepared,
                RuntimeDecisionPlan::Selection,
                crate::ordinary_game::selection::prepared::PreparedManaType::native,
            ),
            progress: Progress::default(),
            initializer: None,
        })
    }

    /// Rejected probe calls have a separate budget from accepted Reply callbacks.
    pub(crate) fn reached_probe_limit(&self) -> Option<u64> {
        self.recipe.reached_probe_limit
    }
    fn entry(&self, p: &Progress, reference: &EntryRef) -> Result<StackEntryId, String> {
        match reference {
            EntryRef::CastEntry { operation } => p.casts.get(operation),
            EntryRef::TriggerEntry { trigger } => p.entries.get(trigger),
            EntryRef::CopyEntry { copy } => p.copies.get(copy),
        }
        .copied()
        .ok_or_else(|| format!("unbound actual entry {reference:?}"))
    }
    fn stack(&self, p: &Progress, refs: &[EntryRef]) -> Result<Vec<StackEntryId>, String> {
        refs.iter().map(|r| self.entry(p, r)).collect()
    }
    fn point(&self, p: &Progress, state: &GameState, point: &Point) -> Result<(), String> {
        if !self.boundary(state, &point.boundary)
            || state
                .execution()
                .window()
                .entries()
                .iter()
                .map(|e| e.id())
                .collect::<Vec<_>>()
                != self.stack(p, &point.stack_bottom_first)?
            || state.pending_decision().is_some()
            || state.execution().resolution_state().is_some()
        {
            return Err(format!("mandatory instruction point mismatch: {point:?}"));
        }
        Ok(())
    }
    fn boundary(&self, state: &GameState, b: &Boundary) -> bool {
        state.turn().is_some_and(|t| {
            t.position().number == b.turn
                && t.position().step == b.step.native()
                && t.position().cleanup_iteration == 0
        }) && state.execution().window().active_player() == b.active.seat()
            && state.execution().window().phase()
                == WindowPhase::Priority(PriorityPhase::Ready {
                    holder: b.holder.seat(),
                })
    }
    fn ready_empty(&self, state: &GameState) -> bool {
        matches!(
            state.execution().window().phase(),
            WindowPhase::Priority(PriorityPhase::Ready { .. })
        ) && state.pending_decision().is_none()
            && state.execution().window().entries().is_empty()
            && state.execution().waiting_triggers().next().is_none()
            && state.execution().resolution_state().is_none()
    }
    fn birth_authority(
        &self,
        p: &Progress,
        entry: &EntryRef,
        instruction: u64,
        creator: Role,
        member: u64,
    ) -> Result<BirthAuthority, String> {
        let key = (
            self.entry(p, entry)?,
            usize::try_from(instruction).map_err(|_| "instruction overflow")?,
            creator.seat(),
            usize::try_from(member).map_err(|_| "member overflow")?,
        );
        p.births
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("actual token birth missing: {key:?}"))
    }
    fn birth(
        &self,
        p: &Progress,
        entry: &EntryRef,
        instruction: u64,
        creator: Role,
        member: u64,
    ) -> Result<ObjectId, String> {
        Ok(self
            .birth_authority(p, entry, instruction, creator, member)?
            .appearance
            .object
            .id())
    }
    fn causal_authority(
        &self,
        p: &Progress,
        card: Option<CardId>,
        birth: Option<ObjectId>,
        cause: &str,
        appearance: Appearance,
    ) -> Result<CausalObjectAuthority, String> {
        let events = p
            .causes
            .get(cause)
            .ok_or_else(|| format!("actual cause unbound: {cause}"))?;
        let mut objects = BTreeSet::new();
        let mut links = vec![];
        for id in events {
            let event = p.events.get(id).ok_or("cause event absent")?;
            for o in &event.occurrences {
                let value = match (&o.value, appearance) {
                    (RulesOccurrence::ZoneChange(z), Appearance::Before) => Some(&z.before),
                    (RulesOccurrence::ZoneChange(z), Appearance::After) => Some(&z.after),
                    (RulesOccurrence::TokenCreated { after, .. }, Appearance::After) => {
                        Some(after.as_ref())
                    }
                    (RulesOccurrence::CastCompleted(cast), Appearance::After) => Some(cast.spell()),
                    _ => None,
                };
                if let Some(a) = value
                    && ((card.is_some() && a.object.card_id() == card)
                        || (birth.is_some() && p.lineage.get(&a.object.id()).copied() == birth))
                {
                    objects.insert(a.object.id());
                    links.push(CausalAppearanceLink::Occurrence {
                        event: event.id,
                        occurrence: o.id,
                        appearance: a.clone(),
                    });
                }
            }
        }
        // A surviving watcher is represented by its actual discovery capture at this event.
        for trigger in p.triggers.values() {
            if events.contains(&trigger.causes.event())
                && card.is_some()
                && trigger.source.object.card_id() == card
            {
                let moved=p.events.get(&trigger.causes.event()).is_some_and(|e|e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(z) if z.before.object.card_id()==card)));
                if !moved {
                    objects.insert(trigger.source.object.id());
                    links.push(CausalAppearanceLink::TriggerCapture {
                        event: trigger.causes.event(),
                        instance: trigger.id,
                        appearance: trigger.source.clone(),
                    });
                }
            }
        }
        let matches = objects.into_iter().collect::<Vec<_>>();
        let [object] = matches.as_slice() else {
            return Err(format!("ambiguous or absent object appearance at {cause}"));
        };
        Ok(CausalObjectAuthority {
            cause: cause.into(),
            appearance,
            object: *object,
            links,
        })
    }
    fn at_cause(
        &self,
        p: &Progress,
        card: Option<CardId>,
        birth: Option<ObjectId>,
        cause: &str,
        appearance: Appearance,
    ) -> Result<ObjectId, String> {
        Ok(self
            .causal_authority(p, card, birth, cause, appearance)?
            .object)
    }
    fn resolve_object(
        &self,
        p: &Progress,
        state: &GameState,
        reference: &ObjectRef,
    ) -> Result<ResolvedObject, String> {
        let current = |object: ObjectId| {
            state
                .execution()
                .window()
                .cards()
                .get(object)
                .cloned()
                .ok_or_else(|| "resolved current object missing from native owner".to_owned())
        };
        let (object, origin) = match reference {
            ObjectRef::PhysicalCurrent { alias } => {
                let card = self.fixture.card(alias)?;
                let object = self.fixture.current_object(state, alias)?;
                (
                    object,
                    ObjectOrigin::CurrentPhysical {
                        card,
                        actual: current(object)?,
                    },
                )
            }
            ObjectRef::PhysicalAtCause {
                alias,
                cause,
                appearance,
            } => {
                let card = self.fixture.card(alias)?;
                let authority = self.causal_authority(p, Some(card), None, cause, *appearance)?;
                (
                    authority.object,
                    ObjectOrigin::PhysicalCause { card, authority },
                )
            }
            ObjectRef::SavedObject { binding } => {
                let authority = p
                    .saved
                    .get(binding)
                    .cloned()
                    .ok_or_else(|| format!("saved actual object absent: {binding}"))?;
                (
                    authority.object.id(),
                    ObjectOrigin::Saved {
                        binding: binding.clone(),
                        authority,
                    },
                )
            }
            ObjectRef::CreatedToken {
                creation_entry,
                instruction,
                creator,
                member,
            } => {
                let birth =
                    self.birth_authority(p, creation_entry, *instruction, *creator, *member)?;
                let initial = birth.appearance.object.id();
                let objects = state
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .filter(|o| p.lineage.get(&o.id()) == Some(&initial))
                    .map(|o| o.id())
                    .collect::<Vec<_>>();
                let [object] = objects.as_slice() else {
                    return Err("created token has no unique current incarnation".into());
                };
                (
                    *object,
                    ObjectOrigin::CurrentToken {
                        birth,
                        current: current(*object)?,
                        lineage_birth: initial,
                    },
                )
            }
            ObjectRef::TokenAtCause {
                token,
                cause,
                appearance,
            } => {
                let birth = self.birth_authority(
                    p,
                    &token.creation_entry,
                    token.instruction,
                    token.creator,
                    token.member,
                )?;
                let authority = self.causal_authority(
                    p,
                    None,
                    Some(birth.appearance.object.id()),
                    cause,
                    *appearance,
                )?;
                (
                    authority.object,
                    ObjectOrigin::TokenCause { birth, authority },
                )
            }
        };
        Ok(ResolvedObject {
            reference: reference.clone(),
            object,
            origin: Box::new(origin),
        })
    }
    fn object(
        &self,
        p: &Progress,
        state: &GameState,
        reference: &ObjectRef,
    ) -> Result<ObjectId, String> {
        Ok(self.resolve_object(p, state, reference)?.object)
    }
    fn resolve_entry(&self, p: &Progress, reference: &EntryRef) -> Result<ResolvedEntry, String> {
        Ok(ResolvedEntry {
            reference: reference.clone(),
            entry: self.entry(p, reference)?,
        })
    }
    fn resolve_target(
        &self,
        p: &Progress,
        state: &GameState,
        reference: &Target,
    ) -> Result<ResolvedTarget, String> {
        Ok(match reference {
            Target::Object { object } => {
                ResolvedTarget::Object(self.resolve_object(p, state, object)?)
            }
            Target::Stack { entry } => ResolvedTarget::Stack(self.resolve_entry(p, entry)?),
            Target::Player { role } => ResolvedTarget::Player {
                role: *role,
                player: role.seat(),
            },
        })
    }
    fn target(
        &self,
        p: &Progress,
        state: &GameState,
        reference: &Target,
    ) -> Result<TargetRef, String> {
        Ok(self.resolve_target(p, state, reference)?.native())
    }
    fn ability(value: &Ability) -> Result<ManaAbilityRef, String> {
        Ok(match value {
            Ability::BasicLand { land_type } => ManaAbilityRef::BasicLand(match land_type {
                Land::Plains => BasicLandType::Plains,
                Land::Island => BasicLandType::Island,
                Land::Swamp => BasicLandType::Swamp,
                Land::Forest => BasicLandType::Forest,
            }),
            Ability::Printed { ability_id } if ability_id == "1" => {
                ManaAbilityRef::Printed(ManaAbilityId(1))
            }
            _ => return Err("unknown mandatory printed mana ability".into()),
        })
    }
    fn payment<Kind: NativeManaKind>(value: &Payment<Kind>) -> Result<ManaPayment, String> {
        let mut seen = BTreeSet::new();
        let mut spend = vec![];
        for row in value {
            let mana_type = row.mana_type.native();
            if !seen.insert(mana_type) {
                return Err("duplicate mandatory mana spend type".into());
            }
            let amount = super::integer(&row.amount)?;
            if amount <= 0.into() {
                return Err("nonpositive mandatory spend".into());
            }
            spend.push(ManaSpend { mana_type, amount });
        }
        Ok(ManaPayment {
            from_lots: Default::default(),
            spend,
        })
    }
    fn mana(value: Mana) -> ManaType {
        match value {
            Mana::White => ManaType::White,
            Mana::Blue => ManaType::Blue,
            Mana::Black => ManaType::Black,
            Mana::Red => ManaType::Red,
            Mana::Green => ManaType::Green,
        }
    }
    fn amounts<Kind: NativeManaKind>(
        value: &Payment<Kind>,
        allow_zero: bool,
    ) -> Result<BTreeMap<ManaType, spellwake_contracts::RulesInteger>, String> {
        let mut result = BTreeMap::new();
        for row in value {
            let amount = super::integer(&row.amount)?;
            if amount < 0.into()
                || (!allow_zero && amount == 0.into())
                || result.insert(row.mana_type.native(), amount).is_some()
            {
                return Err("duplicate or invalid finite mana amount".into());
            }
        }
        Ok(result)
    }
    fn quote(value: &Quote) -> Result<ManaCost, String> {
        let generic = super::integer(&value.generic)?;
        if generic < 0.into() {
            return Err("negative declared resolution quote".into());
        }
        Ok(ManaCost {
            generic,
            required: Self::amounts(&value.required, true)?,
        })
    }
    fn optional(value: OptionalChoice) -> OptionalEffectChoice {
        match value {
            OptionalChoice::Take => OptionalEffectChoice::Take,
            OptionalChoice::Decline => OptionalEffectChoice::Decline,
        }
    }
    fn payment_parent(
        &self,
        p: &Progress,
        state: &GameState,
        plan: usize,
        suspended: bool,
    ) -> Result<(), String> {
        let Some(DecisionPlan::Payment {
            entry,
            instruction,
            payer,
            cost,
            ..
        }) = self
            .recipe
            .decision_plans
            .get(plan)
            .and_then(RuntimeDecisionPlan::legacy)
        else {
            return Err("payment annotation lacks declared plan".into());
        };
        match state.execution().resolution_choice_state() {
            Some(ResolutionChoiceState::Payment {
                cursor,
                payer: actual,
                cost: quoted,
                suspended_for_mana_color,
                ..
            }) if cursor.entry() == self.entry(p, entry)?
                && u64::try_from(cursor.instruction()).ok() == Some(*instruction)
                && actual == payer.seat()
                && quoted == &Self::quote(cost)?
                && suspended_for_mana_color == suspended =>
            {
                Ok(())
            }
            _ => Err("actual payment continuation differs from declared origin/quote".into()),
        }
    }
    fn pending_action(
        &self,
        state: &GameState,
        p: &Progress,
        frame: &RuleFrame,
    ) -> Result<Option<(GameCommand, AcceptedKind, String)>, String> {
        if !matches!(self.fixture.case, CaseId::Optional(_)) {
            return Ok(None);
        }
        let candidates = self
            .recipe
            .decision_plans
            .iter()
            .enumerate()
            .filter_map(|(index, plan)| plan.legacy().map(|plan| (index, plan)))
            .filter(|(i, _)| !p.decisions.contains(i))
            .filter(|(i, plan)| match (frame, plan) {
                (
                    RuleFrame::OptionalEffect(f),
                    DecisionPlan::Optional {
                        entry,
                        instruction,
                        chooser,
                        ..
                    },
                ) => {
                    self.entry(p, entry).ok() == Some(f.entry())
                        && u64::try_from(f.instruction()).ok() == Some(*instruction)
                        && chooser.seat() == f.chooser()
                }
                (
                    RuleFrame::ResolutionPayment(f),
                    DecisionPlan::Payment {
                        entry,
                        instruction,
                        payer,
                        ..
                    },
                ) => {
                    self.entry(p, entry).ok() == Some(f.entry())
                        && u64::try_from(f.instruction()).ok() == Some(*instruction)
                        && payer.seat() == f.payer()
                }
                (RuleFrame::ManaColor(_), DecisionPlan::Payment { .. }) => {
                    p.funding.get(i).is_some_and(|v| v.awaiting_color)
                }
                (
                    RuleFrame::CopyTargets(f),
                    DecisionPlan::CopyTargets {
                        entry,
                        instruction,
                        original,
                        chooser,
                        ..
                    },
                ) => {
                    self.entry(p, entry).ok() == Some(f.entry())
                        && u64::try_from(f.instruction_index()).ok() == Some(*instruction)
                        && self.entry(p, original).ok() == Some(f.original())
                        && chooser.seat() == f.chooser()
                }
                _ => false,
            })
            .collect::<Vec<_>>();
        if !matches!(
            frame,
            RuleFrame::OptionalEffect(_)
                | RuleFrame::ResolutionPayment(_)
                | RuleFrame::ManaColor(_)
                | RuleFrame::CopyTargets(_)
        ) {
            return Ok(None);
        }
        let [(index, plan)] = candidates.as_slice() else {
            return Err("new decision lacks one unconsumed exact finite plan".into());
        };
        let index = *index;
        let decision = state
            .pending_decision()
            .ok_or("finite pending action lacks actual decision")?
            .id();
        let reply = |choice| GameCommand::Reply { decision, choice };
        Ok(Some(match (frame, plan) {
            (
                RuleFrame::OptionalEffect(f),
                DecisionPlan::Optional {
                    id,
                    options,
                    choice,
                    ..
                },
            ) => {
                let offered = options
                    .iter()
                    .copied()
                    .map(Self::optional)
                    .collect::<BTreeSet<_>>();
                if offered.len() != options.len()
                    || &offered != f.options()
                    || !offered.contains(&Self::optional(*choice))
                {
                    return Err("optional declared domain/choice mismatch".into());
                }
                (
                    reply(DecisionReply::OptionalEffect(Self::optional(*choice))),
                    AcceptedKind::Decision(index),
                    id.clone(),
                )
            }
            (
                RuleFrame::ResolutionPayment(_),
                DecisionPlan::Payment {
                    id,
                    funding,
                    outcome,
                    ..
                },
            ) => {
                self.payment_parent(p, state, index, false)?;
                let progress = p.funding.get(&index).cloned().unwrap_or_default();
                if progress.awaiting_color {
                    return Err("payment restored before planned color receipt".into());
                }
                if let Some(step) = funding.get(progress.next) {
                    let (source, ability, color) = match step {
                        Funding::Fixed {
                            source, ability, ..
                        } => (source, ability, false),
                        Funding::Color {
                            source, ability, ..
                        } => (source, ability, true),
                    };
                    let source = self.object(p, state, source)?;
                    let ability = Self::ability(ability)?;
                    (
                        GameCommand::ActivateMana { source, ability },
                        AcceptedKind::Funding {
                            plan: index,
                            step: progress.next,
                            color,
                            source,
                            ability,
                        },
                        format!("{id}.fund.{}", progress.next),
                    )
                } else {
                    let choice = match outcome {
                        PaymentOutcome::Pay { allocation } => {
                            ResolutionPaymentReply::Pay(Self::payment(allocation)?)
                        }
                        PaymentOutcome::Decline {} => ResolutionPaymentReply::Decline,
                    };
                    (
                        reply(DecisionReply::ResolutionPayment(choice)),
                        AcceptedKind::Decision(index),
                        id.clone(),
                    )
                }
            }
            (RuleFrame::ManaColor(f), DecisionPlan::Payment { id, funding, .. }) => {
                self.payment_parent(p, state, index, true)?;
                let progress = p
                    .funding
                    .get(&index)
                    .ok_or("color has no accepted funding prefix")?;
                let Some(Funding::Color {
                    source,
                    ability,
                    amount,
                    colors,
                    choice,
                }) = funding.get(progress.next)
                else {
                    return Err("unexpected color child outside exact funding step".into());
                };
                let source = self.object(p, state, source)?;
                let ability = Self::ability(ability)?;
                let offered = colors
                    .iter()
                    .copied()
                    .map(Self::mana)
                    .collect::<BTreeSet<_>>();
                if f.source() != source
                    || f.ability() != ability
                    || f.amount() != &super::integer(amount)?
                    || f.colors() != &offered
                    || !offered.contains(&Self::mana(*choice))
                {
                    return Err("actual color source/ability/amount/domain differs".into());
                }
                (
                    reply(DecisionReply::ManaColor(Self::mana(*choice))),
                    AcceptedKind::Color {
                        plan: index,
                        step: progress.next,
                        source,
                        ability,
                    },
                    format!("{id}.color.{}", progress.next),
                )
            }
            (RuleFrame::CopyTargets(_), DecisionPlan::CopyTargets { id, choice, .. }) => {
                let selected = match choice {
                    CopyChoice::Retain {} => CopyTargetChoice::Retain,
                    CopyChoice::Change { target } => {
                        CopyTargetChoice::Change(self.target(p, state, target)?)
                    }
                };
                (
                    reply(DecisionReply::CopyTargets(selected)),
                    AcceptedKind::Copy(index),
                    id.clone(),
                )
            }
            _ => return Err("finite new decision plan family mismatch".into()),
        }))
    }
    fn settlement<'a>(&'a self, p: &Progress) -> Option<&'a Settlement> {
        match self.recipe.instructions.get(p.cursor) {
            Some(
                Instruction::CastUntargeted { settlement, .. }
                | Instruction::CastTargeted { settlement, .. }
                | Instruction::Combat { settlement, .. },
            ) => Some(settlement),
            _ => None,
        }
    }
    fn normalize(&self, state: &GameState, p: &mut Progress) -> Result<(), String> {
        for _ in 0..=self.recipe.instructions.len() {
            let Some(instruction) = self.recipe.instructions.get(p.cursor) else {
                return Ok(());
            };
            let done = match (&p.stage, instruction) {
                (Stage::Settlement, _) => {
                    match self.settlement(p).ok_or("settlement without instruction")? {
                        Settlement::RetainPriority {} => true,
                        Settlement::Drain { .. } => {
                            self.ready_empty(state) || state.outcome().is_some()
                        }
                        Settlement::YieldAtStack {
                            stack_bottom_first, ..
                        } => {
                            let expected = self.stack(p, stack_bottom_first);
                            state.pending_decision().is_none()
                                && state.execution().resolution_state().is_none()
                                && matches!(
                                    state.execution().window().phase(),
                                    WindowPhase::Priority(PriorityPhase::Ready { .. })
                                )
                                && expected.is_ok_and(|expected| {
                                    state
                                        .execution()
                                        .window()
                                        .entries()
                                        .iter()
                                        .map(|e| e.id())
                                        .collect::<Vec<_>>()
                                        == expected
                                })
                        }
                    }
                }
                (Stage::Idle, Instruction::AdvanceReady { boundary, .. }) => {
                    self.boundary(state, boundary) && self.ready_empty(state)
                }
                (
                    Stage::Idle,
                    Instruction::PassTo {
                        boundary,
                        stack_bottom_first,
                        receiver,
                        ..
                    },
                ) => {
                    let mut b = boundary.clone();
                    b.holder = *receiver;
                    self.boundary(state, &b)
                        && state
                            .execution()
                            .window()
                            .entries()
                            .iter()
                            .map(|e| e.id())
                            .collect::<Vec<_>>()
                            == self.stack(p, stack_bottom_first)?
                }
                (
                    Stage::Combat {
                        attacked: true,
                        blocked: true,
                    },
                    Instruction::Combat { turn, active, .. },
                ) => {
                    if state.turn().is_some_and(|t| {
                        t.position().number == *turn && t.position().step == Step::CombatDamage
                    }) && state.execution().window().active_player() == active.seat()
                    {
                        p.stage = Stage::Settlement;
                        continue;
                    }
                    false
                }
                _ => false,
            };
            if done {
                p.cursor += 1;
                p.stage = Stage::Idle;
            } else {
                return Ok(());
            }
        }
        Err("mandatory annotation cursor failed to advance".into())
    }
    pub(crate) fn prepare(
        &self,
        state: &GameState,
    ) -> Result<(GameInput, String, FiniteIntent), String> {
        if matches!(self.fixture.case, CaseId::Selection(_)) && self.initializer.is_none() {
            return Err("selected missing initializer authority".into());
        }
        if state.basis().game != self.fixture.game
            || state.basis().context != self.fixture.context
            || state.basis().epoch != 0
            || state.outcome().is_some()
        {
            return Err("mandatory game/context/terminal mismatch".into());
        }
        if self.progress.accepted >= self.recipe.budget.maximum_accepted_commands
            || self.progress.callbacks >= self.recipe.budget.maximum_callbacks
            || state
                .turn()
                .is_some_and(|t| t.position().number > self.recipe.budget.maximum_individual_turn)
        {
            return Err("mandatory native command/turn watchdog exhausted".into());
        }
        let mut p = self.annotated(state)?;
        let instruction = self
            .recipe
            .instructions
            .get(p.cursor)
            .ok_or("mandatory inputs consumed before required completion")?;
        let name = instruction.id().to_owned();
        if let Some(pending) = state.pending_decision() {
            let (command, kind, name) = if let Some(action) =
                if matches!(self.fixture.case, CaseId::Selection(_)) {
                    self.selection_pending_action(state, &mut p)?
                } else {
                    self.pending_action(state, &p, pending.frame())?
                } {
                action
            } else {
                let (choice, kind) = self.choice(state, &mut p, pending.frame())?;
                (
                    GameCommand::Reply {
                        decision: pending.id(),
                        choice,
                    },
                    kind,
                    name,
                )
            };
            let input = GameInput {
                basis: state.basis().clone(),
                actor: pending.chooser(),
                command,
            };
            return Ok((
                input.clone(),
                format!("{name}.reply.{}", state.basis().revision),
                FiniteIntent {
                    next: p,
                    kind,
                    input,
                },
            ));
        }
        // Opening choices are issued before the first authored instruction can execute.
        if state.turn().is_none() {
            return Err("mandatory opening has no issued chooser".into());
        }
        let (actor, command, kind) = match (&p.stage, instruction) {
            (Stage::Idle, Instruction::PlayLand { point, source, .. }) => {
                self.point(&p, state, point)?;
                (
                    point.actor.seat(),
                    GameCommand::PlayLand {
                        land: self.object(&p, state, source)?,
                    },
                    AcceptedKind::Simple,
                )
            }
            (
                Stage::Idle,
                Instruction::ActivateMana {
                    point,
                    source,
                    ability,
                    ..
                },
            ) => {
                self.point(&p, state, point)?;
                (
                    point.actor.seat(),
                    GameCommand::ActivateMana {
                        source: self.object(&p, state, source)?,
                        ability: Self::ability(ability)?,
                    },
                    AcceptedKind::Simple,
                )
            }
            (
                Stage::Idle,
                Instruction::CastUntargeted {
                    point,
                    source,
                    payment,
                    ..
                },
            ) => {
                self.point(&p, state, point)?;
                self.admit_groups(&mut p)?;
                (
                    point.actor.seat(),
                    GameCommand::CastUntargeted {
                        spell: self.object(&p, state, source)?,
                        payment: Some(Self::payment(payment)?),
                    },
                    AcceptedKind::Cast(name.clone()),
                )
            }
            (
                Stage::Idle,
                Instruction::CastTargeted {
                    point,
                    source,
                    payment,
                    target,
                    ..
                },
            ) => {
                self.point(&p, state, point)?;
                self.admit_groups(&mut p)?;
                (
                    point.actor.seat(),
                    GameCommand::CastPaid {
                        spell: self.object(&p, state, source)?,
                        target: self.target(&p, state, target)?,
                        payment: Self::payment(payment)?,
                    },
                    AcceptedKind::Cast(name.clone()),
                )
            }
            (
                Stage::Idle,
                Instruction::PassTo {
                    boundary,
                    stack_bottom_first,
                    ..
                },
            ) => {
                if state.turn().is_none_or(|t| {
                    t.position().number != boundary.turn
                        || t.position().step != boundary.step.native()
                }) || state.execution().window().active_player() != boundary.active.seat()
                    || state
                        .execution()
                        .window()
                        .entries()
                        .iter()
                        .map(|e| e.id())
                        .collect::<Vec<_>>()
                        != self.stack(&p, stack_bottom_first)?
                {
                    return Err("pass_to left declared stack/boundary".into());
                }
                let actor = holder(state)?;
                (actor, GameCommand::Pass, AcceptedKind::Pass)
            }
            (Stage::Idle, Instruction::AdvanceReady { boundary, .. }) => {
                if !self.ready_empty(state) {
                    return Err("advance_ready encountered unexpected stack/work".into());
                }
                if state.turn().is_some_and(|t| {
                    t.position().number > boundary.turn
                        || (t.position().number == boundary.turn
                            && step_index(t.position().step) > step_index(boundary.step.native()))
                }) {
                    return Err("advance_ready passed its exact boundary".into());
                }
                (holder(state)?, GameCommand::Pass, AcceptedKind::Pass)
            }
            (Stage::Idle, Instruction::Combat { turn, active, .. }) => {
                self.admit_groups(&mut p)?;
                p.stage = Stage::Combat {
                    attacked: false,
                    blocked: false,
                };
                if state.turn().is_none_or(|t| {
                    t.position().number > *turn
                        || (t.position().number == *turn
                            && state.execution().window().active_player() != active.seat())
                }) || !self.ready_empty(state)
                {
                    return Err("combat seek initial boundary mismatch".into());
                }
                (holder(state)?, GameCommand::Pass, AcceptedKind::Pass)
            }
            (Stage::Combat { .. }, Instruction::Combat { turn, active, .. }) => {
                if state.turn().is_none_or(|t| {
                    t.position().number > *turn
                        || (t.position().number == *turn
                            && state.execution().window().active_player() != active.seat())
                }) || !self.ready_empty(state)
                {
                    return Err("combat seek encountered unexpected stack/work".into());
                }
                (holder(state)?, GameCommand::Pass, AcceptedKind::Pass)
            }
            (Stage::Settlement, _) => {
                if self.ready_empty(state) {
                    return Err(
                        "settlement reached empty Ready before its declared stack cut".into(),
                    );
                }
                if state.execution().waiting_triggers().next().is_some() {
                    return Err("settlement waiting without issued native decision".into());
                }
                (holder(state)?, GameCommand::Pass, AcceptedKind::Pass)
            }
            _ => return Err("mandatory instruction/stage mismatch".into()),
        };
        let suffix = if matches!(kind, AcceptedKind::Pass) {
            format!(".pass.{}.{}", state.basis().revision, actor.0)
        } else {
            String::new()
        };
        let input = GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        };
        Ok((
            input.clone(),
            format!("{name}{suffix}"),
            FiniteIntent {
                next: p,
                kind,
                input,
            },
        ))
    }
    fn admit_groups(&self, p: &mut Progress) -> Result<(), String> {
        let groups = match self.settlement(p).ok_or("missing settlement")? {
            Settlement::RetainPriority {} => return Ok(()),
            Settlement::Drain { groups } | Settlement::YieldAtStack { groups, .. } => groups,
        };
        for name in groups {
            if !self.recipe.trigger_groups.iter().any(|g| g.name == *name)
                || !p.allowed.insert(name.clone())
            {
                return Err("missing or reused trigger settlement group".into());
            }
        }
        Ok(())
    }
    fn choice(
        &self,
        state: &GameState,
        p: &mut Progress,
        frame: &RuleFrame,
    ) -> Result<(DecisionReply, AcceptedKind), String> {
        Ok(match frame {
            RuleFrame::Opening(OpeningDecision::Declare { player, taken, .. }) => {
                let RuntimeInitialization::Opening { keep_choices } = &self.recipe.initialization
                else {
                    return Err("scheduled case issued opening choice".into());
                };
                let choice = keep_choices
                    .get(p.opening)
                    .ok_or("unexpected extra opening chooser")?;
                if choice.role.seat() != *player
                    || choice.taken != u64::from(*taken)
                    || choice.choice != "keep_seven"
                {
                    return Err("actual starter-first opening frame mismatch".into());
                }
                (
                    DecisionReply::Mulligan(MulliganChoice::Keep),
                    AcceptedKind::Opening,
                )
            }
            RuleFrame::Triggers(order) => {
                let mut matches = vec![];
                for group in &self.recipe.trigger_groups {
                    if !p.allowed.contains(&group.name)
                        || p.orders.contains(&(group.name.clone(), order.chooser()))
                    {
                        continue;
                    }
                    let mut selected = vec![];
                    let mut complete = true;
                    for name in &group.order_bottom_first {
                        if let Some(trigger) = p.triggers.get(name) {
                            if trigger.controller == order.chooser() {
                                selected.push(trigger.id);
                            }
                        } else {
                            complete = false;
                        }
                    }
                    if complete && !selected.is_empty() && same_set(&selected, order.instances()) {
                        matches.push((group.name.clone(), selected));
                    }
                }
                let [(name, selected)] = matches.as_slice() else {
                    return Err(
                        "trigger order frame does not biject one declared APNAP controller group"
                            .into(),
                    );
                };
                p.orders.insert((name.clone(), order.chooser()));
                (
                    DecisionReply::OrderTriggers(selected.clone()),
                    AcceptedKind::Choice,
                )
            }
            RuleFrame::TriggerTarget(target) => {
                let names = p
                    .triggers
                    .iter()
                    .filter(|(_, t)| t.id == target.instance())
                    .map(|(n, _)| n)
                    .collect::<Vec<_>>();
                let [name] = names.as_slice() else {
                    return Err("target frame lacks one actual trigger binding".into());
                };
                let choices = self
                    .recipe
                    .player_choices
                    .iter()
                    .enumerate()
                    .filter(|(i, c)| c.trigger == **name && !p.players.contains(i))
                    .collect::<Vec<_>>();
                let [(i, choice)] = choices.as_slice() else {
                    return Err("undeclared or reused trigger player choice".into());
                };
                let expected = choice
                    .domain_roles
                    .iter()
                    .map(|r| TargetRef::Player(r.seat()))
                    .collect::<Vec<_>>();
                if choice.chooser.seat() != target.chooser()
                    || !same_set(&expected, target.domain())
                    || self.entry(
                        p,
                        &EntryRef::TriggerEntry {
                            trigger: (**name).to_owned(),
                        },
                    )? != target.entry()
                {
                    return Err("trigger target authority/domain/entry mismatch".into());
                }
                p.players.insert(*i);
                (
                    DecisionReply::TriggerTarget(TargetRef::Player(choice.target.seat())),
                    AcceptedKind::Choice,
                )
            }
            RuleFrame::TimestampOrder(order) => {
                let choices = self
                    .recipe
                    .timestamp_choices
                    .iter()
                    .enumerate()
                    .filter(|(i, c)| {
                        !p.timestamps.contains(i)
                            && self.entry(p, &c.creation_entry).ok() == Some(order.entry())
                            && usize::try_from(c.instruction).ok() == Some(order.instruction())
                            && c.chooser.seat() == order.chooser()
                    })
                    .collect::<Vec<_>>();
                let [(i, choice)] = choices.as_slice() else {
                    return Err("unbound or ambiguous timestamp instruction choice".into());
                };
                let selected = choice
                    .members_earliest_first
                    .iter()
                    .map(|m| {
                        usize::try_from(*m)
                            .map(TokenBatchMember::new)
                            .map_err(|_| "token member overflow".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let offered = order.members().iter().map(|m| m.member).collect::<Vec<_>>();
                if !same_set(&selected, &offered)
                    || order
                        .members()
                        .iter()
                        .any(|m| m.creator != choice.chooser.seat())
                {
                    return Err("timestamp choice differs from actual creation cohort".into());
                }
                p.timestamps.insert(*i);
                (
                    DecisionReply::TimestampOrder(selected),
                    AcceptedKind::Choice,
                )
            }
            RuleFrame::Checks(CheckDecision::GraveyardOrder(order)) => {
                let choices = self
                    .recipe
                    .graveyard_choices
                    .iter()
                    .enumerate()
                    .filter(|(i, c)| {
                        !p.graveyards.contains(i) && c.chooser.seat() == order.chooser()
                    })
                    .collect::<Vec<_>>();
                let [(i, choice)] = choices.as_slice() else {
                    return Err(
                        "unexpected graveyard-order chooser, including forbidden singleton prompt"
                            .into(),
                    );
                };
                let actual = |r: &ObjectRef| -> Result<ObjectId, String> {
                    match r {
                        ObjectRef::PhysicalAtCause {
                            alias,
                            cause,
                            appearance: Appearance::Before,
                        } if cause == &choice.cause => self.fixture.current_object(state, alias),
                        _ => self.object(p, state, r),
                    }
                };
                let domain = choice
                    .domain
                    .iter()
                    .map(actual)
                    .collect::<Result<Vec<_>, _>>()?;
                let selected = choice
                    .entrants_top_first
                    .iter()
                    .map(actual)
                    .collect::<Result<Vec<_>, _>>()?;
                if !same_set(&domain, order.domain()) || !same_set(&selected, &domain) {
                    return Err("same-owner graveyard pre-move permutation mismatch".into());
                }
                let Some(Instruction::Combat { cause, .. }) =
                    self.recipe.instructions.get(p.cursor)
                else {
                    return Err("graveyard choice outside declared combat".into());
                };
                if cause != &choice.cause {
                    return Err("graveyard choice cause mismatch".into());
                }
                p.graveyards.insert(*i);
                (
                    DecisionReply::OrderGraveyard(selected),
                    AcceptedKind::Choice,
                )
            }
            RuleFrame::Combat(CombatDecision::Attackers(frame)) => {
                if let Some(Instruction::Combat {
                    turn,
                    active,
                    attackers,
                    ..
                }) = self.recipe.instructions.get(p.cursor)
                {
                    if state.turn().is_some_and(|t| t.position().number < *turn) {
                        return Ok((
                            DecisionReply::DeclareAttackers(vec![]),
                            AcceptedKind::Choice,
                        ));
                    }
                    if state.turn().is_none_or(|t| t.position().number != *turn)
                        || frame.chooser() != active.seat()
                    {
                        return Err("combat attacker frame turn/controller mismatch".into());
                    }
                    let selected = attackers
                        .iter()
                        .map(|r| self.object(p, state, r))
                        .collect::<Result<Vec<_>, _>>()?;
                    if selected
                        .iter()
                        .any(|o| !frame.eligible_attackers().contains(o))
                        || frame.defenders() != [other(active.seat())?]
                    {
                        return Err("declared attackers outside real offered domain".into());
                    }
                    p.stage = Stage::Combat {
                        attacked: false,
                        blocked: false,
                    };
                    (
                        DecisionReply::DeclareAttackers(
                            selected
                                .into_iter()
                                .map(|attacker| AttackDeclaration {
                                    attacker,
                                    defender: other(active.seat()).expect("validated finite seat"),
                                    defending_planeswalker: None,
                                })
                                .collect(),
                        ),
                        AcceptedKind::Attack,
                    )
                } else if matches!(
                    self.recipe.instructions.get(p.cursor),
                    Some(Instruction::AdvanceReady { .. })
                ) {
                    (
                        DecisionReply::DeclareAttackers(vec![]),
                        AcceptedKind::Choice,
                    )
                } else {
                    return Err("unexpected attacker prompt outside combat/advance".into());
                }
            }
            RuleFrame::Combat(CombatDecision::Blockers(frame)) => {
                let Some(Instruction::Combat {
                    active,
                    attackers,
                    blockers,
                    ..
                }) = self.recipe.instructions.get(p.cursor)
                else {
                    return Err("unexpected blocker frame outside declared combat".into());
                };
                let expected_attackers = attackers
                    .iter()
                    .map(|r| self.object(p, state, r))
                    .collect::<Result<Vec<_>, _>>()?;
                if frame.chooser() != other(active.seat())?
                    || !same_set(&expected_attackers, frame.attackers())
                {
                    return Err("blocker frame authority/attacker domain mismatch".into());
                }
                let mut selected = vec![];
                for row in blockers {
                    let blocker = self.object(p, state, &row.blocker)?;
                    let attacker = self.object(p, state, &row.attacker)?;
                    if !frame.eligible_blockers().contains(&blocker)
                        || !frame.attackers().contains(&attacker)
                    {
                        return Err("declared blocker outside actual legal domain".into());
                    }
                    selected.push(BlockDeclaration { blocker, attacker });
                }
                (
                    DecisionReply::DeclareBlockers(selected),
                    AcceptedKind::Block,
                )
            }
            RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) => {
                if !matches!(
                    self.recipe.instructions.get(p.cursor),
                    Some(Instruction::Combat { .. })
                ) {
                    return Err("unexpected damage assignment outside declared combat".into());
                }
                let mut selected = vec![];
                for domain in frame.domains() {
                    let [target] = domain.recipients() else {
                        return Err("finite mandatory combat requires exactly one legal recipient per source".into());
                    };
                    selected.push(CombatDamageAssignment {
                        source: domain.source(),
                        target: *target,
                        amount: domain.power().clone(),
                    });
                }
                (
                    DecisionReply::AssignCombatDamage(selected),
                    AcceptedKind::Choice,
                )
            }
            _ => {
                return Err(format!(
                    "unexpected mandatory native decision family: {frame:?}"
                ));
            }
        })
    }
    fn accept_decision(
        &self,
        p: &mut Progress,
        input: &GameInput,
        candidate: &NativeCandidate,
        kind: &AcceptedKind,
    ) -> Result<(), String> {
        let records = &candidate.rules_journal().records;
        let controls = records
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    RulesRecord::OptionalEffectOutcome { .. }
                        | RulesRecord::ResolutionPaymentOutcome { .. }
                )
            })
            .collect::<Vec<_>>();
        match kind {
            AcceptedKind::Selection { .. } => {}
            AcceptedKind::Decision(index) => {
                let plan = self
                    .recipe
                    .decision_plans
                    .get(*index)
                    .and_then(RuntimeDecisionPlan::legacy)
                    .ok_or("accepted decision lacks legacy plan")?;
                let matched = match plan {
                    DecisionPlan::Optional {
                        entry,
                        instruction,
                        chooser,
                        choice,
                        ..
                    } => {
                        matches!(&input.command, GameCommand::Reply { choice: DecisionReply::OptionalEffect(actual), .. } if *actual == Self::optional(*choice))
                            && matches!(controls.as_slice(), [RulesRecord::OptionalEffectOutcome { entry: actual_entry, instruction: actual_instruction, chooser: actual_chooser, outcome, .. }]
                            if *actual_entry == self.entry(p, entry)? && u64::try_from(*actual_instruction).ok() == Some(*instruction)
                                && *actual_chooser == chooser.seat() && *outcome == Self::optional(*choice))
                    }
                    DecisionPlan::Payment {
                        entry,
                        instruction,
                        payer,
                        cost,
                        funding,
                        outcome,
                        ..
                    } => {
                        let expected_entry = self.entry(p, entry)?;
                        let quoted = Self::quote(cost)?;
                        let funding_progress = p.funding.get(index).cloned().unwrap_or_default();
                        if funding_progress.next != funding.len() || funding_progress.awaiting_color
                        {
                            return Err("payment outcome precedes accepted funding receipts".into());
                        }
                        let [
                            RulesRecord::ResolutionPaymentOutcome {
                                entry: actual_entry,
                                instruction: actual_instruction,
                                payer: actual_payer,
                                cost: actual_cost,
                                outcome: actual,
                                ..
                            },
                        ] = controls.as_slice()
                        else {
                            return Err(
                                "payment reply lacks exactly one actual payment outcome".into()
                            );
                        };
                        *actual_entry == expected_entry
                            && u64::try_from(*actual_instruction).ok() == Some(*instruction)
                            && *actual_payer == payer.seat()
                            && *actual_cost == quoted
                            && match (outcome, actual, &input.command) {
                                (
                                    PaymentOutcome::Decline {},
                                    ResolutionPaymentOutcome::Declined,
                                    GameCommand::Reply {
                                        choice:
                                            DecisionReply::ResolutionPayment(
                                                ResolutionPaymentReply::Decline,
                                            ),
                                        ..
                                    },
                            ) => {
                                !records.iter().filter_map(|record|match record { RulesRecord::Event(event)=>Some(event),_=>None })
                                    .flat_map(|event|&event.occurrences).any(|value|matches!(&value.value, RulesOccurrence::ManaDebited { payment, .. } if payment.source==expected_entry))
                            },
                                (
                                    PaymentOutcome::Pay { allocation },
                                    ResolutionPaymentOutcome::Paid(paid),
                                    GameCommand::Reply {
                                        choice:
                                            DecisionReply::ResolutionPayment(
                                                ResolutionPaymentReply::Pay(selected),
                                            ),
                                        ..
                                    },
                                ) => {
                                    let expected = Self::amounts(allocation, false)?;
                                    let actual = paid
                                        .spend
                                        .iter()
                                        .map(|v| (v.mana_type, v.amount.clone()))
                                        .collect::<BTreeMap<_, _>>();
                                    let mut debited = BTreeMap::new();
                                    for occurrence in records.iter().filter_map(|record|match record { RulesRecord::Event(event)=>Some(event),_=>None }).flat_map(|event|&event.occurrences) {
                                        if let RulesOccurrence::ManaDebited { payment, mana_type, amount, .. } = &occurrence.value
                                            && payment.source == expected_entry
                                            && (payment != paid || debited.insert(*mana_type, amount.clone()).is_some()) {
                                            return Err("resolution debit differs from paid outcome record".into());
                                        }
                                    }
                                    paid.source == expected_entry
                                        && paid.payer == payer.seat()
                                        && paid.cost == quoted
                                        && actual.len() == paid.spend.len()
                                        && actual == expected
                                        && debited == expected
                                        && selected == &Self::payment(allocation)?
                                }
                                _ => false,
                            }
                    }
                    _ => false,
                };
                if !matched || !p.decisions.insert(*index) {
                    return Err(
                        "accepted resolution control differs from exact prepared plan".into(),
                    );
                }
            }
            AcceptedKind::Funding {
                plan,
                step,
                color,
                source,
                ability,
            } => {
                if !controls.is_empty()
                    || input.command
                        != (GameCommand::ActivateMana {
                            source: *source,
                            ability: *ability,
                        })
                {
                    return Err("payment funding input/outcome differs".into());
                }
                let activation = records.iter().filter_map(|r| match r { RulesRecord::Event(e) => Some(e), _ => None }).flat_map(|e|&e.occurrences)
                    .filter(|o|matches!(&o.value, RulesOccurrence::ManaAbility { source: actual, ability: actual_ability, controller }
                        if actual.id()==*source && actual_ability==ability && *controller==input.actor)).count();
                if activation != 1
                    || !candidate
                        .state()
                        .execution()
                        .window()
                        .cards()
                        .iter()
                        .any(|o| o.id() == *source && o.tapped() == Some(true))
                {
                    return Err("payment funding lacks actual single activation/tap receipt".into());
                }
                let Some(DecisionPlan::Payment { funding, .. }) = self
                    .recipe
                    .decision_plans
                    .get(*plan)
                    .and_then(RuntimeDecisionPlan::legacy)
                else {
                    return Err("funding receipt lacks payment plan".into());
                };
                let progress = p.funding.get(plan).cloned().unwrap_or_default();
                if progress.next != *step || progress.awaiting_color {
                    return Err("funding receipt prefix changed".into());
                }
                if *color {
                    if candidate.mana_production().is_some()
                        || !matches!(funding.get(*step), Some(Funding::Color { .. }))
                    {
                        return Err("color activation produced mana before its reply".into());
                    }
                    self.payment_parent(p, candidate.state(), *plan, true)?;
                    if !matches!(candidate.state().pending_decision().map(|d|d.frame()), Some(RuleFrame::ManaColor(f)) if f.source()==*source && f.ability()==*ability && f.chooser()==input.actor)
                    {
                        return Err("accepted activation lacks its actual color child".into());
                    }
                    p.funding.insert(
                        *plan,
                        FundingProgress {
                            next: *step,
                            awaiting_color: true,
                        },
                    );
                } else {
                    let Some(Funding::Fixed { produced, .. }) = funding.get(*step) else {
                        return Err("fixed receipt differs from funding step".into());
                    };
                    Self::production(
                        candidate,
                        *source,
                        *ability,
                        input.actor,
                        &Self::amounts(produced, true)?,
                    )?;
                    self.payment_parent(p, candidate.state(), *plan, false)?;
                    p.funding.insert(
                        *plan,
                        FundingProgress {
                            next: step.checked_add(1).ok_or("funding index overflow")?,
                            awaiting_color: false,
                        },
                    );
                }
            }
            AcceptedKind::Color {
                plan,
                step,
                source,
                ability,
            } => {
                let Some(DecisionPlan::Payment { funding, .. }) = self
                    .recipe
                    .decision_plans
                    .get(*plan)
                    .and_then(RuntimeDecisionPlan::legacy)
                else {
                    return Err("color receipt lacks payment plan".into());
                };
                let Some(Funding::Color { amount, choice, .. }) = funding.get(*step) else {
                    return Err("color receipt differs from funding step".into());
                };
                if !controls.is_empty()
                    || !matches!(&input.command, GameCommand::Reply { choice: DecisionReply::ManaColor(color), .. } if *color==Self::mana(*choice))
                    || !p
                        .funding
                        .get(plan)
                        .is_some_and(|v| v.next == *step && v.awaiting_color)
                {
                    return Err("color receipt/input/prefix mismatch".into());
                }
                Self::production(
                    candidate,
                    *source,
                    *ability,
                    input.actor,
                    &BTreeMap::from([(Self::mana(*choice), super::integer(amount)?)]),
                )?;
                self.payment_parent(p, candidate.state(), *plan, false)?;
                p.funding.insert(
                    *plan,
                    FundingProgress {
                        next: step.checked_add(1).ok_or("funding index overflow")?,
                        awaiting_color: false,
                    },
                );
            }
            _ => {}
        }
        Ok(())
    }
    fn production(
        candidate: &NativeCandidate,
        source: ObjectId,
        ability: ManaAbilityRef,
        controller: SeatId,
        produced: &BTreeMap<ManaType, spellwake_contracts::RulesInteger>,
    ) -> Result<(), String> {
        let actual = candidate
            .mana_production()
            .ok_or("funding lacks actual production")?;
        if actual.source != source
            || actual.ability != ability
            || actual.controller != controller
            || &actual.produced != produced
        {
            return Err("actual funding production differs from declared source/amount".into());
        }
        Ok(())
    }
    pub(crate) fn accept(
        &mut self,
        input: &GameInput,
        candidate: &NativeCandidate,
        intent: FiniteIntent,
    ) -> Result<(), String> {
        if intent.input != *input
            || candidate.base() != &CandidateBase::Advance(input.basis.clone())
            || candidate.state().basis().game != input.basis.game
            || candidate.state().basis().context != input.basis.context
            || candidate.state().basis().epoch != input.basis.epoch
            || input.basis.revision.checked_add(1) != Some(candidate.state().basis().revision)
        {
            return Err(
                "finite acceptance differs from the original prepared input/successor".into(),
            );
        }
        let mut p = intent.next;
        self.accept_decision(&mut p, input, candidate, &intent.kind)?;
        if let AcceptedKind::Cast(name) = &intent.kind {
            let source = match self.recipe.instructions.get(p.cursor) {
                Some(
                    Instruction::CastUntargeted { source, .. }
                    | Instruction::CastTargeted { source, .. },
                ) => source,
                _ => return Err("cast intent instruction mismatch".into()),
            };
            let ObjectRef::PhysicalCurrent { alias } = source else {
                return Err("finite cast source must bind its current physical alias".into());
            };
            let cast = crate::ordinary_game::policy::accepted_cast(
                &self.fixture,
                input,
                candidate,
                alias,
            )?;
            if p.casts.insert(name.clone(), cast.entry).is_some() {
                return Err("named actual cast entry reused".into());
            }
        }
        // Retain all actual events before relating captures, including departed and transient tokens.
        for record in &candidate.rules_journal().records {
            if let RulesRecord::Event(event) = record {
                if p.events.insert(event.id, event.clone()).is_some() {
                    return Err("actual event identity repeated".into());
                }
                for operation in &self.recipe.causal_operations {
                    let (name, selected) = match operation {
                        CausalOperation::CastResolution { name, entry }
                        | CausalOperation::TriggerResolution { name, entry }
                        | CausalOperation::CopyResolution { name, entry } => {
                            let expected = self.entry(&p, entry).ok();
                            let found = match event.cause {
                                EventCause::Instruction { entry, .. }
                                | EventCause::ResolutionCompleted { entry, .. }
                                | EventCause::CounterSpell { entry, .. } => Some(entry),
                                _ => None,
                            };
                            (name, expected.is_some() && expected == found)
                        }
                        CausalOperation::CastCompleted { name, entry } => {
                            let expected = self.entry(&p, entry).ok();
                            (
                                name,
                                expected.is_some()
                                    && matches!(event.cause, EventCause::CastCompleted { entry } if Some(entry)==expected),
                            )
                        }
                        CausalOperation::CombatDamage {
                            name,
                            combat_instruction,
                        } => {
                            let active = self
                                .recipe
                                .instructions
                                .get(p.cursor)
                                .is_some_and(|i| i.id() == combat_instruction);
                            (
                                name,
                                active
                                    && matches!(
                                        event.cause,
                                        EventCause::DamageAdjusted { .. }
                                            | EventCause::DamageDealt { .. }
                                            | EventCause::DamageResults { .. }
                                            | EventCause::StateBasedActions { .. }
                                    ),
                            )
                        }
                    };
                    if selected {
                        p.causes.entry(name.clone()).or_default().insert(event.id);
                    }
                }
                let mut token_members: BTreeMap<SeatId, usize> = BTreeMap::new();
                for occurrence in &event.occurrences {
                    match &occurrence.value {
                        RulesOccurrence::CopyCreated { .. } => {
                            self.bind_copy(&mut p, event, &occurrence.value, candidate.state())?;
                        }
                        RulesOccurrence::TokenCreated { creator, after } => {
                            let EventCause::Instruction { entry, instruction } = event.cause else {
                                return Err("token birth lacks actual creating instruction".into());
                            };
                            let member = token_members.entry(*creator).or_default();
                            let key = (entry, instruction, *creator, *member);
                            if p.births
                                .insert(
                                    key,
                                    BirthAuthority {
                                        entry,
                                        instruction,
                                        creator: *creator,
                                        member: *member,
                                        event: event.id,
                                        occurrence: occurrence.id,
                                        appearance: after.as_ref().clone(),
                                    },
                                )
                                .is_some()
                                || p.lineage
                                    .insert(after.object.id(), after.object.id())
                                    .is_some()
                            {
                                return Err("ambiguous or repeated token creation cohort".into());
                            }
                            *member += 1;
                        }
                        RulesOccurrence::ZoneChange(z) => {
                            if let Some(birth) = p.lineage.get(&z.before.object.id()).copied()
                                && p.lineage.insert(z.after.object.id(), birth).is_some()
                            {
                                return Err("token incarnation lineage reused".into());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for record in &candidate.rules_journal().records {
            match record {
                RulesRecord::TriggerDiscovered(waiting) => {
                    let mut names = vec![];
                    for group in &self.recipe.trigger_groups {
                        if p.allowed.contains(&group.name) {
                            for selector in &group.triggers {
                                if !p.triggers.contains_key(&selector.name)
                                    && self.capture_matches(&p, selector, waiting)?
                                {
                                    names.push(selector.name.clone());
                                }
                            }
                        }
                    }
                    let [name] = names.as_slice() else {
                        return Err(format!(
                            "actual discovery has no unique declared complete cause/source match: {:?} candidates {names:?}",
                            waiting.id
                        ));
                    };
                    p.triggers.insert(name.clone(), waiting.as_ref().clone());
                }
                RulesRecord::TriggerPlacementStarted(place) | RulesRecord::TriggerPlaced(place) => {
                    let names = p
                        .triggers
                        .iter()
                        .filter(|(_, t)| t.id == place.instance)
                        .map(|(name, _)| name.clone())
                        .collect::<Vec<_>>();
                    let [name] = names.as_slice() else {
                        return Err("actual placement lacks unique captured trigger".into());
                    };
                    if p.triggers[name].controller != place.controller {
                        return Err("placement changed captured controller".into());
                    }
                    if let Some(old) = p.entries.insert(name.clone(), place.entry)
                        && old != place.entry
                    {
                        return Err("provisional/installed trigger entry changed".into());
                    }
                }
                _ => {}
            }
        }
        match &intent.kind {
            AcceptedKind::Simple => {
                p.cursor += 1;
                p.stage = Stage::Idle;
            }
            AcceptedKind::Cast(_) => p.stage = Stage::Settlement,
            AcceptedKind::Opening => p.opening += 1,
            AcceptedKind::Attack => {
                p.stage = Stage::Combat {
                    attacked: true,
                    blocked: false,
                }
            }
            AcceptedKind::Block => {
                p.stage = Stage::Combat {
                    attacked: true,
                    blocked: true,
                }
            }
            AcceptedKind::Pass | AcceptedKind::Choice => {}
            AcceptedKind::Decision(_)
            | AcceptedKind::Selection { .. }
            | AcceptedKind::Funding { .. }
            | AcceptedKind::Color { .. } => {}
            AcceptedKind::Copy(index) => {
                if !p.decisions.contains(index) {
                    return Err("copy reply lacks its actual CopyCreated binding".into());
                }
            }
        }
        for declaration in candidate.combat_declarations() {
            if let spellwake_engine::combat::CombatDeclarationRecord::Blockers {
                origin: spellwake_engine::combat::DeclarationOrigin::ForcedEmpty,
                blockers,
                ..
            } = declaration
                && matches!(p.stage, Stage::Combat { attacked: true, .. })
            {
                let Some(Instruction::Combat {
                    blockers: expected, ..
                }) = self.recipe.instructions.get(p.cursor)
                else {
                    return Err("forced blockers outside combat".into());
                };
                if !expected.is_empty() || !blockers.is_empty() {
                    return Err("forced empty blocker declaration differs from recipe".into());
                }
                p.stage = Stage::Combat {
                    attacked: true,
                    blocked: true,
                };
            }
        }
        if matches!(
            input.command,
            GameCommand::Reply {
                choice: DecisionReply::TriggerTarget(_),
                ..
            }
        ) {
            p.last_player_target_revision = Some(candidate.state().basis().revision);
        }
        if matches!(input.command, GameCommand::Reply { .. }) {
            p.callbacks = p
                .callbacks
                .checked_add(1)
                .ok_or("mandatory callback counter overflow")?;
        }
        p.accepted = p
            .accepted
            .checked_add(1)
            .ok_or("mandatory accepted counter overflow")?;
        if let AcceptedKind::Selection { reached, .. } = &intent.kind {
            self.accept_selection(&mut p, input, candidate, reached)?;
        }
        p.probes.clear();
        self.progress = p;
        Ok(())
    }
    fn bind_copy(
        &self,
        p: &mut Progress,
        event: &RulesEvent,
        occurrence: &RulesOccurrence,
        state: &GameState,
    ) -> Result<(), String> {
        let RulesOccurrence::CopyCreated {
            entry: copy,
            original,
            controller,
            targets,
            ..
        } = occurrence
        else {
            return Err("copy binding requires actual CopyCreated occurrence".into());
        };
        let (copy, original, controller) = (*copy, *original, *controller);
        let EventCause::Instruction {
            entry: creating,
            instruction: at,
        } = event.cause
        else {
            return Err("actual copy lacks creating instruction cause".into());
        };
        let matches = self
            .recipe
            .decision_plans
            .iter()
            .enumerate()
            .filter_map(|(index, plan)| plan.legacy().map(|plan| (index, plan)))
            .filter_map(|(index, plan)| match plan {
                DecisionPlan::CopyTargets {
                    entry,
                    instruction,
                    original: expected,
                    chooser,
                    choice,
                    bind_copy,
                    ..
                } if !p.decisions.contains(&index)
                    && self.entry(p, entry).ok() == Some(creating)
                    && usize::try_from(*instruction).ok() == Some(at)
                    && self.entry(p, expected).ok() == Some(original)
                    && chooser.seat() == controller =>
                {
                    Some((index, choice, bind_copy))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let [(index, choice, name)] = matches.as_slice() else {
            return Err("CopyCreated lacks one actual origin/original/controller plan".into());
        };
        let expected = match choice {
            CopyChoice::Retain {} => state
                .execution()
                .window()
                .entries()
                .iter()
                .find(|v| v.id() == original)
                .ok_or("copy original absent at creation")?
                .targets()
                .targets()
                .iter()
                .map(|v| v.choice().target)
                .collect::<Vec<_>>(),
            CopyChoice::Change { target } => vec![self.target(p, state, target)?],
        };
        let actual = targets.iter().map(|v| v.target).collect::<Vec<_>>();
        let native = state
            .execution()
            .window()
            .entries()
            .iter()
            .find(|v| v.id() == copy)
            .ok_or("created copy lacks native stack entry")?;
        if expected != actual
            || native.controller() != controller
            || !matches!(native.kind(), spellwake_engine::stack_window::StackKind::SpellCopy { copied_from, .. } if *copied_from==original)
            || !state
                .execution()
                .window()
                .spell_copies()
                .contains_key(&copy)
            || name.is_empty()
            || p.copies.insert((*name).clone(), copy).is_some()
            || !p.decisions.insert(*index)
        {
            return Err("actual copy target/native identity/binding mismatch".into());
        }
        Ok(())
    }
    fn capture_matches(
        &self,
        p: &Progress,
        s: &Trigger,
        w: &WaitingInstance,
    ) -> Result<bool, String> {
        if s.definition_id != "1"
            || w.definition.0 != 1
            || w.controller != s.captured_controller.seat()
            || w.causes.len() != 1
            || !p
                .causes
                .get(&s.cause_operation)
                .is_some_and(|e| e.contains(&w.causes.event()))
        {
            return Ok(false);
        }
        let source = match &s.source {
            ObjectRef::PhysicalAtCause {
                alias,
                cause,
                appearance,
            } if cause == &s.cause_operation => {
                if w.source.object.card_id() != Some(self.fixture.card(alias)?) {
                    return Ok(false);
                }
                let event = p
                    .events
                    .get(&w.causes.event())
                    .ok_or("captured event missing")?;
                let moves = event
                    .occurrences
                    .iter()
                    .filter_map(|o| match &o.value {
                        RulesOccurrence::ZoneChange(z)
                            if z.before.object.card_id() == w.source.object.card_id() =>
                        {
                            Some(z)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if let [z] = moves.as_slice() {
                    match appearance {
                        Appearance::Before => z.before.object.id(),
                        Appearance::After => z.after.object.id(),
                    }
                } else if moves.is_empty() {
                    w.source.object.id()
                } else {
                    return Err("ambiguous captured source appearance".into());
                }
            }
            _ => return Err("finite trigger source requires causal physical appearance".into()),
        };
        if source != w.source.object.id() {
            return Ok(false);
        }
        let member = &w.causes.members()[0];
        let (subject, event) = match &member.related {
            TriggerRelated::ZoneChange { before, after } => {
                if before.object.zone() == ObjectZone::Battlefield
                    && matches!(after.object.zone(), ObjectZone::Graveyard(_))
                {
                    (before.object.id(), TriggerEvent::Dies)
                } else if after.object.zone() == ObjectZone::Battlefield {
                    (after.object.id(), TriggerEvent::Entry)
                } else {
                    return Ok(false);
                }
            }
            TriggerRelated::TokenCreated { after, .. } => (after.object.id(), TriggerEvent::Entry),
            TriggerRelated::SpellCast(cast) => {
                let event = p
                    .events
                    .get(&w.causes.event())
                    .ok_or("cast trigger cause event absent")?;
                if s.caster.map(Role::seat) != Some(cast.controller())
                    || !matches!(event.cause, EventCause::CastCompleted { entry } if entry==cast.entry())
                    || !p.casts.values().any(|entry|*entry==cast.entry())
                    || event.occurrences.iter().filter(|o|matches!(&o.value, RulesOccurrence::CastCompleted(actual) if actual.as_ref()==cast.as_ref())).count()!=1
                { return Ok(false); }
                (cast.spell().object.id(), TriggerEvent::Cast)
            }
            _ => return Ok(false),
        };
        if event != s.event {
            return Ok(false);
        }
        // Subject references at a cause resolve solely from retained real events and birth lineage.
        let expected = match &s.subject {
            ObjectRef::PhysicalAtCause {
                alias,
                cause,
                appearance,
            } => self.at_cause(p, Some(self.fixture.card(alias)?), None, cause, *appearance)?,
            ObjectRef::TokenAtCause {
                token,
                cause,
                appearance,
            } => self.at_cause(
                p,
                None,
                Some(self.birth(
                    p,
                    &token.creation_entry,
                    token.instruction,
                    token.creator,
                    token.member,
                )?),
                cause,
                *appearance,
            )?,
            _ => return Err("finite trigger subject requires actual causal appearance".into()),
        };
        Ok(subject == expected)
    }
    fn annotated(&self, state: &GameState) -> Result<Progress, String> {
        let mut p = self.progress.clone();
        p.probes.clear();
        self.normalize(state, &mut p)?;
        for _ in 0..=self.recipe.instructions.len() {
            match self.recipe.instructions.get(p.cursor) {
                Some(Instruction::SaveObject {
                    point,
                    source,
                    binding,
                    ..
                }) => {
                    self.point(&p, state, point)?;
                    let resolved = self.resolve_object(&p, state, source)?;
                    let saved = SavedObjectAuthority {
                        basis: state.basis().clone(),
                        instruction: self.recipe.instructions[p.cursor].id().into(),
                        object: resolved.actual_object()?.clone(),
                    };
                    if p.saved.insert(binding.clone(), saved).is_some() {
                        return Err("saved annotation reused".into());
                    }
                    p.cursor += 1;
                }
                Some(Instruction::ProbeMana {
                    id,
                    point,
                    source,
                    ability,
                    mutation,
                }) => {
                    self.point(&p, state, point)?;
                    let expected = match mutation {
                        ProbeMutation::Young => {
                            GameError::Effect(EffectError::CreatureTapRestricted)
                        }
                        ProbeMutation::Tapped => {
                            GameError::Effect(EffectError::Window(StackWindowError::SourceTapped))
                        }
                        ProbeMutation::WrongActor => {
                            GameError::Effect(EffectError::Window(StackWindowError::Priority(
                                spellwake_engine::priority::PriorityError::WrongPlayer,
                            )))
                        }
                        ProbeMutation::StaleIncarnation => {
                            GameError::Effect(EffectError::UnknownObject)
                        }
                    };
                    p.probes.push(RejectionProbe {
                        id: id.clone(),
                        expectation_ref: format!("T09.{mutation:?}"),
                        attachment: "actual-mandatory-reached-state".into(),
                        input: GameInput {
                            basis: state.basis().clone(),
                            actor: point.actor.seat(),
                            command: GameCommand::ActivateMana {
                                source: self.object(&p, state, source)?,
                                ability: Self::ability(ability)?,
                            },
                        },
                        expected,
                    });
                    p.cursor += 1;
                }
                _ => return Ok(p),
            }
        }
        Err("mandatory annotation watchdog exhausted".into())
    }
    #[cfg(test)]
    pub(crate) fn observed_instances(&self) -> BTreeSet<TriggerInstanceId> {
        self.progress.triggers.values().map(|t| t.id).collect()
    }
    pub(crate) fn prior_nonmember(
        &self,
        current: &[TriggerInstanceId],
    ) -> Result<TriggerInstanceId, String> {
        let values = self
            .progress
            .triggers
            .values()
            .filter(|t| !current.contains(&t.id))
            .map(|t| t.id)
            .collect::<BTreeSet<_>>();
        let values = values.into_iter().collect::<Vec<_>>();
        let [id] = values.as_slice() else {
            return Err("prior actual trigger nonmember missing or ambiguous".into());
        };
        Ok(*id)
    }
    pub(crate) fn player_choices_consumed(&self) -> usize {
        self.progress.players.len()
    }
    pub(crate) fn just_accepted_player_target(&self, revision: u64) -> bool {
        self.progress.last_player_target_revision == Some(revision)
    }
    pub(crate) fn required_probe_ids(&self) -> Vec<String> {
        let mut ids = self
            .recipe
            .instructions
            .iter()
            .filter_map(|i| match i {
                Instruction::ProbeMana { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for plan in &self.recipe.decision_plans {
            if let RuntimeDecisionPlan::Selection(plan) = plan {
                ids.extend(Self::selection_fields(plan).6.iter().filter_map(|declaration| match declaration { crate::ordinary_game::selection::data::SelectionProbeDeclaration::Execute { obligation, .. } => Some(obligation.as_str().into()), _ => None }));
            }
        }
        ids
    }

    pub(crate) fn completion_probes(
        &self,
        state: &GameState,
    ) -> Result<Vec<RejectionProbe>, String> {
        let p = self.annotated(state)?;
        if p.cursor != self.recipe.instructions.len() {
            return Err("completion probes requested before all instructions".into());
        }
        Ok(p.probes)
    }
    fn consumed(&self, p: &Progress) -> bool {
        self.selection_obligations_complete(p) && self.recipe.causal_operations.iter().all(|operation|match operation{
            CausalOperation::CastResolution{name,entry}|CausalOperation::TriggerResolution{name,entry}|CausalOperation::CopyResolution{name,entry}=>self.entry(p,entry).is_ok_and(|entry|p.causes.get(name).is_some_and(|ids|ids.iter().any(|id|p.events.get(id).is_some_and(|event|matches!(event.cause,EventCause::ResolutionCompleted{entry:actual,..} if actual==entry))))),
            CausalOperation::CastCompleted{name,entry}=>self.entry(p,entry).is_ok_and(|entry|p.causes.get(name).is_some_and(|ids|ids.iter().any(|id|p.events.get(id).is_some_and(|event|matches!(event.cause,EventCause::CastCompleted{entry:actual} if actual==entry))))),
            CausalOperation::CombatDamage{name,..}=>p.causes.get(name).is_some_and(|ids|ids.iter().any(|id|p.events.get(id).is_some_and(|event|matches!(event.cause,EventCause::DamageDealt{..}|EventCause::DamageResults{..})))),
        }) && p.cursor == self.recipe.instructions.len()
            && p.triggers.len()
                == self
                    .recipe
                    .trigger_groups
                    .iter()
                    .map(|g| g.triggers.len())
                    .sum::<usize>()
            && p.entries.len() == p.triggers.len()
            && p.players.len() == self.recipe.player_choices.len()
            && p.timestamps.len() == self.recipe.timestamp_choices.len()
            && p.graveyards.len() == self.recipe.graveyard_choices.len()
            && p.allowed.len() == self.recipe.trigger_groups.len()
            && p.decisions.len() == self.recipe.decision_plans.len()
            && match &self.recipe.initialization {
                RuntimeInitialization::Opening { keep_choices } => p.opening == keep_choices.len(),
                RuntimeInitialization::Prepared => p.opening == 0,
            }
    }
    pub(crate) fn completed_boundary(&self, state: &GameState) -> Result<Option<StopKind>, String> {
        let (boundary, stack) = match &self.recipe.stop {
            Stop::BoundedReady { boundary } => (boundary, None),
            Stop::BoundedAtStack {
                boundary,
                stack_bottom_first,
            } => (boundary, Some(stack_bottom_first)),
            Stop::NaturalOutcome { .. } => return Ok(None),
        };
        let p = self.annotated(state)?;
        // Final survivor references acquire their actual entry identities only
        // as the recipe executes. Do not resolve them during an earlier cut.
        if !self.consumed(&p) || state.outcome().is_some() || !self.boundary(state, boundary) {
            return Ok(None);
        }
        let ready = if let Some(expected) = stack {
            state.pending_decision().is_none()
                && state.execution().resolution_state().is_none()
                && state.execution().waiting_triggers().next().is_none()
                && state
                    .execution()
                    .window()
                    .entries()
                    .iter()
                    .map(|v| v.id())
                    .collect::<Vec<_>>()
                    == self.stack(&p, expected)?
        } else {
            self.ready_empty(state)
        };
        if ready {
            Ok(Some(StopKind::PreparedReadyPriority {
                boundary_ref: self.recipe.id.as_str(),
                position: state.turn().ok_or("bounded stop lacks turn")?.position(),
                active: boundary.active.seat(),
                holder: boundary.holder.seat(),
            }))
        } else {
            Ok(None)
        }
    }
    pub(crate) fn finish(&self, state: &GameState) -> Result<(), String> {
        let p = self.annotated(state)?;
        if !self.consumed(&p) {
            return Err(format!(
                "unconsumed mandatory instructions/choices: cursor {} of {}, triggers {}, players {}, timestamps {}, graveyards {}",
                p.cursor,
                self.recipe.instructions.len(),
                p.triggers.len(),
                p.players.len(),
                p.timestamps.len(),
                p.graveyards.len()
            ));
        }
        match &self.recipe.stop {
            Stop::BoundedReady { .. } | Stop::BoundedAtStack { .. } => {
                if self.completed_boundary(state)?.is_none() {
                    return Err("mandatory bounded-ready stop mismatch".into());
                }
            }
            Stop::NaturalOutcome { turn, step, active } => {
                if state.outcome().is_none()
                    || state.turn().is_none_or(|t| {
                        t.position().number != *turn || t.position().step != step.native()
                    })
                    || state.execution().window().active_player() != active.seat()
                {
                    return Err("mandatory natural outcome timing mismatch".into());
                }
            }
        }
        if matches!(
            self.recipe.initialization,
            RuntimeInitialization::Opening { .. }
        ) && state.entropy().is_none_or(|t| t.remaining() != 0)
        {
            return Err("mandatory unused opening entropy".into());
        }
        Ok(())
    }
}
fn other(seat: SeatId) -> Result<SeatId, String> {
    match seat {
        A => Ok(B),
        B => Ok(A),
        _ => Err("unknown mandatory seat".into()),
    }
}
fn holder(state: &GameState) -> Result<SeatId, String> {
    match state.execution().window().phase() {
        WindowPhase::Priority(PriorityPhase::Ready { holder }) => Ok(holder),
        _ => Err("mandatory command lacks actual ready priority".into()),
    }
}
fn same_set<T: Ord + Clone>(left: &[T], right: &[T]) -> bool {
    left.len() == right.len()
        && left.iter().cloned().collect::<BTreeSet<_>>()
            == right.iter().cloned().collect::<BTreeSet<_>>()
}

fn step_index(s: Step) -> u8 {
    match s {
        Step::Untap => 0,
        Step::Upkeep => 1,
        Step::Draw => 2,
        Step::PrecombatMain => 3,
        Step::BeginningCombat => 4,
        Step::DeclareAttackers => 5,
        Step::DeclareBlockers => 6,
        Step::CombatDamage => 7,
        Step::EndCombat => 8,
        Step::PostcombatMain => 9,
        Step::End => 10,
        Step::Cleanup => 11,
    }
}

#[cfg(test)]
mod normalization_tests;

#[cfg(test)]
mod selection_tests;
