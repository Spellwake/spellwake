//! Ordered library selections and two-pile choices. The objects stay in place while players choose.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};
use crate::program::{PublicZoneDestination as Zone, RevealedHandSelection, ZoneActionKind};
use crate::stack_window::StackEntry;

/// A choice among identified objects. Dividing a pile can leave either pile empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PileChoice {
    Divide(Vec<ObjectId>),
    DividePermanents(Vec<ObjectId>),
    Sacrifice {
        first: Vec<ObjectId>,
        second: Vec<ObjectId>,
    },
    Creature(Vec<ObjectId>),
    /// The controller privately chooses one of the looked-at cards to keep.
    OneToHand(Vec<ObjectId>),
    OneToHandRestBottom(Vec<ObjectId>),
    /// Privately select up to one eligible card to enter, then order all other looked-at cards.
    OptionalCreatureToBattlefield {
        looked: Vec<ObjectId>,
        eligible: Vec<ObjectId>,
    },
    /// Select up to `max` distinct eligible cards from the looked-at or revealed cards.
    OptionalUpTo {
        looked: Vec<ObjectId>,
        eligible: Vec<ObjectId>,
        max: usize,
    },
    Take {
        first: Vec<ObjectId>,
        second: Vec<ObjectId>,
    },
    GraveyardOrder(Vec<ObjectId>),
    BottomOrder(Vec<ObjectId>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PileChoiceFrame {
    revision: u64,
    pub(super) chooser: SeatId,
    choice: PileChoice,
}
impl PileChoiceFrame {
    pub(super) fn graveyard_order(revision: u64, chooser: SeatId, domain: Vec<ObjectId>) -> Self {
        Self {
            revision,
            chooser,
            choice: PileChoice::GraveyardOrder(domain),
        }
    }
    pub(super) fn validate_graveyard_order(&self, order: &[ObjectId]) -> Result<(), EffectError> {
        if matches!(&self.choice, PileChoice::GraveyardOrder(domain)
            if order.len() == domain.len() && order.iter().collect::<BTreeSet<_>>() == domain.iter().collect())
        {
            Ok(())
        } else {
            Err(EffectError::InvalidLibraryPartition)
        }
    }

    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn choice(&self) -> &PileChoice {
        &self.choice
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RevealedPiles {
    pub(super) cards: Vec<ObjectId>,
    creature: Option<Option<ObjectId>>,
    entry_offer: Option<EntryChoiceFrame>,
    pub(super) aura_offer: Option<LinkedAuraChoiceFrame>,
    entry_choice: Option<crate::land::EntryChoice>,
    first: Option<Vec<ObjectId>>,
    pub(super) hand: Option<Vec<ObjectId>>,
    pub(super) secondary_hand: Option<Vec<ObjectId>>,
    pub(super) pending_random_bottom: Option<Vec<ObjectId>>,
    batches: Vec<(Zone, Vec<ObjectId>, ZoneReplacementBatch)>,
    graveyard_order: Option<Vec<ObjectId>>,
    pub(super) bottom_order: Option<Vec<ObjectId>>,
    offered: Option<PileChoiceFrame>,
}
impl RevealedPiles {
    pub(super) fn new(cards: Vec<ObjectId>) -> Self {
        Self {
            cards,
            creature: None,
            entry_offer: None,
            aura_offer: None,
            entry_choice: None,
            first: None,
            hand: None,
            secondary_hand: None,
            pending_random_bottom: None,
            batches: Vec::new(),
            graveyard_order: None,
            bottom_order: None,
            offered: None,
        }
    }
}
impl EffectWindow {
    pub(super) fn progress_permanent_piles(
        &mut self,
        entry: &StackEntry,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let TargetRef::Player(player) = self.instruction_target(entry)?.choice().target else {
            return Err(EffectError::InconsistentProgram);
        };
        let controller = entry.controller();
        let cursor = self
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let mut work = if let Some(work) = cursor.revealed_piles.take() {
            work
        } else {
            let cards = self
                .evaluated_characteristics()?
                .objects
                .values()
                .filter(|object| {
                    object.zone == ObjectZone::Battlefield
                        && object.controller == Some(player)
                        && self
                            .window
                            .cards()
                            .phased_out_under(object.object)
                            .is_none()
                })
                .map(|object| object.object)
                .collect::<Vec<_>>();
            if cards.is_empty() {
                self.advance_instruction()?;
                return Ok(None);
            }
            Box::new(RevealedPiles::new(cards))
        };
        if let Some(pile) = work.hand.clone() {
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .revealed_piles = Some(work);
            self.begin_fixed_sacrifice(controller, player, &pile)?;
            return Ok(None);
        }
        let (chooser, choice) = match &work.first {
            Some(first) => (
                player,
                PileChoice::Sacrifice {
                    first: first.clone(),
                    second: work
                        .cards
                        .iter()
                        .copied()
                        .filter(|id| !first.contains(id))
                        .collect(),
                },
            ),
            None => (controller, PileChoice::DividePermanents(work.cards.clone())),
        };
        let frame = PileChoiceFrame {
            revision: self.revision,
            chooser,
            choice,
        };
        work.offered = Some(frame.clone());
        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .revealed_piles = Some(work);
        Ok(Some(EffectDecision::RevealedPiles(frame)))
    }

    fn revealed_batch(
        &self,
        entry: StackEntryId,
        instruction: usize,
        player: SeatId,
        zone: Zone,
        cards: &[ObjectId],
    ) -> Result<ZoneReplacementBatch, EffectError> {
        let (action, reason) = match zone {
            Zone::Battlefield => (
                ZoneActionKind::EffectEnterBattlefield,
                ZoneReason::LookTopToBattlefield,
            ),
            Zone::Hand => (ZoneActionKind::EffectLookToHand, ZoneReason::LookTopToHand),
            Zone::Graveyard => (
                ZoneActionKind::EffectRevealedToGraveyard,
                ZoneReason::RevealedToGraveyard,
            ),
            Zone::Exile => (ZoneActionKind::EffectExile, ZoneReason::ExiledByEffect),
            _ => return Err(EffectError::InconsistentProgram),
        };
        let input = zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::Instruction { entry, instruction },
            action,
            intents: cards
                .iter()
                .map(|id| zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason,
                })
                .collect(),
        };
        if zone == Zone::Battlefield {
            self.begin_zone_replacements_with_entry(
                input,
                Some(&cards.iter().map(|&id| (id, player)).collect()),
            )
        } else {
            self.begin_zone_replacements(input)
        }
    }
    pub(super) fn progress_revealed_piles(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        player: SeatId,
        journal: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let revealed_name = match self.current_instruction()? {
            Instruction::RevealToNameAfterSourceShuffle(name) => Some(name.clone()),
            _ => None,
        };
        let hand_filter = match self.current_instruction()? {
            Instruction::RevealTopHandRestGraveyard { hand, .. } => Some(*hand),
            _ => None,
        };
        let look_top_choose = match self.current_instruction()? {
            Instruction::LookTopChoose {
                filter,
                reveal_only,
                to_battlefield,
                enters_tapped,
                take_all_matching,
                max_picks,
                rest_to_graveyard,
                rest_to_exile,
                rest_to_top,
                random_rest_order,
                secondary,
                ..
            } => Some((
                filter.clone(),
                *reveal_only,
                *to_battlefield,
                *enters_tapped,
                *take_all_matching,
                *max_picks,
                *rest_to_graveyard,
                *rest_to_exile,
                *rest_to_top,
                *random_rest_order,
                secondary.clone(),
            )),
            _ => None,
        };
        let reveal_until = match self.current_instruction()? {
            Instruction::RevealUntilMatch {
                filter,
                count,
                to_battlefield,
                enters_tapped,
                keep_all_revealed,
                optional_keep,
                exile_instead_of_reveal,
                rest_to_graveyard,
                rest_to_exile,
                rest_to_bottom,
                random_rest_order,
            } => Some((
                filter.clone(),
                *count,
                *to_battlefield,
                *enters_tapped,
                *keep_all_revealed,
                *optional_keep,
                *exile_instead_of_reveal,
                *rest_to_graveyard,
                *rest_to_exile,
                *rest_to_bottom,
                *random_rest_order,
            )),
            _ => None,
        };
        let look_top_choose_ext = match self.current_instruction()? {
            Instruction::LookTopChooseExt {
                chooser,
                filter,
                max_picks,
                take_all_matching,
                keep_dest,
                rest_dest,
                reveal_kept,
                ..
            } => Some((
                *chooser,
                filter.clone(),
                *max_picks,
                *take_all_matching,
                *keep_dest,
                *rest_dest,
                *reveal_kept,
            )),
            _ => None,
        };
        let reveal_until_ext = match self.current_instruction()? {
            Instruction::RevealUntilMatchExt {
                chooser,
                filter,
                count,
                match_dest,
                rest_dest,
                ..
            } => Some((
                *chooser,
                filter.clone(),
                count.clone(),
                *match_dest,
                *rest_dest,
            )),
            _ => None,
        };
        let reveal_two_piles = match self.current_instruction()? {
            Instruction::RevealTopTwoPiles {
                controller_divides,
                target_opponent,
                chosen_dest,
                other_dest,
                ..
            } => Some((
                *controller_divides,
                *target_opponent,
                *chosen_dest,
                *other_dest,
            )),
            _ => None,
        };
        let ext_keep_dest = look_top_choose_ext
            .as_ref()
            .map(|(_, _, _, _, keep_dest, ..)| *keep_dest)
            .or_else(|| {
                reveal_until_ext
                    .as_ref()
                    .map(|(_, _, _, match_dest, _)| *match_dest)
            })
            .or_else(|| reveal_two_piles.map(|(_, _, chosen_dest, _)| chosen_dest));
        let ext_rest_dest = look_top_choose_ext
            .as_ref()
            .map(|(_, _, _, _, _, rest_dest, _)| *rest_dest)
            .or_else(|| {
                reveal_until_ext
                    .as_ref()
                    .map(|(_, _, _, _, rest_dest)| *rest_dest)
            })
            .or_else(|| reveal_two_piles.map(|(_, _, _, other_dest)| other_dest));
        let (count, private, take_all, other_pile_to_bottom) = match self.current_instruction()? {
            Instruction::RevealTopTwoPiles {
                count, other_dest, ..
            } => (
                self.resolve_value_expr(
                    self.window.resolving().map_or(player, |e| e.controller()),
                    count,
                )?
                .max(0.into())
                .min(RulesInteger::from_usize(
                    self.window.cards().library(player).len(),
                ))
                .to_usize()
                .ok_or(EffectError::InconsistentProgram)?,
                false,
                false,
                matches!(
                    other_dest,
                    crate::program::LookTopRestDest::BottomRandom
                        | crate::program::LookTopRestDest::BottomOrder
                        | crate::program::LookTopRestDest::TopOrder
                ),
            ),
            Instruction::LookTopChoose {
                count,
                reveal,
                reveal_only,
                rest_to_graveyard,
                rest_to_exile,
                ..
            } => (
                self.resolve_value_expr(
                    self.window.resolving().map_or(player, |e| e.controller()),
                    count,
                )?
                .max(0.into())
                .min(RulesInteger::from_usize(
                    self.window.cards().library(player).len(),
                ))
                .to_usize()
                .ok_or(EffectError::InconsistentProgram)?,
                !*reveal && !*reveal_only,
                false,
                !*reveal_only && !*rest_to_graveyard && !*rest_to_exile,
            ),
            Instruction::LookTopChooseExt {
                count, rest_dest, ..
            } => (
                self.resolve_value_expr(
                    self.window.resolving().map_or(player, |e| e.controller()),
                    count,
                )?
                .max(0.into())
                .min(RulesInteger::from_usize(
                    self.window.cards().library(player).len(),
                ))
                .to_usize()
                .ok_or(EffectError::InconsistentProgram)?,
                true,
                false,
                matches!(
                    rest_dest,
                    crate::program::LookTopRestDest::BottomRandom
                        | crate::program::LookTopRestDest::BottomOrder
                        | crate::program::LookTopRestDest::TopOrder
                ),
            ),
            Instruction::RevealUntilMatch { rest_to_bottom, .. } => {
                (usize::MAX, false, false, *rest_to_bottom)
            }
            Instruction::RevealUntilMatchExt {
                count, rest_dest, ..
            } => {
                let resolved_need = self
                    .resolve_value_expr(
                        self.window.resolving().map_or(player, |e| e.controller()),
                        count,
                    )?
                    .max(0.into());
                let max_cards = if resolved_need.is_positive() {
                    usize::MAX
                } else {
                    0
                };
                (
                    max_cards,
                    false,
                    false,
                    matches!(
                        rest_dest,
                        crate::program::LookTopRestDest::BottomRandom
                            | crate::program::LookTopRestDest::BottomOrder
                            | crate::program::LookTopRestDest::TopOrder
                    ),
                )
            }
            Instruction::LookTopOneRestGraveyard { count } => {
                (usize::from(*count), true, false, false)
            }
            Instruction::LookTopOneRestBottom { count } => (
                self.resolve_value_expr(player, count)?
                    .max(0.into())
                    .min(RulesInteger::from_usize(
                        self.window.cards().library(player).len(),
                    ))
                    .to_usize()
                    .ok_or(EffectError::InconsistentProgram)?,
                true,
                false,
                true,
            ),
            Instruction::LookTopCreatureSharingAttachedSubtype { count } => {
                (usize::from(*count), true, false, true)
            }
            Instruction::RevealTopCreaturesAndLandsRestBottom { count } => {
                (usize::from(*count), false, true, true)
            }
            Instruction::RevealHandAndBottomPiles => (3, false, false, true),
            Instruction::RevealToNameAfterSourceShuffle(_) => (usize::MAX, false, false, false),
            Instruction::RevealTopHandRestGraveyard { count, .. } => {
                (usize::from(*count), false, false, false)
            }
            _ => (5, false, false, false),
        };
        let creature_choice = revealed_name.is_some()
            || matches!(
                self.current_instruction()?,
                Instruction::RevealOpponentChoosesCreature
            );
        let share_attached_type = matches!(
            self.current_instruction()?,
            Instruction::LookTopCreatureSharingAttachedSubtype { .. }
        );
        let mut work = if let Some(work) = self
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .revealed_piles
            .take()
        {
            work
        } else {
            let library = self.window.cards().library(player);
            let mut reveal_until_matches: Vec<ObjectId> = Vec::new();
            let named = if let Some(name) = &revealed_name {
                let evaluated = self.evaluated_characteristics()?;
                library.iter().position(|id| {
                    evaluated
                        .get(*id)
                        .is_some_and(|card| self.name_rules().has_name(&card.current_names, name))
                })
            } else if let Some((filter, target_count, ..)) = &reveal_until {
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let need = usize::from((*target_count).max(1));
                let mut stop_idx = None;
                for (idx, id) in library.iter().enumerate() {
                    if evaluated.get(*id).is_some_and(|card| {
                        filter.matches_static_characteristics(card, &excluded, || {
                            self.copiable_program(*id).map(Program::mana_value)
                        })
                    }) {
                        reveal_until_matches.push(*id);
                        if reveal_until_matches.len() >= need {
                            stop_idx = Some(idx);
                            break;
                        }
                    }
                }
                stop_idx
            } else if let Some((_, filter, target_count_expr, ..)) = &reveal_until_ext {
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let need = self
                    .resolve_value_expr(
                        self.window.resolving().map_or(player, |e| e.controller()),
                        target_count_expr,
                    )?
                    .max(0.into())
                    .to_usize()
                    .unwrap_or(0);
                let mut stop_idx = None;
                if need > 0 {
                    for (idx, id) in library.iter().enumerate() {
                        if evaluated.get(*id).is_some_and(|card| {
                            filter.matches_static_characteristics(card, &excluded, || {
                                self.copiable_program(*id).map(Program::mana_value)
                            })
                        }) {
                            reveal_until_matches.push(*id);
                            if reveal_until_matches.len() >= need {
                                stop_idx = Some(idx);
                                break;
                            }
                        }
                    }
                }
                stop_idx
            } else {
                None
            };
            let cards: Vec<_> = library
                .iter()
                .take(named.map_or(count, |index| index + 1))
                .copied()
                .collect();
            let named_card = named.map(|index| library[index]);
            if cards.is_empty() {
                if (look_top_choose_ext.is_some() || reveal_until_ext.is_some())
                    && let Some(cursor) = self.resolution_cursor_mut()
                {
                    cursor.results.last_action_succeeded = false;
                    cursor.results.last_action_count = Some(0.into());
                    cursor.results.last_moved_objects.clear();
                }
                return Ok(None);
            }
            if !private {
                let objects = cards
                    .iter()
                    .map(|id| {
                        self.window
                            .cards()
                            .get(*id)
                            .cloned()
                            .ok_or(EffectError::UnknownObject)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let before = self.rules_snapshot(None)?;
                self.emit_from(
                    EventCause::Instruction { entry, instruction },
                    &before,
                    None,
                    journal,
                    |_, _, _| {
                        Ok(objects
                            .into_iter()
                            .map(|object| RulesOccurrence::LibraryCardRevealed { object })
                            .collect())
                    },
                )?;
            }
            if look_top_choose
                .as_ref()
                .is_some_and(|(_, reveal_only, ..)| *reveal_only)
            {
                let evaluated = self.evaluated_characteristics()?;
                let revealed_types: BTreeSet<CardType> = cards
                    .iter()
                    .filter_map(|id| evaluated.get(*id))
                    .flat_map(|card| card.current_types.iter().copied())
                    .collect();
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.revealed_top_types = revealed_types;
                    cursor.results.revealed_top_card = cards.first().copied();
                }
                return Ok(None);
            }
            let mut work = RevealedPiles::new(cards);
            if revealed_name.is_some() {
                work.creature = Some(named_card);
            } else if let Some((_, _, _, _, keep_all_revealed, optional_keep, ..)) = &reveal_until {
                if *keep_all_revealed {
                    work.hand = Some(work.cards.clone());
                } else if !*optional_keep || reveal_until_matches.is_empty() {
                    work.hand = Some(reveal_until_matches);
                }
            } else if reveal_until_ext.is_some() {
                work.hand = Some(reveal_until_matches);
            }
            Box::new(work)
        };
        let opponents = self
            .combat_rules
            .opponents(self.window.participants(), player)?;
        let opponent = if reveal_two_piles.is_some_and(|(_, target_opp, ..)| target_opp) {
            let Some(resolving) = self.window.resolving() else {
                return Err(EffectError::WrongResolutionFrame);
            };
            let TargetRef::Player(opp) = self.instruction_target(resolving)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            opp
        } else if private
            || take_all
            || hand_filter.is_some()
            || revealed_name.is_some()
            || look_top_choose.is_some()
            || look_top_choose_ext.is_some()
            || reveal_until.is_some()
            || reveal_until_ext.is_some()
        {
            player
        } else {
            match opponents.as_slice() {
                [] => return Ok(None),
                [only] => *only,
                _ => match self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.chosen_opponent)
                {
                    Some(opponent) => opponent,
                    None => {
                        self.resolution_cursor_mut()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .revealed_piles = Some(work);
                        return Ok(Some(EffectDecision::OpponentChoice(OpponentChoiceFrame {
                            revision: self.revision,
                            entry,
                            instruction,
                            chooser: player,
                            opponents,
                        })));
                    }
                },
            }
        };
        let chooser = if let Some(role) = look_top_choose_ext
            .as_ref()
            .map(|(role, ..)| *role)
            .or_else(|| reveal_until_ext.as_ref().map(|(role, ..)| *role))
        {
            let cursor_snap = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .clone();
            self.resolving_player_identity_at(role, Some(&cursor_snap))?
        } else {
            self.window.resolving().map_or(player, |e| e.controller())
        };
        let choice = if reveal_until_ext.is_some() {
            None
        } else if let Some((_, filter, max_picks, take_all_matching, _, rest_dest, _)) =
            &look_top_choose_ext
        {
            if work.hand.is_some() {
                None
            } else if !*take_all_matching && *max_picks == 0 {
                work.hand = Some(Vec::new());
                None
            } else if let Some(filter) = filter {
                let current = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let eligible: Vec<_> = work
                    .cards
                    .iter()
                    .copied()
                    .filter(|id| {
                        current.get(*id).is_some_and(|card| {
                            filter.matches_static_characteristics(card, &excluded, || {
                                self.copiable_program(*id).map(Program::mana_value)
                            })
                        })
                    })
                    .collect();
                if eligible.is_empty() || *take_all_matching {
                    work.hand = Some(eligible);
                    None
                } else if *max_picks > 1 {
                    Some((
                        chooser,
                        PileChoice::OptionalUpTo {
                            looked: work.cards.clone(),
                            eligible,
                            max: usize::from(*max_picks),
                        },
                    ))
                } else {
                    Some((
                        chooser,
                        PileChoice::OptionalCreatureToBattlefield {
                            looked: work.cards.clone(),
                            eligible,
                        },
                    ))
                }
            } else if *take_all_matching || work.cards.len() <= usize::from((*max_picks).max(1)) {
                work.hand = Some(work.cards.clone());
                None
            } else if *max_picks > 1 {
                Some((
                    chooser,
                    PileChoice::OptionalUpTo {
                        looked: work.cards.clone(),
                        eligible: work.cards.clone(),
                        max: usize::from(*max_picks),
                    },
                ))
            } else {
                let choice = if matches!(
                    rest_dest,
                    crate::program::LookTopRestDest::BottomOrder
                        | crate::program::LookTopRestDest::BottomRandom
                ) {
                    PileChoice::OneToHandRestBottom(work.cards.clone())
                } else {
                    PileChoice::OneToHand(work.cards.clone())
                };
                Some((chooser, choice))
            }
        } else if let Some((filter, _, _, _, _, optional_keep, ..)) = &reveal_until {
            if work.hand.is_some() || !*optional_keep {
                None
            } else {
                let current = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let eligible: Vec<_> = work
                    .cards
                    .iter()
                    .copied()
                    .filter(|id| {
                        current.get(*id).is_some_and(|card| {
                            filter.matches_static_characteristics(card, &excluded, || {
                                self.copiable_program(*id).map(Program::mana_value)
                            })
                        })
                    })
                    .collect();
                if eligible.is_empty() {
                    work.hand = Some(Vec::new());
                    None
                } else if eligible.len() > 1 {
                    Some((
                        chooser,
                        PileChoice::OptionalUpTo {
                            looked: work.cards.clone(),
                            eligible: eligible.clone(),
                            max: eligible.len(),
                        },
                    ))
                } else {
                    Some((
                        chooser,
                        PileChoice::OptionalCreatureToBattlefield {
                            looked: work.cards.clone(),
                            eligible,
                        },
                    ))
                }
            }
        } else if let Some((
            filter,
            _reveal_only,
            _to_battlefield,
            _enters_tapped,
            take_all_matching,
            max_picks,
            _rest_to_graveyard,
            _rest_to_exile,
            _rest_to_top,
            _random_rest_order,
            secondary,
        )) = &look_top_choose
        {
            if let Some(primary_chosen) = &work.hand {
                if let Some(sec) = secondary
                    && work.secondary_hand.is_none()
                {
                    let current = self.evaluated_characteristics()?;
                    let excluded = BTreeSet::new();
                    let eligible: Vec<_> = work
                        .cards
                        .iter()
                        .copied()
                        .filter(|id| {
                            !primary_chosen.contains(id)
                                && current.get(*id).is_some_and(|card| {
                                    sec.filter.matches_static_characteristics(
                                        card,
                                        &excluded,
                                        || self.copiable_program(*id).map(Program::mana_value),
                                    )
                                })
                        })
                        .collect();
                    if eligible.is_empty() || sec.max_picks == 0 {
                        work.secondary_hand = Some(Vec::new());
                        None
                    } else if sec.max_picks > 1 {
                        Some((
                            chooser,
                            PileChoice::OptionalUpTo {
                                looked: work.cards.clone(),
                                eligible,
                                max: usize::from(sec.max_picks),
                            },
                        ))
                    } else {
                        Some((
                            chooser,
                            PileChoice::OptionalCreatureToBattlefield {
                                looked: work.cards.clone(),
                                eligible,
                            },
                        ))
                    }
                } else {
                    None
                }
            } else if !*take_all_matching && *max_picks == 0 {
                work.hand = Some(Vec::new());
                None
            } else if let Some(filter) = filter {
                let current = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let eligible: Vec<_> = work
                    .cards
                    .iter()
                    .copied()
                    .filter(|id| {
                        current.get(*id).is_some_and(|card| {
                            filter.matches_static_characteristics(card, &excluded, || {
                                self.copiable_program(*id).map(Program::mana_value)
                            })
                        })
                    })
                    .collect();
                if eligible.is_empty() || *take_all_matching {
                    work.hand = Some(eligible);
                    None
                } else if *max_picks > 1 {
                    Some((
                        chooser,
                        PileChoice::OptionalUpTo {
                            looked: work.cards.clone(),
                            eligible,
                            max: usize::from(*max_picks),
                        },
                    ))
                } else {
                    Some((
                        chooser,
                        PileChoice::OptionalCreatureToBattlefield {
                            looked: work.cards.clone(),
                            eligible,
                        },
                    ))
                }
            } else if *take_all_matching || work.cards.len() <= usize::from((*max_picks).max(1)) {
                work.hand = Some(work.cards.clone());
                None
            } else if *max_picks > 1 {
                Some((
                    chooser,
                    PileChoice::OptionalUpTo {
                        looked: work.cards.clone(),
                        eligible: work.cards.clone(),
                        max: usize::from(*max_picks),
                    },
                ))
            } else {
                let choice = if other_pile_to_bottom {
                    PileChoice::OneToHandRestBottom(work.cards.clone())
                } else {
                    PileChoice::OneToHand(work.cards.clone())
                };
                Some((chooser, choice))
            }
        } else if let Some(hand_filter) = hand_filter {
            if work.hand.is_some() {
                None
            } else {
                let current = self.evaluated_characteristics()?;
                let eligible: Vec<_> = work
                    .cards
                    .iter()
                    .copied()
                    .filter(|id| {
                        current.get(*id).is_some_and(|card| match hand_filter {
                            RevealedHandSelection::AllLands => card.has_type(CardType::Land),
                            RevealedHandSelection::OneCreature => card.is_creature(),
                        })
                    })
                    .collect();
                if hand_filter == RevealedHandSelection::AllLands || eligible.is_empty() {
                    work.hand = Some(eligible);
                    None
                } else {
                    Some((player, PileChoice::OneToHand(eligible)))
                }
            }
        } else if take_all {
            if work.hand.is_none() {
                let current = self.evaluated_characteristics()?;
                work.hand = Some(
                    work.cards
                        .iter()
                        .copied()
                        .filter(|id| {
                            current.get(*id).is_some_and(|card| {
                                card.is_creature() || card.has_type(CardType::Land)
                            })
                        })
                        .collect(),
                );
            }
            None
        } else if share_attached_type {
            if work.hand.is_some() {
                None
            } else {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let StackKind::TriggeredAbility { source } = entry.kind() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let current = self.evaluated_characteristics()?;
                let kinds = match self.attachment_host_for_ability(source.id()) {
                    Some(TargetRef::Object(host)) => current
                        .get(host)
                        .filter(|value| value.zone == ObjectZone::Battlefield)
                        .map(|value| &value.current_creature_subtypes)
                        .or_else(|| {
                            self.growth_last_known
                                .get(&host)
                                .map(|value| &value.creature_subtypes)
                        }),
                    _ => None,
                };
                let eligible = work
                    .cards
                    .iter()
                    .copied()
                    .filter(|id| {
                        current.get(*id).is_some_and(|value| {
                            value.is_creature()
                                && kinds.is_some_and(|kinds| {
                                    !kinds.is_disjoint(&value.current_creature_subtypes)
                                })
                        })
                    })
                    .collect::<Vec<_>>();
                Some((
                    player,
                    PileChoice::OptionalCreatureToBattlefield {
                        looked: work.cards.clone(),
                        eligible,
                    },
                ))
            }
        } else if private {
            if work.hand.is_some() {
                None
            } else if work.cards.len() == 1 {
                work.hand = Some(work.cards.clone());
                None
            } else {
                let choice = if other_pile_to_bottom {
                    PileChoice::OneToHandRestBottom(work.cards.clone())
                } else {
                    PileChoice::OneToHand(work.cards.clone())
                };
                Some((player, choice))
            }
        } else if creature_choice {
            if work.creature.is_some() {
                None
            } else {
                let evaluated = self.evaluated_characteristics()?;
                let eligible: Vec<_> = work
                    .cards
                    .iter()
                    .copied()
                    .filter(|id| evaluated.get(*id).is_some_and(|value| value.is_creature()))
                    .collect();
                match eligible.as_slice() {
                    [] => {
                        work.creature = Some(None);
                        None
                    }
                    [only] => {
                        work.creature = Some(Some(*only));
                        None
                    }
                    _ => Some((opponent, PileChoice::Creature(eligible))),
                }
            }
        } else if let Some(first) = &work.first {
            if work.hand.is_none() {
                let second = work
                    .cards
                    .iter()
                    .copied()
                    .filter(|id| !first.contains(id))
                    .collect();
                let taker = if reveal_two_piles.is_some_and(|(ctrl_div, ..)| ctrl_div) {
                    opponent
                } else {
                    player
                };
                Some((
                    taker,
                    PileChoice::Take {
                        first: first.clone(),
                        second,
                    },
                ))
            } else {
                None
            }
        } else {
            let divider = if reveal_two_piles.is_some_and(|(ctrl_div, ..)| ctrl_div) {
                player
            } else {
                opponent
            };
            Some((divider, PileChoice::Divide(work.cards.clone())))
        };
        if let Some((chooser, choice)) = choice {
            let frame = PileChoiceFrame {
                revision: self.revision,
                chooser,
                choice,
            };
            work.offered = Some(frame.clone());
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .revealed_piles = Some(work);
            return Ok(Some(EffectDecision::RevealedPiles(frame)));
        }
        let keep_stays_on_top = matches!(
            ext_keep_dest,
            Some(crate::program::LookTopKeepDest::LibraryTop)
        );
        if work.batches.is_empty() {
            let (chosen_zone, chosen) = if creature_choice {
                (
                    Zone::Battlefield,
                    work.creature
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .into_iter()
                        .collect::<Vec<_>>(),
                )
            } else if let Some(keep_dest) = ext_keep_dest {
                let zone = match keep_dest {
                    crate::program::LookTopKeepDest::Hand
                    | crate::program::LookTopKeepDest::LibraryTop => Zone::Hand,
                    crate::program::LookTopKeepDest::Battlefield { .. } => Zone::Battlefield,
                    crate::program::LookTopKeepDest::Graveyard => Zone::Graveyard,
                    crate::program::LookTopKeepDest::Exile { .. } => Zone::Exile,
                };
                (
                    zone,
                    work.hand.clone().ok_or(EffectError::InconsistentProgram)?,
                )
            } else {
                (
                    if share_attached_type
                        || look_top_choose
                            .as_ref()
                            .is_some_and(|(_, _, to_battlefield, ..)| *to_battlefield)
                        || reveal_until
                            .as_ref()
                            .is_some_and(|(_, _, to_battlefield, ..)| *to_battlefield)
                    {
                        Zone::Battlefield
                    } else {
                        Zone::Hand
                    },
                    work.hand.clone().ok_or(EffectError::InconsistentProgram)?,
                )
            };
            let secondary_chosen = work.secondary_hand.clone().unwrap_or_default();
            let secondary_zone = look_top_choose
                .as_ref()
                .and_then(|(.., sec)| sec.as_ref())
                .map_or(Zone::Hand, |sec| {
                    if sec.to_battlefield {
                        Zone::Battlefield
                    } else {
                        Zone::Hand
                    }
                });
            let remainder: Vec<_> = work
                .cards
                .iter()
                .copied()
                .filter(|id| !chosen.contains(id) && !secondary_chosen.contains(id))
                .collect();
            let random_rest_order = look_top_choose
                .as_ref()
                .is_some_and(|(_, _, _, _, _, _, _, _, _, random_rest, _)| *random_rest)
                || reveal_until
                    .as_ref()
                    .is_some_and(|(.., random_rest)| *random_rest)
                || matches!(
                    ext_rest_dest,
                    Some(crate::program::LookTopRestDest::BottomRandom)
                );
            if other_pile_to_bottom && work.bottom_order.is_none() {
                if remainder.len() > 1 {
                    if random_rest_order {
                        work.pending_random_bottom = Some(remainder);
                        self.resolution_cursor_mut()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .revealed_piles = Some(work);
                        let cursor = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .clone();
                        self.pending_work =
                            Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
                                cursor,
                                chooser,
                                options: BTreeSet::new(),
                                rebound_or_buyback: false,
                                cast: None,
                            })));
                        return Ok(None);
                    }
                    let frame = PileChoiceFrame {
                        revision: self.revision,
                        chooser,
                        choice: PileChoice::BottomOrder(remainder),
                    };
                    work.offered = Some(frame.clone());
                    self.resolution_cursor_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .revealed_piles = Some(work);
                    return Ok(Some(EffectDecision::RevealedPiles(frame)));
                }
                work.bottom_order = Some(remainder.clone());
            }
            let rest_zone = if let Some(rest_dest) = ext_rest_dest {
                match rest_dest {
                    crate::program::LookTopRestDest::Exile => Zone::Exile,
                    crate::program::LookTopRestDest::Hand => Zone::Hand,
                    crate::program::LookTopRestDest::Graveyard
                    | crate::program::LookTopRestDest::BottomRandom
                    | crate::program::LookTopRestDest::BottomOrder
                    | crate::program::LookTopRestDest::TopOrder => Zone::Graveyard,
                }
            } else if look_top_choose
                .as_ref()
                .is_some_and(|(_, _, _, _, _, _, _, rest_to_exile, ..)| *rest_to_exile)
                || reveal_until
                    .as_ref()
                    .is_some_and(|(_, _, _, _, _, _, _, _, rest_to_exile, ..)| *rest_to_exile)
            {
                Zone::Exile
            } else {
                Zone::Graveyard
            };
            let leave_rest_in_library = reveal_until.as_ref().is_some_and(
                |(_, _, _, _, _, _, _, rest_to_gy, rest_to_exile, rest_to_bottom, _)| {
                    !*rest_to_gy && !*rest_to_exile && !*rest_to_bottom
                },
            );
            let rest_cards = if other_pile_to_bottom || leave_rest_in_library {
                Vec::new()
            } else {
                remainder
            };
            let kept_for_move = if keep_stays_on_top {
                Vec::new()
            } else {
                chosen
            };
            for (zone, cards) in [
                (chosen_zone, kept_for_move),
                (secondary_zone, secondary_chosen),
                (rest_zone, rest_cards),
            ] {
                if !cards.is_empty() {
                    let batch = self.revealed_batch(entry, instruction, chooser, zone, &cards)?;
                    work.batches.push((zone, cards, batch));
                }
            }
        }
        let mut moves = Vec::new();
        let mut graveyard = Vec::new();
        let mut reasons = BTreeMap::new();
        for (zone, cards, batch) in &mut work.batches {
            if let Some(frame) = batch.progress(self.revision)? {
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .revealed_piles = Some(work);
                return Ok(Some(frame));
            }
            batch.verify_initial(self.revealed_batch(
                entry,
                instruction,
                chooser,
                *zone,
                cards,
            )?)?;
            self.prospective_entry_copies.extend(batch.entry_copies());
            self.prospective_entry_controllers
                .extend(batch.entry_controllers());
            for &id in cards.iter() {
                if batch.entry_prohibited(id) {
                    continue;
                }
                let destination = batch.destination(id)?;
                if destination == Zone::Battlefield {
                    let controller = batch.entry_controller(id).unwrap_or(player);
                    if let Some(domain) = self.linked_aura_domain(id, controller, batch.evaluated())
                    {
                        if domain.is_empty() {
                            self.prospective_entry_copies.remove(&id);
                            self.prospective_entry_controllers.remove(&id);
                            continue;
                        }
                        if let Some(host) = self.resolution_aura_choices.get(&id) {
                            if !domain.contains(host) {
                                return Err(EffectError::IllegalTarget);
                            }
                        } else {
                            let frame = LinkedAuraChoiceFrame {
                                revision: self.revision,
                                chooser: controller,
                                object: id,
                                domain,
                            };
                            work.aura_offer = Some(frame.clone());
                            self.resolution_cursor_mut()
                                .ok_or(EffectError::WrongResolutionFrame)?
                                .revealed_piles = Some(work);
                            return Ok(Some(EffectDecision::LinkedAuraChoice(frame)));
                        }
                    }
                }
                reasons.insert(
                    id,
                    match zone {
                        Zone::Hand => ZoneReason::LookTopToHand,
                        Zone::Battlefield => ZoneReason::LookTopToBattlefield,
                        Zone::Exile => ZoneReason::ExiledByEffect,
                        _ => ZoneReason::RevealedToGraveyard,
                    },
                );
                if destination == Zone::Graveyard {
                    graveyard.push(id);
                } else {
                    moves.push((
                        id,
                        destination
                            .physical(Some(chooser))
                            .ok_or(EffectError::InconsistentProgram)?,
                    ));
                }
            }
        }
        if graveyard.len() > 1 && work.graveyard_order.is_none() {
            let frame = PileChoiceFrame::graveyard_order(self.revision, player, graveyard);
            work.offered = Some(frame.clone());
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .revealed_piles = Some(work);
            return Ok(Some(EffectDecision::RevealedPiles(frame)));
        }
        if let Some((&(id, _), frame)) = moves
            .iter()
            .find(|(_, destination)| matches!(destination, ObjectDestination::Battlefield { .. }))
            .map(|movement| {
                Ok::<_, EffectError>((
                    movement,
                    self.object_entry_choice_frame(
                        match movement.1 {
                            ObjectDestination::Battlefield { controller } => controller,
                            _ => chooser,
                        },
                        movement.0,
                    )?,
                ))
            })
            .transpose()?
            && let Some(frame) = frame
            && work.entry_choice.is_none()
        {
            debug_assert_eq!(frame.object(), id);
            work.entry_offer = Some(frame.clone());
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .revealed_piles = Some(work);
            return Ok(Some(EffectDecision::EntryChoice(frame)));
        }
        for (_, _, batch) in &work.batches {
            batch.commit_devour(self, journal)?;
        }
        let order = work.graveyard_order.unwrap_or(graveyard);
        moves.extend(
            order
                .into_iter()
                .rev()
                .map(|id| (id, ObjectDestination::Graveyard)),
        );
        if look_top_choose_ext
            .as_ref()
            .is_some_and(|(.., reveal_kept)| *reveal_kept)
            && let Some(kept) = work.hand.as_ref()
            && !kept.is_empty()
        {
            let objects = kept
                .iter()
                .map(|id| {
                    self.window
                        .cards()
                        .get(*id)
                        .cloned()
                        .ok_or(EffectError::UnknownObject)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let reveal_before = self.rules_snapshot(None)?;
            self.emit_from(
                EventCause::Instruction { entry, instruction },
                &reveal_before,
                None,
                journal,
                |_, _, _| {
                    Ok(objects
                        .into_iter()
                        .map(|object| RulesOccurrence::LibraryCardRevealed { object })
                        .collect())
                },
            )?;
        }
        let before = self.rules_snapshot(None)?;
        if !self.window.cards().library(player).starts_with(&work.cards) {
            return Err(EffectError::InvalidLibraryPartition);
        }
        let mut movements = if moves.is_empty() {
            Vec::new()
        } else {
            let (window, movements) = self.window.record_library_search_moves(
                self.window.revision(),
                entry,
                player,
                chooser,
                &moves,
            )?;
            self.window = window;
            movements
        };
        let rest_is_top = look_top_choose
            .as_ref()
            .is_some_and(|(_, _, _, _, _, _, _, _, rest_to_top, ..)| *rest_to_top)
            || matches!(
                ext_rest_dest,
                Some(crate::program::LookTopRestDest::TopOrder)
            );
        if keep_stays_on_top {
            let kept_top = work.hand.clone().unwrap_or_default();
            if let Some(rest_order) = work.bottom_order {
                if rest_is_top {
                    let mut combined = kept_top;
                    combined.extend(rest_order);
                    self.top_remaining_looked_at(player, entry, &combined)?;
                } else {
                    let total = kept_top.len() + rest_order.len();
                    if total > 0 {
                        let top_slice = self
                            .window
                            .cards()
                            .library(player)
                            .get(..total)
                            .ok_or(EffectError::InvalidLibraryPartition)?
                            .to_vec();
                        let (window, _) = self.window.record_library_top(
                            self.window.revision(),
                            entry,
                            crate::stack_window::LibraryTopCommit {
                                player,
                                frozen: &top_slice,
                                top: &kept_top,
                                bottom: &rest_order,
                                moves_application: &[],
                            },
                        )?;
                        self.window = window;
                    }
                }
            } else if !kept_top.is_empty() {
                self.top_remaining_looked_at(player, entry, &kept_top)?;
            }
        } else if let Some(bottom) = work.bottom_order {
            if rest_is_top {
                self.top_remaining_looked_at(player, entry, &bottom)?;
            } else {
                self.bottom_remaining_looked_at(player, entry, &bottom)?;
            }
        }
        let mut counters = Vec::new();
        let secondary_ids: BTreeSet<ObjectId> = work
            .secondary_hand
            .as_deref()
            .unwrap_or_default()
            .iter()
            .copied()
            .collect();
        let secondary_tapped = look_top_choose
            .as_ref()
            .and_then(|(.., sec)| sec.as_ref())
            .is_some_and(|sec| sec.enters_tapped);
        for movement in &mut movements {
            if movement.after.zone() != ObjectZone::Battlefield {
                continue;
            }
            let id = movement.after.id();
            if let Some(choice) = work.entry_choice {
                self.resolution_entry_choices
                    .insert(movement.before.id(), choice);
            }
            self.stage_resolution_entry_choice(movement.before.id(), id)?;
            self.characteristics.register_battlefield_entry(id)?;
            let should_tap = if secondary_ids.contains(&movement.before.id()) {
                secondary_tapped
            } else {
                look_top_choose
                    .as_ref()
                    .is_some_and(|(_, _, _, enters_tapped, ..)| *enters_tapped)
                    || reveal_until
                        .as_ref()
                        .is_some_and(|(_, _, _, enters_tapped, ..)| *enters_tapped)
                    || matches!(
                        ext_keep_dest,
                        Some(crate::program::LookTopKeepDest::Battlefield { tapped: true, .. })
                    )
            };
            if should_tap {
                self.window.enter_tapped(id)?;
            }
            if !secondary_ids.contains(&movement.before.id())
                && matches!(
                    ext_keep_dest,
                    Some(crate::program::LookTopKeepDest::Battlefield {
                        attacking: true,
                        ..
                    })
                )
                && self.combat.is_some()
                && self.window.cards().get(id).and_then(GameObject::controller)
                    == Some(self.window.active_player())
                && let Ok(current_eval) = self.evaluated_characteristics()
                && let Ok(defenders) =
                    self.attack_destinations(self.window.active_player(), &current_eval)
                && let Some((&target, &seat)) = defenders.first_key_value()
            {
                let _ = self.put_attacking_at(id, seat, target);
            }
            if let Some(host) = self.resolution_aura_choices.remove(&movement.before.id()) {
                self.characteristics.attach(id, host, true)?;
            }
            let added =
                self.apply_entry_counters(&before, id, PowerToughnessCounters::default(), 0)?;
            movement.after = self
                .window
                .cards()
                .get(id)
                .cloned()
                .ok_or(EffectError::InconsistentProgram)?;
            if added.valid_placement() {
                counters.push((movement.before.id(), id, added));
            }
        }
        if let Some(crate::program::LookTopKeepDest::Exile {
            playable: true,
            free,
            cast_only,
            any_color,
            duration,
        }) = ext_keep_dest
        {
            let kept_before: BTreeSet<ObjectId> = work
                .hand
                .as_deref()
                .unwrap_or_default()
                .iter()
                .copied()
                .collect();
            let play_duration = match duration {
                crate::characteristics::ResolvedDuration::UntilEndOfTurn
                | crate::characteristics::ResolvedDuration::UntilEndOfCombat => {
                    CardPlayDuration::ThroughTurn
                }
                crate::characteristics::ResolvedDuration::ThroughControllerNextTurn
                | crate::characteristics::ResolvedDuration::UntilControllerNextTurn
                | crate::characteristics::ResolvedDuration::ThroughNextTurn { .. }
                | crate::characteristics::ResolvedDuration::UntilPlayerNextTurn { .. } => {
                    if self.window.active_player() == chooser {
                        CardPlayDuration::ThroughNextOwnTurnSkipCurrent(chooser)
                    } else {
                        CardPlayDuration::ThroughNextOwnTurn(chooser)
                    }
                }
                _ => CardPlayDuration::WhileInZone,
            };
            for movement in &movements {
                if kept_before.contains(&movement.before.id())
                    && movement.after.zone() == ObjectZone::Exile
                    && movement.after.card_id().is_some()
                {
                    self.card_play_permissions.insert(CardPlayPermission {
                        zone: CardPlayZone::Exile,
                        object: movement.after.id(),
                        player: chooser,
                        duration: play_duration,
                        any_color,
                        free,
                        cast_only,
                    });
                }
            }
        }
        if look_top_choose.is_some() {
            let took_card = movements.iter().any(|movement| {
                matches!(
                    movement.after.zone(),
                    ObjectZone::Hand(_) | ObjectZone::Battlefield
                )
            });
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .results
                .last_action_succeeded = took_card;
        }
        if look_top_choose_ext.is_some() || reveal_until_ext.is_some() {
            let kept_before: BTreeSet<ObjectId> = work
                .hand
                .as_deref()
                .unwrap_or_default()
                .iter()
                .copied()
                .collect();
            let moved_kept: Vec<ObjectId> = movements
                .iter()
                .filter(|movement| kept_before.contains(&movement.before.id()))
                .flat_map(|movement| movement.arrivals())
                .collect();
            let kept_ids = if keep_stays_on_top {
                work.hand.clone().unwrap_or_default()
            } else {
                moved_kept
            };
            let cursor = self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?;
            cursor.results.last_action_succeeded = !kept_ids.is_empty();
            cursor.results.last_action_count = Some(RulesInteger::from_usize(kept_ids.len()));
            cursor.results.last_moved_objects = kept_ids;
        }
        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .chosen_opponent = None;
        self.emit_from(
            EventCause::Instruction { entry, instruction },
            &before,
            None,
            journal,
            |previous, current, controllers| {
                let mut rows = movements
                    .iter()
                    .map(|movement| {
                        zone_occurrence_at(
                            previous,
                            current,
                            movement,
                            *reasons
                                .get(&movement.before.id())
                                .ok_or(EffectError::InconsistentProgram)?,
                            controllers,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                for (old, id, added) in counters {
                    rows.push(RulesOccurrence::CountersPlaced {
                        object: id,
                        before: previous.characteristics.counters(id),
                        added,
                        after: current.characteristics.counters(id),
                        before_appearance: Box::new(
                            previous
                                .appearance(old)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            current
                                .appearance(id)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    });
                }
                Ok(rows)
            },
        )?;
        Ok(None)
    }
    pub(super) fn resume_pile_choice(
        &self,
        expected: u64,
        frame: &PileChoiceFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if actor != frame.chooser {
            return Err(EffectError::WrongChooser);
        }
        let mut next = self.clone();
        let work = next
            .resolution_cursor_mut()
            .and_then(|cursor| cursor.revealed_piles.as_mut())
            .ok_or(EffectError::WrongResolutionFrame)?;
        if work.offered.take().as_ref() != Some(frame) || frame.revision != self.revision {
            return Err(EffectError::WrongResolutionFrame);
        }
        match (&frame.choice, reply) {
            (
                PileChoice::OneToHand(domain) | PileChoice::OneToHandRestBottom(domain),
                DecisionReply::SelectResolution(selected),
            ) if selected.len() == 1 && domain.contains(&selected[0]) => work.hand = Some(selected),
            (
                PileChoice::OptionalCreatureToBattlefield { eligible, .. },
                DecisionReply::SelectResolution(selected),
            ) if selected.len() <= 1 && selected.iter().all(|id| eligible.contains(id)) => {
                if work.hand.is_some() && work.secondary_hand.is_none() {
                    work.secondary_hand = Some(selected);
                } else {
                    work.hand = Some(selected);
                }
            }
            (
                PileChoice::OptionalUpTo { eligible, max, .. },
                DecisionReply::SelectResolution(selected),
            ) if selected.len() <= *max
                && selected.iter().all(|id| eligible.contains(id))
                && selected.iter().collect::<BTreeSet<_>>().len() == selected.len() =>
            {
                if work.hand.is_some() && work.secondary_hand.is_none() {
                    work.secondary_hand = Some(selected);
                } else {
                    work.hand = Some(selected);
                }
            }
            (PileChoice::Creature(domain), DecisionReply::SelectResolution(selected))
                if selected.len() == 1 && domain.contains(&selected[0]) =>
            {
                work.creature = Some(Some(selected[0]))
            }
            (
                PileChoice::Divide(domain) | PileChoice::DividePermanents(domain),
                DecisionReply::SelectResolution(selected),
            ) if selected.iter().all(|id| domain.contains(id))
                && selected.iter().collect::<BTreeSet<_>>().len() == selected.len() =>
            {
                work.first = Some(selected)
            }
            (
                PileChoice::Take { first, second } | PileChoice::Sacrifice { first, second },
                DecisionReply::ChoosePile(index @ (1 | 2)),
            ) => work.hand = Some(if index == 1 { first } else { second }.clone()),
            (PileChoice::BottomOrder(domain), DecisionReply::OrderResolutionGraveyard(order))
                if order.len() == domain.len()
                    && order.iter().collect::<BTreeSet<_>>() == domain.iter().collect() =>
            {
                work.bottom_order = Some(order);
            }
            (PileChoice::GraveyardOrder(_), DecisionReply::OrderResolutionGraveyard(order)) => {
                frame.validate_graveyard_order(&order)?;
                work.graveyard_order = Some(order)
            }
            _ => return Err(EffectError::InvalidLibraryPartition),
        }
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
    pub(super) fn resume_pile_entry(
        &self,
        expected: u64,
        frame: &EntryChoiceFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let DecisionReply::EntryChoice(choice) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if frame.chooser() != actor || !self.valid_entry_choice(frame, choice) {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        let work = next
            .resolution_cursor_mut()
            .and_then(|cursor| cursor.revealed_piles.as_mut())
            .ok_or(EffectError::WrongResolutionFrame)?;
        if work.entry_offer.take().as_ref() != Some(frame) || frame.revision() != expected {
            return Err(EffectError::WrongResolutionFrame);
        }
        work.entry_choice = Some(choice);
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
    pub(super) fn resume_pile_replacement(
        &self,
        expected: u64,
        frame: &ZoneReplacementFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let DecisionReply::ZoneReplacement(reply) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let mut next = self.clone();
        let work = next
            .resolution_cursor_mut()
            .and_then(|cursor| cursor.revealed_piles.as_mut())
            .ok_or(EffectError::WrongResolutionFrame)?;
        let (_, _, batch) = work
            .batches
            .iter_mut()
            .find(|(_, _, batch)| batch.frame(expected).as_ref() == Some(frame))
            .ok_or(EffectError::WrongResolutionFrame)?;
        batch.accept(expected, frame, actor, reply)?;
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
}
