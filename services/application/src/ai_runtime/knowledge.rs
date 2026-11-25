//! Durable, deterministic, seat-scoped revealed-card knowledge for Spellwake's native AI.
//!
//! Advances strictly from accepted `(prior: &GameState, candidate: &NativeCandidate)`
//! transitions (including deterministic cold checkpoint recovery) so each seat retains
//! only information legitimately disclosed to that seat:
//! - cards currently known to be in an opponent's hand,
//! - cards currently known at positions in own or opponent libraries,
//! - historical reveals with explicit status tracking and location invalidation on
//!   library shuffles and zone transitions.
//!
//! Private looks (`Scry`, `Surveil`, opening mulligan bottoms, private top-of-library
//! access, private look-top, and private tutors) are strictly scoped to the authorized
//! viewer seat and never leak to opponents. Random-order bottom placements retain
//! `VisibleLibraryPosition::BottomUnordered` so hidden library order is never disclosed.
use crate::{
    native_game::{NativeDisplaySource, NativeScope, NativeSeatBinding},
    player_wire::{
        PlayerCharacteristics, PlayerHandle, PlayerObjectIdentity, PlayerSnapshot, PlayerZoneKind,
    },
};
use serde::{Deserialize, Serialize};
use spellwake_engine::{
    effects::{DrawReplacementDomain, LibraryActionKind, LibraryChoiceKind, PileChoice},
    game::{GameState, NativeCandidate, RuleFrame},
    object_identity::{CardId, GameObject, ObjectId, ObjectZone},
    occurrence::{OpeningMovePhase, RulesOccurrence, RulesRecord, ZoneReason},
    priority::SeatId,
    shuffle::ShuffleRecord,
};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Maximum number of historical reveal entries retained per seat.
const MAX_HISTORICAL_REVEALS: usize = 64;

/// How a card became known to a seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSource {
    /// Publicly revealed in hand (e.g., Duress/Thoughtseize, reveal land, cast cost reveal).
    HandReveal,
    /// Publicly revealed via a Ninjutsu activation in hand.
    NinjutsuReveal,
    /// Moved from a publicly visible face-up zone into a hand (e.g., Unsummon, Raise Dead).
    PublicZoneToHand,
    /// Publicly revealed by a library search (e.g., Rampant Growth, Typecycling, Mystical Tutor).
    SearchReveal,
    /// Publicly revealed during a draw or Miracle trigger.
    DrawReveal,
    /// Publicly revealed by Cascade.
    CascadeReveal,
    /// Publicly revealed when a face-down card left play or turned face up.
    FaceDownReveal,
    /// Publicly revealed from a library (e.g., Goblin Guide, Explore, Clash, Kinship).
    LibraryReveal,
    /// Publicly visible top card of a library (e.g., Courser of Kruphix, Future Sight).
    PublicLibraryTop,
    /// Moved from a publicly visible face-up zone into a library (e.g., Memory Lapse).
    PublicZoneToLibrary,
    /// Drawn by an opponent from a library top position already known to this viewer.
    DrawnFromKnownLibrary,
    /// Privately visible top card of own library (e.g., Vizier of the Menagerie).
    PrivateLibraryTop,
    /// Privately positioned during Scry (scoped strictly to the scrying seat).
    PrivateScry,
    /// Privately positioned during Surveil (scoped strictly to the surveiling seat).
    PrivateSurveil,
    /// Privately bottomed during opening London mulligan (scoped strictly to owner).
    PrivateMulliganBottom,
    /// Privately inspected top cards of library without public reveal.
    PrivateLookTop,
    /// Privately placed on top of library after a non-revealing search (e.g., Vampiric Tutor).
    PrivateSearch,
}

impl KnowledgeSource {
    /// Stable wire string for compact prompt serialization.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HandReveal => "hand_reveal",
            Self::NinjutsuReveal => "ninjutsu_reveal",
            Self::PublicZoneToHand => "public_zone_to_hand",
            Self::SearchReveal => "search_reveal",
            Self::DrawReveal => "draw_reveal",
            Self::CascadeReveal => "cascade_reveal",
            Self::FaceDownReveal => "face_down_reveal",
            Self::LibraryReveal => "library_reveal",
            Self::PublicLibraryTop => "public_library_top",
            Self::PublicZoneToLibrary => "public_zone_to_library",
            Self::DrawnFromKnownLibrary => "drawn_from_known_library",
            Self::PrivateLibraryTop => "private_library_top",
            Self::PrivateScry => "private_scry",
            Self::PrivateSurveil => "private_surveil",
            Self::PrivateMulliganBottom => "private_mulligan_bottom",
            Self::PrivateLookTop => "private_look_top",
            Self::PrivateSearch => "private_search",
        }
    }
}

/// Confidence level for a tracked card inside a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LibraryPositionConfidence {
    /// Exact deterministic library order known to this seat.
    ExactOrder,
    /// Known to be on the bottom of the library in random/unknown relative order.
    BottomUnordered,
}

/// Resolved seat-safe position of a visible or remembered library card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VisibleLibraryPosition {
    /// Exact top card of the library (1st from top).
    Top,
    /// Exact 1-based position from the top (`index >= 2`).
    FromTop { index: usize },
    /// Exact bottom card of the library (1st from bottom).
    Bottom,
    /// Exact 1-based position from the bottom (`index >= 2`).
    FromBottom { index: usize },
    /// Known to be on the bottom in unknown/random relative order.
    BottomUnordered,
}

impl VisibleLibraryPosition {
    /// Compact human- and LLM-readable label for `compact_v1`.
    pub fn compact_label(self) -> String {
        match self {
            Self::Top => "top".to_owned(),
            Self::FromTop { index } => format!("top+{index}"),
            Self::Bottom => "bottom".to_owned(),
            Self::FromBottom { index } => format!("bottom+{index}"),
            Self::BottomUnordered => "bottom_unordered".to_owned(),
        }
    }

    fn sort_key(self) -> (u8, usize) {
        match self {
            Self::Top => (0, 1),
            Self::FromTop { index } => (1, index),
            Self::FromBottom { index } => (2, usize::MAX.saturating_sub(index)),
            Self::Bottom => (3, 0),
            Self::BottomUnordered => (4, 0),
        }
    }
}

/// Current lifecycle status of a historical reveal entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RevealStatusView {
    /// Still tracked in an opponent's hand.
    InOpponentHand,
    /// Still tracked at a known library position.
    InLibrary,
    /// Left its previously tracked hidden zone for a known public zone.
    LeftZone { destination: String },
    /// Tracked library position was invalidated by a library shuffle.
    InvalidatedByShuffle,
    /// Tracked library position was invalidated by an opaque library reorder (e.g., opponent scry).
    InvalidatedByLibraryReorder,
    /// Tracked hidden-zone location was invalidated without disclosing a new hidden destination.
    Invalidated,
}

impl RevealStatusView {
    /// Compact string tag for `compact_v1`.
    pub fn compact_label(&self) -> String {
        match self {
            Self::InOpponentHand => "in_opponent_hand".to_owned(),
            Self::InLibrary => "in_library".to_owned(),
            Self::LeftZone { destination } => format!("left_to_{destination}"),
            Self::InvalidatedByShuffle => "invalidated_by_shuffle".to_owned(),
            Self::InvalidatedByLibraryReorder => "invalidated_by_library_reorder".to_owned(),
            Self::Invalidated => "invalidated".to_owned(),
        }
    }
}

/// One card currently known by the viewer to be in an opponent's hand.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownOpponentHandCard {
    pub owner_seat_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<PlayerHandle>,
    pub characteristics: PlayerCharacteristics,
    pub source: KnowledgeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revealed_turn: Option<u64>,
    pub revealed_revision: u64,
}

/// One card currently known by the viewer to be at a position in a library.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleLibraryCard {
    pub owner_seat_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<PlayerHandle>,
    pub characteristics: PlayerCharacteristics,
    pub position: VisibleLibraryPosition,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub may_play: bool,
    pub source: KnowledgeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revealed_turn: Option<u64>,
    pub revealed_revision: u64,
}

/// One historical reveal record remembered by the viewer seat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalRevealView {
    pub owner_seat_id: Uuid,
    pub card_name: String,
    pub source: KnowledgeSource,
    pub status: RevealStatusView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revealed_turn: Option<u64>,
    pub revealed_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidated_turn: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidated_revision: Option<u64>,
}

/// Seat-safe projected view of durable revealed-card knowledge for a single viewer seat.
///
/// Contains zero raw native IDs (`ObjectId`/`CardId`) and zero private information
/// belonging to other seats.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatKnowledgeView {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known_opponent_hand: Vec<KnownOpponentHandCard>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visible_library_cards: Vec<VisibleLibraryCard>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_reveals: Vec<HistoricalRevealView>,
}

impl SeatKnowledgeView {
    /// Returns `true` when no opponent hand, library, or historical reveal knowledge exists.
    pub fn is_empty(&self) -> bool {
        self.known_opponent_hand.is_empty()
            && self.visible_library_cards.is_empty()
            && self.historical_reveals.is_empty()
    }

    /// Build a single-cut fallback view directly from a seat-filtered `PlayerSnapshot`
    /// when a full `SeatKnowledgeLedger` is not supplied.
    pub fn from_snapshot_only(snapshot: &PlayerSnapshot) -> Self {
        let mut view = Self::default();
        let revision = snapshot.basis.revision.get();
        let turn = snapshot
            .turn
            .as_ref()
            .and_then(|t| String::from(t.number.clone()).parse::<u64>().ok());

        let mut zone_meta: BTreeMap<&str, (PlayerZoneKind, Option<Uuid>, &[PlayerHandle])> =
            BTreeMap::new();
        for z in &snapshot.zones {
            zone_meta.insert(
                z.handle().as_str(),
                (z.zone_type(), z.owner_seat_id(), z.objects()),
            );
        }

        for obj in &snapshot.visible_objects {
            let PlayerObjectIdentity::Known {
                characteristics, ..
            } = &obj.identity
            else {
                continue;
            };
            let Some(&(zone_kind, owner_opt, zone_objs)) = zone_meta.get(obj.zone.as_str()) else {
                continue;
            };
            let owner = owner_opt.unwrap_or(obj.owner_seat_id);
            match zone_kind {
                PlayerZoneKind::Hand if owner != snapshot.viewer_seat_id => {
                    view.known_opponent_hand.push(KnownOpponentHandCard {
                        owner_seat_id: owner,
                        handle: Some(obj.handle.clone()),
                        characteristics: (**characteristics).clone(),
                        source: KnowledgeSource::NinjutsuReveal,
                        revealed_turn: turn,
                        revealed_revision: revision,
                    });
                }
                PlayerZoneKind::Library => {
                    let pos = zone_objs.iter().position(|h| h == &obj.handle).map_or(
                        VisibleLibraryPosition::Top,
                        |idx| {
                            if idx == 0 {
                                VisibleLibraryPosition::Top
                            } else {
                                VisibleLibraryPosition::FromTop { index: idx + 1 }
                            }
                        },
                    );
                    let source = if owner == snapshot.viewer_seat_id {
                        KnowledgeSource::PrivateLibraryTop
                    } else {
                        KnowledgeSource::PublicLibraryTop
                    };
                    view.visible_library_cards.push(VisibleLibraryCard {
                        owner_seat_id: owner,
                        handle: Some(obj.handle.clone()),
                        characteristics: (**characteristics).clone(),
                        position: pos,
                        may_play: obj.may_play_from_library,
                        source,
                        revealed_turn: turn,
                        revealed_revision: revision,
                    });
                }
                _ => {}
            }
        }

        for seat in &snapshot.players {
            let search_cards: Vec<&PlayerCharacteristics> = if !seat.search_reveals.is_empty() {
                seat.search_reveals.iter().collect()
            } else {
                seat.search_reveal.iter().collect()
            };
            for ch in search_cards {
                view.historical_reveals.push(HistoricalRevealView {
                    owner_seat_id: seat.seat_id,
                    card_name: ch.name.clone(),
                    source: KnowledgeSource::SearchReveal,
                    status: RevealStatusView::Invalidated,
                    revealed_turn: turn,
                    revealed_revision: revision,
                    invalidated_turn: None,
                    invalidated_revision: None,
                });
            }
            for ch in &seat.cascade_reveals {
                view.historical_reveals.push(HistoricalRevealView {
                    owner_seat_id: seat.seat_id,
                    card_name: ch.name.clone(),
                    source: KnowledgeSource::CascadeReveal,
                    status: RevealStatusView::Invalidated,
                    revealed_turn: turn,
                    revealed_revision: revision,
                    invalidated_turn: None,
                    invalidated_revision: None,
                });
            }
            for ch in &seat.face_down_reveals {
                view.historical_reveals.push(HistoricalRevealView {
                    owner_seat_id: seat.seat_id,
                    card_name: ch.name.clone(),
                    source: KnowledgeSource::FaceDownReveal,
                    status: RevealStatusView::Invalidated,
                    revealed_turn: turn,
                    revealed_revision: revision,
                    invalidated_turn: None,
                    invalidated_revision: None,
                });
            }
        }

        view
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackedHandCard {
    owner: SeatId,
    object: GameObject,
    source: KnowledgeSource,
    revealed_turn: Option<u64>,
    revealed_revision: u64,
    history_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackedLibraryCard {
    owner: SeatId,
    object: GameObject,
    confidence: LibraryPositionConfidence,
    source: KnowledgeSource,
    revealed_turn: Option<u64>,
    revealed_revision: u64,
    history_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InternalRevealStatus {
    InOpponentHand,
    InLibrary,
    LeftZone(ObjectZone),
    InvalidatedByShuffle,
    InvalidatedByLibraryReorder,
    Invalidated,
}

impl InternalRevealStatus {
    fn to_view(&self) -> RevealStatusView {
        match self {
            Self::InOpponentHand => RevealStatusView::InOpponentHand,
            Self::InLibrary => RevealStatusView::InLibrary,
            Self::LeftZone(zone) => RevealStatusView::LeftZone {
                destination: zone_name(*zone).to_owned(),
            },
            Self::InvalidatedByShuffle => RevealStatusView::InvalidatedByShuffle,
            Self::InvalidatedByLibraryReorder => RevealStatusView::InvalidatedByLibraryReorder,
            Self::Invalidated => RevealStatusView::Invalidated,
        }
    }

    fn is_active(&self) -> bool {
        matches!(self, Self::InOpponentHand | Self::InLibrary)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HistoricalRevealRecord {
    id: u64,
    owner: SeatId,
    object: GameObject,
    source: KnowledgeSource,
    status: InternalRevealStatus,
    revealed_turn: Option<u64>,
    revealed_revision: u64,
    invalidated_turn: Option<u64>,
    invalidated_revision: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PerSeatKnowledge {
    next_history_id: u64,
    known_opponent_hand: BTreeMap<ObjectId, TrackedHandCard>,
    known_library: BTreeMap<ObjectId, TrackedLibraryCard>,
    historical_reveals: Vec<HistoricalRevealRecord>,
}

impl PerSeatKnowledge {
    fn alloc_history_id(&mut self) -> u64 {
        let id = self.next_history_id;
        self.next_history_id = self.next_history_id.saturating_add(1);
        id
    }

    fn record_opponent_hand(
        &mut self,
        object: &GameObject,
        owner: SeatId,
        source: KnowledgeSource,
        turn: Option<u64>,
        revision: u64,
    ) {
        let id = object.id();
        if let Some(existing) = self.known_opponent_hand.get_mut(&id) {
            existing.object = object.clone();
            return;
        }
        let history_id = self.alloc_history_id();
        self.known_opponent_hand.insert(
            id,
            TrackedHandCard {
                owner,
                object: object.clone(),
                source,
                revealed_turn: turn,
                revealed_revision: revision,
                history_id,
            },
        );
        self.push_history(HistoricalRevealRecord {
            id: history_id,
            owner,
            object: object.clone(),
            source,
            status: InternalRevealStatus::InOpponentHand,
            revealed_turn: turn,
            revealed_revision: revision,
            invalidated_turn: None,
            invalidated_revision: None,
        });
    }

    fn record_library_card(
        &mut self,
        object: &GameObject,
        owner: SeatId,
        confidence: LibraryPositionConfidence,
        source: KnowledgeSource,
        turn: Option<u64>,
        revision: u64,
    ) {
        let id = object.id();
        if let Some(existing) = self.known_library.get_mut(&id) {
            existing.object = object.clone();
            if confidence == LibraryPositionConfidence::ExactOrder {
                existing.confidence = LibraryPositionConfidence::ExactOrder;
            }
            if source == KnowledgeSource::PublicLibraryTop
                && existing.source == KnowledgeSource::PrivateLibraryTop
            {
                existing.source = source;
            }
            return;
        }
        let history_id = self.alloc_history_id();
        self.known_library.insert(
            id,
            TrackedLibraryCard {
                owner,
                object: object.clone(),
                confidence,
                source,
                revealed_turn: turn,
                revealed_revision: revision,
                history_id,
            },
        );
        self.push_history(HistoricalRevealRecord {
            id: history_id,
            owner,
            object: object.clone(),
            source,
            status: InternalRevealStatus::InLibrary,
            revealed_turn: turn,
            revealed_revision: revision,
            invalidated_turn: None,
            invalidated_revision: None,
        });
    }

    fn record_historical_only(
        &mut self,
        object: &GameObject,
        owner: SeatId,
        source: KnowledgeSource,
        status: InternalRevealStatus,
        turn: Option<u64>,
        revision: u64,
    ) {
        let already_logged = self.historical_reveals.iter().any(|rec| {
            rec.revealed_revision == revision
                && rec.owner == owner
                && rec.object.id() == object.id()
                && rec.source == source
        });
        if already_logged {
            return;
        }
        let history_id = self.alloc_history_id();
        self.push_history(HistoricalRevealRecord {
            id: history_id,
            owner,
            object: object.clone(),
            source,
            status,
            revealed_turn: turn,
            revealed_revision: revision,
            invalidated_turn: None,
            invalidated_revision: None,
        });
    }

    fn update_history_status(
        &mut self,
        history_id: u64,
        status: InternalRevealStatus,
        turn: Option<u64>,
        revision: u64,
    ) {
        if let Some(rec) = self
            .historical_reveals
            .iter_mut()
            .find(|r| r.id == history_id)
        {
            rec.status = status;
            if !rec.status.is_active() {
                rec.invalidated_turn = turn;
                rec.invalidated_revision = Some(revision);
            }
        }
    }

    fn invalidate_library_for_shuffle(
        &mut self,
        owner: SeatId,
        shuffled_ids: Option<&BTreeSet<ObjectId>>,
        turn: Option<u64>,
        revision: u64,
    ) {
        let to_remove: Vec<(ObjectId, u64)> = self
            .known_library
            .iter()
            .filter(|(id, entry)| {
                entry.owner == owner
                    && shuffled_ids.is_none_or(|ids| ids.is_empty() || ids.contains(id))
            })
            .map(|(id, entry)| (*id, entry.history_id))
            .collect();
        for (id, history_id) in to_remove {
            self.known_library.remove(&id);
            self.update_history_status(
                history_id,
                InternalRevealStatus::InvalidatedByShuffle,
                turn,
                revision,
            );
        }
    }

    fn invalidate_library_objects(
        &mut self,
        objects: &[ObjectId],
        status: InternalRevealStatus,
        turn: Option<u64>,
        revision: u64,
    ) {
        for id in objects {
            if let Some(entry) = self.known_library.remove(id) {
                self.update_history_status(entry.history_id, status.clone(), turn, revision);
            }
        }
    }

    fn push_history(&mut self, record: HistoricalRevealRecord) {
        self.historical_reveals.push(record);
        if self.historical_reveals.len() > MAX_HISTORICAL_REVEALS {
            if let Some(idx) = self
                .historical_reveals
                .iter()
                .position(|rec| !rec.status.is_active())
            {
                self.historical_reveals.remove(idx);
            } else {
                self.historical_reveals.remove(0);
            }
        }
    }
}

/// Durable, deterministic, seat-scoped revealed-card knowledge ledger across a game.
///
/// Carried on `NativeCheckpoint` (without altering manifest bytes or hashes) and
/// replayable deterministically from revision 0 through the current accepted tip.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SeatKnowledgeLedger {
    roster: Vec<NativeSeatBinding>,
    seats: BTreeMap<SeatId, PerSeatKnowledge>,
}

impl SeatKnowledgeLedger {
    /// Initialize an empty seat-scoped ledger for `roster` and observe any continuous
    /// or initial reveals present on `state`.
    pub fn initial(state: &GameState, roster: &[NativeSeatBinding]) -> Self {
        let mut ledger = Self {
            roster: roster.to_vec(),
            seats: roster
                .iter()
                .map(|seat| (seat.native_seat, PerSeatKnowledge::default()))
                .collect(),
        };
        let revision = state.basis().revision;
        let turn = state.turn().map(|t| t.position().number);
        ledger.observe_current_visibility(state, turn, revision);
        ledger.reconcile_live_state(state, turn, revision);
        ledger
    }

    /// Advance every seat's isolated knowledge across one accepted native transition.
    pub fn advance(&mut self, prior: &GameState, candidate: &NativeCandidate) {
        let next = candidate.state();
        let revision = next.basis().revision;
        let turn = next.turn().map(|t| t.position().number);
        let all_seats: Vec<SeatId> = self.seats.keys().copied().collect();

        // Step 1: Gather transition-wide public reveal sets for cross-referencing with zone moves.
        let mut public_revealed_hand_ids = BTreeSet::new();
        let mut public_revealed_library_ids = BTreeSet::new();
        let mut public_face_down_revealed_ids = BTreeSet::new();
        let mut public_face_down_revealed_cards = BTreeSet::new();

        for record in &candidate.rules_journal().records {
            let RulesRecord::Event(event) = record else {
                continue;
            };
            for row in &event.occurrences {
                match &row.value {
                    RulesOccurrence::HandCardRevealed { object } => {
                        public_revealed_hand_ids.insert(object.id());
                    }
                    RulesOccurrence::LibraryCardRevealed { object } => {
                        public_revealed_library_ids.insert(object.id());
                    }
                    RulesOccurrence::FaceDownRevealed { object } => {
                        public_face_down_revealed_ids.insert(object.id());
                        if let Some(cid) = object.card_id() {
                            public_face_down_revealed_cards.insert(cid);
                        }
                    }
                    RulesOccurrence::MiracleRevealed { object } => {
                        public_revealed_hand_ids.insert(object.object.id());
                    }
                    _ => {}
                }
            }
        }

        let public_search_reveal_cards: BTreeSet<CardId> = next
            .search_reveals()
            .into_iter()
            .filter_map(|(card, _)| card.card_id())
            .collect();

        // Step 2: Walk the native RulesJournal in exact chronological order.
        for record in &candidate.rules_journal().records {
            let RulesRecord::Event(event) = record else {
                continue;
            };
            for row in &event.occurrences {
                match &row.value {
                    RulesOccurrence::Shuffled(shuffle) => {
                        self.apply_shuffle(shuffle, turn, revision);
                    }
                    RulesOccurrence::HandCardRevealed { object } => {
                        let owner = object.owner();
                        for &viewer in &all_seats {
                            if let Some(seat_state) = self.seats.get_mut(&viewer) {
                                if viewer != owner {
                                    seat_state.record_opponent_hand(
                                        object,
                                        owner,
                                        KnowledgeSource::HandReveal,
                                        turn,
                                        revision,
                                    );
                                } else {
                                    seat_state.record_historical_only(
                                        object,
                                        owner,
                                        KnowledgeSource::HandReveal,
                                        InternalRevealStatus::LeftZone(object.zone()),
                                        turn,
                                        revision,
                                    );
                                }
                            }
                        }
                    }
                    RulesOccurrence::LibraryCardRevealed { object } => {
                        let owner = object.owner();
                        for &viewer in &all_seats {
                            if let Some(seat_state) = self.seats.get_mut(&viewer) {
                                seat_state.record_library_card(
                                    object,
                                    owner,
                                    LibraryPositionConfidence::ExactOrder,
                                    KnowledgeSource::LibraryReveal,
                                    turn,
                                    revision,
                                );
                            }
                        }
                    }
                    RulesOccurrence::MiracleRevealed { object } => {
                        let card_obj = &object.object;
                        let owner = card_obj.owner();
                        for &viewer in &all_seats {
                            if let Some(seat_state) = self.seats.get_mut(&viewer) {
                                if viewer != owner && card_obj.zone() == ObjectZone::Hand(owner) {
                                    seat_state.record_opponent_hand(
                                        card_obj,
                                        owner,
                                        KnowledgeSource::DrawReveal,
                                        turn,
                                        revision,
                                    );
                                } else {
                                    seat_state.record_historical_only(
                                        card_obj,
                                        owner,
                                        KnowledgeSource::DrawReveal,
                                        InternalRevealStatus::LeftZone(card_obj.zone()),
                                        turn,
                                        revision,
                                    );
                                }
                            }
                        }
                    }
                    RulesOccurrence::FaceDownRevealed { object } => {
                        let owner = object.owner();
                        for &viewer in &all_seats {
                            if let Some(seat_state) = self.seats.get_mut(&viewer) {
                                seat_state.record_historical_only(
                                    object,
                                    owner,
                                    KnowledgeSource::FaceDownReveal,
                                    InternalRevealStatus::LeftZone(object.zone()),
                                    turn,
                                    revision,
                                );
                            }
                        }
                    }
                    RulesOccurrence::LibraryAction(action) => {
                        let actor = action.player;
                        let next_cards = next.execution().window().cards();
                        match action.kind {
                            LibraryActionKind::Scry => {
                                if let Some(seat_state) = self.seats.get_mut(&actor) {
                                    for &id in
                                        action.top_first.iter().chain(&action.bottom_top_first)
                                    {
                                        if let Some(obj) = next_cards.get(id)
                                            && obj.zone() == ObjectZone::Library(actor)
                                        {
                                            seat_state.record_library_card(
                                                obj,
                                                actor,
                                                LibraryPositionConfidence::ExactOrder,
                                                KnowledgeSource::PrivateScry,
                                                turn,
                                                revision,
                                            );
                                        }
                                    }
                                }
                                if action.looked_top_first.len() > 1 {
                                    for &viewer in &all_seats {
                                        if viewer != actor
                                            && let Some(seat_state) = self.seats.get_mut(&viewer)
                                        {
                                            seat_state.invalidate_library_objects(
                                                &action.looked_top_first,
                                                InternalRevealStatus::InvalidatedByLibraryReorder,
                                                turn,
                                                revision,
                                            );
                                        }
                                    }
                                }
                            }
                            LibraryActionKind::Surveil => {
                                if let Some(seat_state) = self.seats.get_mut(&actor) {
                                    for &id in &action.top_first {
                                        if let Some(obj) = next_cards.get(id)
                                            && obj.zone() == ObjectZone::Library(actor)
                                        {
                                            seat_state.record_library_card(
                                                obj,
                                                actor,
                                                LibraryPositionConfidence::ExactOrder,
                                                KnowledgeSource::PrivateSurveil,
                                                turn,
                                                revision,
                                            );
                                        }
                                    }
                                }
                                if action.looked_top_first.len() > 1 && action.top_first.len() > 1 {
                                    for &viewer in &all_seats {
                                        if viewer != actor
                                            && let Some(seat_state) = self.seats.get_mut(&viewer)
                                        {
                                            seat_state.invalidate_library_objects(
                                                &action.top_first,
                                                InternalRevealStatus::InvalidatedByLibraryReorder,
                                                turn,
                                                revision,
                                            );
                                        }
                                    }
                                }
                            }
                            LibraryActionKind::Mill => {}
                        }
                    }
                    RulesOccurrence::ZoneChange(change) => {
                        let before_obj = &change.before.object;
                        let after_obj = &change.after.object;
                        let before_id = before_obj.id();
                        let after_id = after_obj.id();
                        let before_zone = before_obj.zone();
                        let after_zone = after_obj.zone();
                        let dest_is_concealed = after_obj.face_down().is_some()
                            || change.reason == ZoneReason::Foretell;

                        let before_was_public_zone = matches!(
                            before_zone,
                            ObjectZone::Battlefield
                                | ObjectZone::Stack
                                | ObjectZone::Graveyard(_)
                                | ObjectZone::Exile
                                | ObjectZone::Command
                        );
                        let before_was_hidden_in_public_zone = before_obj.face_down().is_some()
                            || (before_zone == ObjectZone::Exile
                                && (prior.execution().foretold_on(before_id).is_some()
                                    || prior
                                        .execution()
                                        .face_down_exile_viewer(before_id)
                                        .is_some()));
                        let before_revealed_publicly = public_face_down_revealed_ids
                            .contains(&before_id)
                            || before_obj
                                .card_id()
                                .is_some_and(|cid| public_face_down_revealed_cards.contains(&cid))
                            || public_revealed_library_ids.contains(&before_id)
                            || public_revealed_hand_ids.contains(&before_id)
                            || before_obj
                                .card_id()
                                .is_some_and(|cid| public_search_reveal_cards.contains(&cid));
                        let move_is_publicly_known = (before_was_public_zone
                            && !before_was_hidden_in_public_zone)
                            || before_revealed_publicly;

                        for &viewer in &all_seats {
                            let Some(seat_state) = self.seats.get_mut(&viewer) else {
                                continue;
                            };

                            // Departure from tracked opponent hand.
                            if let Some(tracked) = seat_state.known_opponent_hand.remove(&before_id)
                            {
                                let moved_secretly = dest_is_concealed
                                    || (matches!(after_zone, ObjectZone::Library(_))
                                        && !move_is_publicly_known);
                                let new_status = if moved_secretly {
                                    InternalRevealStatus::Invalidated
                                } else {
                                    InternalRevealStatus::LeftZone(after_zone)
                                };
                                seat_state.update_history_status(
                                    tracked.history_id,
                                    new_status,
                                    turn,
                                    revision,
                                );
                            }

                            // Departure from tracked library position.
                            let mut transferred_from_known_library = false;
                            if let Some(tracked) = seat_state.known_library.remove(&before_id) {
                                if matches!(change.reason, ZoneReason::Draw { .. })
                                    && matches!(after_zone, ObjectZone::Hand(owner) if owner != viewer)
                                    && !dest_is_concealed
                                {
                                    let owner = after_obj.owner();
                                    seat_state.known_opponent_hand.insert(
                                        after_id,
                                        TrackedHandCard {
                                            owner,
                                            object: after_obj.clone(),
                                            source: KnowledgeSource::DrawnFromKnownLibrary,
                                            revealed_turn: turn,
                                            revealed_revision: revision,
                                            history_id: tracked.history_id,
                                        },
                                    );
                                    seat_state.update_history_status(
                                        tracked.history_id,
                                        InternalRevealStatus::InOpponentHand,
                                        turn,
                                        revision,
                                    );
                                    transferred_from_known_library = true;
                                } else {
                                    let new_status = if dest_is_concealed {
                                        InternalRevealStatus::Invalidated
                                    } else {
                                        InternalRevealStatus::LeftZone(after_zone)
                                    };
                                    seat_state.update_history_status(
                                        tracked.history_id,
                                        new_status,
                                        turn,
                                        revision,
                                    );
                                }
                            }

                            // Arrival into opponent's hand.
                            if let ObjectZone::Hand(owner) = after_zone
                                && viewer != owner
                                && !dest_is_concealed
                                && !transferred_from_known_library
                                && move_is_publicly_known
                            {
                                let source = if before_obj
                                    .card_id()
                                    .is_some_and(|cid| public_search_reveal_cards.contains(&cid))
                                {
                                    KnowledgeSource::SearchReveal
                                } else if public_revealed_library_ids.contains(&before_id) {
                                    KnowledgeSource::LibraryReveal
                                } else if public_face_down_revealed_ids.contains(&before_id)
                                    || before_obj.card_id().is_some_and(|cid| {
                                        public_face_down_revealed_cards.contains(&cid)
                                    })
                                {
                                    KnowledgeSource::FaceDownReveal
                                } else {
                                    KnowledgeSource::PublicZoneToHand
                                };
                                seat_state
                                    .record_opponent_hand(after_obj, owner, source, turn, revision);
                            }

                            // Arrival into a library.
                            if let ObjectZone::Library(owner) = after_zone {
                                match change.reason {
                                    ZoneReason::ShuffleGraveyardIntoLibrary
                                    | ZoneReason::ShuffleHandAndGraveyardIntoLibrary
                                    | ZoneReason::ShufflePermanentIntoLibrary => {}
                                    ZoneReason::Opening(OpeningMovePhase::ChargedBottom) => {
                                        if viewer == owner {
                                            seat_state.record_library_card(
                                                after_obj,
                                                owner,
                                                LibraryPositionConfidence::ExactOrder,
                                                KnowledgeSource::PrivateMulliganBottom,
                                                turn,
                                                revision,
                                            );
                                        }
                                    }
                                    ZoneReason::CascadeBottom
                                    | ZoneReason::ExiledCardsToLibraryBottom => {
                                        let source = if change.reason == ZoneReason::CascadeBottom {
                                            KnowledgeSource::CascadeReveal
                                        } else {
                                            KnowledgeSource::PublicZoneToLibrary
                                        };
                                        seat_state.record_library_card(
                                            after_obj,
                                            owner,
                                            LibraryPositionConfidence::BottomUnordered,
                                            source,
                                            turn,
                                            revision,
                                        );
                                    }
                                    ZoneReason::PutIntoOwnersLibrary(_) => {
                                        if move_is_publicly_known {
                                            seat_state.record_library_card(
                                                after_obj,
                                                owner,
                                                LibraryPositionConfidence::ExactOrder,
                                                KnowledgeSource::PublicZoneToLibrary,
                                                turn,
                                                revision,
                                            );
                                        } else if viewer == owner
                                            && matches!(before_zone, ObjectZone::Hand(h) if h == owner)
                                        {
                                            seat_state.record_library_card(
                                                after_obj,
                                                owner,
                                                LibraryPositionConfidence::ExactOrder,
                                                KnowledgeSource::PrivateLookTop,
                                                turn,
                                                revision,
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Step 3: Process shuffle_journal (covers search_shuffle and shuffle_library_with_top
        // which record shuffles on facts.shuffle_journal).
        for shuffle in candidate.shuffle_journal() {
            self.apply_shuffle(shuffle, turn, revision);
        }

        // Step 4: Post-transition public reveals and private decision completions.
        self.apply_post_transition_reveals(
            prior,
            candidate,
            &public_revealed_library_ids,
            &all_seats,
            turn,
            revision,
        );

        // Step 5: Observe live continuous and pending visibility on `next`.
        self.observe_current_visibility(next, turn, revision);

        // Step 6: Belt-and-suspenders live-state reconciliation so no stale location survives.
        self.reconcile_live_state(next, turn, revision);
    }

    fn apply_shuffle(&mut self, shuffle: &ShuffleRecord, turn: Option<u64>, revision: u64) {
        let mut shuffled_ids: BTreeSet<ObjectId> = shuffle.before.iter().copied().collect();
        shuffled_ids.extend(shuffle.after.iter().copied());
        let filter = (!shuffled_ids.is_empty()).then_some(&shuffled_ids);
        for seat_state in self.seats.values_mut() {
            seat_state.invalidate_library_for_shuffle(shuffle.player, filter, turn, revision);
        }
    }

    fn apply_post_transition_reveals(
        &mut self,
        prior: &GameState,
        candidate: &NativeCandidate,
        public_revealed_library_ids: &BTreeSet<ObjectId>,
        all_seats: &[SeatId],
        turn: Option<u64>,
        revision: u64,
    ) {
        let next = candidate.state();
        let next_cards = next.execution().window().cards();

        // 4a. Public search_reveals (e.g., Rampant Growth, Typecycling, Mystical Tutor,
        //     revealed LookTop selection).
        let mut public_search_top_ids = BTreeSet::new();
        for (revealed_obj, _) in next.search_reveals() {
            let owner = revealed_obj.owner();
            let current_obj = revealed_obj
                .card_id()
                .and_then(|cid| next_cards.iter().find(|obj| obj.card_id() == Some(cid)))
                .unwrap_or(revealed_obj);
            match current_obj.zone() {
                ObjectZone::Hand(hand_owner) => {
                    for &viewer in all_seats {
                        if let Some(seat_state) = self.seats.get_mut(&viewer) {
                            if viewer != hand_owner {
                                seat_state.record_opponent_hand(
                                    current_obj,
                                    hand_owner,
                                    KnowledgeSource::SearchReveal,
                                    turn,
                                    revision,
                                );
                            } else {
                                seat_state.record_historical_only(
                                    current_obj,
                                    hand_owner,
                                    KnowledgeSource::SearchReveal,
                                    InternalRevealStatus::LeftZone(current_obj.zone()),
                                    turn,
                                    revision,
                                );
                            }
                        }
                    }
                }
                ObjectZone::Library(lib_owner) => {
                    public_search_top_ids.insert(current_obj.id());
                    for &viewer in all_seats {
                        if let Some(seat_state) = self.seats.get_mut(&viewer) {
                            seat_state.record_library_card(
                                current_obj,
                                lib_owner,
                                LibraryPositionConfidence::ExactOrder,
                                KnowledgeSource::SearchReveal,
                                turn,
                                revision,
                            );
                        }
                    }
                }
                other_zone => {
                    for &viewer in all_seats {
                        if let Some(seat_state) = self.seats.get_mut(&viewer) {
                            seat_state.record_historical_only(
                                current_obj,
                                owner,
                                KnowledgeSource::SearchReveal,
                                InternalRevealStatus::LeftZone(other_zone),
                                turn,
                                revision,
                            );
                        }
                    }
                }
            }
        }

        // 4b. Private LookTop / Search-to-Top / RevealedPiles completion from `prior.pending_decision()`.
        if let Some(pending) = prior.pending_decision() {
            match pending.frame() {
                RuleFrame::LibraryTop(frame) if frame.is_look_top() => {
                    let chooser = frame.chooser();
                    let mut remaining_by_owner: BTreeMap<SeatId, Vec<ObjectId>> = BTreeMap::new();
                    for &id in frame.domain_top_first() {
                        if let Some(obj) = next_cards.get(id)
                            && let ObjectZone::Library(owner) = obj.zone()
                        {
                            remaining_by_owner.entry(owner).or_default().push(id);
                        }
                    }
                    for (owner, remaining) in remaining_by_owner {
                        let lib = next_cards.library(owner);
                        let exact_top =
                            remaining.len() == 1 || lib.starts_with(remaining.as_slice());
                        let confidence = if exact_top {
                            LibraryPositionConfidence::ExactOrder
                        } else {
                            LibraryPositionConfidence::BottomUnordered
                        };
                        for id in remaining {
                            let Some(obj) = next_cards.get(id) else {
                                continue;
                            };
                            if public_revealed_library_ids.contains(&id) {
                                for &viewer in all_seats {
                                    if let Some(seat_state) = self.seats.get_mut(&viewer) {
                                        seat_state.record_library_card(
                                            obj,
                                            owner,
                                            confidence,
                                            KnowledgeSource::LibraryReveal,
                                            turn,
                                            revision,
                                        );
                                    }
                                }
                            } else if let Some(seat_state) = self.seats.get_mut(&chooser) {
                                seat_state.record_library_card(
                                    obj,
                                    owner,
                                    confidence,
                                    KnowledgeSource::PrivateLookTop,
                                    turn,
                                    revision,
                                );
                            }
                        }
                    }
                }
                RuleFrame::LibraryTop(frame) if frame.kind() == LibraryChoiceKind::Search => {
                    let chooser = frame.chooser();
                    for shuffle in candidate.shuffle_journal() {
                        let owner = shuffle.player;
                        if let Some(&top_id) = next_cards.library(owner).first()
                            && !shuffle.after.contains(&top_id)
                            && !public_search_top_ids.contains(&top_id)
                            && let Some(obj) = next_cards.get(top_id)
                            && let Some(seat_state) = self.seats.get_mut(&chooser)
                        {
                            seat_state.record_library_card(
                                obj,
                                owner,
                                LibraryPositionConfidence::ExactOrder,
                                KnowledgeSource::PrivateSearch,
                                turn,
                                revision,
                            );
                        }
                    }
                }
                RuleFrame::RevealedPiles(frame) => {
                    if let PileChoice::BottomOrder(ids) = frame.choice() {
                        let chooser = frame.chooser();
                        for &id in ids {
                            let Some(obj) = next_cards.get(id) else {
                                continue;
                            };
                            let ObjectZone::Library(owner) = obj.zone() else {
                                continue;
                            };
                            let is_public = public_revealed_library_ids.contains(&id)
                                || self.seats.iter().any(|(&seat, st)| {
                                    seat != chooser
                                        && st.historical_reveals.iter().any(|r| r.object.id() == id)
                                });
                            if is_public {
                                for &viewer in all_seats {
                                    if let Some(seat_state) = self.seats.get_mut(&viewer) {
                                        seat_state.record_library_card(
                                            obj,
                                            owner,
                                            LibraryPositionConfidence::ExactOrder,
                                            KnowledgeSource::LibraryReveal,
                                            turn,
                                            revision,
                                        );
                                    }
                                }
                            } else if let Some(seat_state) = self.seats.get_mut(&chooser) {
                                seat_state.record_library_card(
                                    obj,
                                    owner,
                                    LibraryPositionConfidence::ExactOrder,
                                    KnowledgeSource::PrivateLookTop,
                                    turn,
                                    revision,
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // 4c. Public cascade_reveals and face_down_reveals (including revealed cast-cost cards
        //     and terminal game-end face-down reveals).
        for (obj, _) in next.cascade_reveals() {
            let owner = obj.owner();
            let status = match obj.zone() {
                ObjectZone::Library(_) => InternalRevealStatus::InLibrary,
                other => InternalRevealStatus::LeftZone(other),
            };
            for &viewer in all_seats {
                if let Some(seat_state) = self.seats.get_mut(&viewer) {
                    seat_state.record_historical_only(
                        obj,
                        owner,
                        KnowledgeSource::CascadeReveal,
                        status.clone(),
                        turn,
                        revision,
                    );
                }
            }
        }
        for (obj, _) in next.face_down_reveals() {
            let owner = obj.owner();
            let current_obj = obj
                .card_id()
                .and_then(|cid| next_cards.iter().find(|c| c.card_id() == Some(cid)))
                .unwrap_or(obj);
            for &viewer in all_seats {
                if let Some(seat_state) = self.seats.get_mut(&viewer) {
                    if viewer != owner && current_obj.zone() == ObjectZone::Hand(owner) {
                        seat_state.record_opponent_hand(
                            current_obj,
                            owner,
                            KnowledgeSource::HandReveal,
                            turn,
                            revision,
                        );
                    } else {
                        seat_state.record_historical_only(
                            obj,
                            owner,
                            KnowledgeSource::FaceDownReveal,
                            InternalRevealStatus::LeftZone(current_obj.zone()),
                            turn,
                            revision,
                        );
                    }
                }
            }
        }
    }

    fn observe_current_visibility(&mut self, state: &GameState, turn: Option<u64>, revision: u64) {
        let all_seats: Vec<SeatId> = self.seats.keys().copied().collect();
        let cards = state.execution().window().cards();

        // Ninjutsu-revealed cards in hand.
        for obj in cards.iter() {
            if let ObjectZone::Hand(owner) = obj.zone()
                && state.execution().revealed_by_ninjutsu(obj.id())
            {
                for &viewer in &all_seats {
                    if let Some(seat_state) = self.seats.get_mut(&viewer) {
                        if viewer != owner {
                            seat_state.record_opponent_hand(
                                obj,
                                owner,
                                KnowledgeSource::NinjutsuReveal,
                                turn,
                                revision,
                            );
                        } else {
                            seat_state.record_historical_only(
                                obj,
                                owner,
                                KnowledgeSource::NinjutsuReveal,
                                InternalRevealStatus::LeftZone(obj.zone()),
                                turn,
                                revision,
                            );
                        }
                    }
                }
            }
        }

        // Continuous library-top access (public vs private).
        if let Ok(evaluated) = state.execution().evaluated_characteristics() {
            for &owner in &all_seats {
                let Some(&top_id) = cards.library(owner).first() else {
                    continue;
                };
                let Some(top_obj) = cards.get(top_id) else {
                    continue;
                };
                let is_public = all_seats.iter().any(|&other| {
                    other != owner
                        && state
                            .execution()
                            .library_top_access(&evaluated, owner, other)
                            .visible
                });
                for &viewer in &all_seats {
                    if state
                        .execution()
                        .library_top_access(&evaluated, owner, viewer)
                        .visible
                        && let Some(seat_state) = self.seats.get_mut(&viewer)
                    {
                        let source = if is_public || viewer != owner {
                            KnowledgeSource::PublicLibraryTop
                        } else {
                            KnowledgeSource::PrivateLibraryTop
                        };
                        seat_state.record_library_card(
                            top_obj,
                            owner,
                            LibraryPositionConfidence::ExactOrder,
                            source,
                            turn,
                            revision,
                        );
                    }
                }
            }
        }

        // Active draw_reveals (public).
        for &owner in &all_seats {
            for &id in state.execution().draw_reveals(owner) {
                let Some(obj) = cards.get(id) else {
                    continue;
                };
                if obj.zone() != ObjectZone::Library(owner) {
                    continue;
                }
                for &viewer in &all_seats {
                    if let Some(seat_state) = self.seats.get_mut(&viewer) {
                        seat_state.record_library_card(
                            obj,
                            owner,
                            LibraryPositionConfidence::ExactOrder,
                            KnowledgeSource::DrawReveal,
                            turn,
                            revision,
                        );
                    }
                }
            }
        }

        // Pending private look decisions (strictly scoped to `chooser`).
        if let Some(pending) = state.pending_decision() {
            match pending.frame() {
                RuleFrame::LibraryTop(frame)
                    if matches!(
                        frame.kind(),
                        LibraryChoiceKind::Scry | LibraryChoiceKind::Surveil
                    ) || frame.is_look_top() =>
                {
                    let chooser = frame.chooser();
                    let source = match frame.kind() {
                        LibraryChoiceKind::Scry => KnowledgeSource::PrivateScry,
                        LibraryChoiceKind::Surveil => KnowledgeSource::PrivateSurveil,
                        LibraryChoiceKind::Search => KnowledgeSource::PrivateLookTop,
                    };
                    if let Some(seat_state) = self.seats.get_mut(&chooser) {
                        for &id in frame.domain_top_first() {
                            if let Some(obj) = cards.get(id)
                                && let ObjectZone::Library(owner) = obj.zone()
                            {
                                seat_state.record_library_card(
                                    obj,
                                    owner,
                                    LibraryPositionConfidence::ExactOrder,
                                    source,
                                    turn,
                                    revision,
                                );
                            }
                        }
                    }
                }
                RuleFrame::DrawReplacement(frame) => {
                    if let DrawReplacementDomain::Miracle { card } = frame.domain
                        && let Some(obj) = cards.get(card)
                        && let ObjectZone::Library(owner) = obj.zone()
                        && let Some(seat_state) = self.seats.get_mut(&frame.chooser)
                    {
                        seat_state.record_library_card(
                            obj,
                            owner,
                            LibraryPositionConfidence::ExactOrder,
                            KnowledgeSource::PrivateLookTop,
                            turn,
                            revision,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn reconcile_live_state(&mut self, state: &GameState, turn: Option<u64>, revision: u64) {
        let cards = state.execution().window().cards();
        for seat_state in self.seats.values_mut() {
            let stale_hand: Vec<(ObjectId, u64)> = seat_state
                .known_opponent_hand
                .iter()
                .filter(|(id, entry)| {
                    cards
                        .get(**id)
                        .is_none_or(|obj| obj.zone() != ObjectZone::Hand(entry.owner))
                })
                .map(|(id, entry)| (*id, entry.history_id))
                .collect();
            for (id, history_id) in stale_hand {
                seat_state.known_opponent_hand.remove(&id);
                seat_state.update_history_status(
                    history_id,
                    InternalRevealStatus::Invalidated,
                    turn,
                    revision,
                );
            }

            let stale_library: Vec<(ObjectId, u64)> = seat_state
                .known_library
                .iter()
                .filter(|(id, entry)| {
                    cards
                        .get(**id)
                        .is_none_or(|obj| obj.zone() != ObjectZone::Library(entry.owner))
                })
                .map(|(id, entry)| (*id, entry.history_id))
                .collect();
            for (id, history_id) in stale_library {
                seat_state.known_library.remove(&id);
                seat_state.update_history_status(
                    history_id,
                    InternalRevealStatus::Invalidated,
                    turn,
                    revision,
                );
            }
        }
    }

    fn app_seat(&self, native: SeatId) -> Option<Uuid> {
        self.roster
            .iter()
            .find(|seat| seat.native_seat == native)
            .map(|seat| seat.seat_id)
    }

    fn native_seat(&self, viewer: Uuid) -> Option<SeatId> {
        self.roster
            .iter()
            .find(|seat| seat.seat_id == viewer)
            .map(|seat| seat.native_seat)
    }

    /// Project a seat-safe `SeatKnowledgeView` strictly for `viewer`.
    ///
    /// Never accesses any other seat's `PerSeatKnowledge` and never emits raw native IDs.
    pub fn view_for_seat(
        &self,
        viewer: Uuid,
        state: &GameState,
        display: &dyn NativeDisplaySource,
        scope: Option<&NativeScope<'_>>,
        snapshot: &PlayerSnapshot,
    ) -> SeatKnowledgeView {
        let Some(viewer_native) = self.native_seat(viewer) else {
            return SeatKnowledgeView::from_snapshot_only(snapshot);
        };
        let Some(seat_state) = self.seats.get(&viewer_native) else {
            return SeatKnowledgeView::from_snapshot_only(snapshot);
        };

        let cards = state.execution().window().cards();
        let mut known_opponent_hand = Vec::new();
        for (&id, entry) in &seat_state.known_opponent_hand {
            let Some(live_obj) = cards.get(id) else {
                continue;
            };
            if live_obj.zone() != ObjectZone::Hand(entry.owner) {
                continue;
            }
            let Some(owner_seat_id) = self.app_seat(entry.owner) else {
                continue;
            };
            let handle = scope.and_then(|s| s.handle_of_object(id));
            let characteristics = resolve_characteristics(state, display, live_obj, &entry.object);
            known_opponent_hand.push(KnownOpponentHandCard {
                owner_seat_id,
                handle,
                characteristics,
                source: entry.source,
                revealed_turn: entry.revealed_turn,
                revealed_revision: entry.revealed_revision,
            });
        }
        known_opponent_hand.sort_by(|a, b| {
            (
                a.owner_seat_id,
                &a.characteristics.name,
                a.revealed_revision,
            )
                .cmp(&(
                    b.owner_seat_id,
                    &b.characteristics.name,
                    b.revealed_revision,
                ))
        });

        let mut visible_library_cards = Vec::new();
        for (&id, entry) in &seat_state.known_library {
            let Some(live_obj) = cards.get(id) else {
                continue;
            };
            if live_obj.zone() != ObjectZone::Library(entry.owner) {
                continue;
            }
            let Some(owner_seat_id) = self.app_seat(entry.owner) else {
                continue;
            };
            let lib = cards.library(entry.owner);
            let Some(pos_idx) = lib.iter().position(|&item| item == id) else {
                continue;
            };
            let position = match entry.confidence {
                LibraryPositionConfidence::BottomUnordered => {
                    VisibleLibraryPosition::BottomUnordered
                }
                LibraryPositionConfidence::ExactOrder => {
                    let from_top = pos_idx + 1;
                    let from_bottom = lib.len().saturating_sub(pos_idx);
                    if from_top == 1 {
                        VisibleLibraryPosition::Top
                    } else if from_bottom == 1 {
                        VisibleLibraryPosition::Bottom
                    } else if from_top <= from_bottom {
                        VisibleLibraryPosition::FromTop { index: from_top }
                    } else {
                        VisibleLibraryPosition::FromBottom { index: from_bottom }
                    }
                }
            };
            let handle = scope.and_then(|s| s.handle_of_object(id));
            let may_play = handle.as_ref().is_some_and(|h| {
                snapshot
                    .visible_objects
                    .iter()
                    .any(|o| &o.handle == h && o.may_play_from_library)
            });
            let characteristics = resolve_characteristics(state, display, live_obj, &entry.object);
            visible_library_cards.push(VisibleLibraryCard {
                owner_seat_id,
                handle,
                characteristics,
                position,
                may_play,
                source: entry.source,
                revealed_turn: entry.revealed_turn,
                revealed_revision: entry.revealed_revision,
            });
        }
        // Deterministic ordering: viewer's own library first, then opponents by seat ID,
        // then exact position order; unordered bottom entries sort alphabetically by name
        // (never by hidden library index).
        visible_library_cards.sort_by(|a, b| {
            let a_mine = a.owner_seat_id != viewer;
            let b_mine = b.owner_seat_id != viewer;
            (
                a_mine,
                a.owner_seat_id,
                a.position.sort_key(),
                &a.characteristics.name,
            )
                .cmp(&(
                    b_mine,
                    b.owner_seat_id,
                    b.position.sort_key(),
                    &b.characteristics.name,
                ))
        });

        let historical_reveals = seat_state
            .historical_reveals
            .iter()
            .filter_map(|rec| {
                let owner_seat_id = self.app_seat(rec.owner)?;
                let ch = resolve_characteristics(state, display, &rec.object, &rec.object);
                Some(HistoricalRevealView {
                    owner_seat_id,
                    card_name: ch.name,
                    source: rec.source,
                    status: rec.status.to_view(),
                    revealed_turn: rec.revealed_turn,
                    revealed_revision: rec.revealed_revision,
                    invalidated_turn: rec.invalidated_turn,
                    invalidated_revision: rec.invalidated_revision,
                })
            })
            .collect();

        SeatKnowledgeView {
            known_opponent_hand,
            visible_library_cards,
            historical_reveals,
        }
    }
}

fn resolve_characteristics(
    state: &GameState,
    display: &dyn NativeDisplaySource,
    primary: &GameObject,
    fallback: &GameObject,
) -> PlayerCharacteristics {
    for candidate in [primary, fallback] {
        let program = state
            .execution()
            .printed_face(candidate)
            .or_else(|| state.execution().base_program(candidate.id()))
            .or_else(|| state.execution().copiable_program(candidate.id()));
        if let Some(program) = program {
            if let Ok(resolved) = display.object(candidate, program) {
                return resolved.characteristics;
            }
            if let Some(name) = program.names.iter().next() {
                return PlayerCharacteristics {
                    name: name.as_str().to_owned(),
                    type_line: String::new(),
                    rules_text: String::new(),
                    mana_cost: None,
                    printed_power: None,
                    printed_toughness: None,
                    printed_loyalty: None,
                    printed_defense: None,
                };
            }
        }
    }
    PlayerCharacteristics {
        name: "Unknown Card".to_owned(),
        type_line: String::new(),
        rules_text: String::new(),
        mana_cost: None,
        printed_power: None,
        printed_toughness: None,
        printed_loyalty: None,
        printed_defense: None,
    }
}

const fn zone_name(zone: ObjectZone) -> &'static str {
    match zone {
        ObjectZone::Library(_) => "library",
        ObjectZone::Hand(_) => "hand",
        ObjectZone::Graveyard(_) => "graveyard",
        ObjectZone::Battlefield => "battlefield",
        ObjectZone::Stack => "stack",
        ObjectZone::Exile => "exile",
        ObjectZone::Command => "command",
    }
}
