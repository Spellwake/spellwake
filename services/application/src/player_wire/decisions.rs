//! Public frozen choice domains. These validators check supplied relationships;
//! only a qualified adapter can establish that they came from actual native state.
use super::*;
use crate::source_protocol::parse_utc_microsecond;
/// One issued option, with display text kept separate from its opaque meaning.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerOption {
    pub handle: PlayerHandle,
    pub label: String,
}
/// One offered legend conflict and the current visible handles within it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegendDomain {
    pub conflict: PlayerHandle,
    pub objects: Vec<PlayerHandle>,
}
/// An offered attacker and its permitted defending targets.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttackerDomain {
    pub attacker: PlayerHandle,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub defenders: Vec<PlayerTarget>,
}
/// One visible blocker and its finite offered attacker relationships.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlockerDomain {
    pub blocker: PlayerHandle,
    pub attackers: Vec<PlayerHandle>,
    pub minimum: RulesCount,
    pub maximum: RulesCount,
}
/// Exact source allocation total; native legality still checks ordering/lethal rules.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CombatDamageDomain {
    pub source: PlayerHandle,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub targets: Vec<PlayerTarget>,
    pub total: RulesCount,
}
/// Offered prevention application/attempt handle, never a native DamageAttemptId.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreventionDomain {
    pub attempt: PlayerHandle,
    pub maximum: RulesCount,
    pub label: String,
}
/// One normalized response allocation into a frozen offered prevention domain.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreventionAllocation {
    pub attempt: PlayerHandle,
    pub amount: RulesCount,
}
/// Finite public choice structures, not arbitrary serialized engine JSON.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlayerDomain {
    Options {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        options: Vec<PlayerOption>,
        #[serde(deserialize_with = "nullable_handle")]
        query: Option<PlayerHandle>,
    },
    Objects {
        objects: Vec<PlayerHandle>,
        minimum: RulesCount,
        maximum: RulesCount,
    },
    Ordering {
        objects: Vec<PlayerHandle>,
        /// Safe descriptions for exactly these offered references, including
        /// token previews and trigger instances that are not physical objects.
        #[serde(deserialize_with = "crate::provider_json::objects")]
        labels: Vec<PlayerOption>,
    },
    Number {
        minimum: RulesInteger,
        maximum: RulesInteger,
    },
    Targets {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
    },
    Legend {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        conflicts: Vec<LegendDomain>,
    },
    Payment {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        minimum_spend: Vec<ManaSpend>,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        maximum_spend: Vec<ManaSpend>,
        total: RulesCount,
        allow_decline: bool,
    },
    Distribution {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
        required: RulesCount,
        minimum_per_target: RulesCount,
    },
    Attackers {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        choices: Vec<AttackerDomain>,
        required: Vec<PlayerHandle>,
    },
    Blockers {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        choices: Vec<BlockerDomain>,
        required: Vec<PlayerHandle>,
    },
    CombatDamage {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        sources: Vec<CombatDamageDomain>,
    },
    Prevention {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        attempts: Vec<PreventionDomain>,
        required: RulesCount,
    },
}
pub(super) fn nullable_handle<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<PlayerHandle>, D::Error> {
    Option::<PlayerHandle>::deserialize(d)
}
pub(super) fn display_text(s: &str, max: usize) -> bool {
    s.len() <= max && !s.contains('\0')
}
fn handles(xs: &[PlayerHandle]) -> bool {
    list(xs) && unique(xs.iter())
}
fn targets(xs: &[PlayerTarget]) -> bool {
    list(xs) && unique(xs.iter()) && xs.iter().all(PlayerTarget::valid)
}
fn options(xs: &[PlayerOption]) -> bool {
    list(xs)
        && unique(xs.iter().map(|x| &x.handle))
        && xs
            .iter()
            .all(|x| !x.label.is_empty() && display_text(&x.label, 1024))
}
fn count(n: usize) -> RulesInteger {
    RulesInteger::from_usize(n)
}
fn within(n: usize, min: &RulesCount, max: &RulesCount) -> bool {
    min.as_rules_integer() <= &count(n) && &count(n) <= max.as_rules_integer()
}
fn sum<'a>(values: impl Iterator<Item = &'a RulesCount>) -> RulesInteger {
    values.fold(0.into(), |v, x| v.plus(x.as_rules_integer()))
}
fn mana(xs: &[ManaSpend]) -> bool {
    xs.len() <= 6 && unique(xs.iter().map(|x| x.color))
}
fn spend(xs: &[ManaSpend], color: ManaColor) -> RulesInteger {
    xs.iter()
        .find(|x| x.color == color)
        .map_or_else(|| 0.into(), |x| x.amount.as_rules_integer().clone())
}
impl PlayerDomain {
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::Options { options: xs, query } => {
                options(xs) && (!xs.is_empty() || query.is_some())
            }
            Self::Objects {
                objects,
                minimum,
                maximum,
            } => {
                handles(objects)
                    && minimum.as_rules_integer() <= maximum.as_rules_integer()
                    && maximum.as_rules_integer() <= &count(objects.len())
            }
            Self::Ordering { objects, labels } => {
                handles(objects)
                    && options(labels)
                    && labels.len() == objects.len()
                    && labels.iter().all(|entry| objects.contains(&entry.handle))
            }
            Self::Number { minimum, maximum } => minimum <= maximum,
            Self::Targets { targets: xs } => !xs.is_empty() && targets(xs),
            Self::Legend { conflicts } => {
                !conflicts.is_empty()
                    && list(conflicts)
                    && unique(conflicts.iter().map(|x| &x.conflict))
                    && conflicts
                        .iter()
                        .all(|x| !x.objects.is_empty() && handles(&x.objects))
            }
            Self::Payment {
                minimum_spend,
                maximum_spend,
                total,
                ..
            } => {
                mana(minimum_spend)
                    && mana(maximum_spend)
                    && minimum_spend
                        .iter()
                        .all(|x| x.amount.as_rules_integer() <= &spend(maximum_spend, x.color))
                    && sum(minimum_spend.iter().map(|x| &x.amount)) <= *total.as_rules_integer()
                    && sum(maximum_spend.iter().map(|x| &x.amount)) >= *total.as_rules_integer()
            }
            Self::Distribution {
                targets: xs,
                required,
                minimum_per_target,
            } => {
                targets(xs)
                    && (!xs.is_empty() || !positive(required))
                    && (!positive(required)
                        || minimum_per_target.as_rules_integer() <= required.as_rules_integer())
            }
            Self::Attackers { choices, required } => {
                list(choices)
                    && unique(choices.iter().map(|x| &x.attacker))
                    && choices.iter().all(|x| {
                        !x.defenders.is_empty()
                            && targets(&x.defenders)
                            && x.defenders
                                .iter()
                                .all(|x| !matches!(x, PlayerTarget::Stack { .. }))
                    })
                    && handles(required)
                    && required
                        .iter()
                        .all(|x| choices.iter().any(|c| c.attacker == *x))
            }
            Self::Blockers { choices, required } => {
                list(choices)
                    && unique(choices.iter().map(|x| &x.blocker))
                    && choices.iter().all(|x| {
                        handles(&x.attackers)
                            && x.minimum.as_rules_integer() <= x.maximum.as_rules_integer()
                            && x.maximum.as_rules_integer() <= &count(x.attackers.len())
                    })
                    && handles(required)
                    && required
                        .iter()
                        .all(|x| choices.iter().any(|c| c.blocker == *x))
            }
            Self::CombatDamage { sources } => {
                list(sources)
                    && unique(sources.iter().map(|x| &x.source))
                    && sources.iter().all(|x| {
                        targets(&x.targets) && (!positive(&x.total) || !x.targets.is_empty())
                    })
            }
            Self::Prevention { attempts, required } => {
                list(attempts)
                    && unique(attempts.iter().map(|x| &x.attempt))
                    && attempts
                        .iter()
                        .all(|entry| !entry.label.is_empty() && display_text(&entry.label, 1024))
                    && sum(attempts.iter().map(|x| &x.maximum)) >= *required.as_rules_integer()
            }
        }
    }
    fn supports(&self, kind: DecisionKind) -> bool {
        use DecisionKind as K;
        match self {
            Self::Options { query, .. } => {
                (kind == K::ZoneReplacement && query.is_none())
                    || matches!(
                        kind,
                        K::ReplacementChooser
                            | K::OptionalEffect
                            | K::Mulligan
                            | K::CopyTargets
                            | K::DamageOrder
                            | K::ManaColor
                            | K::CreatureType
                            | K::Modes
                            | K::CardName
                    )
            }
            Self::Objects {
                minimum, maximum, ..
            } => {
                kind == K::Discard
                    || kind == K::Bottom && minimum == maximum
                    || kind == K::SelectResolution && minimum == maximum && positive(minimum)
            }
            Self::Ordering { objects, .. } => {
                matches!(
                    kind,
                    K::TimestampOrder | K::OrderGraveyard | K::OrderTriggers
                ) || matches!(kind, K::OrderResolutionGraveyard | K::OrderLibraryGraveyard)
                    && objects.len() > 1
                    || matches!(kind, K::Scry | K::Surveil) && !objects.is_empty()
            }
            Self::Number { .. } => kind == K::Number,
            Self::Targets { .. } => kind == K::TriggerTarget,
            Self::Legend { .. } => kind == K::KeepLegend,
            Self::Payment { .. } => kind == K::ResolutionPayment,
            Self::Distribution { .. } => kind == K::Distribution,
            Self::Attackers { .. } => kind == K::DeclareAttackers,
            Self::Blockers { .. } => kind == K::DeclareBlockers,
            Self::CombatDamage { .. } => kind == K::AssignCombatDamage,
            Self::Prevention { .. } => kind == K::AllocatePrevention,
        }
    }
    fn allows(&self, response: &PlayerChoice, extra_options: &[PlayerHandle]) -> bool {
        match (self, response) {
            (Self::Options { options, query }, PlayerChoice::Option { option }) => {
                options.iter().any(|x| x.handle == *option)
                    || (query.is_some() && extra_options.contains(option))
            }
            (
                Self::Objects {
                    objects,
                    minimum,
                    maximum,
                },
                PlayerChoice::Objects { objects: selected },
            ) => {
                within(selected.len(), minimum, maximum)
                    && selected.iter().all(|x| objects.contains(x))
            }
            (Self::Ordering { objects, .. }, PlayerChoice::LibraryPartition { top, other }) => {
                objects.len() == top.len().saturating_add(other.len())
                    && top.iter().chain(other).all(|x| objects.contains(x))
            }
            (Self::Ordering { objects, .. }, PlayerChoice::Ordering { ordered }) => {
                objects.len() == ordered.len() && ordered.iter().all(|x| objects.contains(x))
            }
            (Self::Number { minimum, maximum }, PlayerChoice::Number { value }) => {
                minimum <= value && value <= maximum
            }
            (Self::Targets { targets }, PlayerChoice::Target { target }) => {
                targets.contains(target)
            }
            (Self::Legend { conflicts }, PlayerChoice::Legend { conflict, keep }) => conflicts
                .iter()
                .any(|x| x.conflict == *conflict && x.objects.contains(keep)),
            (
                Self::Payment {
                    minimum_spend,
                    maximum_spend,
                    total,
                    ..
                },
                PlayerChoice::Pay { payment },
            ) => {
                sum(payment.spend.iter().map(|x| &x.amount)) == *total.as_rules_integer()
                    && minimum_spend
                        .iter()
                        .all(|x| spend(&payment.spend, x.color) >= *x.amount.as_rules_integer())
                    && payment
                        .spend
                        .iter()
                        .all(|x| x.amount.as_rules_integer() <= &spend(maximum_spend, x.color))
            }
            (Self::Payment { allow_decline, .. }, PlayerChoice::Decline {}) => *allow_decline,
            (
                Self::Distribution {
                    targets,
                    required,
                    minimum_per_target,
                },
                PlayerChoice::Distribution { allocations },
            ) => {
                sum(allocations.iter().map(|x| &x.amount)) == *required.as_rules_integer()
                    && allocations.iter().all(|x| {
                        targets.contains(&x.target)
                            && (!positive(&x.amount)
                                || x.amount.as_rules_integer()
                                    >= minimum_per_target.as_rules_integer())
                    })
            }
            (Self::Attackers { choices, required }, PlayerChoice::Attackers { assignments }) => {
                required
                    .iter()
                    .all(|r| assignments.iter().any(|a| a.attacker == *r))
                    && assignments.iter().all(|a| {
                        choices
                            .iter()
                            .any(|c| c.attacker == a.attacker && c.defenders.contains(&a.defender))
                    })
            }
            (Self::Blockers { choices, required }, PlayerChoice::Blockers { assignments }) => {
                required
                    .iter()
                    .all(|r| assignments.iter().any(|a| a.blocker == *r))
                    && assignments.iter().all(|a| {
                        choices.iter().any(|c| {
                            c.blocker == a.blocker
                                && within(a.attackers.len(), &c.minimum, &c.maximum)
                                && a.attackers.iter().all(|x| c.attackers.contains(x))
                        })
                    })
            }
            (Self::CombatDamage { sources }, PlayerChoice::CombatDamage { assignments }) => {
                assignments.iter().all(|a| {
                    sources
                        .iter()
                        .any(|s| s.source == a.source && s.targets.contains(&a.target))
                }) && sources.iter().all(|s| {
                    sum(assignments
                        .iter()
                        .filter(|a| a.source == s.source)
                        .map(|a| &a.amount))
                        == *s.total.as_rules_integer()
                })
            }
            (Self::Prevention { attempts, required }, PlayerChoice::Prevention { allocations }) => {
                sum(allocations.iter().map(|x| &x.amount)) == *required.as_rules_integer()
                    && allocations.iter().all(|a| {
                        attempts.iter().any(|s| {
                            s.attempt == a.attempt
                                && a.amount.as_rules_integer() <= s.maximum.as_rules_integer()
                        })
                    })
            }
            _ => false,
        }
    }
}
/// Viewer-facing decision; visibility and frozen basis are checked contextually.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerDecision {
    pub decision: PlayerHandle,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub basis: PlayerBasis,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    pub responding_seat_ids: Vec<Uuid>,
    pub visibility: DecisionVisibility,
    pub kind: DecisionKind,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub domain: PlayerDomain,
    #[serde(deserialize_with = "crate::provider_json::optional_string")]
    pub deadline_at: Option<String>,
}
impl PlayerDecision {
    /// Check offered syntax, responder membership, exact basis and visibility.
    /// Full disclosure must already be authorized by the participant replay adapter.
    /// # Errors
    /// Rejects malformed domains, unknown roster seats, foreign basis or hidden decisions.
    pub fn validate(
        &self,
        basis: &PlayerBasis,
        viewer: Uuid,
        roster: &[Uuid],
        authorized_full_disclosure: bool,
    ) -> Result<(), InvalidPlayerWire> {
        let valid = (1..=6).contains(&roster.len())
            && unique(roster.iter())
            && roster.iter().all(|x| !x.is_nil())
            && !basis.game_id.is_nil()
            && !basis.context_id.is_nil()
            && self.basis == *basis
            && !viewer.is_nil()
            && roster.contains(&viewer)
            && (1..=6).contains(&self.responding_seat_ids.len())
            && unique(self.responding_seat_ids.iter())
            && self.responding_seat_ids.iter().all(|x| roster.contains(x))
            && (!matches!(
                self.kind,
                DecisionKind::SelectResolution | DecisionKind::OrderResolutionGraveyard
            ) || self.responding_seat_ids.len() == 1)
            && (!matches!(
                self.kind,
                DecisionKind::Scry
                    | DecisionKind::Surveil
                    | DecisionKind::OrderLibraryGraveyard
                    | DecisionKind::ZoneReplacement
            ) || self.responding_seat_ids.len() == 1
                && self.visibility == DecisionVisibility::RespondingSeat)
            && self.domain.valid()
            && self.domain.supports(self.kind)
            && self
                .deadline_at
                .as_ref()
                .is_none_or(|x| parse_utc_microsecond(x).is_some())
            && (authorized_full_disclosure
                || self.visibility == DecisionVisibility::Public
                || self.responding_seat_ids.contains(&viewer));
        if valid {
            Ok(())
        } else {
            Err(InvalidPlayerWire)
        }
    }
    pub(super) fn references_in(
        &self,
        objects: &std::collections::BTreeSet<PlayerHandle>,
        stack: &std::collections::BTreeSet<PlayerHandle>,
        roster: &[Uuid],
    ) -> bool {
        let check = |t: &PlayerTarget| target_in(t, objects, stack, roster);
        match &self.domain {
            PlayerDomain::Targets { targets } | PlayerDomain::Distribution { targets, .. } => {
                targets.iter().all(check)
            }
            PlayerDomain::Objects { objects: xs, .. } => xs.iter().all(|x| objects.contains(x)),
            PlayerDomain::Ordering { objects: xs, .. }
                if matches!(
                    self.kind,
                    DecisionKind::OrderGraveyard | DecisionKind::OrderResolutionGraveyard
                ) =>
            {
                xs.iter().all(|x| objects.contains(x))
            }
            PlayerDomain::Legend { conflicts } => conflicts
                .iter()
                .all(|c| c.objects.iter().all(|x| objects.contains(x))),
            PlayerDomain::Attackers { choices, .. } => choices
                .iter()
                .all(|c| objects.contains(&c.attacker) && c.defenders.iter().all(check)),
            PlayerDomain::Blockers { choices, .. } => choices.iter().all(|c| {
                objects.contains(&c.blocker) && c.attackers.iter().all(|x| objects.contains(x))
            }),
            PlayerDomain::CombatDamage { sources } => sources
                .iter()
                .all(|s| objects.contains(&s.source) && s.targets.iter().all(check)),
            _ => true,
        }
    }
    /// Validate a response against independently supplied frozen descriptor/page facts.
    /// This method does not establish those facts or run native legality.
    /// # Errors
    /// Rejects foreign command/decision/viewer, incompatible shape or out-of-domain choices.
    pub fn validate_response(
        &self,
        command: &PlayerCommand,
        viewer: Uuid,
        roster: &[Uuid],
        pages: &[PlayerChoicePage],
    ) -> Result<(), InvalidPlayerWire> {
        command.validate()?;
        self.validate(&command.basis, viewer, roster, false)?;
        if pages.len() > 100 {
            return Err(InvalidPlayerWire);
        }
        let PlayerAction::Choose { decision, response } = &command.action else {
            return Err(InvalidPlayerWire);
        };
        let query = match &self.domain {
            PlayerDomain::Options { query, .. } => query.as_ref(),
            _ => None,
        };
        let mut extra = Vec::new();
        for page in pages {
            if page.basis != self.basis
                || page.viewer_seat_id != viewer
                || page.decision != self.decision
                || Some(&page.query) != query
                || !page.valid()
            {
                return Err(InvalidPlayerWire);
            }
            extra.extend(page.options.iter().map(|x| x.handle.clone()));
        }
        if self.basis == command.basis
            && self.decision == *decision
            && self.responding_seat_ids.contains(&viewer)
            && self.domain.valid()
            && self.domain.supports(self.kind)
            && (matches!(self.kind, DecisionKind::Scry | DecisionKind::Surveil)
                == matches!(response, PlayerChoice::LibraryPartition { .. }))
            && self.domain.allows(response, &extra)
        {
            Ok(())
        } else {
            Err(InvalidPlayerWire)
        }
    }
}
/// Bounded choice query. The actual server resolves the cursor's scope separately.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerChoiceQuery {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub basis: PlayerBasis,
    pub decision: PlayerHandle,
    pub query: PlayerHandle,
    #[serde(deserialize_with = "nullable_handle")]
    pub cursor: Option<PlayerHandle>,
    pub limit: NativeRevision,
}
impl PlayerChoiceQuery {
    /// Check a manually constructed bounded query before a future dispatch.
    /// # Errors
    /// Rejects nil basis IDs and a page limit outside1..=100.
    pub fn validate(&self) -> Result<(), InvalidPlayerWire> {
        if self.basis.game_id.is_nil()
            || self.basis.context_id.is_nil()
            || !(1..=100).contains(&self.limit.get())
        {
            Err(InvalidPlayerWire)
        } else {
            Ok(())
        }
    }
}
/// Original query/cursor and viewer echoed for exact response correlation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerChoicePage {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub basis: PlayerBasis,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub viewer_seat_id: Uuid,
    pub decision: PlayerHandle,
    pub query: PlayerHandle,
    #[serde(deserialize_with = "nullable_handle")]
    pub cursor: Option<PlayerHandle>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub options: Vec<PlayerOption>,
    #[serde(deserialize_with = "nullable_handle")]
    pub next_cursor: Option<PlayerHandle>,
}
impl PlayerChoicePage {
    fn valid(&self) -> bool {
        self.options.len() <= 100
            && options(&self.options)
            && self
                .next_cursor
                .as_ref()
                .is_none_or(|x| !self.options.is_empty() && Some(x) != self.cursor.as_ref())
    }
    /// Bind a bounded page to its original query and trusted viewer seat.
    /// # Errors
    /// Rejects invalid page bounds, changed query/basis/viewer/cursor or duplicate options.
    pub fn validate(
        &self,
        request: &PlayerChoiceQuery,
        viewer: Uuid,
    ) -> Result<(), InvalidPlayerWire> {
        request.validate()?;
        if viewer.is_nil() {
            return Err(InvalidPlayerWire);
        }
        let limit = usize::try_from(request.limit.get()).map_err(|_| InvalidPlayerWire)?;
        if (1..=100).contains(&limit)
            && self.options.len() <= limit
            && self.valid()
            && self.basis == request.basis
            && self.viewer_seat_id == viewer
            && self.decision == request.decision
            && self.query == request.query
            && self.cursor == request.cursor
            && (self.next_cursor.is_none() || self.options.len() == limit)
        {
            Ok(())
        } else {
            Err(InvalidPlayerWire)
        }
    }
}
