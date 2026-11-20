//! Seat-safe projected records. Supplied projection facts require a real trusted
//! projector; shape checks never establish native visibility or authorization.
use super::*;
/// Locate the current participant view. Viewer and disclosure are server-selected.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerViewRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub context_id: Uuid,
}
impl PlayerViewRequest {
    /// # Errors
    /// Rejects absent game/context identities, including internally constructed nil IDs.
    pub fn validate(&self) -> Result<(), InvalidPlayerWire> {
        if self.game_id.is_nil() || self.context_id.is_nil() {
            return Err(InvalidPlayerWire);
        }
        Ok(())
    }
}
use crate::amount::Amount;
use std::collections::{BTreeMap, BTreeSet};
fn nullable_rules<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<RulesInteger>, D::Error> {
    Option::<RulesInteger>::deserialize(d)
}
/// Public counter names and exact nonnegative counts.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerCounter {
    pub name: String,
    pub count: RulesCount,
}
/// Printed/public description, never effective arithmetic or private state text.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerCharacteristics {
    pub name: String,
    pub type_line: String,
    pub rules_text: String,
    #[serde(deserialize_with = "crate::provider_json::optional_string")]
    pub mana_cost: Option<String>,
    #[serde(deserialize_with = "crate::provider_json::optional_string")]
    pub printed_power: Option<String>,
    #[serde(deserialize_with = "crate::provider_json::optional_string")]
    pub printed_toughness: Option<String>,
    #[serde(deserialize_with = "crate::provider_json::optional_string")]
    pub printed_loyalty: Option<String>,
}
impl PlayerCharacteristics {
    pub(crate) fn valid(&self) -> bool {
        !self.name.is_empty()
            && display_text(&self.name, 256)
            && display_text(&self.type_line, 1024)
            && display_text(&self.rules_text, 8192)
            && self.mana_cost.as_ref().is_none_or(|x| display_text(x, 256))
            && [
                &self.printed_power,
                &self.printed_toughness,
                &self.printed_loyalty,
            ]
            .iter()
            .all(|x| x.as_ref().is_none_or(|s| display_text(s, 128)))
    }
}
/// Concealed identity cannot carry a hidden printing, name, oracle text or image.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlayerObjectIdentity {
    Known {
        #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
        definition_id: Option<Uuid>,
        #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
        printing_id: Option<Uuid>,
        #[serde(deserialize_with = "crate::provider_json::object")]
        characteristics: Box<PlayerCharacteristics>,
        current_names: spellwake_contracts::names::ObjectNames,
    },
    Concealed {},
}
/// A public creature-type label; syntax alone is not vocabulary membership.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlayerCreatureTypeLabel(String);
impl TryFrom<String> for PlayerCreatureTypeLabel {
    type Error = InvalidPlayerWire;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        // Both native label domains use exact, nonempty, trim/control-free text.
        let label = spellwake_contracts::names::EnglishCardName::new(value)
            .map_err(|_| InvalidPlayerWire)?;
        Ok(Self(label.into()))
    }
}
impl From<PlayerCreatureTypeLabel> for String {
    fn from(value: PlayerCreatureTypeLabel) -> String {
        value.0
    }
}
impl PlayerCreatureTypeLabel {
    /// Exact original label for the pinned native vocabulary lookup.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// An issued public ability reference, not a native ability ID or legality claim.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerAbilityView {
    pub handle: PlayerHandle,
    pub kind: PlayerAbilityKind,
    pub label: String,
}
/// Expected source/object/ability-kind relation supplied by the native projector.
pub struct PlayerAbilityBinding {
    pub source: PlayerHandle,
    pub handle: PlayerHandle,
    pub kind: PlayerAbilityKind,
}
/// Independently public effective values. No free name/oracle/identity field.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerPublicProperties {
    pub card_types: Vec<PlayerCardType>,
    pub supertypes: Vec<PlayerSupertype>,
    pub creature_types: Vec<PlayerCreatureTypeLabel>,
    pub land_types: Vec<PlayerLandType>,
    pub colors: Vec<ManaColor>,
    pub keywords: Vec<PlayerKeyword>,
    #[serde(deserialize_with = "nullable_rules")]
    pub power: Option<RulesInteger>,
    #[serde(deserialize_with = "nullable_rules")]
    pub toughness: Option<RulesInteger>,
}
impl PlayerPublicProperties {
    fn valid(
        &self,
        vocabulary: Option<&spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    ) -> bool {
        self.supertypes.len() <= 5
            && unique(self.supertypes.iter())
            && self.land_types.len() <= 17
            && unique(self.land_types.iter())
            && list(&self.creature_types)
            && unique(self.creature_types.iter())
            && self
                .creature_types
                .iter()
                .all(|label| vocabulary.is_some_and(|v| v.resolve_label(label.as_str()).is_some()))
            && self.card_types.len() <= 7
            && unique(self.card_types.iter())
            && self.colors.len() <= 5
            && unique(self.colors.iter())
            && !self.colors.contains(&ManaColor::Colourless)
            && self.keywords.len() <= 2
            && unique(self.keywords.iter())
    }
}
/// Visible object/public seat subject; stack entries have no counters/attachment role.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PlayerTarget", into = "PlayerTarget")]
pub struct VisibleSubject(PlayerTarget);
impl TryFrom<PlayerTarget> for VisibleSubject {
    type Error = InvalidPlayerWire;
    fn try_from(value: PlayerTarget) -> Result<Self, Self::Error> {
        if matches!(value, PlayerTarget::Stack { .. }) || !value.valid() {
            Err(InvalidPlayerWire)
        } else {
            Ok(Self(value))
        }
    }
}
impl From<VisibleSubject> for PlayerTarget {
    fn from(value: VisibleSubject) -> Self {
        value.0
    }
}
impl VisibleSubject {
    /// Public subject role, never a native local identifier.
    pub fn target(&self) -> &PlayerTarget {
        &self.0
    }
}
/// A visible board record can have a concealed identity, e.g. a face-down permanent.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerObject {
    pub handle: PlayerHandle,
    pub zone: PlayerHandle,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub owner_seat_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub controller_seat_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub identity: PlayerObjectIdentity,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub public_properties: Option<PlayerPublicProperties>,
    pub tapped: bool,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub counters: Vec<PlayerCounter>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub abilities: Vec<PlayerAbilityView>,
    pub damage_marked: RulesCount,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub attached_to: Option<VisibleSubject>,
}
/// The concealed portion of any zone is anonymous, including mixed visibility.
/// Order-unknown visible portions use sorted opaque handles, not native order.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "visibility",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PlayerZone {
    Hidden {
        handle: PlayerHandle,
        zone_type: PlayerZoneKind,
        #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
        owner_seat_id: Option<Uuid>,
        hidden_count: RulesCount,
    },
    Visible {
        handle: PlayerHandle,
        zone_type: PlayerZoneKind,
        #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
        owner_seat_id: Option<Uuid>,
        objects: Vec<PlayerHandle>,
        order_known: bool,
    },
    Mixed {
        handle: PlayerHandle,
        zone_type: PlayerZoneKind,
        #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
        owner_seat_id: Option<Uuid>,
        objects: Vec<PlayerHandle>,
        hidden_count: RulesCount,
        order_known: bool,
    },
}
impl PlayerZone {
    /// Scoped public zone handle.
    pub fn handle(&self) -> &PlayerHandle {
        match self {
            Self::Hidden { handle, .. }
            | Self::Visible { handle, .. }
            | Self::Mixed { handle, .. } => handle,
        }
    }
    /// Native zone role, without native ownership IDs.
    pub fn zone_type(&self) -> PlayerZoneKind {
        match self {
            Self::Hidden { zone_type, .. }
            | Self::Visible { zone_type, .. }
            | Self::Mixed { zone_type, .. } => *zone_type,
        }
    }
    /// Player-owned library/hand/graveyard versus shared native zones.
    pub fn owner_seat_id(&self) -> Option<Uuid> {
        match self {
            Self::Hidden { owner_seat_id, .. }
            | Self::Visible { owner_seat_id, .. }
            | Self::Mixed { owner_seat_id, .. } => *owner_seat_id,
        }
    }
    /// Only the public/visible part. Concealed entries never get stable ordered IDs.
    pub fn objects(&self) -> &[PlayerHandle] {
        match self {
            Self::Hidden { .. } => &[],
            Self::Visible { objects, .. } | Self::Mixed { objects, .. } => objects,
        }
    }
    /// Whether native order of the visible portion is explicitly disclosed.
    pub fn order_known(&self) -> bool {
        match self {
            Self::Hidden { .. } => false,
            Self::Visible { order_known, .. } | Self::Mixed { order_known, .. } => *order_known,
        }
    }
    fn count(&self) -> RulesInteger {
        let visible = RulesInteger::from_usize(self.objects().len());
        match self {
            Self::Hidden { hidden_count, .. } | Self::Mixed { hidden_count, .. } => {
                visible.plus(hidden_count.as_rules_integer())
            }
            Self::Visible { .. } => visible,
        }
    }
    fn valid(&self, roster: &[Uuid], ordered: &BTreeSet<PlayerHandle>) -> bool {
        let personal = matches!(
            self.zone_type(),
            PlayerZoneKind::Library | PlayerZoneKind::Hand | PlayerZoneKind::Graveyard
        );
        let ownership = if personal {
            self.owner_seat_id().is_some_and(|x| roster.contains(&x))
        } else {
            self.owner_seat_id().is_none()
        };
        let ids = self.objects();
        let order = if self.order_known() {
            ordered.contains(self.handle())
        } else {
            ids.windows(2).all(|x| x[0] < x[1])
        };
        ownership
            && list(ids)
            && unique(ids.iter())
            && order
            && match self {
                Self::Mixed { hidden_count, .. } => !ids.is_empty() && positive(hidden_count),
                _ => true,
            }
    }
}
/// Public per-seat numeric information, without account/controller secrets.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSeatView {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub seat_id: Uuid,
    pub life: RulesInteger,
    pub poison: RulesCount,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub counters: Vec<PlayerCounter>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub mana_pool: Vec<ManaSpend>,
    pub hand_count: RulesCount,
    pub library_count: RulesCount,
    pub eliminated: bool,
}
/// Current native turn position; opening decisions can precede a turn entirely.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerTurn {
    pub number: RulesCount,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    /// Empty while a departed multiplayer turn owner finishes their current turn.
    pub active_seat_ids: Vec<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    pub priority_seat_ids: Vec<Uuid>,
    /// Server-owned expiry disclosed to the current priority holder on a live view.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_deadline"
    )]
    pub priority_deadline_at: Option<String>,
    pub phase: PlayerPhase,
    pub step: PlayerStep,
}
fn present_deadline<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
    String::deserialize(de).map(Some)
}
impl PlayerTurn {
    fn valid(&self, roster: &[Uuid]) -> bool {
        let phase = match self.step {
            PlayerStep::Untap | PlayerStep::Upkeep | PlayerStep::Draw => PlayerPhase::Beginning,
            PlayerStep::PrecombatMain => PlayerPhase::PrecombatMain,
            PlayerStep::PostcombatMain => PlayerPhase::PostcombatMain,
            PlayerStep::End | PlayerStep::Cleanup => PlayerPhase::Ending,
            _ => PlayerPhase::Combat,
        };
        positive(&self.number)
            && self.active_seat_ids.len() <= 6
            && self.priority_seat_ids.len() <= 6
            && unique(self.active_seat_ids.iter())
            && unique(self.priority_seat_ids.iter())
            && self
                .active_seat_ids
                .iter()
                .chain(&self.priority_seat_ids)
                .all(|x| roster.contains(x))
            && self.phase == phase
    }
}
/// Ordered bottom-to-top public stack, separate from underlying object identities.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerStackEntry {
    pub handle: PlayerHandle,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub controller_seat_id: Uuid,
    #[serde(deserialize_with = "nullable_handle")]
    pub source_object: Option<PlayerHandle>,
    pub label: String,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub targets: Vec<PlayerTarget>,
    /// Bound occurrences with no current permitted public reference. This is
    /// not a legality judgment and never rebinds a departed incarnation.
    pub unavailable_targets: RulesCount,
}
pub(super) fn counters_valid(values: &[PlayerCounter]) -> bool {
    list(values)
        && unique(values.iter().map(|x| &x.name))
        && values
            .iter()
            .all(|x| !x.name.is_empty() && display_text(&x.name, 128))
}
pub(super) fn target_in(
    t: &PlayerTarget,
    objects: &BTreeSet<PlayerHandle>,
    stack: &BTreeSet<PlayerHandle>,
    roster: &[Uuid],
) -> bool {
    match t {
        PlayerTarget::Object { handle } => objects.contains(handle),
        PlayerTarget::Stack { handle } => stack.contains(handle),
        PlayerTarget::Seat { seat_id } => roster.contains(seat_id),
    }
}
/// Independently supplied projection scope from the accepted native/source adapter.
/// Neither serializable nor proof-bearing: these facts require actual qualification.
pub struct PlayerProjectionFacts<'a> {
    pub basis: &'a PlayerBasis,
    pub viewer: Uuid,
    pub roster: &'a [Uuid],
    pub through_sequence: &'a Amount,
    pub visible_objects: &'a [PlayerHandle],
    pub known_identities: &'a [PlayerHandle],
    pub ordered_zones: &'a [PlayerHandle],
    pub authorized_full_disclosure: bool,
    pub creature_vocabulary:
        Option<&'a spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    pub ability_bindings: &'a [PlayerAbilityBinding],
}
/// Complete bounded seat-safe snapshot, never a private checkpoint or replay archive.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSnapshot {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub basis: PlayerBasis,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub viewer_seat_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub players: Vec<PlayerSeatView>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub turn: Option<PlayerTurn>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub visible_objects: Vec<PlayerObject>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub zones: Vec<PlayerZone>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub stack: Vec<PlayerStackEntry>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub decisions: Vec<PlayerDecision>,
    pub through_event_sequence: Amount,
}
impl PlayerSnapshot {
    /// Validate complete supplied projection facts and all public reference/count paths.
    /// A qualified real projector must still establish the facts and actual information flow.
    /// # Errors
    /// Rejects foreign basis/viewer/frontier, identity/order leaks, missing zones,
    /// inconsistent counts, invalid descriptors and references outside the permitted view.
    pub fn validate(&self, facts: &PlayerProjectionFacts<'_>) -> Result<(), InvalidPlayerWire> {
        let valid = self.valid(facts);
        if valid {
            Ok(())
        } else {
            Err(InvalidPlayerWire)
        }
    }
    fn valid(&self, f: &PlayerProjectionFacts<'_>) -> bool {
        if self.basis != *f.basis
            || self.viewer_seat_id != f.viewer
            || self.through_event_sequence != *f.through_sequence
            || f.viewer.is_nil()
            || f.basis.game_id.is_nil()
            || f.basis.context_id.is_nil()
            || !(1..=6).contains(&f.roster.len())
            || !unique(f.roster.iter())
            || !f.roster.contains(&f.viewer)
            || f.roster.iter().any(Uuid::is_nil)
            || self
                .players
                .iter()
                .map(|x| x.seat_id)
                .ne(f.roster.iter().copied())
        {
            return false;
        }
        if !list(&self.visible_objects)
            || !list(&self.stack)
            || !list(&self.decisions)
            || self.zones.len() != 3 * f.roster.len() + 4
            || !unique(f.visible_objects.iter())
            || !unique(f.known_identities.iter())
            || !unique(f.ordered_zones.iter())
        {
            return false;
        }
        if !list(f.ability_bindings)
            || !unique(f.ability_bindings.iter().map(|x| (&x.source, &x.handle)))
        {
            return false;
        }
        let expected_abilities: BTreeSet<_> = f
            .ability_bindings
            .iter()
            .map(|x| (x.source.clone(), x.handle.clone(), x.kind))
            .collect();
        let actual_abilities: BTreeSet<_> = self
            .visible_objects
            .iter()
            .flat_map(|o| {
                o.abilities
                    .iter()
                    .map(|a| (o.handle.clone(), a.handle.clone(), a.kind))
            })
            .collect();
        if expected_abilities != actual_abilities {
            return false;
        }
        let objects: BTreeSet<_> = self
            .visible_objects
            .iter()
            .map(|x| x.handle.clone())
            .collect();
        let allowed: BTreeSet<_> = f.visible_objects.iter().cloned().collect();
        let known: BTreeSet<_> = f.known_identities.iter().cloned().collect();
        let ordered: BTreeSet<_> = f.ordered_zones.iter().cloned().collect();
        let stack: BTreeSet<_> = self.stack.iter().map(|x| x.handle.clone()).collect();
        let zones: BTreeMap<_, _> = self.zones.iter().map(|x| (x.handle().clone(), x)).collect();
        if objects != allowed
            || !known.is_subset(&objects)
            || objects.len() != self.visible_objects.len()
            || stack.len() != self.stack.len()
            || zones.len() != self.zones.len()
            || !ordered.iter().all(|x| zones.contains_key(x))
            || !self
                .visible_objects
                .windows(2)
                .all(|x| x[0].handle < x[1].handle)
            || !unique(
                self.zones
                    .iter()
                    .map(|z| (z.zone_type(), z.owner_seat_id())),
            )
        {
            return false;
        }
        for seat in f.roster {
            for kind in [
                PlayerZoneKind::Library,
                PlayerZoneKind::Hand,
                PlayerZoneKind::Graveyard,
            ] {
                if !self
                    .zones
                    .iter()
                    .any(|z| z.zone_type() == kind && z.owner_seat_id() == Some(*seat))
                {
                    return false;
                }
            }
        }
        for kind in [
            PlayerZoneKind::Battlefield,
            PlayerZoneKind::Stack,
            PlayerZoneKind::Exile,
            PlayerZoneKind::Command,
        ] {
            if !self
                .zones
                .iter()
                .any(|z| z.zone_type() == kind && z.owner_seat_id().is_none())
            {
                return false;
            }
        }
        let mut assigned = BTreeSet::new();
        for z in &self.zones {
            if !z.valid(f.roster, &ordered)
                || f.authorized_full_disclosure && !matches!(z, PlayerZone::Visible { .. })
            {
                return false;
            }
            for h in z.objects() {
                if !objects.contains(h) || !assigned.insert(h.clone()) {
                    return false;
                }
            }
        }
        if assigned != objects {
            return false;
        }
        for o in &self.visible_objects {
            let Some(z) = zones.get(&o.zone) else {
                return false;
            };
            if !z.objects().contains(&o.handle)
                || z.owner_seat_id().is_some_and(|x| x != o.owner_seat_id)
                || !f.roster.contains(&o.owner_seat_id)
                || !o.controller_seat_id.is_none_or(|x| f.roster.contains(&x))
                || !counters_valid(&o.counters)
                || !list(&o.abilities)
                || !unique(o.abilities.iter().map(|x| &x.handle))
                || o.abilities
                    .iter()
                    .any(|a| a.label.is_empty() || !display_text(&a.label, 1024))
                || !o
                    .public_properties
                    .as_ref()
                    .is_none_or(|p| p.valid(f.creature_vocabulary))
                || !o
                    .attached_to
                    .as_ref()
                    .is_none_or(|x| target_in(x.target(), &objects, &stack, f.roster))
            {
                return false;
            }
            match &o.identity {
                PlayerObjectIdentity::Known {
                    characteristics,
                    current_names,
                    ..
                } => {
                    if !known.contains(&o.handle)
                        || !characteristics.valid()
                        || current_names.len() > MAX_PLAYER_ITEMS
                    {
                        return false;
                    }
                }
                PlayerObjectIdentity::Concealed {} => {
                    if known.contains(&o.handle) || f.authorized_full_disclosure {
                        return false;
                    }
                }
            }
        }
        for p in &self.players {
            if !counters_valid(&p.counters)
                || p.mana_pool.len() > 6
                || !unique(p.mana_pool.iter().map(|x| x.color))
            {
                return false;
            }
            for (kind, count) in [
                (PlayerZoneKind::Hand, &p.hand_count),
                (PlayerZoneKind::Library, &p.library_count),
            ] {
                if !self.zones.iter().any(|z| {
                    z.zone_type() == kind
                        && z.owner_seat_id() == Some(p.seat_id)
                        && z.count() == *count.as_rules_integer()
                }) {
                    return false;
                }
            }
        }
        if !self.turn.as_ref().is_none_or(|x| x.valid(f.roster)) {
            return false;
        }
        if let Some(turn) = &self.turn {
            if turn.active_seat_ids.is_empty()
                && (f.roster.len() < 3 || !self.players.iter().any(|player| player.eliminated))
            {
                return false;
            }
            if let Some(deadline) = &turn.priority_deadline_at
                && (f.roster.len() < 3
                    || turn.priority_seat_ids.as_slice() != [self.viewer_seat_id]
                    || !self.decisions.is_empty()
                    || crate::source_protocol::parse_utc_microsecond(deadline).is_none())
            {
                return false;
            }
            if turn.priority_seat_ids.iter().any(|seat| {
                self.players
                    .iter()
                    .any(|player| player.seat_id == *seat && player.eliminated)
            }) {
                return false;
            }
        }
        for s in &self.stack {
            if !f.roster.contains(&s.controller_seat_id)
                || s.label.is_empty()
                || !display_text(&s.label, 1024)
                || !s.source_object.as_ref().is_none_or(|x| objects.contains(x))
                || !list(&s.targets)
                || !s
                    .targets
                    .iter()
                    .all(|x| target_in(x, &objects, &stack, f.roster))
            {
                return false;
            }
        }
        if !unique(self.decisions.iter().map(|x| &x.decision)) {
            return false;
        }
        for d in &self.decisions {
            if d.validate(
                &self.basis,
                f.viewer,
                f.roster,
                f.authorized_full_disclosure,
            )
            .is_err()
                || !d.references_in(&objects, &stack, f.roster)
            {
                return false;
            }
        }
        true
    }
}
