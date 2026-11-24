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
/// A public ballot cast earlier in this resolution.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerCardVote {
    pub seat_id: Uuid,
    pub object: PlayerHandle,
}
/// One offered legend conflict and the current visible handles within it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegendDomain {
    pub conflict: PlayerHandle,
    pub objects: Vec<PlayerHandle>,
}
/// One issued activation within an attack payment transaction.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerAttackMana {
    pub id: u64,
    pub source: Option<PlayerHandle>,
    pub reversible: bool,
    pub depends_on: Vec<u64>,
}
/// Printed classes of creatures that can participate in qualified bands.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayerBandQuality {
    LegendaryCreatures,
    CreaturesNamedWolvesOfTheHunt,
}

/// An offered attacker and its permitted defending targets.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttackerDomain {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub band_qualities: Vec<PlayerBandQuality>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bands_with_other: Vec<PlayerBandQuality>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub banding: bool,
    pub attacker: PlayerHandle,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub defenders: Vec<PlayerTarget>,
}
/// The price of declaring one attacker against this defender.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerAttackPrice {
    pub defender: PlayerTarget,
    pub generic: RulesCount,
    pub white_phyrexian: RulesCount,
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
/// Minimum number of distinct creatures assigned to an attacker if it is blocked.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlockMinimum {
    pub attacker: PlayerHandle,
    pub minimum: RulesCount,
}
/// Minimum damage to one blocker before any trample damage is assigned to the defender.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrampleMinimum {
    pub blocker: PlayerHandle,
    pub amount: RulesCount,
}
/// Exact source allocation total and any trample constraints.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CombatDamageDomain {
    pub source: PlayerHandle,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub targets: Vec<PlayerTarget>,
    pub total: RulesCount,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::provider_json::objects"
    )]
    pub trample_lethal: Vec<TrampleMinimum>,
}
impl CombatDamageDomain {
    fn valid_trample(&self) -> bool {
        self.trample_lethal.is_empty()
            || (list(&self.trample_lethal)
                && unique(self.trample_lethal.iter().map(|minimum| &minimum.blocker))
                && self
                    .targets
                    .iter()
                    .filter(|target| matches!(target, PlayerTarget::Seat { .. }))
                    .count()
                    == 1
                && self.targets.len() == self.trample_lethal.len() + 1
                && self.trample_lethal.iter().all(|minimum| {
                    self.targets.contains(&PlayerTarget::Object {
                        handle: minimum.blocker.clone(),
                    })
                }))
    }

    fn permits_trample(&self, assignments: &[CombatAllocation]) -> bool {
        self.trample_lethal.is_empty() || !assignments.iter().any(|row|
            row.source == self.source && matches!(row.target, PlayerTarget::Seat { .. }) && positive(&row.amount)
        ) || self.trample_lethal.iter().all(|minimum| {
            !positive(&minimum.amount) || assignments.iter().any(|row| {
                row.source == self.source && matches!(&row.target, PlayerTarget::Object { handle } if handle == &minimum.blocker)
                    && row.amount.as_rules_integer() >= minimum.amount.as_rules_integer()
            })
        })
    }
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
/// Available pre-entry colors for an eligible private library card.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSearchColorOffer {
    pub object: PlayerHandle,
    pub colors: Vec<ManaColor>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSearchRevealOffer {
    pub object: PlayerHandle,
    pub cards: Vec<PlayerHandle>,
}
/// How an ordered library-search reply assigns its selected cards.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayerSearchMode {
    #[default]
    Single,
    /// Choose at most one from a privately inspected top segment, without searching.
    LookTop,
    /// Choose whether the publicly revealed nonland card goes to its owner’s graveyard.
    Explore,
    /// Select the first of two public piles. The complement becomes the second pile.
    DividePile,
    /// First choice enters tapped; second choice goes to hand.
    BattlefieldThenHand,
    /// Choose the battlefield or hand for one result.
    BattlefieldOrHand,
    /// All choices enter simultaneously in the supplied relative timestamp order.
    BattlefieldTimestampOrder,
    /// Up to the supplied maximum move to hand. Legacy domains default to three.
    HandBatch,
    /// Selected cards move to linked exile.
    ExileBatch,
    /// Select distinct cards for hand and graveyard, taking both when available.
    HandGraveyard,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerSecondaryTargets {
    pub primary: PlayerTarget,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub targets: Vec<PlayerTarget>,
}

impl PlayerSearchMode {
    fn is_single(&self) -> bool {
        *self == Self::Single
    }
}

fn search_choice_permitted(
    eligible: &[PlayerOption],
    required: bool,
    mode: PlayerSearchMode,
    maximum: Option<usize>,
    offers: &[PlayerSearchColorOffer],
    objects: &[PlayerHandle],
    colors: &[PlayerSearchEntryColor],
) -> bool {
    if matches!(
        mode,
        PlayerSearchMode::HandGraveyard | PlayerSearchMode::BattlefieldOrHand
    ) {
        return false;
    }
    let max = match mode {
        PlayerSearchMode::Single | PlayerSearchMode::LookTop | PlayerSearchMode::Explore => 1,
        PlayerSearchMode::DividePile => 5,
        PlayerSearchMode::HandBatch | PlayerSearchMode::ExileBatch => maximum.unwrap_or(3),
        _ => 2,
    };
    if (required && objects.is_empty())
        || objects.len() > max
        || !unique(objects.iter())
        || !unique(colors.iter().map(|entry| &entry.object))
        || objects
            .iter()
            .any(|object| !eligible.iter().any(|option| &option.handle == object))
    {
        return false;
    }
    let entering = if mode == PlayerSearchMode::BattlefieldThenHand {
        &objects[..objects.len().min(1)]
    } else {
        objects
    };
    for choice in colors {
        if !entering.contains(&choice.object)
            || !offers
                .iter()
                .any(|offer| offer.object == choice.object && offer.colors.contains(&choice.color))
        {
            return false;
        }
    }
    true
}

fn search_reveals_permitted(
    mode: PlayerSearchMode,
    offers: &[PlayerSearchRevealOffer],
    objects: &[PlayerHandle],
    reveals: &[PlayerSearchEntryReveal],
) -> bool {
    let entering = if mode == PlayerSearchMode::BattlefieldThenHand {
        &objects[..objects.len().min(1)]
    } else {
        objects
    };
    unique(reveals.iter().map(|choice| &choice.object))
        && reveals.iter().all(|choice| {
            entering.contains(&choice.object)
                && offers.iter().any(|offer| {
                    offer.object == choice.object
                        && choice
                            .card
                            .as_ref()
                            .is_none_or(|card| offer.cards.contains(card))
                })
        })
}

/// Legal targets for one printed mode of an impulse-cast spell.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerImpulseMode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiple: Option<[usize; 2]>,
    pub index: u8,
    pub target_required: bool,
    pub targets: Vec<PlayerTarget>,
}
/// Printed alternative mana price for an impulse cast.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerImpulsePrice {
    pub generic: RulesCount,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub required: Vec<ManaSpend>,
}
/// Target bounds and legal permanents for a multi-target impulse cast.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerImpulsePermanentTargets {
    pub minimum: u16,
    pub maximum: u16,
    pub targets: Vec<PlayerTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub division_total: Option<u16>,
}
/// Cardinality and per-mode target domains for a modal impulse cast.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerImpulseModes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entwine_index: Option<u16>,
    pub minimum: u8,
    pub maximum: u8,
    pub allow_repeat: bool,
    pub modes: Vec<PlayerImpulseMode>,
}
/// First color is already counted in the base cost. A null alternate color costs two life.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerHybridSymbol {
    pub first: ManaColor,
    pub alternate_color: Option<ManaColor>,
}

/// Finite public choice structures.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlayerDomain {
    SplitCast {
        left: Option<Box<PlayerDomain>>,
        right: Box<PlayerDomain>,
    },
    AdventureCast {
        normal: Option<Box<PlayerDomain>>,
        adventure: Box<PlayerDomain>,
    },
    CounterRemoval {
        permanent: PlayerHandle,
        maximum: u8,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        available: Vec<PlayerCounterQuantity>,
    },
    TriggerModes {
        minimum: u8,
        maximum: u8,
        distinct_targets: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        allow_repeat: bool,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        modes: Vec<PlayerImpulseMode>,
    },
    MadnessCast {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        allows_x: bool,
        spell: PlayerHandle,
        generic: RulesCount,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        required: Vec<ManaSpend>,
        target_required: bool,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
    },
    ImpulseCast {
        spell: PlayerHandle,
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::provider_json::objects"
        )]
        hybrid_symbols: Vec<PlayerHybridSymbol>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        allows_x: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        allows_alternative: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        alternative_targetless: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alternative_price: Option<PlayerImpulsePrice>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modes: Option<PlayerImpulseModes>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permanent_targets: Option<PlayerImpulsePermanentTargets>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fixed_targets: Option<Vec<Vec<PlayerTarget>>>,
        generic: RulesCount,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        required: Vec<ManaSpend>,
        target_required: bool,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
    },
    CascadeCast {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        allows_x: bool,
        spell: PlayerHandle,
        target_required: bool,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
    },
    ReboundCast {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        allows_x: bool,
        spell: PlayerHandle,
        target_required: bool,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
    },
    SuspendedCast {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        allows_x: bool,
        spell: PlayerHandle,
        target_required: bool,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
    },
    /// All inspected names and opaque eligible choices, private to the searching seat.
    LibrarySearch {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        shared_land_type: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        forest_and_plains: bool,
        #[serde(default, skip_serializing_if = "PlayerSearchMode::is_single")]
        mode: PlayerSearchMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maximum: Option<usize>,
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::provider_json::objects"
        )]
        entry_color_offers: Vec<PlayerSearchColorOffer>,
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::provider_json::objects"
        )]
        entry_reveal_offers: Vec<PlayerSearchRevealOffer>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        required: bool,
        inspected: Vec<String>,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        eligible: Vec<PlayerOption>,
    },
    LibraryTop {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        inspected: Vec<PlayerOption>,
    },
    Options {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        options: Vec<PlayerOption>,
        #[serde(deserialize_with = "nullable_handle")]
        query: Option<PlayerHandle>,
    },
    Objects {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        prior_votes: Vec<PlayerCardVote>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        prior_choices: Vec<PlayerCardVote>,
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
        /// The number allocates this much mana to the first color. The rest goes to the second.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mana_split: Option<[ManaColor; 2]>,
    },
    TargetList {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
        minimum: RulesCount,
        maximum: RulesCount,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        secondary_targets: Vec<PlayerSecondaryTargets>,
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
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        up_to: bool,
    },
    Distribution {
        #[serde(deserialize_with = "crate::provider_json::objects")]
        targets: Vec<PlayerTarget>,
        required: RulesCount,
        minimum_per_target: RulesCount,
    },
    AttackCost {
        generic: RulesCount,
        white_phyrexian: RulesCount,
        #[serde(default, deserialize_with = "crate::provider_json::objects")]
        activations: Vec<PlayerAttackMana>,
    },
    Attackers {
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::identifiers::uuid_list"
        )]
        capped_defenders: Vec<Uuid>,
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::provider_json::objects"
        )]
        prices: Vec<PlayerAttackPrice>,
        /// Under a one-attacker limit, any one of the required attackers satisfies the requirement.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        single_attacker: bool,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        choices: Vec<AttackerDomain>,
        required: Vec<PlayerHandle>,
    },
    Blockers {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        defending_seat_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        single_blocker: bool,
        #[serde(deserialize_with = "crate::provider_json::objects")]
        choices: Vec<BlockerDomain>,
        required: Vec<PlayerHandle>,
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::provider_json::objects"
        )]
        minimum_if_blocked: Vec<BlockMinimum>,
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
    fn spell_handle(&self) -> Option<&PlayerHandle> {
        match self {
            Self::ImpulseCast { spell, .. }
            | Self::MadnessCast { spell, .. }
            | Self::CascadeCast { spell, .. }
            | Self::ReboundCast { spell, .. }
            | Self::SuspendedCast { spell, .. } => Some(spell),
            _ => None,
        }
    }
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::SplitCast {
                left: normal,
                right: adventure,
            }
            | Self::AdventureCast { normal, adventure } => {
                adventure.spell_handle().is_some()
                    && adventure.valid()
                    && normal.as_ref().is_none_or(|face| {
                        face.spell_handle() == adventure.spell_handle()
                            && std::mem::discriminant(face.as_ref())
                                == std::mem::discriminant(adventure.as_ref())
                            && face.valid()
                    })
            }
            Self::TriggerModes {
                minimum,
                maximum,
                modes,
                ..
            } => {
                minimum <= maximum
                    && usize::from(*maximum) <= modes.len()
                    && list(modes)
                    && unique(modes.iter().map(|mode| mode.index))
                    && modes.iter().all(|mode| {
                        targets(&mode.targets)
                            && match mode.multiple {
                                Some([minimum, maximum]) => {
                                    mode.target_required
                                        && minimum <= maximum
                                        && maximum <= mode.targets.len()
                                }
                                None => mode.target_required != mode.targets.is_empty(),
                            }
                    })
            }
            Self::MadnessCast {
                targets: xs,
                target_required,
                required,
                ..
            } => targets(xs) && (*target_required != xs.is_empty()) && mana(required),
            Self::ImpulseCast {
                hybrid_symbols,
                targets: xs,
                target_required,
                required,
                modes,
                permanent_targets,
                fixed_targets,
                alternative_targetless,
                alternative_price,
                allows_alternative,
                ..
            } => {
                list(hybrid_symbols)
                    && hybrid_symbols.iter().all(|s| {
                        s.first != ManaColor::Colourless
                            && s.alternate_color.is_none_or(|color| {
                                color != ManaColor::Colourless && color != s.first
                            })
                    })
                    && targets(xs)
                    && ((!*target_required && xs.is_empty())
                        || (*target_required && (!xs.is_empty() || *alternative_targetless)))
                    && (!*alternative_targetless || *allows_alternative)
                    && (alternative_price.is_some() == *alternative_targetless)
                    && alternative_price
                        .as_ref()
                        .is_none_or(|price| mana(&price.required))
                    && mana(required)
                    && (modes.is_none() || permanent_targets.is_none())
                    && fixed_targets.as_ref().is_none_or(|clauses| {
                        modes.is_none()
                            && permanent_targets.is_none()
                            && !*target_required
                            && xs.is_empty()
                            && list(clauses)
                            && clauses.iter().all(|values| targets(values))
                    })
                    && modes.as_ref().is_none_or(|spec| {
                        !*target_required
                            && xs.is_empty()
                            && spec.minimum <= spec.maximum
                            && list(&spec.modes)
                            && unique(spec.modes.iter().map(|m| m.index))
                            && spec.modes.iter().all(|m| {
                                targets(&m.targets) && (m.target_required || m.targets.is_empty())
                            })
                    })
                    && permanent_targets.as_ref().is_none_or(|spec| {
                        !*target_required
                            && xs.is_empty()
                            && spec.minimum <= spec.maximum
                            && targets(&spec.targets)
                    })
            }
            Self::CascadeCast {
                targets: xs,
                target_required,
                ..
            }
            | Self::ReboundCast {
                targets: xs,
                target_required,
                ..
            }
            | Self::SuspendedCast {
                targets: xs,
                target_required,
                ..
            } => targets(xs) && (*target_required != xs.is_empty()),
            Self::LibrarySearch {
                inspected,
                eligible,
                entry_color_offers,
                entry_reveal_offers,
                ..
            } => {
                list(entry_color_offers)
                    && unique(entry_color_offers.iter().map(|offer| &offer.object))
                    && list(entry_reveal_offers)
                    && unique(entry_reveal_offers.iter().map(|offer| &offer.object))
                    && entry_reveal_offers.iter().all(|offer| {
                        eligible.iter().any(|option| option.handle == offer.object)
                            && list(&offer.cards)
                            && unique(offer.cards.iter())
                    })
                    && entry_color_offers.iter().all(|offer| {
                        eligible.iter().any(|option| option.handle == offer.object)
                            && !offer.colors.is_empty()
                            && unique(offer.colors.iter())
                            && !offer.colors.contains(&ManaColor::Colourless)
                    })
                    && inspected.len() <= MAX_PLAYER_ITEMS
                    && inspected
                        .iter()
                        .all(|name| !name.is_empty() && display_text(name, 1024))
                    && options(eligible)
                    && eligible
                        .iter()
                        .all(|option| inspected.contains(&option.label))
            }
            Self::LibraryTop { inspected } => options(inspected) && !inspected.is_empty(),
            Self::Options { options: xs, query } => {
                options(xs) && (!xs.is_empty() || query.is_some())
            }
            Self::Objects {
                prior_choices,
                prior_votes,
                objects,
                minimum,
                maximum,
            } => {
                list(prior_choices)
                    && unique(prior_choices.iter().map(|row| row.seat_id))
                    && prior_choices.iter().all(|row| !row.seat_id.is_nil())
                    && prior_votes
                        .iter()
                        .all(|vote| objects.contains(&vote.object))
                    && handles(objects)
                    && minimum.as_rules_integer() <= maximum.as_rules_integer()
                    && maximum.as_rules_integer() <= &count(objects.len())
            }
            Self::Ordering { objects, labels } => {
                handles(objects)
                    && options(labels)
                    && labels.len() == objects.len()
                    && labels.iter().all(|entry| objects.contains(&entry.handle))
            }
            Self::CounterRemoval {
                maximum, available, ..
            } => {
                *maximum > 0
                    && !available.is_empty()
                    && counter_quantities(available)
                    && available.iter().all(|entry| entry.count <= *maximum)
            }
            Self::Number {
                minimum,
                maximum,
                mana_split,
            } => {
                minimum <= maximum
                    && mana_split.is_none_or(|[first, second]| {
                        first != second && *minimum == 0.into() && maximum.is_positive()
                    })
            }
            Self::TargetList {
                targets: xs,
                minimum,
                maximum,
                secondary_targets,
            } => {
                targets(xs)
                    && minimum.as_rules_integer() <= maximum.as_rules_integer()
                    && if secondary_targets.is_empty() {
                        maximum.as_rules_integer() <= &count(xs.len())
                    } else {
                        minimum.as_rules_integer() == &1.into()
                            && maximum.as_rules_integer() <= &2.into()
                            && unique(secondary_targets.iter().map(|group| &group.primary))
                            && secondary_targets
                                .iter()
                                .all(|group| xs.contains(&group.primary) && targets(&group.targets))
                    }
            }
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
            Self::AttackCost {
                generic,
                white_phyrexian,
                activations,
            } => {
                (positive(generic) || positive(white_phyrexian))
                    && list(activations)
                    && activations.iter().enumerate().all(|(i, a)| {
                        usize::try_from(a.id) == Ok(i)
                            && unique(a.depends_on.iter())
                            && a.depends_on.iter().all(|previous| *previous < a.id)
                    })
            }
            Self::Attackers {
                choices,
                required,
                prices,
                capped_defenders,
                ..
            } => {
                list(capped_defenders)
                    && unique(capped_defenders.iter())
                    && list(prices)
                    && unique(prices.iter().map(|price| &price.defender))
                    && prices.iter().all(|price| {
                        (positive(&price.generic) || positive(&price.white_phyrexian))
                            && choices
                                .iter()
                                .any(|option| option.defenders.contains(&price.defender))
                    })
                    && list(choices)
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
            Self::Blockers {
                choices,
                required,
                minimum_if_blocked,
                ..
            } => {
                list(choices)
                    && unique(choices.iter().map(|x| &x.blocker))
                    && list(minimum_if_blocked)
                    && unique(minimum_if_blocked.iter().map(|group| &group.attacker))
                    && minimum_if_blocked.iter().all(|group| {
                        let available = choices
                            .iter()
                            .filter(|row| {
                                positive(&row.maximum) && row.attackers.contains(&group.attacker)
                            })
                            .count();
                        group.minimum.as_rules_integer() > &count(1)
                            && group.minimum.as_rules_integer() <= &count(available)
                    })
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
                        targets(&x.targets)
                            && (!positive(&x.total) || !x.targets.is_empty())
                            && x.valid_trample()
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
            Self::TriggerModes { .. } => kind == K::Modes,
            Self::SplitCast { .. }
            | Self::AdventureCast { .. }
            | Self::ImpulseCast { .. }
            | Self::MadnessCast { .. }
            | Self::CascadeCast { .. }
            | Self::ReboundCast { .. }
            | Self::SuspendedCast { .. } => kind == K::OptionalEffect,
            Self::LibrarySearch { .. } => kind == K::LibrarySearch,
            Self::LibraryTop { .. } => matches!(kind, K::Scry | K::Surveil),
            Self::Options { .. } => matches!(
                kind,
                K::PlaneswalkerType
                    | K::AttackDirection
                    | K::OptionalEffect
                    | K::ZoneReplacement
                    | K::Mulligan
                    | K::CopyTargets
                    | K::DamageOrder
                    | K::ManaColor
                    | K::CreatureType
                    | K::Modes
                    | K::CardName
            ),
            Self::TargetList { .. } => matches!(
                kind,
                K::MyriadDefender
                    | K::Proliferate
                    | K::TriggerTarget
                    | K::ReselectAttackDefender
                    | K::EntryPlayers
            ),
            Self::Objects {
                minimum,
                maximum,
                objects,
                ..
            } => {
                kind == K::TriggerTarget
                    || kind == K::ReturnCommanders
                        && !positive(minimum)
                        && !objects.is_empty()
                        && maximum.as_rules_integer() == &count(objects.len())
                    || kind == K::Discard
                    || kind == K::EntryReveal
                        && !positive(minimum)
                        && maximum.as_rules_integer() == &count(objects.len().min(1))
                    || kind == K::Bottom && minimum == maximum
                    || kind == K::SelectResolution && positive(maximum)
            }
            Self::Ordering { objects, .. } => {
                matches!(
                    kind,
                    K::TransformOrder
                        | K::TimestampOrder
                        | K::OrderGraveyard
                        | K::OrderTriggers
                        | K::LinkedReturnOrder
                ) || kind == K::OrderResolutionGraveyard && objects.len() > 1
            }
            Self::Number { .. } => kind == K::Number,
            Self::CounterRemoval { .. } => kind == K::CounterRemoval,
            Self::Targets { .. } => matches!(
                kind,
                K::TriggerTarget
                    | K::TokenDefender
                    | K::OpponentChoice
                    | K::ReplacementChooser
                    | K::DamageRecipient
                    | K::LinkedAuraChoice
            ),
            Self::Legend { .. } => kind == K::KeepLegend,
            Self::Payment { .. } => matches!(kind, K::ResolutionPayment | K::CastRecovery),
            Self::Distribution { .. } => kind == K::Distribution,
            Self::AttackCost { .. } | Self::Attackers { .. } => kind == K::DeclareAttackers,
            Self::Blockers { .. } => kind == K::DeclareBlockers,
            Self::CombatDamage { .. } => kind == K::AssignCombatDamage,
            Self::Prevention { .. } => kind == K::AllocatePrevention,
        }
    }
    fn allows(&self, response: &PlayerChoice, extra_options: &[PlayerHandle]) -> bool {
        match (self, response) {
            (Self::SplitCast { left, right }, _) => {
                if matches!(response, PlayerChoice::Decline {}) {
                    return true;
                }
                let face = if response.cast_face() == Some(PlayerCastFace::SplitRight) {
                    Some(right.as_ref())
                } else {
                    left.as_deref()
                };
                face.is_some_and(|face| face.allows(response, extra_options))
            }
            (Self::AdventureCast { normal, adventure }, _) => {
                if matches!(response, PlayerChoice::Decline {}) {
                    return true;
                }
                let face = if response.casts_adventure() {
                    Some(adventure.as_ref())
                } else {
                    normal.as_deref()
                };
                face.is_some_and(|face| face.allows(response, extra_options))
            }
            (
                Self::TriggerModes {
                    minimum,
                    maximum,
                    distinct_targets,
                    allow_repeat,
                    modes,
                },
                PlayerChoice::TriggerModes { modes: chosen },
            ) => {
                (usize::from(*minimum)..=usize::from(*maximum)).contains(&chosen.len())
                    && (*allow_repeat || unique(chosen.iter().map(|choice| choice.mode)))
                    && (!*distinct_targets
                        || unique(
                            chosen
                                .iter()
                                .flat_map(|choice| choice.target.iter().chain(&choice.targets)),
                        ))
                    && chosen.iter().all(|choice| {
                        modes
                            .iter()
                            .find(|mode| mode.index == choice.mode)
                            .is_some_and(|mode| {
                                if let Some([minimum, maximum]) = mode.multiple {
                                    choice.target.is_none()
                                        && (minimum..=maximum).contains(&choice.targets.len())
                                        && unique(choice.targets.iter())
                                        && choice
                                            .targets
                                            .iter()
                                            .all(|target| mode.targets.contains(target))
                                } else {
                                    choice.targets.is_empty()
                                        && match &choice.target {
                                            Some(target) => {
                                                mode.target_required
                                                    && mode.targets.contains(target)
                                            }
                                            None => !mode.target_required,
                                        }
                                }
                            })
                    })
            }
            (
                Self::ImpulseCast {
                    hybrid_symbols,
                    target_required,
                    targets,
                    allows_x,
                    allows_alternative,
                    modes: domain,
                    permanent_targets: permanent_domain,
                    fixed_targets: fixed_domain,
                    alternative_targetless,
                    ..
                },
                PlayerChoice::ImpulseCast {
                    hybrid_choices,
                    target,
                    chosen_x,
                    use_alternative,
                    modes,
                    permanent_targets,
                    target_division,
                    fixed_targets,
                    additional,
                    ..
                },
            ) => {
                let division_total = permanent_domain
                    .as_ref()
                    .and_then(|domain| domain.division_total);
                if let Some(total) = division_total {
                    if target_division.len() != permanent_targets.len()
                        || target_division.contains(&0)
                        || target_division
                            .iter()
                            .map(|share| u64::from(*share))
                            .sum::<u64>()
                            != u64::from(total)
                    {
                        return false;
                    }
                } else if !target_division.is_empty() {
                    return false;
                }
                if !hybrid_choices.is_empty()
                    && (*use_alternative || hybrid_choices.len() != hybrid_symbols.len())
                {
                    return false;
                }
                if chosen_x.is_some() && !*allows_x || *use_alternative && !*allows_alternative {
                    return false;
                }
                if *use_alternative && *alternative_targetless {
                    return target.is_none()
                        && modes.is_empty()
                        && permanent_targets.is_empty()
                        && fixed_targets.is_empty();
                }
                if let Some(clauses) = fixed_domain {
                    return target.is_none()
                        && modes.is_empty()
                        && permanent_targets.is_empty()
                        && fixed_targets.len() == clauses.len()
                        && fixed_targets
                            .iter()
                            .zip(clauses)
                            .all(|(chosen, legal)| legal.contains(chosen));
                }
                if !fixed_targets.is_empty() {
                    return false;
                }
                if let Some(spec) = permanent_domain {
                    return target.is_none()
                        && modes.is_empty()
                        && permanent_targets.len() >= usize::from(spec.minimum)
                        && permanent_targets.len() <= usize::from(spec.maximum)
                        && unique(permanent_targets.iter())
                        && permanent_targets
                            .iter()
                            .all(|target| spec.targets.contains(target));
                }
                if !permanent_targets.is_empty() {
                    return false;
                }
                if let Some(spec) = domain {
                    let entwined = additional.iter().any(|payment| {
                        Some(payment.index) == spec.entwine_index
                            && payment.times.as_rules_integer() == &1.into()
                    });
                    let (min, max) = if entwined {
                        (spec.modes.len(), spec.modes.len())
                    } else {
                        (usize::from(spec.minimum), usize::from(spec.maximum))
                    };
                    if target.is_some()
                        || modes.len() < min
                        || modes.len() > max
                        || !spec.allow_repeat && !unique(modes.iter().map(|mode| mode.mode))
                    {
                        return false;
                    }
                    modes.iter().all(|choice| {
                        spec.modes
                            .iter()
                            .find(|mode| mode.index == choice.mode)
                            .is_some_and(|mode| {
                                if let Some([min, max]) = mode.multiple {
                                    choice.target.is_none()
                                        && (min..=max).contains(&choice.targets.len())
                                        && unique(choice.targets.iter())
                                        && choice
                                            .targets
                                            .iter()
                                            .all(|target| mode.targets.contains(target))
                                } else {
                                    choice.targets.is_empty()
                                        && match &choice.target {
                                            Some(target) => {
                                                mode.target_required
                                                    && mode.targets.contains(target)
                                            }
                                            None => !mode.target_required,
                                        }
                                }
                            })
                    })
                } else {
                    modes.is_empty()
                        && match target {
                            Some(value) => *target_required && targets.contains(value),
                            None => !*target_required,
                        }
                }
            }
            (
                Self::SuspendedCast {
                    target_required,
                    allows_x,
                    targets,
                    ..
                },
                PlayerChoice::SuspendCast {
                    target, chosen_x, ..
                },
            )
            | (
                Self::CascadeCast {
                    target_required,
                    targets,
                    allows_x,
                    ..
                },
                PlayerChoice::CascadeCast {
                    target, chosen_x, ..
                },
            )
            | (
                Self::MadnessCast {
                    target_required,
                    targets,
                    allows_x,
                    ..
                },
                PlayerChoice::MadnessCast {
                    target, chosen_x, ..
                },
            )
            | (
                Self::ReboundCast {
                    target_required,
                    allows_x,
                    targets,
                    ..
                },
                PlayerChoice::ReboundCast {
                    target, chosen_x, ..
                },
            ) => {
                (chosen_x.is_none() || *allows_x)
                    && match target {
                        Some(t) => *target_required && targets.contains(t),
                        None => !*target_required,
                    }
            }
            (
                Self::ImpulseCast { .. }
                | Self::MadnessCast { .. }
                | Self::CascadeCast { .. }
                | Self::SuspendedCast { .. }
                | Self::ReboundCast { .. },
                PlayerChoice::Decline {},
            ) => true,
            (
                Self::LibrarySearch {
                    eligible,
                    required,
                    mode,
                    maximum,
                    entry_color_offers,
                    entry_reveal_offers,
                    ..
                },
                PlayerChoice::Objects { objects },
            ) => {
                search_choice_permitted(
                    eligible,
                    *required,
                    *mode,
                    *maximum,
                    entry_color_offers,
                    objects,
                    &[],
                ) && search_reveals_permitted(*mode, entry_reveal_offers, objects, &[])
            }
            (
                Self::LibrarySearch {
                    eligible,
                    required,
                    mode,
                    maximum,
                    entry_color_offers,
                    entry_reveal_offers,
                    ..
                },
                PlayerChoice::SearchWithEntryColors { objects, colors },
            ) => {
                search_choice_permitted(
                    eligible,
                    *required,
                    *mode,
                    *maximum,
                    entry_color_offers,
                    objects,
                    colors,
                ) && search_reveals_permitted(*mode, entry_reveal_offers, objects, &[])
            }
            (
                Self::LibrarySearch {
                    eligible,
                    required,
                    mode,
                    maximum,
                    entry_color_offers,
                    entry_reveal_offers,
                    ..
                },
                PlayerChoice::SearchWithEntryChoices {
                    objects,
                    colors,
                    reveals,
                },
            ) => {
                search_choice_permitted(
                    eligible,
                    *required,
                    *mode,
                    *maximum,
                    entry_color_offers,
                    objects,
                    colors,
                ) && search_reveals_permitted(*mode, entry_reveal_offers, objects, reveals)
            }
            (
                Self::LibrarySearch {
                    eligible,
                    mode: PlayerSearchMode::HandGraveyard,
                    ..
                },
                PlayerChoice::SearchHandGraveyard { hand, graveyard },
            ) => {
                let selected = hand.iter().chain(graveyard.iter()).collect::<Vec<_>>();
                selected.len() == eligible.len().min(2)
                    && unique(selected.iter().copied())
                    && (selected.is_empty() || hand.is_some())
                    && selected
                        .iter()
                        .all(|id| eligible.iter().any(|option| &option.handle == *id))
            }
            (
                Self::LibrarySearch {
                    eligible,
                    required,
                    mode: PlayerSearchMode::BattlefieldOrHand,
                    ..
                },
                PlayerChoice::SearchToDestination {
                    object,
                    battlefield,
                },
            ) => object.as_ref().map_or(!required && !battlefield, |id| {
                eligible.iter().any(|option| &option.handle == id)
            }),
            (Self::LibraryTop { inspected }, PlayerChoice::Scry { top, bottom }) => {
                top.len() + bottom.len() == inspected.len()
                    && top
                        .iter()
                        .chain(bottom.iter())
                        .all(|id| inspected.iter().any(|option| &option.handle == id))
            }
            (Self::LibraryTop { inspected }, PlayerChoice::Surveil { top, graveyard }) => {
                top.len() + graveyard.len() == inspected.len()
                    && top
                        .iter()
                        .chain(graveyard.iter())
                        .all(|id| inspected.iter().any(|option| &option.handle == id))
            }
            (Self::Options { options, query }, PlayerChoice::Option { option }) => {
                options.iter().any(|x| x.handle == *option)
                    || (query.is_some() && extra_options.contains(option))
            }
            (
                Self::Objects {
                    objects,
                    minimum,
                    maximum,
                    ..
                },
                PlayerChoice::Objects { objects: selected },
            ) => {
                within(selected.len(), minimum, maximum)
                    && selected.iter().all(|x| objects.contains(x))
            }
            (Self::Ordering { objects, .. }, PlayerChoice::Ordering { ordered }) => {
                objects.len() == ordered.len() && ordered.iter().all(|x| objects.contains(x))
            }
            (
                Self::CounterRemoval {
                    maximum, available, ..
                },
                PlayerChoice::CounterRemoval { counters },
            ) => {
                counters
                    .iter()
                    .map(|entry| u32::from(entry.count))
                    .sum::<u32>()
                    <= u32::from(*maximum)
                    && counters.iter().all(|entry| {
                        available.iter().any(|offered| {
                            offered.kind == entry.kind && offered.count >= entry.count
                        })
                    })
            }
            (
                Self::Number {
                    minimum, maximum, ..
                },
                PlayerChoice::Number { value },
            ) => minimum <= value && value <= maximum,
            (
                Self::TargetList {
                    targets,
                    minimum,
                    maximum,
                    secondary_targets,
                },
                PlayerChoice::TargetList { targets: selected },
            ) => {
                within(selected.len(), minimum, maximum)
                    && if secondary_targets.is_empty() {
                        unique(selected.iter())
                            && selected.iter().all(|value| targets.contains(value))
                    } else {
                        selected.split_first().is_some_and(|(first, rest)| {
                            targets.contains(first)
                                && (rest.is_empty()
                                    || secondary_targets.iter().any(|group| {
                                        &group.primary == first
                                            && rest
                                                .iter()
                                                .all(|target| group.targets.contains(target))
                                    }))
                        })
                    }
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
                    up_to,
                    ..
                },
                PlayerChoice::Pay { payment },
            ) => {
                let offered = sum(payment.spend.iter().map(|x| &x.amount));
                (if *up_to {
                    offered <= *total.as_rules_integer()
                } else {
                    offered == *total.as_rules_integer()
                }) && minimum_spend
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
            (
                Self::AttackCost { .. },
                PlayerChoice::Decline {} | PlayerChoice::RedeclareKeepingMana {},
            ) => true,
            (
                Self::AttackCost { activations, .. },
                PlayerChoice::RedeclareReversingMana {
                    activations: selected,
                },
            ) => {
                unique(selected.iter())
                    && selected
                        .iter()
                        .all(|id| activations.iter().any(|a| a.id == *id && a.reversible))
                    && activations.iter().all(|a| {
                        selected.contains(&a.id)
                            || a.depends_on.iter().all(|id| !selected.contains(id))
                    })
            }
            (
                Self::AttackCost {
                    generic,
                    white_phyrexian,
                    ..
                },
                PlayerChoice::PayAttackCost {
                    life_symbols,
                    payment,
                },
            ) => {
                let white = white_phyrexian
                    .as_rules_integer()
                    .minus(life_symbols.as_rules_integer());
                white >= 0.into()
                    && spend(&payment.spend, ManaColor::White) >= white
                    && sum(payment.spend.iter().map(|x| &x.amount))
                        == generic.as_rules_integer().plus(&white)
            }
            (
                Self::Attackers {
                    choices,
                    required,
                    single_attacker,
                    capped_defenders,
                    ..
                },
                PlayerChoice::Attackers { assignments },
            ) => {
                (if *single_attacker {
                    assignments.len() <= 1
                        && (required.is_empty()
                            || assignments.iter().any(|a| required.contains(&a.attacker)))
                } else {
                    required
                        .iter()
                        .all(|r| assignments.iter().any(|a| a.attacker == *r))
                }) && assignments
                    .iter()
                    .filter_map(|row| row.band)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .all(|band| {
                        let members: Vec<_> = assignments
                            .iter()
                            .filter(|row| row.band == Some(band))
                            .collect();
                        let banders = members
                            .iter()
                            .filter(|member| {
                                choices.iter().any(|choice| {
                                    choice.attacker == member.attacker && choice.banding
                                })
                            })
                            .count();
                        let matching = [
                            PlayerBandQuality::LegendaryCreatures,
                            PlayerBandQuality::CreaturesNamedWolvesOfTheHunt,
                        ]
                        .iter()
                        .any(|quality| {
                            members.iter().all(|m| {
                                choices.iter().any(|c| {
                                    c.attacker == m.attacker && c.band_qualities.contains(quality)
                                })
                            }) && members.iter().any(|m| {
                                choices.iter().any(|c| {
                                    c.attacker == m.attacker && c.bands_with_other.contains(quality)
                                })
                            })
                        });
                        ((banders > 0 && members.len() - banders <= 1) || matching)
                            && members
                                .iter()
                                .all(|row| row.defender == members[0].defender)
                    })
                    && capped_defenders.iter().all(|id| {
                        assignments
                            .iter()
                            .filter(|row| row.defender == PlayerTarget::Seat { seat_id: *id })
                            .take(2)
                            .count()
                            <= 1
                    })
                    && assignments.iter().all(|a| {
                        choices
                            .iter()
                            .any(|c| c.attacker == a.attacker && c.defenders.contains(&a.defender))
                    })
            }
            (
                Self::Blockers {
                    choices,
                    required,
                    minimum_if_blocked,
                    single_blocker,
                    ..
                },
                PlayerChoice::Blockers { assignments },
            ) => {
                (!single_blocker
                    || assignments
                        .iter()
                        .filter(|row| !row.attackers.is_empty())
                        .count()
                        <= 1)
                    && required
                        .iter()
                        .all(|r| assignments.iter().any(|a| a.blocker == *r))
                    && minimum_if_blocked.iter().all(|group| {
                        let assigned = assignments
                            .iter()
                            .filter(|row| row.attackers.contains(&group.attacker))
                            .count();
                        assigned == 0 || count(assigned) >= *group.minimum.as_rules_integer()
                    })
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
                        && s.permits_trample(assignments)
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
    /// Safe description of the specific object for an entry decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_label: Option<String>,
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
        let valid = self
            .subject_label
            .as_ref()
            .is_none_or(|label| !label.is_empty() && display_text(label, 1024))
            && (1..=6).contains(&roster.len())
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
                DecisionKind::LibrarySearch
                    | DecisionKind::SelectResolution
                    | DecisionKind::EntryReveal
                    | DecisionKind::OrderResolutionGraveyard
            ) || self.responding_seat_ids.len() == 1)
            && (!matches!(
                self.kind,
                DecisionKind::LibrarySearch | DecisionKind::EntryReveal
            ) || self.visibility == DecisionVisibility::RespondingSeat)
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
        self.domain_references_in(&self.domain, objects, stack, roster)
    }
    fn domain_references_in(
        &self,
        domain: &PlayerDomain,
        objects: &std::collections::BTreeSet<PlayerHandle>,
        stack: &std::collections::BTreeSet<PlayerHandle>,
        roster: &[Uuid],
    ) -> bool {
        let check = |t: &PlayerTarget| target_in(t, objects, stack, roster);
        match domain {
            PlayerDomain::SplitCast {
                left: normal,
                right: adventure,
            }
            | PlayerDomain::AdventureCast { normal, adventure } => {
                normal
                    .as_deref()
                    .is_none_or(|face| self.domain_references_in(face, objects, stack, roster))
                    && self.domain_references_in(adventure, objects, stack, roster)
            }
            PlayerDomain::CounterRemoval { permanent, .. } => objects.contains(permanent),
            PlayerDomain::TriggerModes { modes, .. } => {
                modes.iter().all(|mode| mode.targets.iter().all(check))
            }
            PlayerDomain::ImpulseCast {
                spell,
                targets,
                modes,
                permanent_targets,
                fixed_targets,
                ..
            } => {
                objects.contains(spell)
                    && targets.iter().all(check)
                    && fixed_targets
                        .as_ref()
                        .is_none_or(|clauses| clauses.iter().all(|values| values.iter().all(check)))
                    && modes.as_ref().is_none_or(|spec| {
                        spec.modes.iter().all(|mode| mode.targets.iter().all(check))
                    })
                    && permanent_targets
                        .as_ref()
                        .is_none_or(|spec| spec.targets.iter().all(check))
            }
            PlayerDomain::SuspendedCast { spell, targets, .. }
            | PlayerDomain::MadnessCast { spell, targets, .. }
            | PlayerDomain::CascadeCast { spell, targets, .. }
            | PlayerDomain::ReboundCast { spell, targets, .. } => {
                objects.contains(spell) && targets.iter().all(check)
            }
            PlayerDomain::Targets { targets } | PlayerDomain::Distribution { targets, .. } => {
                targets.iter().all(check)
            }
            PlayerDomain::TargetList {
                targets: xs,
                secondary_targets,
                ..
            } => {
                xs.iter().all(check)
                    && secondary_targets
                        .iter()
                        .all(|group| check(&group.primary) && group.targets.iter().all(check))
            }
            PlayerDomain::Objects {
                objects: xs,
                prior_votes,
                prior_choices,
                ..
            } => {
                prior_choices
                    .iter()
                    .all(|row| roster.contains(&row.seat_id) && objects.contains(&row.object))
                    && xs.iter().all(|x| objects.contains(x))
                    && prior_votes
                        .iter()
                        .all(|vote| roster.contains(&vote.seat_id))
            }
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
            PlayerDomain::Attackers {
                choices,
                capped_defenders,
                ..
            } => {
                capped_defenders.iter().all(|id| roster.contains(id))
                    && choices
                        .iter()
                        .all(|c| objects.contains(&c.attacker) && c.defenders.iter().all(check))
            }
            PlayerDomain::Blockers {
                choices,
                defending_seat_id,
                ..
            } => {
                defending_seat_id.is_none_or(|id| roster.contains(&id))
                    && choices.iter().all(|c| {
                        objects.contains(&c.blocker)
                            && c.attackers.iter().all(|x| objects.contains(x))
                    })
            }
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
            && (response.cast_face() != Some(PlayerCastFace::SplitRight)
                || matches!(self.domain, PlayerDomain::SplitCast { .. }))
            && (!response.casts_adventure()
                || matches!(self.domain, PlayerDomain::AdventureCast { .. }))
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
