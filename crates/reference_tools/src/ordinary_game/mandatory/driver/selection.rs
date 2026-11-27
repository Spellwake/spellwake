//! Selected plans extend the maintained finite controller; GameState owns every rule transition.
use super::*;
use crate::ordinary_game::selection::{
    authority::{
        self, ForeignAuthority, InitializerAuthority, SelectionBindingError,
        SelectionBindingEvidence,
    },
    data::{
        ProbeBasisRef, ProbeDecisionRef, SelectionDecisionPlan, SelectionOperation,
        SelectionProbeAttempt, SelectionProbeDeclaration,
    },
    probes::{self, AcceptedSelectionReceipt, ConsumedPlan, ProbeAuthorities, ReachedSelection},
};
use spellwake_engine::effects::{
    ResolutionAcceptedGraveyardOrder, ResolutionAcceptedSelection, ResolutionSelectionOperation,
    ResolutionSelectionStage,
};
impl FinitePolicy {
    pub(in crate::ordinary_game) fn bind_selection_initializer(
        &mut self,
        source: &crate::ordinary_game::selection::SelectionSource,
        installed: &GameState,
        initializer: &NativeCandidate,
    ) -> Result<SelectionBindingEvidence, SelectionBindingError> {
        self.bind_selection_initializer_using(
            source,
            installed,
            initializer,
            ForeignAuthority::prepare,
        )
    }
    pub(super) fn bind_selection_initializer_using(
        &mut self,
        source: &crate::ordinary_game::selection::SelectionSource,
        installed: &GameState,
        initializer: &NativeCandidate,
        prepare_foreign: impl FnOnce(
            &crate::ordinary_game::selection::SelectionSource,
        ) -> Result<ForeignAuthority, SelectionBindingError>,
    ) -> Result<SelectionBindingEvidence, SelectionBindingError> {
        if self.initializer.is_some() || self.progress.accepted != 0 {
            return Err(SelectionBindingError::caller(
                "selected initializer authority is already bound or policy has accepted inputs",
            ));
        }
        let validation =
            authority::validate_initializer(source, &self.fixture, installed, initializer)?;
        let needs_foreign = self.recipe.decision_plans.iter().any(|plan| match plan {
            RuntimeDecisionPlan::Selection(plan) => {
                Self::selection_fields(plan)
                    .6
                    .iter()
                    .any(|probe| match probe {
                        SelectionProbeDeclaration::Execute { basis, attempt, .. } => {
                            matches!(basis, ProbeBasisRef::ForeignPendingHelper { .. })
                                || matches!(
                                    attempt.as_ref(),
                                    SelectionProbeAttempt::Reply {
                                        decision: ProbeDecisionRef::ForeignPendingHelper { .. },
                                        ..
                                    }
                                )
                        }
                        _ => false,
                    })
            }
            _ => false,
        });
        let foreign = if needs_foreign {
            Some(
                prepare_foreign(source)
                    .map_err(|error| error.with_main_validation(validation.clone()))?,
            )
        } else {
            None
        };
        let evidence = SelectionBindingEvidence {
            installed_initializer: initializer.clone(),
            validation,
            foreign_helper: foreign.as_ref().map(|f| f.trace.clone()),
        };
        self.initializer = Some(InitializerAuthority {
            basis: installed.basis().clone(),
            evidence: evidence.clone(),
            foreign,
        });
        Ok(evidence)
    }
    pub(in crate::ordinary_game) fn selection_observation(
        &self,
    ) -> authority::SelectionProgressObservation {
        authority::SelectionProgressObservation {
            consumed: self.progress.selection_consumed.clone(),
            receipts: self.progress.selection_receipts.clone(),
            pending: self.progress.selection_pending.clone(),
        }
    }
    pub(super) fn selection_obligations_complete(&self, p: &Progress) -> bool {
        if !matches!(self.fixture.case, CaseId::Selection(_)) {
            return true;
        }
        let mut expected = BTreeSet::new();
        let mut pending = BTreeSet::new();
        for plan in &self.recipe.decision_plans {
            if let RuntimeDecisionPlan::Selection(plan) = plan {
                for declaration in Self::selection_fields(plan).6 {
                    match declaration {
                        SelectionProbeDeclaration::Execute { obligation, .. } => {
                            expected.insert(obligation.as_str());
                        }
                        SelectionProbeDeclaration::PendingExactOriginalHistory {
                            obligation,
                            ..
                        } => {
                            pending.insert(obligation.as_str());
                        }
                    }
                }
            }
        }
        self.initializer.is_some()
            && p.selection_receipts.len() == expected.len()
            && p.selection_pending.len() == pending.len()
            && p.selection_receipts
                .iter()
                .map(|r| r.obligation.as_str())
                .collect::<BTreeSet<_>>()
                == expected
            && p.selection_pending
                .iter()
                .map(|r| r.obligation.as_str())
                .collect::<BTreeSet<_>>()
                == pending
    }
    pub(super) fn selection_fields(
        plan: &SelectionDecisionPlan,
    ) -> (
        &str,
        &EntryRef,
        u64,
        SelectionOperation,
        Role,
        &[ObjectRef],
        &[SelectionProbeDeclaration],
    ) {
        match plan {
            SelectionDecisionPlan::ResolutionSelect {
                id,
                entry,
                instruction,
                operation,
                chooser,
                domain,
                probes,
                ..
            } => (
                id,
                entry,
                *instruction,
                *operation,
                *chooser,
                domain,
                probes,
            ),
            SelectionDecisionPlan::ResolutionGraveyardOrder {
                id,
                entry,
                instruction,
                operation,
                owner,
                domain,
                probes,
                ..
            } => (id, entry, *instruction, *operation, *owner, domain, probes),
        }
    }
    fn selection_objects(
        &self,
        p: &Progress,
        state: &GameState,
        refs: &[ObjectRef],
    ) -> Result<Vec<ObjectId>, String> {
        refs.iter().map(|r| self.object(p, state, r)).collect()
    }
    fn bijection(a: &[ObjectId], b: &[ObjectId]) -> bool {
        a.len() == b.len()
            && a.iter().copied().collect::<BTreeSet<_>>().len() == a.len()
            && a.iter().copied().collect::<BTreeSet<_>>() == b.iter().copied().collect()
    }
    fn operation_matches(
        operation: SelectionOperation,
        actual: &ResolutionSelectionOperation,
    ) -> bool {
        match (operation, actual) {
            (
                SelectionOperation::SacrificeCreature,
                ResolutionSelectionOperation::Sacrifice { primary, secondary },
            ) => {
                secondary.is_none()
                    && matches!(
                        primary,
                        spellwake_engine::program::ObjectFilter::Clause(c)
                            if c.required_types == spellwake_engine::program::CardTypeMask::CREATURE
                                && c.forbidden_types.is_empty()
                    )
            }
            (SelectionOperation::DiscardCards, ResolutionSelectionOperation::Discard) => true,
            _ => false,
        }
    }
    fn accepted_selection_prefix(
        &self,
        p: &Progress,
        state: &GameState,
        entry: StackEntryId,
        instruction: usize,
        accepted: &[ResolutionAcceptedSelection],
    ) -> Result<(), String> {
        for row in accepted {
            if row.objects.is_empty() {
                continue;
            }
            let matches=p.selection_consumed.values().filter(|receipt|matches!(receipt.decision().frame(),RuleFrame::ResolutionSelect(f) if f.entry()==entry&&f.instruction()==instruction&&f.chooser()==row.player)).collect::<Vec<_>>();
            let [receipt] = matches.as_slice() else {
                return Err("selected work prefix lacks its actual accepted plan authority".into());
            };
            let RuntimeDecisionPlan::Selection(SelectionDecisionPlan::ResolutionSelect {
                selected,
                ..
            }) = &self.recipe.decision_plans[receipt.index()]
            else {
                return Err("selected work prefix plan family mismatch".into());
            };
            if self.selection_objects(p, state, selected)? != row.objects {
                return Err("selected work prefix differs from accepted authored vector".into());
            }
        }
        Ok(())
    }
    fn accepted_order_prefix(
        &self,
        p: &Progress,
        state: &GameState,
        entry: StackEntryId,
        instruction: usize,
        orders: &[ResolutionAcceptedGraveyardOrder],
        groups: &[spellwake_engine::effects::ResolutionGraveyardGroup],
    ) -> Result<(), String> {
        for (row, group) in orders.iter().zip(groups) {
            if row.owner != group.owner || !Self::bijection(&row.top_to_bottom, &group.entrants) {
                return Err(
                    "owner-order prefix is not the actual complete group permutation".into(),
                );
            }
            if group.entrants.len() == 1 {
                if row.top_to_bottom != group.entrants {
                    return Err("internal singleton owner-order changed".into());
                }
                continue;
            }
            let matches=p.selection_consumed.values().filter(|receipt|matches!(receipt.decision().frame(),RuleFrame::ResolutionGraveyardOrder(f) if f.entry()==entry&&f.instruction()==instruction&&f.chooser()==row.owner)).collect::<Vec<_>>();
            let [receipt] = matches.as_slice() else {
                return Err("owner-order prefix lacks accepted plan authority".into());
            };
            let RuntimeDecisionPlan::Selection(SelectionDecisionPlan::ResolutionGraveyardOrder {
                entrants_top_first,
                ..
            }) = &self.recipe.decision_plans[receipt.index()]
            else {
                return Err("owner-order accepted plan family mismatch".into());
            };
            if self.selection_objects(p, state, entrants_top_first)? != row.top_to_bottom {
                return Err("owner-order prefix differs from accepted authored permutation".into());
            }
        }
        Ok(())
    }
    pub(super) fn selection_pending_action(
        &self,
        state: &GameState,
        p: &mut Progress,
    ) -> Result<Option<(GameCommand, AcceptedKind, String)>, String> {
        let pending = state
            .pending_decision()
            .ok_or("selected matcher lacks native pending decision")?;
        let frame = pending.frame();
        if !matches!(
            frame,
            RuleFrame::ResolutionSelect(_) | RuleFrame::ResolutionGraveyardOrder(_)
        ) {
            return Ok(None);
        }
        let initializer = self
            .initializer
            .as_ref()
            .ok_or("selected missing initializer authority")?;
        if pending.basis() != state.basis() {
            return Err("selected pending basis differs from actual source".into());
        }
        let work = state
            .execution()
            .resolution_selection_state()
            .ok_or("selected native work absent")?;
        let entry = work.cursor.entry();
        let at = work.cursor.instruction();
        let matches = self
            .recipe
            .decision_plans
            .iter()
            .enumerate()
            .filter_map(|(index, plan)| match plan {
                RuntimeDecisionPlan::Selection(plan) if !p.decisions.contains(&index) => {
                    Some((index, plan))
                }
                _ => None,
            })
            .filter(|(_, plan)| {
                let (_, reference, instruction, operation, chooser, _, _) =
                    Self::selection_fields(plan);
                self.entry(p, reference).ok() == Some(entry)
                    && usize::try_from(instruction).ok() == Some(at)
                    && chooser.seat() == pending.chooser()
                    && Self::operation_matches(operation, work.operation)
                    && matches!(
                        (frame, *plan),
                        (
                            RuleFrame::ResolutionSelect(_),
                            SelectionDecisionPlan::ResolutionSelect { .. }
                        ) | (
                            RuleFrame::ResolutionGraveyardOrder(_),
                            SelectionDecisionPlan::ResolutionGraveyardOrder { .. }
                        )
                    )
            })
            .collect::<Vec<_>>();
        let [(index, plan)] = matches.as_slice() else {
            return Err("selected decision lacks one unconsumed exact finite plan".into());
        };
        let index = *index;
        let (id, _, _, _, _, domain, declarations) = Self::selection_fields(plan);
        let expected_domain = self.selection_objects(p, state, domain)?;
        let choice = match (frame, *plan, work.stage) {
            (
                RuleFrame::ResolutionSelect(f),
                SelectionDecisionPlan::ResolutionSelect {
                    required_count,
                    selected,
                    ..
                },
                ResolutionSelectionStage::Choosing {
                    requirements,
                    next_player,
                    accepted,
                },
            ) => {
                let requirement = requirements
                    .get(*next_player)
                    .ok_or("selected native requirement cursor outside work")?;
                if *next_player != accepted.len()
                    || requirements[..*next_player]
                        .iter()
                        .zip(accepted)
                        .any(|(r, a)| r.player != a.player || r.required != a.objects.len())
                    || f.entry() != entry
                    || f.instruction() != at
                    || f.revision() != state.execution().revision()
                    || f.chooser() != requirement.player
                    || f.domain() != requirement.domain
                    || f.required() != requirement.required
                    || required_count.to_usize() != Some(requirement.required)
                    || !Self::bijection(&expected_domain, f.domain())
                {
                    return Err("selected full frame/work/domain/count mismatch".into());
                }
                self.accepted_selection_prefix(p, state, entry, at, accepted)?;
                DecisionReply::SelectResolution(self.selection_objects(p, state, selected)?)
            }
            (
                RuleFrame::ResolutionGraveyardOrder(f),
                SelectionDecisionPlan::ResolutionGraveyardOrder {
                    entrant_count,
                    entrants_top_first,
                    ..
                },
                ResolutionSelectionStage::Ordering {
                    replacements: None,
                    selections,
                    groups,
                    next_owner,
                    orders,
                },
            ) => {
                let group = groups
                    .get(*next_owner)
                    .ok_or("selected native owner cursor outside work")?;
                if *next_owner != orders.len()
                    || f.entry() != entry
                    || f.instruction() != at
                    || f.revision() != state.execution().revision()
                    || f.chooser() != group.owner
                    || f.domain() != group.entrants
                    || entrant_count.to_usize() != Some(group.entrants.len())
                    || !Self::bijection(&expected_domain, f.domain())
                    || group.entrants.iter().any(|id| {
                        state
                            .execution()
                            .window()
                            .cards()
                            .get(*id)
                            .is_none_or(|o| o.card_id().is_none() || o.owner() != group.owner)
                    })
                {
                    return Err("selected full owner frame/work/domain/count mismatch".into());
                }
                self.accepted_selection_prefix(p, state, entry, at, selections)?;
                self.accepted_order_prefix(p, state, entry, at, orders, groups)?;
                DecisionReply::OrderResolutionGraveyard(self.selection_objects(
                    p,
                    state,
                    entrants_top_first,
                )?)
            }
            _ => return Err("selected native frame/work stage mismatch".into()),
        };
        let CaseId::Selection(case) = self.fixture.case else {
            return Err("selected matcher has nonselected fixture".into());
        };
        let reached = ReachedSelection::capture(case, id, index, pending, work);
        let receipt_start = p.selection_receipts.len();
        let pending_start = p.selection_pending.len();
        let batch = probes::lower(
            declarations,
            ProbeAuthorities {
                reached: &reached,
                initializer: Some(&initializer.basis),
                consumed: &p.selection_consumed,
                foreign: initializer.foreign.as_ref(),
            },
            |r| self.resolve_object(p, state, r),
            |r| self.resolve_target(p, state, r),
            Self::ability,
        )
        .map_err(|error| error.to_string())?;
        p.probes.extend(batch.probes);
        p.selection_receipts.extend(batch.receipts);
        p.selection_pending.extend(batch.pending);
        Ok(Some((
            GameCommand::Reply {
                decision: pending.id(),
                choice,
            },
            AcceptedKind::Selection {
                reached,
                receipt_start,
                pending_start,
            },
            id.into(),
        )))
    }
    pub(super) fn accept_selection(
        &self,
        p: &mut Progress,
        input: &GameInput,
        candidate: &NativeCandidate,
        reached: &Arc<ReachedSelection>,
    ) -> Result<(), String> {
        let index = reached.index;
        let decision = &reached.decision;
        let Some(RuntimeDecisionPlan::Selection(plan)) = self.recipe.decision_plans.get(index)
        else {
            return Err("selected accepted reply lacks its original plan index".into());
        };
        let (id, _, _, _, chooser, _, _) = Self::selection_fields(plan);
        if decision.basis() != &input.basis
            || decision.chooser() != input.actor
            || chooser.seat() != input.actor
            || !matches!(&input.command,GameCommand::Reply{decision:actual,..} if *actual==decision.id())
            || !p.decisions.insert(index)
        {
            return Err(
                "selected accepted reply differs from prepared authority or was consumed twice"
                    .into(),
            );
        }
        if p.selection_consumed
            .insert(
                id.into(),
                ConsumedPlan {
                    reached: reached.clone(),
                    accepted: AcceptedSelectionReceipt {
                        ordinal: p.accepted,
                        input: input.clone(),
                        after_basis: candidate.state().basis().clone(),
                    },
                },
            )
            .is_some()
        {
            return Err("selected accepted plan receipt identity reused".into());
        }
        Ok(())
    }
}
