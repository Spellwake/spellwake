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
/// Ordered choice of the first or second side of one printed hybrid mana symbol.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerHybridSide {
    First,
    Second,
}

/// Face proposed for a cast from hand, graveyard, or an offered spell permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayerCastFace {
    SplitLeft,
    SplitRight,
    Disturb,
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
/// A preselected mana activation during an announced spell's payment.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerCastManaActivation {
    pub source: PlayerHandle,
    pub ability: PlayerHandle,
    pub color: Option<ManaColor>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::provider_json::optional_object"
    )]
    pub payment: Option<PlayerPayment>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::provider_json::optional_object"
    )]
    pub bundle: Option<PlayerPayment>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::provider_json::objects"
    )]
    pub cost_objects: Vec<PlayerHandle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_seat_id: Option<Uuid>,
}
/// One aggregate spend, not an invented native mana-unit identifier.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManaSpend {
    pub color: ManaColor,
    pub amount: RulesCount,
}
/// A publicly visible spending restriction carried by floating mana.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayerManaRestriction {
    PlaneswalkerSpell,
    ColorlessSpell,
    CommanderSpell,
    ColorlessEldraziSpellOrAbility,
    NoncreatureSpell,
    SpellWithFlashbackFromGraveyard,
    ArtifactSpellOrAbility,
    CreatureSpell,
    CreatureSpellOfType,
    CreatureSpellOfTypeUncounterable,
    CreatureSpellOrAbilityOfType,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerRestrictedManaSpend {
    pub color: ManaColor,
    pub restriction: PlayerManaRestriction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creature_type: Option<super::views::PlayerCreatureTypeLabel>,
    pub amount: RulesCount,
}
/// Empty allocation represents an explicit zero payment; source checks actual cost.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerPayment {
    /// Additional-effect mana selected for this payment, by the public lot identifier.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub from_lots: std::collections::BTreeMap<u64, RulesCount>,
    /// Optional source of a graveyard-casting permission when more than one applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graveyard_permission: Option<PlayerHandle>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub use_granted_flashback: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exile_cast_type: Option<PlayerCardType>,
    /// Battlefield permanent whose alternate casting cost is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternative_cost_source: Option<PlayerHandle>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub spend: Vec<ManaSpend>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::provider_json::objects"
    )]
    pub restricted_mana: Vec<PlayerRestrictedManaSpend>,
}
impl PlayerPayment {
    pub(super) fn valid(&self) -> bool {
        self.from_lots
            .iter()
            .all(|(&id, n)| id > 0 && n.as_rules_integer().is_positive())
            && (self.from_lots.is_empty() || !self.spend.is_empty())
            && self.exile_cast_type != Some(PlayerCardType::Land)
            && self.spend.len() <= 6
            && unique(self.spend.iter().map(|x| x.color))
            && list(&self.restricted_mana)
            && unique(
                self.restricted_mana
                    .iter()
                    .map(|x| (x.color, x.restriction, &x.creature_type)),
            )
            && self.restricted_mana.iter().all(|item| {
                item.amount.as_rules_integer().is_positive()
                    && (item.creature_type.is_none()
                        || matches!(
                            item.restriction,
                            PlayerManaRestriction::CreatureSpellOfType
                                | PlayerManaRestriction::CreatureSpellOfTypeUncounterable
                                | PlayerManaRestriction::CreatureSpellOrAbilityOfType
                        ))
                    && self
                        .spend
                        .iter()
                        .find(|entry| entry.color == item.color)
                        .is_some_and(|entry| {
                            entry.amount.as_rules_integer() >= item.amount.as_rules_integer()
                        })
            })
            && self.spend.iter().all(|entry| {
                self.restricted_mana
                    .iter()
                    .filter(|item| item.color == entry.color)
                    .fold(spellwake_contracts::RulesInteger::from(0), |sum, item| {
                        sum.plus(item.amount.as_rules_integer())
                    })
                    <= *entry.amount.as_rules_integer()
            })
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
    /// Matching labels identify a band in this declaration only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<u32>,
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
/// A color selected for one library card that will enter the battlefield.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSearchEntryColor {
    pub object: PlayerHandle,
    pub color: ManaColor,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSearchEntryReveal {
    pub object: PlayerHandle,
    pub card: Option<PlayerHandle>,
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
    CounterRemoval {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        counters: Vec<PlayerCounterQuantity>,
    },
    TriggerModes {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        modes: Vec<PlayerCastMode>,
    },
    MadnessCast {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        adventure: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<PlayerCastFace>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chosen_x: Option<RulesCount>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        target: Option<PlayerTarget>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        resources: Vec<PlayerCastResource>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        additional: Vec<PlayerAdditionalCost>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        mana_abilities: Vec<PlayerCastManaActivation>,
    },
    ImpulseCast {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        adventure: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<PlayerCastFace>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hybrid_choices: Vec<PlayerHybridSide>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        fixed_targets: Vec<PlayerTarget>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        permanent_targets: Vec<PlayerTarget>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        target_division: Vec<u16>,
        #[serde(default)]
        chosen_x: Option<RulesCount>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        modes: Vec<PlayerCastMode>,
        #[serde(default)]
        use_alternative: bool,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        target: Option<PlayerTarget>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        resources: Vec<PlayerCastResource>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        additional: Vec<PlayerAdditionalCost>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        mana_abilities: Vec<PlayerCastManaActivation>,
    },
    CascadeCast {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        adventure: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<PlayerCastFace>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chosen_x: Option<RulesCount>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        target: Option<PlayerTarget>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        resources: Vec<PlayerCastResource>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        additional: Vec<PlayerAdditionalCost>,
    },
    ReboundCast {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        adventure: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<PlayerCastFace>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chosen_x: Option<RulesCount>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        target: Option<PlayerTarget>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        resources: Vec<PlayerCastResource>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        additional: Vec<PlayerAdditionalCost>,
    },
    SuspendCast {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        adventure: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<PlayerCastFace>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chosen_x: Option<RulesCount>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        target: Option<PlayerTarget>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        resources: Vec<PlayerCastResource>,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        additional: Vec<PlayerAdditionalCost>,
    },
    Option {
        option: PlayerHandle,
    },
    Objects {
        objects: Vec<PlayerHandle>,
    },
    SearchWithEntryColors {
        objects: Vec<PlayerHandle>,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        colors: Vec<PlayerSearchEntryColor>,
    },
    SearchWithEntryChoices {
        objects: Vec<PlayerHandle>,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        colors: Vec<PlayerSearchEntryColor>,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        reveals: Vec<PlayerSearchEntryReveal>,
    },
    SearchHandGraveyard {
        hand: Option<PlayerHandle>,
        graveyard: Option<PlayerHandle>,
    },
    SearchToDestination {
        object: Option<PlayerHandle>,
        battlefield: bool,
    },
    Ordering {
        ordered: Vec<PlayerHandle>,
    },
    Number {
        value: RulesInteger,
    },
    TargetList {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
    },
    Target {
        #[serde(deserialize_with = "crate::provider_json::object")]
        target: PlayerTarget,
    },
    Legend {
        conflict: PlayerHandle,
        keep: PlayerHandle,
    },
    PayAttackCost {
        life_symbols: RulesCount,
        #[serde(deserialize_with = "crate::provider_json::object")]
        payment: PlayerPayment,
    },
    Pay {
        #[serde(deserialize_with = "crate::provider_json::object")]
        payment: PlayerPayment,
    },
    RedeclareKeepingMana {},
    RedeclareReversingMana {
        activations: Vec<u64>,
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
/// A named battlefield counter and an offered or chosen quantity.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerCounterQuantity {
    pub kind: PlayerCounterKind,
    pub count: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayerCounterKind {
    Defense,
    PlusOne,
    MinusOne,
    Page,
    Quest,
    Verse,
    Suspect,
    Charge,
    Storage,
    Lore,
    Loyalty,
    Time,
    Fade,
    Age,
    Indestructible,
    Corpse,
    Vow,
    Blessing,
    Despair,
    Hatchling,
    Study,
    Eyeball,
    Slime,
}

pub(super) fn counter_quantities(counters: &[PlayerCounterQuantity]) -> bool {
    list(counters)
        && unique(counters.iter().map(|entry| entry.kind))
        && counters.iter().all(|entry| entry.count > 0)
}

impl PlayerChoice {
    pub(super) fn cast_face(&self) -> Option<PlayerCastFace> {
        match self {
            Self::ImpulseCast { face, .. }
            | Self::MadnessCast { face, .. }
            | Self::CascadeCast { face, .. }
            | Self::SuspendCast { face, .. }
            | Self::ReboundCast { face, .. } => *face,
            _ => None,
        }
    }
    pub(super) fn casts_adventure(&self) -> bool {
        matches!(
            self,
            Self::ImpulseCast {
                adventure: true,
                ..
            } | Self::MadnessCast {
                adventure: true,
                ..
            } | Self::CascadeCast {
                adventure: true,
                ..
            } | Self::SuspendCast {
                adventure: true,
                ..
            } | Self::ReboundCast {
                adventure: true,
                ..
            }
        )
    }
    pub(super) fn valid(&self) -> bool {
        if self.cast_face().is_some()
            && (self.casts_adventure() || self.cast_face() == Some(PlayerCastFace::Disturb))
        {
            return false;
        }
        match self {
            Self::CounterRemoval { counters } => counter_quantities(counters),
            Self::Prevention { allocations } => {
                list(allocations) && unique(allocations.iter().map(|x| &x.attempt))
            }
            Self::MadnessCast {
                target,
                payment,
                resources,
                additional,
                mana_abilities,
                ..
            } => {
                valid_cast_payment(target, payment, resources, additional)
                    && list(mana_abilities)
                    && unique(
                        mana_abilities
                            .iter()
                            .map(|a| (&a.source, &a.ability, &a.cost_objects)),
                    )
                    && mana_abilities.iter().all(|a| {
                        a.payment.as_ref().is_none_or(PlayerPayment::valid)
                            && a.bundle.as_ref().is_none_or(PlayerPayment::valid)
                            && a.recipient_seat_id.is_none_or(|seat| !seat.is_nil())
                            && list(&a.cost_objects)
                            && unique(a.cost_objects.iter())
                    })
            }
            Self::ImpulseCast {
                hybrid_choices,
                target,
                payment,
                resources,
                additional,
                mana_abilities,
                modes,
                permanent_targets,
                fixed_targets,
                ..
            } => {
                list(hybrid_choices)
                    && list(fixed_targets)
                    && fixed_targets.iter().all(PlayerTarget::valid)
                    && list(permanent_targets)
                    && permanent_targets.iter().all(PlayerTarget::valid)
                    && unique(permanent_targets.iter())
                    && valid_cast_payment(target, payment, resources, additional)
                    && list(modes)
                    && modes.iter().all(PlayerCastMode::valid)
                    && list(mana_abilities)
                    && unique(
                        mana_abilities
                            .iter()
                            .map(|a| (&a.source, &a.ability, &a.cost_objects)),
                    )
                    && mana_abilities.iter().all(|a| {
                        a.payment.as_ref().is_none_or(PlayerPayment::valid)
                            && a.bundle.as_ref().is_none_or(PlayerPayment::valid)
                            && a.recipient_seat_id.is_none_or(|seat| !seat.is_nil())
                            && list(&a.cost_objects)
                            && unique(a.cost_objects.iter())
                    })
            }
            Self::CascadeCast {
                target,
                payment,
                resources,
                additional,
                ..
            }
            | Self::ReboundCast {
                target,
                payment,
                resources,
                additional,
                ..
            }
            | Self::SuspendCast {
                target,
                payment,
                resources,
                additional,
                ..
            } => valid_cast_payment(target, payment, resources, additional),
            Self::TriggerModes { modes } => {
                list(modes)
                    && unique(modes.iter().map(|mode| mode.mode))
                    && modes.iter().all(PlayerCastMode::valid)
            }
            Self::TargetList { targets } => {
                list(targets) && unique(targets.iter()) && targets.iter().all(PlayerTarget::valid)
            }
            Self::Target { target } => target.valid(),
            Self::Objects { objects } | Self::Ordering { ordered: objects } => {
                list(objects) && unique(objects.iter())
            }
            Self::SearchWithEntryColors { objects, colors } => {
                list(objects)
                    && unique(objects.iter())
                    && list(colors)
                    && unique(colors.iter().map(|entry| &entry.object))
            }
            Self::SearchWithEntryChoices {
                objects,
                colors,
                reveals,
            } => {
                list(objects)
                    && unique(objects.iter())
                    && list(colors)
                    && unique(colors.iter().map(|entry| &entry.object))
                    && list(reveals)
                    && unique(reveals.iter().map(|entry| &entry.object))
            }
            Self::SearchHandGraveyard { hand, graveyard } => hand
                .as_ref()
                .is_none_or(|value| Some(value) != graveyard.as_ref()),
            Self::SearchToDestination {
                object,
                battlefield,
            } => object.is_some() || !battlefield,
            Self::PayAttackCost { payment, .. } => {
                payment.valid()
                    && payment.restricted_mana.is_empty()
                    && payment.exile_cast_type.is_none()
                    && payment.graveyard_permission.is_none()
                    && payment.alternative_cost_source.is_none()
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
/// An announced printed mode and its target, when the mode requires one.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerCastMode {
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::provider_json::objects"
    )]
    pub targets: Vec<PlayerTarget>,
    pub mode: u8,
    #[serde(default, deserialize_with = "crate::provider_json::optional_object")]
    pub target: Option<PlayerTarget>,
}

impl PlayerCastMode {
    fn valid(&self) -> bool {
        self.target.as_ref().is_none_or(PlayerTarget::valid)
            && (self.target.is_none() || self.targets.is_empty())
            && list(&self.targets)
            && unique(self.targets.iter())
            && self.targets.iter().all(PlayerTarget::valid)
    }
}

/// Optional additional cost selected at announcement.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerAdditionalCost {
    pub index: u16,
    pub times: RulesCount,
}

/// Public characteristics used when casting a creature spell face down.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayerFaceDownKind {
    Manifest,
    Forest,
    Morph,
    Disguise,
}

/// Object payments expressed using the requesting seat’s own current handles.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PlayerCastResource {
    /// A revealed card with splice and the independent target for its text.
    Splice {
        source: PlayerHandle,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<PlayerTarget>,
    },
    WebSlinging {
        source: PlayerHandle,
    },
    Emerge {
        source: PlayerHandle,
    },
    Sneak {
        source: PlayerHandle,
    },
    Bargain {
        source: PlayerHandle,
    },
    Convoke {
        source: PlayerHandle,
        #[serde(deserialize_with = "Option::deserialize")]
        color: Option<ManaColor>,
    },
    Improvise {
        source: PlayerHandle,
    },
    Retrace {
        source: PlayerHandle,
    },
    JumpStart {
        source: PlayerHandle,
    },
    Escape {
        source: PlayerHandle,
    },
    DiscardForSpell {
        source: PlayerHandle,
    },
    SacrificeForSpell {
        source: PlayerHandle,
    },
    ExileGraveyardCreatureForSpell {
        source: PlayerHandle,
    },
    ExileGraveyardCardForSpell {
        source: PlayerHandle,
    },
    RevealColorlessCreature {
        source: PlayerHandle,
    },
    ExileHandForAlternative {
        source: PlayerHandle,
    },
    TapForAlternative {
        source: PlayerHandle,
    },
    TapForFlashback {
        source: PlayerHandle,
    },
    TapCreatureForSpell {
        source: PlayerHandle,
    },
    FlashbackSacrifice {
        source: PlayerHandle,
    },
    ExileCreatureForReduction {
        source: PlayerHandle,
    },
    Delve {
        source: PlayerHandle,
    },
}
impl PlayerCastResource {
    pub(crate) fn source(&self) -> &PlayerHandle {
        match self {
            Self::Splice { source, .. }
            | Self::Convoke { source, .. }
            | Self::WebSlinging { source }
            | Self::Emerge { source }
            | Self::Sneak { source }
            | Self::Bargain { source }
            | Self::Retrace { source }
            | Self::JumpStart { source }
            | Self::Improvise { source }
            | Self::Escape { source }
            | Self::RevealColorlessCreature { source }
            | Self::TapForFlashback { source }
            | Self::TapCreatureForSpell { source }
            | Self::ExileHandForAlternative { source }
            | Self::TapForAlternative { source }
            | Self::SacrificeForSpell { source }
            | Self::DiscardForSpell { source }
            | Self::FlashbackSacrifice { source }
            | Self::ExileGraveyardCreatureForSpell { source }
            | Self::ExileGraveyardCardForSpell { source }
            | Self::ExileCreatureForReduction { source }
            | Self::Delve { source } => source,
        }
    }
}
/// Exact action variants preserve current native casting/activation distinctions.
/// Concede remains an approved intent without a supported native constructor.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlayerAction {
    CastFaceDown {
        source: PlayerHandle,
        appearance: PlayerFaceDownKind,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    TurnFaceUp {
        source: PlayerHandle,
        ability: u16,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    CastWithResources {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        adventure: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<PlayerCastFace>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hybrid_choices: Vec<PlayerHybridSide>,
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::provider_json::objects"
        )]
        fixed_targets: Vec<PlayerTarget>,
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::provider_json::objects"
        )]
        permanent_targets: Vec<PlayerTarget>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        target_division: Vec<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chosen_x: Option<RulesCount>,
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::provider_json::objects"
        )]
        modes: Vec<PlayerCastMode>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        use_alternative: bool,
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::provider_json::objects"
        )]
        additional: Vec<PlayerAdditionalCost>,
        source: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        target: Option<PlayerTarget>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        resources: Vec<PlayerCastResource>,
    },
    Plot {
        source: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    Foretell {
        source: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    Suspend {
        source: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
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
    ActivateWithCreatures {
        source: PlayerHandle,
        ability: PlayerHandle,
        creatures: Vec<PlayerHandle>,
    },
    ActivateWithCostObjects {
        source: PlayerHandle,
        ability: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        target: Option<PlayerTarget>,
        objects: Vec<PlayerHandle>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    ActivateUntargetedAbility {
        source: PlayerHandle,
        ability: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    Cycle {
        source: PlayerHandle,
        ability: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    Ninjutsu {
        source: PlayerHandle,
        ability: PlayerHandle,
        attacker: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    Transmute {
        source: PlayerHandle,
        ability: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    ActivateEmblem {
        emblem_index: u32,
        creature: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    ActivatePermanentTargets {
        source: PlayerHandle,
        ability: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    ActivateUntargetedX {
        source: PlayerHandle,
        ability: PlayerHandle,
        chosen_x: RulesCount,
        #[serde(deserialize_with = "crate::provider_json::optional_object")]
        payment: Option<PlayerPayment>,
    },
    ActivateLoyaltyX {
        source: PlayerHandle,
        ability: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::object")]
        target: PlayerTarget,
        chosen_x: RulesCount,
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
    ActivateManaWithCostObjects {
        source: PlayerHandle,
        ability: PlayerHandle,
        objects: Vec<PlayerHandle>,
    },
    ActivateManaForPlayer {
        source: PlayerHandle,
        ability: PlayerHandle,
        recipient_seat_id: Uuid,
    },
    ActivateManaWithPayment {
        source: PlayerHandle,
        ability: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::object")]
        payment: PlayerPayment,
    },
    ActivateHybridMana {
        source: PlayerHandle,
        ability: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::object")]
        payment: PlayerPayment,
        #[serde(deserialize_with = "crate::provider_json::object")]
        bundle: PlayerPayment,
    },
    ActivateManaWithBundle {
        source: PlayerHandle,
        ability: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::object")]
        bundle: PlayerPayment,
    },
    PlayLand {
        source: PlayerHandle,
    },
    PlayLandWithEntryColor {
        source: PlayerHandle,
        color: ManaColor,
    },
    PlayLandWithEntryReveal {
        source: PlayerHandle,
        reveal: Option<PlayerHandle>,
    },
    Pass {},
    Choose {
        decision: PlayerHandle,
        #[serde(deserialize_with = "crate::provider_json::object")]
        response: PlayerChoice,
    },
    Concede {},
}
fn valid_cast_payment(
    target: &Option<PlayerTarget>,
    payment: &Option<PlayerPayment>,
    resources: &[PlayerCastResource],
    additional: &[PlayerAdditionalCost],
) -> bool {
    if !list(additional)
        || !unique(additional.iter().map(|s| s.index))
        || !additional.iter().all(|s| positive(&s.times))
    {
        return false;
    }
    list(resources)
        && unique(
            resources
                .iter()
                .map(|r| (matches!(r, PlayerCastResource::Splice { .. }), r.source())),
        )
        && resources
            .iter()
            .all(|r| !matches!(r, PlayerCastResource::Splice {target:Some(t), ..} if !t.valid()))
        && resources.iter().all(|r| {
            !matches!(
                r,
                PlayerCastResource::Convoke {
                    color: Some(ManaColor::Colourless),
                    ..
                }
            )
        })
        && target.as_ref().is_none_or(PlayerTarget::valid)
        && payment.as_ref().is_none_or(PlayerPayment::valid)
}
impl PlayerAction {
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::ActivateWithCreatures { creatures, .. } => {
                list(creatures) && unique(creatures.iter())
            }
            Self::ActivateManaForPlayer {
                recipient_seat_id, ..
            } => !recipient_seat_id.is_nil(),
            Self::ActivateManaWithCostObjects { objects, .. } => {
                list(objects) && unique(objects.iter())
            }
            Self::ActivateWithCostObjects {
                objects,
                target,
                payment,
                ..
            } => {
                list(objects)
                    && unique(objects.iter())
                    && target.as_ref().is_none_or(PlayerTarget::valid)
                    && payment.as_ref().is_none_or(PlayerPayment::valid)
            }
            Self::ActivateManaWithPayment { payment, .. } => payment.valid(),
            Self::ActivateHybridMana {
                payment, bundle, ..
            } => payment.valid() && bundle.valid(),
            Self::ActivateManaWithBundle { bundle, .. } => bundle.valid(),
            Self::PlayLandWithEntryColor { color, .. } => *color != ManaColor::Colourless,
            Self::CastWithResources {
                adventure: true,
                face: Some(_),
                ..
            } => false,
            Self::CastWithResources {
                hybrid_choices,
                fixed_targets,
                permanent_targets,
                modes,
                target,
                payment,
                resources,
                additional,
                ..
            } => {
                list(hybrid_choices)
                    && list(fixed_targets)
                    && fixed_targets.iter().all(PlayerTarget::valid)
                    && list(permanent_targets)
                    && permanent_targets.iter().all(PlayerTarget::valid)
                    && valid_cast_payment(target, payment, resources, additional)
                    && list(modes)
                    && modes.iter().all(PlayerCastMode::valid)
            }
            Self::Foretell { payment, .. }
            | Self::Suspend { payment, .. }
            | Self::CastFaceDown { payment, .. }
            | Self::TurnFaceUp { payment, .. }
            | Self::Plot { payment, .. }
            | Self::CastUntargeted { payment, .. }
            | Self::Ninjutsu { payment, .. }
            | Self::Cycle { payment, .. }
            | Self::ActivateUntargetedX { payment, .. }
            | Self::ActivateUntargetedAbility { payment, .. }
            | Self::Transmute { payment, .. } => payment.as_ref().is_none_or(PlayerPayment::valid),
            Self::ActivateEmblem { payment, .. } => {
                payment.as_ref().is_none_or(PlayerPayment::valid)
            }
            Self::ActivatePermanentTargets {
                targets, payment, ..
            } => {
                list(targets)
                    && targets.iter().all(PlayerTarget::valid)
                    && payment.as_ref().is_none_or(PlayerPayment::valid)
            }
            Self::CastTargeted {
                payment, target, ..
            }
            | Self::ActivateAbility {
                payment, target, ..
            }
            | Self::ActivateLoyaltyX {
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
