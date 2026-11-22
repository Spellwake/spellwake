//! Exact unrestricted payment and authored tap-mana definitions.
//! EffectWindow owns production, pools and resolution-time mana permission.
//! Casting costs include independent choices for colored hybrid and Phyrexian symbols.
use crate::{
    characteristics::PermanentSelector, object_identity::ObjectId, priority::SeatId,
    stack_window::StackEntryId,
};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary};
use std::collections::{BTreeMap, BTreeSet};

/// Colorless is a sixth mana type, not a sixth color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ManaType {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colourless,
}
impl ManaType {
    pub const ALL: [Self; 6] = [
        Self::White,
        Self::Blue,
        Self::Black,
        Self::Red,
        Self::Green,
        Self::Colourless,
    ];
}

/// An object selected to pay a printed casting cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastResource {
    /// Reveal a physical colorless creature card from the caster’s hand.
    RevealColorlessCreature(crate::object_identity::ObjectId),
    /// Exile a card of the required color from hand as part of an alternative price.
    ExileHandForAlternative(ObjectId),
    /// Tap one permanent toward the alternative price.
    TapForAlternative(crate::object_identity::ObjectId),
    /// Tap a white creature to pay flashback.
    TapForFlashback(crate::object_identity::ObjectId),
    /// A creature tapped for the spell's printed additional cost.
    TapCreatureForSpell(crate::object_identity::ObjectId),
    /// Return a tapped controlled creature to its owner’s hand for web-slinging.
    WebSlinging(crate::object_identity::ObjectId),
    /// Tap a controlled creature for one generic mana or a mana of its color.
    Convoke {
        creature: crate::object_identity::ObjectId,
        color: Option<crate::characteristics::Color>,
    },
    /// Tap an untapped controlled artifact for one generic mana.
    Improvise(crate::object_identity::ObjectId),
    /// Exile another card from the caster's graveyard for one generic mana.
    Delve(crate::object_identity::ObjectId),
    ExileCreatureForReduction(crate::object_identity::ObjectId),
    /// Exile a creature card to pay a mandatory additional casting cost.
    ExileGraveyardCreatureForSpell(ObjectId),
    /// One card exiled from the casting player's graveyard toward the chosen X.
    ExileGraveyardCardForSpell(ObjectId),
    /// Exile another graveyard card as part of the printed escape cost.
    Escape(crate::object_identity::ObjectId),
    /// Sacrifice one controlled artifact, enchantment, or token to bargain.
    Bargain(crate::object_identity::ObjectId),
    /// Return this exact unblocked attacker to its owner to pay a sneak cost.
    Sneak(crate::object_identity::ObjectId),
    /// Qualifying permanent sacrificed for the emerge alternative cost.
    Emerge(crate::object_identity::ObjectId),
    /// Discard a land from hand to cast a spell with retrace from the graveyard.
    Retrace(crate::object_identity::ObjectId),
    /// Discard a card from hand to cast a spell with jump-start from the graveyard.
    JumpStart(crate::object_identity::ObjectId),
    /// A controlled creature sacrificed as part of a nonmana flashback cost.
    FlashbackSacrifice(crate::object_identity::ObjectId),
    /// A card discarded to satisfy the printed mandatory additional cost.
    DiscardForSpell(crate::object_identity::ObjectId),
    /// Creature sacrificed as a mandatory additional cost to cast the spell.
    SacrificeForSpell(crate::object_identity::ObjectId),
}
impl CastResource {
    /// Exact incarnation selected to pay this cost.
    pub fn object(self) -> crate::object_identity::ObjectId {
        match self {
            Self::Convoke { creature, .. } => creature,
            Self::ExileHandForAlternative(id)
            | Self::RevealColorlessCreature(id)
            | Self::TapForFlashback(id)
            | Self::TapCreatureForSpell(id)
            | Self::TapForAlternative(id)
            | Self::Improvise(id)
            | Self::ExileCreatureForReduction(id)
            | Self::ExileGraveyardCreatureForSpell(id)
            | Self::ExileGraveyardCardForSpell(id)
            | Self::Delve(id)
            | Self::Escape(id)
            | Self::WebSlinging(id)
            | Self::Emerge(id)
            | Self::Sneak(id)
            | Self::Bargain(id)
            | Self::Retrace(id)
            | Self::JumpStart(id) => id,
            Self::SacrificeForSpell(id)
            | Self::DiscardForSpell(id)
            | Self::FlashbackSacrifice(id) => id,
        }
    }
}
/// One chosen occurrence of a zero-based printed spell mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastMode {
    /// Ordered selections for a multi-target clause in this mode.
    pub targets: Vec<crate::targets::TargetRef>,
    pub mode: u8,
    pub target: Option<crate::targets::TargetRef>,
}

/// The second way to pay a colored hybrid or Phyrexian symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridAlternative {
    Mana(ManaType),
    TwoLife,
}

/// A printed symbol whose first color is included in the base mana cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HybridSymbol {
    pub first: ManaType,
    pub alternative: HybridAlternative,
}

/// The side of a printed hybrid or Phyrexian symbol selected at announcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridSide {
    First,
    Second,
}

/// One card revealed from hand for splice. List order is the chosen resolution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpliceChoice {
    pub card: ObjectId,
    pub target: Option<crate::targets::TargetRef>,
}

/// Mana allocation and the ordered object payments chosen during casting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CastingPayment {
    pub splice: Vec<SpliceChoice>,
    /// In printed symbol order. Omission selects the first side of each symbol.
    pub hybrid_choices: Vec<HybridSide>,
    /// One announced target for each fixed target clause, in printed order.
    pub fixed_targets: Vec<crate::targets::TargetRef>,
    /// The value announced for a printed {X} cost. Omission selects zero.
    pub chosen_x: Option<RulesInteger>,
    /// Explicit choices of printed modes and their independent targets at announcement.
    pub modes: Vec<CastMode>,
    /// Ordered choices for a single printed multiple-permanent target clause.
    pub permanent_targets: Vec<crate::targets::TargetRef>,
    /// Positive shares in the order of the selected targets for a divided effect.
    pub target_division: Vec<u16>,
    /// Source whose graveyard-cast permission is used when several are available.
    pub graveyard_permission: Option<crate::object_identity::ObjectId>,
    /// Which unused nonland type this cast consumes from its granting effect.
    pub exile_cast_type: Option<crate::program::CardType>,
    /// Battlefield source supplying the chosen alternative price.
    pub alternative_cost_source: Option<crate::object_identity::ObjectId>,
    /// Select this spell’s printed conditional alternative mana cost.
    pub use_alternative: bool,
    /// Choose the granted mana-cost flashback price when a printed one also exists.
    pub use_granted_flashback: bool,
    /// Cast using the printed Adventure characteristics.
    pub adventure: bool,
    /// Select a printed split half. An omitted selection on a split card casts its left half.
    pub split_half: Option<crate::program::SplitHalf>,
    /// Cast the reverse face from your graveyard for its disturb cost.
    pub disturb: bool,
    /// Additional cost index and the positive number of times it is paid.
    pub additional: BTreeMap<usize, RulesInteger>,
    pub mana: Option<ManaPayment>,
    /// Explicit part of the color allocation paid with restricted mana.
    pub restricted_mana: Vec<RestrictedManaSpend>,
    pub objects: Vec<CastResource>,
}

/// An explicit cost; a zero cost is distinct from an absent, unpayable cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaCost {
    pub generic: RulesInteger,
    pub required: BTreeMap<ManaType, RulesInteger>,
}
impl ManaCost {
    /// Materialize the announced price before applying additional costs and reductions.
    /// Returns the life required by the selected Phyrexian symbols.
    pub(crate) fn choose_hybrid(
        &mut self,
        symbols: &[HybridSymbol],
        choices: &[HybridSide],
    ) -> Option<RulesInteger> {
        if !choices.is_empty() && choices.len() != symbols.len() {
            return None;
        }
        let mut life = RulesInteger::from(0);
        for (index, symbol) in symbols.iter().enumerate() {
            if choices.get(index) != Some(&HybridSide::Second) {
                continue;
            }
            let first = self.required.get_mut(&symbol.first)?;
            if !first.is_positive() {
                return None;
            }
            *first = first.minus(&1.into());
            if !first.is_positive() {
                self.required.remove(&symbol.first);
            }
            match symbol.alternative {
                HybridAlternative::Mana(color) => {
                    let amount = self.required.entry(color).or_insert_with(|| 0.into());
                    *amount = amount.plus(&1.into());
                }
                HybridAlternative::TwoLife => life = life.plus(&2.into()),
            }
        }
        Some(life)
    }

    /// Count each distinct payment once when an ability asks for a chosen card's mana cost.
    pub(crate) fn hybrid_prices(&self, symbols: &[HybridSymbol]) -> Vec<(Self, RulesInteger)> {
        let mut prices =
            std::collections::BTreeSet::from([(RulesInteger::from(0), self.required.clone())]);
        for symbol in symbols {
            let alternatives = prices
                .iter()
                .filter_map(|(life, required)| {
                    let mut cost = Self {
                        generic: self.generic.clone(),
                        required: required.clone(),
                    };
                    let additional_life =
                        cost.choose_hybrid(std::slice::from_ref(symbol), &[HybridSide::Second])?;
                    Some((life.plus(&additional_life), cost.required))
                })
                .collect::<Vec<_>>();
            prices.extend(alternatives);
        }
        prices
            .into_iter()
            .map(|(life, required)| {
                (
                    Self {
                        generic: self.generic.clone(),
                        required,
                    },
                    life,
                )
            })
            .collect()
    }

    pub fn one(mana_type: ManaType) -> Self {
        Self::specific(mana_type, 1.into())
    }
    pub fn specific(mana_type: ManaType, amount: RulesInteger) -> Self {
        Self {
            generic: 0.into(),
            required: BTreeMap::from([(mana_type, amount)]),
        }
    }
    pub fn generic(amount: RulesInteger) -> Self {
        Self {
            generic: amount,
            required: BTreeMap::new(),
        }
    }
    pub fn free() -> Self {
        Self::generic(0.into())
    }
    pub fn total(&self) -> RulesInteger {
        self.required
            .values()
            .fold(self.generic.clone(), |sum, amount| sum.plus(amount))
    }
    pub(crate) fn valid(&self) -> bool {
        self.generic >= 0.into() && self.required.values().all(|amount| *amount >= 0.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatingMana {
    pub player: SeatId,
    pub mana_type: ManaType,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaSpend {
    pub mana_type: ManaType,
    pub amount: RulesInteger,
}
/// A rule restricting which spells may be paid for with a floating mana unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ManaRestriction {
    PlaneswalkerSpell,
    ColorlessSpell,
    CommanderSpell,
    ColorlessEldraziSpellOrAbility,
    NoncreatureSpell,
    SpellWithFlashbackFromGraveyard,
    ArtifactSpellOrAbility,
    CreatureSpell,
    CreatureSpellOfType(Option<spellwake_contracts::creature_types::CreatureTypeId>),
    /// The exact creature spell this mana pays for cannot be countered.
    CreatureSpellOfTypeUncounterable(Option<spellwake_contracts::creature_types::CreatureTypeId>),
    CreatureSpellOrAbilityOfType(Option<spellwake_contracts::creature_types::CreatureTypeId>),
}
impl ManaRestriction {
    fn valid_authored(self, vocabulary: &CreatureTypeVocabulary) -> bool {
        match self {
            Self::CreatureSpellOfType(_)
            | Self::CreatureSpellOrAbilityOfType(_)
            | Self::CreatureSpellOfTypeUncounterable(_) => false,
            Self::ColorlessEldraziSpellOrAbility => vocabulary.resolve_label("Eldrazi").is_some(),
            _ => true,
        }
    }
}
/// A selected amount from a particular restricted portion of the pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestrictedManaSpend {
    pub mana_type: ManaType,
    pub restriction: ManaRestriction,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManaPayment {
    /// Explicit allocations from floating mana whose source gives it a spending effect.
    /// An unspecified allocation uses ordinary mana of the same type first.
    pub from_lots: BTreeMap<u64, RulesInteger>,
    pub spend: Vec<ManaSpend>,
}
/// One actual normalized payment for a cast, activation or resolving instruction.
/// Resolution outcomes retain their instruction separately; this is not a pool store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaPaymentRecord {
    /// Explicit source allocations requested by the payer.
    pub from_lots: BTreeMap<u64, RulesInteger>,
    pub source: StackEntryId,
    pub payer: SeatId,
    pub cost: ManaCost,
    pub spend: Vec<ManaSpend>,
    pub restricted_mana: Vec<RestrictedManaSpend>,
}
/// Identifies an authored ability on its source; not a stack entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ManaAbilityId(pub u64);

/// Current mana activation selection. Authored IDs and rule-derived basic
/// abilities occupy disjoint domains, including authored IDs zero and u64::MAX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ManaAbilityRef {
    Inherited(crate::characteristics::EffectInstanceKey),
    Printed(ManaAbilityId),
    BasicLand(crate::land::BasicLandType),
    /// A distinct layer-six grant, identified by the granting effect.
    GrantedAnyColor(crate::characteristics::EffectInstanceKey),
    /// A distinct layer-six fixed-color grant, retaining its granting effect.
    GrantedFixedColor(crate::characteristics::EffectInstanceKey, ManaType),
}

/// Mana ability and any color selected during the mana-ability step of a cast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastManaActivation {
    pub source: crate::object_identity::ObjectId,
    pub ability: ManaAbilityRef,
    /// Mana allocated to the activation cost, if any.
    pub payment: Option<ManaPayment>,
    /// Explicit fixed bundle for a hybrid filter mana ability.
    pub bundle: Option<BTreeMap<ManaType, RulesInteger>>,
    pub color: Option<ManaType>,
    /// Objects chosen to pay a mana ability's non-source cost.
    pub cost_objects: Vec<ObjectId>,
    /// Player chosen to receive mana when the ability permits it.
    pub recipient: Option<crate::priority::SeatId>,
    /// Restricted units selected for this mana ability's own mana cost.
    pub restricted_mana: Vec<RestrictedManaSpend>,
}

/// A spending effect carried by mana independently of whether its producer stays in play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManaSpendEffect {
    CommanderEntryCounters,
    SharedCommanderTypeScry,
    RetainUntilCleanup,
}

/// Public metadata for units with a spending effect. The identifier belongs only to this game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatingManaLot {
    pub player: SeatId,
    pub mana_type: ManaType,
    pub effect: ManaSpendEffect,
    pub remaining: RulesInteger,
    pub source: ObjectId,
}

/// Authored tap mana ability; ordinary creature tap history is checked by the
/// enclosing executor. Output can be fixed, counted or chosen on resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaAbility {
    pub id: ManaAbilityId,
    pub output: ManaOutput,
}
/// Triggered mana ability condition (resolves immediately without using the stack, CR 605.1b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggeredManaRule {
    /// Whenever controller taps a land for mana while monarch, add one chosen color.
    LandTappedWhileMonarch,
    /// Whenever controller taps a land for mana, add one mana of any type that land produced.
    LandTappedSameType,
    /// Whenever a land sharing a name with an imprinted card is tapped for mana, its controller adds that type.
    ImprintedLandTappedSameType,
    /// Whenever controller taps a permanent for `{C}`, add an additional `{C}`.
    PermanentTappedForColorless,
}

/// Sacrifice cost paid alongside activating a mana ability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManaSacrificeCost {
    /// Sacrifice the ability's source, optionally also tapping it.
    Source { tap: bool },
    /// Sacrifice a controlled creature without tapping the source.
    ControlledCreature,
    /// Sacrifice a controlled permanent with the given creature subtype without tapping the source.
    ControlledSubtype(spellwake_contracts::creature_types::CreatureTypeId),
}

/// Scope of spending restriction bound to the source permanent's entry-chosen creature type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChosenTypeManaScope {
    /// Creature spells of the chosen type.
    CreatureSpell,
    /// Creature spells of the chosen type, and that spell can't be countered.
    CreatureSpellUncounterable,
    /// Creature spells or activated abilities of creatures of the chosen type.
    CreatureSpellOrAbility,
}

/// Spending restriction attached to produced mana.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManaOutputRestriction {
    /// Static restriction rule independent of the source's chosen creature type.
    Fixed(ManaRestriction),
    /// Dynamic restriction reading the source's entry-chosen creature type at activation.
    ChosenCreatureType(ChosenTypeManaScope),
}

/// Immediate side effect applied alongside producing mana.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManaSideEffect {
    /// Controller gains this much life.
    GainLife(RulesInteger),
    /// Source deals this much noncombat damage to its controller.
    DamageController(RulesInteger),
}

/// Orthogonal cost, condition, restriction, spend-effect, or side-effect modifier wrapping a [`ManaOutput`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManaOutputModifier {
    /// Pay this mana cost alongside tapping the source.
    PayMana(ManaCost),
    /// Sacrifice a permanent as the mana ability's cost.
    Sacrifice(ManaSacrificeCost),
    /// Tap an untapped controlled artifact instead of tapping the source.
    TapControlledArtifact,
    /// Activation requires controlling at least `minimum` battlefield permanents matching `selector`.
    RequirePermanents {
        selector: Box<PermanentSelector>,
        minimum: RulesInteger,
    },
    /// Restrict how the produced mana may be spent.
    Restrict(ManaOutputRestriction),
    /// Attach a floating-mana spending effect to the produced units.
    SpendEffect(ManaSpendEffect),
    /// Apply a side effect alongside producing mana.
    SideEffect(ManaSideEffect),
}

/// Counter-driven mana production modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterManaYield {
    /// Tap to add `{C}` for each charge counter on the source.
    SourceChargeCounters,
    /// Tap and remove any chosen number of storage counters for that much `{C}`.
    RemoveStorageCounters,
    /// Remove a charge counter without tapping to add one mana of any chosen color.
    RemoveChargeForColor,
}

/// Color-domain mana production modes derived from game state or recipient choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialColorYield {
    /// Add one mana of a color in the activating player's commander color identity.
    CommanderColorIdentity,
    /// Add one mana of any color a land an opponent controls could produce (CR 106.7).
    OpponentsLandColors,
    /// Add the fixed color or the color selected as this permanent entered.
    EntryChosenColor { fixed: ManaType },
    /// Tap, choose a player, and that player chooses one color for their mana.
    ChosenPlayerColor,
    /// Pay one of two colors and tap to add any two-mana bundle of those colors.
    HybridFilter { colors: [ManaType; 2] },
}

/// Dynamic quantity or conditional-bundle mana production modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicManaYield {
    /// Add X mana in any combination of colors, where X is the greatest power among controlled creatures.
    GreatestCreaturePower,
    /// Choose one color and add `matching` if controlling a creature with at least `minimum_power`, else `otherwise`.
    ControlledCreaturePowerColor {
        minimum_power: RulesInteger,
        otherwise: RulesInteger,
        matching: RulesInteger,
    },
    /// Choose `fulfilled` if controlled lands satisfy every subtype group in `requirements`, else `normal`.
    ConditionalControlledLandTypes {
        requirements: Vec<BTreeSet<crate::land::LandType>>,
        normal: BTreeMap<ManaType, RulesInteger>,
        fulfilled: BTreeMap<ManaType, RulesInteger>,
    },
}

// Keep PermanentSelector inline in ManaOutput::Counted to avoid extra indirection in mana builders.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManaOutput {
    /// Produce a fixed bundle of mana.
    Fixed(BTreeMap<ManaType, RulesInteger>),
    /// Choose one color from `colors` and add `amount` of it.
    ChooseColor {
        amount: RulesInteger,
        colors: BTreeSet<ManaType>,
    },
    /// Triggered mana ability (resolves immediately without using the stack, never activated directly).
    Triggered(TriggeredManaRule),
    /// Compose an orthogonal cost, condition, restriction, spend effect, or side effect over an inner output.
    Modified {
        modifier: ManaOutputModifier,
        output: Box<ManaOutput>,
    },
    /// Add one mana per matching battlefield permanent (`Some(t)` = fixed type, `None` = one chosen color).
    Counted {
        mana_type: Option<ManaType>,
        permanents: PermanentSelector,
    },
    /// Add two colored mana (`distinct: true` = two different colors, `false` = any two colored mana).
    TwoColors { distinct: bool },
    /// Counter-based mana production.
    CounterBased(CounterManaYield),
    /// Special color-domain or recipient-selected mana production.
    SpecialColor(SpecialColorYield),
    /// Dynamic amount or conditional-bundle mana production.
    Dynamic(DynamicManaYield),
}

impl ManaOutput {
    /// All five colored mana types (excluding colorless).
    #[must_use]
    pub fn all_colors() -> BTreeSet<ManaType> {
        ManaType::ALL
            .into_iter()
            .filter(|color| *color != ManaType::Colourless)
            .collect()
    }

    /// Wrap this output with an orthogonal modifier.
    #[must_use]
    pub fn with_modifier(self, modifier: ManaOutputModifier) -> Self {
        Self::Modified {
            modifier,
            output: Box::new(self),
        }
    }

    /// Require paying `cost` alongside tapping the source.
    #[must_use]
    pub fn paid(self, cost: ManaCost) -> Self {
        self.with_modifier(ManaOutputModifier::PayMana(cost))
    }

    /// Attach a static spending restriction to this output.
    #[must_use]
    pub fn restricted(self, restriction: ManaRestriction) -> Self {
        self.with_modifier(ManaOutputModifier::Restrict(ManaOutputRestriction::Fixed(
            restriction,
        )))
    }

    /// Choose one mana of any color restricted to the source's entry-chosen creature type.
    #[must_use]
    pub fn choose_color_for_creature_type(scope: ChosenTypeManaScope) -> Self {
        Self::ChooseColor {
            amount: 1.into(),
            colors: Self::all_colors(),
        }
        .with_modifier(ManaOutputModifier::Restrict(
            ManaOutputRestriction::ChosenCreatureType(scope),
        ))
    }

    /// Require sacrificing a permanent as the activation cost.
    #[must_use]
    pub fn sacrifice(self, sacrifice: ManaSacrificeCost) -> Self {
        self.with_modifier(ManaOutputModifier::Sacrifice(sacrifice))
    }

    /// Require tapping an untapped controlled artifact instead of the source.
    #[must_use]
    pub fn tap_controlled_artifact(self) -> Self {
        self.with_modifier(ManaOutputModifier::TapControlledArtifact)
    }

    /// Require controlling at least `minimum` battlefield permanents matching `selector`.
    #[must_use]
    pub fn require_permanents(self, selector: PermanentSelector, minimum: RulesInteger) -> Self {
        self.with_modifier(ManaOutputModifier::RequirePermanents {
            selector: Box::new(selector),
            minimum,
        })
    }

    /// Attach a floating-mana spending effect to the produced units.
    #[must_use]
    pub fn spend_effect(self, effect: ManaSpendEffect) -> Self {
        self.with_modifier(ManaOutputModifier::SpendEffect(effect))
    }

    /// Apply an immediate side effect alongside producing mana.
    #[must_use]
    pub fn side_effect(self, effect: ManaSideEffect) -> Self {
        self.with_modifier(ManaOutputModifier::SideEffect(effect))
    }

    /// Whether this entry represents a triggered mana ability rather than an activated one.
    #[must_use]
    pub fn is_triggered(&self) -> bool {
        matches!(self, Self::Triggered(_))
    }

    /// Whether activating this ability taps something other than the source (or nothing at all).
    #[must_use]
    pub fn skips_source_tap(&self) -> bool {
        match self {
            Self::CounterBased(CounterManaYield::RemoveChargeForColor) => true,
            Self::Modified { modifier, output } => {
                matches!(modifier, ManaOutputModifier::TapControlledArtifact)
                    || output.skips_source_tap()
            }
            _ => false,
        }
    }

    /// Return the floating-mana spending effect carried by this output, if any.
    #[must_use]
    pub fn spend_effect_rule(&self) -> Option<ManaSpendEffect> {
        match self {
            Self::Modified {
                modifier: ManaOutputModifier::SpendEffect(effect),
                ..
            } => Some(*effect),
            Self::Modified { output, .. } => output.spend_effect_rule(),
            _ => None,
        }
    }

    /// Return the fixed color of an entry-chosen-color ability, if present.
    #[must_use]
    pub fn entry_chosen_fixed_color(&self) -> Option<ManaType> {
        match self {
            Self::SpecialColor(SpecialColorYield::EntryChosenColor { fixed }) => Some(*fixed),
            Self::Modified { output, .. } => output.entry_chosen_fixed_color(),
            _ => None,
        }
    }

    /// Unwrap all `Modified` layers, returning the innermost base output.
    #[must_use]
    pub fn base(&self) -> &Self {
        match self {
            Self::Modified { output, .. } => output.base(),
            other => other,
        }
    }

    /// Whether this output is a plain or statically-restricted `Fixed`/`ChooseColor` yield.
    fn valid_for_sacrifice_inner(&self) -> bool {
        match self {
            Self::Fixed(_) | Self::ChooseColor { .. } => true,
            Self::Modified {
                modifier: ManaOutputModifier::Restrict(ManaOutputRestriction::Fixed(_)),
                output,
            } => matches!(output.as_ref(), Self::Fixed(_) | Self::ChooseColor { .. }),
            _ => false,
        }
    }

    /// Whether this output is valid inside a `RequirePermanents` condition.
    fn valid_for_conditional_inner(&self) -> bool {
        match self {
            Self::Fixed(_) | Self::ChooseColor { .. } => true,
            Self::Modified {
                modifier: ManaOutputModifier::Restrict(ManaOutputRestriction::Fixed(_)),
                output,
            } => matches!(output.as_ref(), Self::Fixed(_)),
            _ => false,
        }
    }

    pub(crate) fn valid(&self, vocabulary: &CreatureTypeVocabulary) -> bool {
        match self {
            Self::Fixed(produced) => {
                !produced.is_empty() && produced.values().all(RulesInteger::is_positive)
            }
            Self::ChooseColor { amount, colors } => {
                amount.is_positive()
                    && !colors.is_empty()
                    && !colors.contains(&ManaType::Colourless)
            }
            Self::Triggered(_) | Self::TwoColors { .. } | Self::CounterBased(_) => true,
            Self::Counted { permanents, .. } => permanents.validate(vocabulary).is_ok(),
            Self::SpecialColor(yield_kind) => match yield_kind {
                SpecialColorYield::CommanderColorIdentity
                | SpecialColorYield::OpponentsLandColors
                | SpecialColorYield::ChosenPlayerColor => true,
                SpecialColorYield::EntryChosenColor { fixed } => *fixed != ManaType::Colourless,
                SpecialColorYield::HybridFilter { colors } => {
                    colors[0] != colors[1] && !colors.contains(&ManaType::Colourless)
                }
            },
            Self::Dynamic(yield_kind) => match yield_kind {
                DynamicManaYield::GreatestCreaturePower => true,
                DynamicManaYield::ControlledCreaturePowerColor {
                    otherwise,
                    matching,
                    ..
                } => otherwise.is_positive() && matching.is_positive(),
                DynamicManaYield::ConditionalControlledLandTypes {
                    requirements,
                    normal,
                    fulfilled,
                } => {
                    !requirements.is_empty()
                        && requirements.iter().all(|group| !group.is_empty())
                        && !normal.is_empty()
                        && !fulfilled.is_empty()
                        && normal
                            .values()
                            .chain(fulfilled.values())
                            .all(RulesInteger::is_positive)
                }
            },
            Self::Modified { modifier, output } => {
                let modifier_ok = match modifier {
                    ManaOutputModifier::PayMana(cost) => {
                        let allows_zero = matches!(
                            output.base(),
                            Self::Dynamic(DynamicManaYield::GreatestCreaturePower)
                                | Self::SpecialColor(SpecialColorYield::CommanderColorIdentity)
                        );
                        cost.valid() && (allows_zero || cost.total().is_positive())
                    }
                    ManaOutputModifier::Sacrifice(cost) => {
                        let subtype_ok = match cost {
                            ManaSacrificeCost::ControlledSubtype(subtype) => {
                                vocabulary.label(*subtype).is_some()
                            }
                            ManaSacrificeCost::Source { .. }
                            | ManaSacrificeCost::ControlledCreature => true,
                        };
                        subtype_ok && output.valid_for_sacrifice_inner()
                    }
                    ManaOutputModifier::TapControlledArtifact => {
                        matches!(output.as_ref(), Self::Fixed(_))
                    }
                    ManaOutputModifier::RequirePermanents { selector, minimum } => {
                        minimum.is_positive()
                            && selector.validate(vocabulary).is_ok()
                            && output.valid_for_conditional_inner()
                    }
                    ManaOutputModifier::Restrict(restriction) => match restriction {
                        ManaOutputRestriction::Fixed(rule) => rule.valid_authored(vocabulary),
                        ManaOutputRestriction::ChosenCreatureType(_) => true,
                    },
                    ManaOutputModifier::SpendEffect(effect) => {
                        *effect != ManaSpendEffect::RetainUntilCleanup
                    }
                    ManaOutputModifier::SideEffect(
                        ManaSideEffect::GainLife(n) | ManaSideEffect::DamageController(n),
                    ) => n.is_positive(),
                };
                modifier_ok && output.valid(vocabulary)
            }
        }
    }
}

impl ManaAbility {
    pub(crate) fn valid(&self, vocabulary: &CreatureTypeVocabulary) -> bool {
        self.output.valid(vocabulary)
    }
}
/// Current native activation evidence, not an archive or durable acceptance.
/// `produced` is empty when a counted ability resolves with a zero count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaProductionRecord {
    pub source: ObjectId,
    pub ability: ManaAbilityRef,
    pub controller: SeatId,
    /// If different, the player who received the produced mana.
    pub recipient: Option<SeatId>,
    pub produced: BTreeMap<ManaType, RulesInteger>,
    pub restriction: Option<ManaRestriction>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManaPaymentError {
    InvalidCost,
    NonPositiveSpend,
    DuplicateType,
    MissingRequired,
    WrongTotal,
    InsufficientMana,
    PaymentRequired,
}

/// Pure validation. The caller alone owns and debits the actual pool after all
/// cast checks succeed. Normalization orders types, never chooses generic spend.
pub(crate) fn prepare_payment(
    cost: &ManaCost,
    selected: Option<&ManaPayment>,
    available: impl Fn(ManaType) -> RulesInteger,
) -> Result<Vec<ManaSpend>, ManaPaymentError> {
    if !cost.valid() {
        return Err(ManaPaymentError::InvalidCost);
    }
    let Some(selected) = selected else {
        let mut residual: RulesInteger = 0.into();
        for mana_type in ManaType::ALL {
            let pool = available(mana_type);
            let required = cost
                .required
                .get(&mana_type)
                .cloned()
                .unwrap_or_else(|| 0.into());
            if pool < required {
                return Err(ManaPaymentError::InsufficientMana);
            }
            residual = residual.plus(&pool.minus(&required));
        }
        if residual < cost.generic {
            return Err(ManaPaymentError::InsufficientMana);
        }
        if cost.generic.is_positive() {
            return Err(ManaPaymentError::PaymentRequired);
        }
        return Ok(cost
            .required
            .iter()
            .filter(|(_, amount)| amount.is_positive())
            .map(|(&mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.clone(),
            })
            .collect());
    };
    let mut spend = BTreeMap::new();
    for entry in &selected.spend {
        if !entry.amount.is_positive() {
            return Err(ManaPaymentError::NonPositiveSpend);
        }
        if spend
            .insert(entry.mana_type, entry.amount.clone())
            .is_some()
        {
            return Err(ManaPaymentError::DuplicateType);
        }
    }
    for (&mana_type, required) in &cost.required {
        if spend.get(&mana_type).cloned().unwrap_or_else(|| 0.into()) < *required {
            return Err(ManaPaymentError::MissingRequired);
        }
    }
    let total = spend
        .values()
        .fold(RulesInteger::from(0), |sum, amount| sum.plus(amount));
    if total != cost.total() {
        return Err(ManaPaymentError::WrongTotal);
    }
    for (&mana_type, amount) in &spend {
        if *amount > available(mana_type) {
            return Err(ManaPaymentError::InsufficientMana);
        }
    }
    Ok(spend
        .into_iter()
        .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
        .collect())
}
