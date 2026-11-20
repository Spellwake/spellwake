//! Original public intents; syntax never resolves a privileged native identity.
use super::*;
use std::collections::BTreeSet;
/// Public rules limits bound a message, not the native engine's arithmetic.
pub const MAX_PLAYER_ITEMS: usize = 1000;
pub(super) fn unique<T: Ord>(items: impl Iterator<Item = T>) -> bool {
    let mut seen = BTreeSet::new();
    items.into_iter().all(|x| seen.insert(x))
}
pub(super) fn list<T>(xs: &[T]) -> bool {
    xs.len() <= MAX_PLAYER_ITEMS
}
pub(super) fn positive(n: &RulesCount) -> bool {
    n.as_rules_integer().is_positive()
}
/// Exact native mana vocabulary, independent of payment's payer/cost authority.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", try_from = "String")]
pub enum ManaColor {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colourless,
}
impl TryFrom<String> for ManaColor {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "white" => Ok(Self::White),
            "blue" => Ok(Self::Blue),
            "black" => Ok(Self::Black),
            "red" => Ok(Self::Red),
            "green" => Ok(Self::Green),
            "colourless" => Ok(Self::Colourless),
            _ => Err(InvalidPlayerWire),
        }
    }
}
/// One aggregate spend, not an invented native mana-unit identifier.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManaSpend {
    pub color: ManaColor,
    pub amount: RulesCount,
}
/// Empty allocation represents an explicit zero payment; source checks actual cost.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerPayment {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub spend: Vec<ManaSpend>,
}
impl PlayerPayment {
    pub(super) fn valid(&self) -> bool {
        self.spend.len() <= 6 && unique(self.spend.iter().map(|x| x.color))
    }
}
/// Opaque objects and stack entries stay distinct from public roster seats.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlayerTarget {
    Object {
        handle: PlayerHandle,
    },
    Stack {
        handle: PlayerHandle,
    },
    Seat {
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        seat_id: Uuid,
    },
}
impl PlayerTarget {
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::Seat { seat_id } => !seat_id.is_nil(),
            _ => true,
        }
    }
}
/// An allocation into an offered damage/prevention/distribution target.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetAllocation {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub target: PlayerTarget,
    pub amount: RulesCount,
}
/// One offered combat source and its explicit target/amount.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CombatAllocation {
    pub source: PlayerHandle,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub target: PlayerTarget,
    pub amount: RulesCount,
}
/// Attacker and its offered defending player/planeswalker/battle target.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttackerAssignment {
    pub attacker: PlayerHandle,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub defender: PlayerTarget,
}
/// Order is retained for the actual native blocker relationship.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlockerAssignment {
    pub blocker: PlayerHandle,
    pub attackers: Vec<PlayerHandle>,
}
/// Concrete response syntax. Frozen descriptors supply meaning and allowed domain.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlayerChoice {
    /// Complete private library partition. Both vectors preserve submitted order;
    /// `other` is Scry's bottom segment or Surveil's selected departures.
    LibraryPartition {
        top: Vec<PlayerHandle>,
        other: Vec<PlayerHandle>,
    },
    Option {
        option: PlayerHandle,
    },
    Objects {
        objects: Vec<PlayerHandle>,
    },
    Ordering {
        ordered: Vec<PlayerHandle>,
    },
    Number {
        value: RulesInteger,
    },
    Target {
        #[serde(deserialize_with = "crate::provider_json::object")]
        target: PlayerTarget,
    },
    Legend {
        conflict: PlayerHandle,
        keep: PlayerHandle,
    },
    Pay {
        #[serde(deserialize_with = "crate::provider_json::object")]
        payment: PlayerPayment,
    },
    Decline {},
    Prevention {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        allocations: Vec<PreventionAllocation>,
    },
    Distribution {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        allocations: Vec<TargetAllocation>,
    },
    Attackers {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        assignments: Vec<AttackerAssignment>,
    },
    Blockers {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        assignments: Vec<BlockerAssignment>,
    },
    CombatDamage {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        assignments: Vec<CombatAllocation>,
    },
}
impl PlayerChoice {
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::LibraryPartition { top, other } => {
                top.len().saturating_add(other.len()) <= MAX_PLAYER_ITEMS
                    && list(top)
                    && list(other)
                    && unique(top.iter().chain(other))
            }
            Self::Prevention { allocations } => {
                list(allocations) && unique(allocations.iter().map(|x| &x.attempt))
            }
            Self::Target { target } => target.valid(),
            Self::Objects { objects } | Self::Ordering { ordered: objects } => {
                list(objects) && unique(objects.iter())
            }
            Self::Pay { payment } => payment.valid(),
            Self::Distribution { allocations } => {
                list(allocations)
                    && unique(allocations.iter().map(|x| &x.target))
                    && allocations.iter().all(|x| x.target.valid())
            }
            Self::Attackers { assignments } => {
                list(assignments)
                    && unique(assignments.iter().map(|x| &x.attacker))
                    && assignments.iter().all(|x| x.defender.valid())
            }
            Self::Blockers { assignments } => {
                list(assignments)
                    && unique(assignments.iter().map(|x| &x.blocker))
                    && assignments.iter().all(|x| {
                        !x.attackers.is_empty() && list(&x.attackers) && unique(x.attackers.iter())
                    })
            }
            Self::CombatDamage { assignments } => {
                list(assignments)
                    && unique(assignments.iter().map(|x| (&x.source, &x.target)))
                    && assignments.iter().all(|x| x.target.valid())
            }
            _ => true,
        }
    }
}
/// Exact action variants preserve current native casting/activation distinctions.
/// Concede follows the currently supported native departure/conclusion boundaries.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlayerAction {
    CastUntargeted {
        source: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    CastTargeted {
        source: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::object")]
        target: PlayerTarget,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    ActivateAbility {
        source: PlayerHandle,
        ability: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::object")]
        target: PlayerTarget,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    ActivateMana {
        source: PlayerHandle,
        ability: PlayerHandle,
    },
    PlayLand {
        source: PlayerHandle,
    },
    Pass {},
    Choose {
        decision: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::object")]
        response: PlayerChoice,
    },
    Concede {},
}
impl PlayerAction {
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::CastUntargeted { payment, .. } => {
                payment.as_ref().is_none_or(PlayerPayment::valid)
            }
            Self::CastTargeted {
                payment, target, ..
            }
            | Self::ActivateAbility {
                payment, target, ..
            } => target.valid() && payment.as_ref().is_none_or(PlayerPayment::valid),
            Self::Choose { response, .. } => response.valid(),
            _ => true,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireCommand {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    request_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    basis: PlayerBasis,
    #[serde(deserialize_with = "crate::provider_json::object")]
    action: PlayerAction,
}
/// Original user intent only. Verified session/controller derives actor and seat.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "WireCommand")]
pub struct PlayerCommand {
    pub request_id: Uuid,
    pub basis: PlayerBasis,
    pub action: PlayerAction,
}
impl TryFrom<WireCommand> for PlayerCommand {
    type Error = InvalidPlayerWire;
    fn try_from(w: WireCommand) -> Result<Self, Self::Error> {
        if !w.action.valid() {
            return Err(InvalidPlayerWire);
        }
        Ok(Self {
            request_id: w.request_id,
            basis: w.basis,
            action: w.action,
        })
    }
}
impl PlayerCommand {
    /// Validate manually constructed intent and its expected basis before dispatch.
    /// # Errors
    /// Rejects nil IDs and invalid aggregate/duplicate/over-budget intent structure.
    pub fn validate(&self) -> Result<(), InvalidPlayerWire> {
        if self.request_id.is_nil()
            || self.basis.game_id.is_nil()
            || self.basis.context_id.is_nil()
            || !self.action.valid()
        {
            Err(InvalidPlayerWire)
        } else {
            Ok(())
        }
    }
}
