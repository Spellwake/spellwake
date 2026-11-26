//! Clean, composable builders emitting universal `Instruction::ZoneOp` / `ForEachPlayerQuery`
//! programs for all zone-movement, exile, return, destroy, sacrifice, discard, shuffle, and
//! reveal operations.

use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    activated::CostExileZone,
    effects::CounterKind,
    program::{
        AffectedPlayers, BodyTarget, CardType, CardTypeMask, DelayedReturnTiming, EffectSubject,
        ExecutableBody, Instruction, LibraryPlacement, LinkedExileGroup, MoveSubjectRef,
        ObjectClause, ObjectFilter, PermanentTypeFilter, PublicZoneDestination, ResolvingPlayer,
        SourceZoneFilter, SubjectDestination, ValueExpr,
    },
    toolbox::{
        BindingId, ChoiceCategory, GraveyardSemantics, LibraryOrderPos, ObjectQuery,
        PlayerSelector, QuerySlice, QueryZone, ZoneController, ZoneDest, ZoneOpSpec, ZoneSelection,
    },
};

#[must_use]
pub(crate) fn move_subject_to_query(subject: MoveSubjectRef) -> ObjectQuery {
    match subject {
        MoveSubjectRef::TargetObject => ObjectQuery::Targets,
        MoveSubjectRef::AbilitySource => ObjectQuery::Source,
        MoveSubjectRef::TriggeringObject => ObjectQuery::TriggerSubject,
        MoveSubjectRef::AttachedHost => ObjectQuery::AttachedHost,
        MoveSubjectRef::RevealedCard => ObjectQuery::Revealed,
        MoveSubjectRef::CreatedTokens => ObjectQuery::CreatedTokens,
        MoveSubjectRef::LastMoved => ObjectQuery::LastMoved,
        MoveSubjectRef::RememberedFirst => ObjectQuery::LinkedTarget { index: 0 },
        MoveSubjectRef::RememberedSecond => ObjectQuery::LinkedTarget { index: 1 },
    }
}

#[must_use]
pub(crate) fn subject_dest_to_zone_dest(destination: SubjectDestination) -> ZoneDest {
    match destination {
        SubjectDestination::Battlefield {
            tapped,
            attacking,
            transformed,
            under_owner_control,
            with_counter,
            attach_to_self,
        } => {
            let mut entering_counters = Vec::new();
            if let Some((kind, count)) = with_counter
                && count > 0
            {
                entering_counters.push((
                    kind,
                    Box::new(ValueExpr::literal(RulesInteger::from_usize(usize::from(
                        count,
                    )))),
                ));
            }
            ZoneDest::Battlefield {
                controller: if under_owner_control {
                    ZoneController::Owner
                } else {
                    ZoneController::Player(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    )))
                },
                tapped,
                attacking,
                transformed,
                face_down: false,
                entering_counters,
                attach_to: attach_to_self.then(|| Box::new(ObjectQuery::Source)),
            }
        }
        SubjectDestination::Hand => ZoneDest::Hand,
        SubjectDestination::Graveyard => ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Move,
        },
        SubjectDestination::Exile { delayed_return } => ZoneDest::Exile {
            face_down: false,
            viewer: None,
            link_to_source: matches!(delayed_return, Some(DelayedReturnTiming::UntilSourceLeaves)),
            delayed_return,
            link_group: None,
        },
        SubjectDestination::Library(placement) => ZoneDest::Library {
            position: LibraryOrderPos::from_placement(placement),
        },
    }
}

#[must_use]
pub(crate) fn source_zone_query(
    players: AffectedPlayers,
    zone: SourceZoneFilter,
    filter: ObjectFilter,
) -> ObjectQuery {
    let p_sel = || Box::new(PlayerSelector::Group(players));
    let all_players = || Box::new(PlayerSelector::Group(AffectedPlayers::EachPlayer));
    let opponents = || Box::new(PlayerSelector::Group(AffectedPlayers::EachOpponent));
    let controller = || Box::new(PlayerSelector::Resolving(ResolvingPlayer::Controller));
    let target_player = || Box::new(PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer));
    match zone {
        SourceZoneFilter::Hand => ObjectQuery::zone(QueryZone::Hand(p_sel()), filter),
        SourceZoneFilter::Graveyard => ObjectQuery::zone(QueryZone::Graveyard(p_sel()), filter),
        SourceZoneFilter::HandOrGraveyard => ObjectQuery::Union(vec![
            ObjectQuery::zone(QueryZone::Hand(p_sel()), filter),
            ObjectQuery::zone(QueryZone::Graveyard(p_sel()), filter),
        ]),
        SourceZoneFilter::Battlefield => {
            let scoped = match players {
                AffectedPlayers::One(ResolvingPlayer::Controller) => filter.controlled(),
                AffectedPlayers::EachOpponent => filter.not_controlled(),
                _ => filter,
            };
            ObjectQuery::zone(QueryZone::Battlefield, scoped)
        }
        SourceZoneFilter::AnyBattlefield => ObjectQuery::zone(QueryZone::Battlefield, filter),
        SourceZoneFilter::Exile => ObjectQuery::zone(QueryZone::Exile, filter),
        SourceZoneFilter::OpponentGraveyard => {
            ObjectQuery::zone(QueryZone::Graveyard(opponents()), filter)
        }
        SourceZoneFilter::AnyGraveyard => {
            ObjectQuery::zone(QueryZone::Graveyard(all_players()), filter)
        }
        SourceZoneFilter::YourLibrary => {
            ObjectQuery::zone(QueryZone::Library(controller()), filter)
        }
        SourceZoneFilter::OpponentExile => {
            ObjectQuery::zone(QueryZone::ExileOwnedBy(opponents()), filter)
        }
        SourceZoneFilter::LinkedExile => {
            ObjectQuery::filtered(ObjectQuery::LinkedExile { group: None }, filter)
        }
        SourceZoneFilter::LastMoved => ObjectQuery::filtered(ObjectQuery::LastMoved, filter),
        SourceZoneFilter::TargetPlayerHand => {
            ObjectQuery::zone(QueryZone::Hand(target_player()), filter)
        }
        SourceZoneFilter::TargetPlayerGraveyard => {
            ObjectQuery::zone(QueryZone::Graveyard(target_player()), filter)
        }
        SourceZoneFilter::OpponentHand => ObjectQuery::zone(QueryZone::Hand(opponents()), filter),
        SourceZoneFilter::CommandZone => ObjectQuery::zone(QueryZone::Command, filter),
    }
}

#[must_use]
pub(crate) fn zone_op(spec: ZoneOpSpec) -> Instruction {
    Instruction::ZoneOp(Box::new(spec))
}

#[must_use]
pub(crate) fn move_subject(
    subject: MoveSubjectRef,
    destination: SubjectDestination,
) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        move_subject_to_query(subject),
        subject_dest_to_zone_dest(destination),
    ))
}

#[must_use]
pub(crate) fn exile_group_from_zone(
    players: AffectedPlayers,
    zone: SourceZoneFilter,
    filter: ObjectFilter,
) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        source_zone_query(players, zone, filter),
        ZoneDest::exile(),
    ))
}

#[must_use]
pub(crate) fn exile_player_graveyards(players: AffectedPlayers) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::zone(
            QueryZone::Graveyard(Box::new(PlayerSelector::Group(players))),
            ObjectFilter::any(),
        ),
        ZoneDest::exile(),
    ))
}

#[must_use]
pub(crate) fn exile_all_graveyards() -> Instruction {
    exile_player_graveyards(AffectedPlayers::EachPlayer)
}

#[must_use]
pub(crate) fn exile_ability_source() -> Instruction {
    zone_op(ZoneOpSpec::move_all(ObjectQuery::Source, ZoneDest::exile()))
}

#[must_use]
pub(crate) fn exile_resolving_spell() -> Instruction {
    zone_op(ZoneOpSpec::move_all(ObjectQuery::Source, ZoneDest::exile()))
}

#[must_use]
pub(crate) fn exile_subject_until_source_leaves(subject: EffectSubject) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::EffectSubject(Box::new(subject)),
        ZoneDest::Exile {
            face_down: false,
            viewer: None,
            link_to_source: true,
            delayed_return: Some(DelayedReturnTiming::UntilSourceLeaves),
            link_group: None,
        },
    ))
}

#[must_use]
pub(crate) fn sacrifice_subject(subject: EffectSubject) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::EffectSubject(Box::new(subject)),
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Sacrifice,
        },
    ))
}

#[must_use]
pub(crate) fn destroy_subject(subject: EffectSubject, no_regeneration: bool) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::EffectSubject(Box::new(subject)),
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Destroy {
                allow_regenerate: !no_regeneration,
            },
        },
    ))
}

#[must_use]
pub(crate) fn shuffle_player_library(player: ResolvingPlayer) -> Instruction {
    zone_op(ZoneOpSpec::shuffle(PlayerSelector::Resolving(player)))
}

#[must_use]
pub(crate) fn shuffle_own_library() -> Instruction {
    shuffle_player_library(ResolvingPlayer::Controller)
}

#[must_use]
pub(crate) fn shuffle_subject_into_library(
    subject: EffectSubject,
    zone: Option<SourceZoneFilter>,
) -> Instruction {
    let source = match zone {
        None => ObjectQuery::EffectSubject(Box::new(subject)),
        Some(zf) => match subject {
            EffectSubject::Matching(filter) => source_zone_query(
                AffectedPlayers::One(ResolvingPlayer::Controller),
                zf,
                filter,
            ),
            other => ObjectQuery::EffectSubject(Box::new(other)),
        },
    };
    zone_op(ZoneOpSpec::move_all(
        source,
        ZoneDest::Library {
            position: LibraryOrderPos::ShuffleInto,
        },
    ))
}

#[must_use]
pub(crate) fn shuffle_target_into_library() -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Targets,
        ZoneDest::Library {
            position: LibraryOrderPos::ShuffleInto,
        },
    ))
}

#[must_use]
pub(crate) fn shuffle_ability_source_into_library() -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Source,
        ZoneDest::Library {
            position: LibraryOrderPos::ShuffleInto,
        },
    ))
}

#[must_use]
pub(crate) fn move_graveyard_group_owned(
    players: AffectedPlayers,
    filter: ObjectFilter,
    destination: SubjectDestination,
) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::zone(
            QueryZone::Graveyard(Box::new(PlayerSelector::Group(players))),
            filter,
        ),
        subject_dest_to_zone_dest(destination),
    ))
}

#[must_use]
pub(crate) fn move_graveyard_group(
    filter: ObjectFilter,
    all_graveyards: bool,
    to_battlefield: bool,
    tapped: bool,
) -> Instruction {
    let players = if all_graveyards {
        AffectedPlayers::EachPlayer
    } else {
        AffectedPlayers::One(ResolvingPlayer::Controller)
    };
    let dest = if to_battlefield {
        ZoneDest::Battlefield {
            controller: ZoneController::Player(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::Controller,
            ))),
            tapped,
            attacking: false,
            transformed: false,
            face_down: false,
            entering_counters: Vec::new(),
            attach_to: None,
        }
    } else {
        ZoneDest::Hand
    };
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::zone(
            QueryZone::Graveyard(Box::new(PlayerSelector::Group(players))),
            filter,
        ),
        dest,
    ))
}

#[must_use]
pub(crate) fn move_group_to_library(
    group: ObjectFilter,
    placement: LibraryPlacement,
) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::zone(QueryZone::Battlefield, group),
        ZoneDest::Library {
            position: LibraryOrderPos::from_placement(placement),
        },
    ))
}

#[must_use]
pub(crate) fn put_group_on_library_bottom(group: ObjectFilter) -> Instruction {
    move_group_to_library(group, LibraryPlacement::Bottom)
}

#[must_use]
pub(crate) fn move_last_moved_filtered(
    filter: Option<ObjectFilter>,
    destination: SubjectDestination,
) -> Instruction {
    let source = match filter {
        Some(f) => ObjectQuery::filtered(ObjectQuery::LastMoved, f),
        None => ObjectQuery::LastMoved,
    };
    zone_op(ZoneOpSpec::move_all(
        source,
        subject_dest_to_zone_dest(destination),
    ))
}

#[must_use]
pub(crate) fn put_resolving_spell_in_library(placement: LibraryPlacement) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Source,
        ZoneDest::Library {
            position: LibraryOrderPos::from_placement(placement),
        },
    ))
}

#[must_use]
pub(crate) fn put_subject_into_library_from_top(
    subject: MoveSubjectRef,
    offset: ValueExpr,
) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        move_subject_to_query(subject),
        ZoneDest::Library {
            position: LibraryOrderPos::NthFromTop(Box::new(offset)),
        },
    ))
}

#[must_use]
pub(crate) fn put_target_in_library(placement: LibraryPlacement) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Targets,
        ZoneDest::Library {
            position: LibraryOrderPos::from_placement(placement),
        },
    ))
}

#[must_use]
pub(crate) fn return_resolving_spell_to_hand() -> Instruction {
    zone_op(ZoneOpSpec::move_all(ObjectQuery::Source, ZoneDest::Hand))
}

#[must_use]
pub(crate) fn return_own_graveyard_cards_of_type_to_hand(card_type: CardType) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::zone(
            QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::Controller,
            ))),
            ObjectFilter::from_clause(
                ObjectClause::any().any_types(CardTypeMask::from_card_type(card_type)),
            ),
        ),
        ZoneDest::Hand,
    ))
}

#[must_use]
pub(crate) fn return_linked_exile(
    destination: SubjectDestination,
    only_first: bool,
) -> Instruction {
    let base = ObjectQuery::LinkedExile { group: None };
    let source = if only_first {
        ObjectQuery::Take {
            source: Box::new(base),
            count: Box::new(ValueExpr::literal(1)),
        }
    } else {
        base
    };
    zone_op(ZoneOpSpec::move_all(
        source,
        subject_dest_to_zone_dest(destination),
    ))
}

#[must_use]
pub(crate) fn return_cards_exiled_with_link(
    group: LinkedExileGroup,
    under_controller_control: bool,
    tapped: bool,
) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::LinkedExile { group: Some(group) },
        ZoneDest::Battlefield {
            controller: if under_controller_control {
                ZoneController::Player(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::Controller,
                )))
            } else {
                ZoneController::Owner
            },
            tapped,
            attacking: false,
            transformed: false,
            face_down: false,
            entering_counters: Vec::new(),
            attach_to: None,
        },
    ))
}

#[must_use]
pub(crate) fn reveal_player_hand(player: ResolvingPlayer) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::zone(
            QueryZone::Hand(Box::new(PlayerSelector::Resolving(player))),
            ObjectFilter::any(),
        ),
        ZoneDest::RevealOnly,
    ))
}

#[must_use]
pub(crate) fn exile_library_top_linked(
    player: ResolvingPlayer,
    amount: ValueExpr,
    group: LinkedExileGroup,
    face_down: bool,
) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::ZoneScan {
            zone: QueryZone::Library(Box::new(PlayerSelector::Resolving(player))),
            filter: Box::new(ObjectFilter::any()),
            slice: QuerySlice::Top(Box::new(amount)),
        },
        ZoneDest::Exile {
            face_down,
            viewer: face_down.then_some(PlayerSelector::Resolving(ResolvingPlayer::Controller)),
            link_to_source: false,
            delayed_return: None,
            link_group: Some(group),
        },
    ))
}

#[must_use]
pub(crate) fn per_player_prompt_zone_op(
    players: AffectedPlayers,
    zone_fn: impl Fn(PlayerSelector) -> QueryZone,
    filter: ObjectFilter,
    min: ValueExpr,
    max: ValueExpr,
    destination: ZoneDest,
    hidden: bool,
) -> Instruction {
    const SEAT_BIND: BindingId = BindingId(900);
    let inner = zone_op(ZoneOpSpec::simple(
        ObjectQuery::zone(zone_fn(PlayerSelector::Bound(SEAT_BIND)), filter),
        ZoneSelection::Prompt {
            chooser: PlayerSelector::Bound(SEAT_BIND),
            min: Box::new(min),
            max: Box::new(max),
            constraint: None,
            category: ChoiceCategory::Generic,
            hidden,
        },
        destination,
    ));
    Instruction::ForEachPlayerQuery {
        selector: Box::new(PlayerSelector::Group(players)),
        bind_player: SEAT_BIND,
        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
    }
}

pub(crate) fn rewrite_player_selector(
    sel: &mut PlayerSelector,
    map_rp: &impl Fn(&mut ResolvingPlayer),
    map_ap: &impl Fn(&mut AffectedPlayers),
    map_ve: &impl Fn(&mut ValueExpr),
) {
    match sel {
        PlayerSelector::Resolving(rp) => map_rp(rp),
        PlayerSelector::Group(ap) => map_ap(ap),
        PlayerSelector::Bound(_) => {}
        PlayerSelector::OwnersOf(q) | PlayerSelector::ControllersOf(q) => {
            rewrite_object_query(q, map_rp, map_ap, map_ve);
        }
    }
}

pub(crate) fn rewrite_query_zone(
    z: &mut QueryZone,
    map_rp: &impl Fn(&mut ResolvingPlayer),
    map_ap: &impl Fn(&mut AffectedPlayers),
    map_ve: &impl Fn(&mut ValueExpr),
) {
    match z {
        QueryZone::Hand(p)
        | QueryZone::Library(p)
        | QueryZone::Graveyard(p)
        | QueryZone::ExileOwnedBy(p)
        | QueryZone::BattlefieldControlledBy(p) => {
            rewrite_player_selector(p, map_rp, map_ap, map_ve)
        }
        QueryZone::Battlefield
        | QueryZone::Exile
        | QueryZone::Stack
        | QueryZone::Command
        | QueryZone::AllZones => {}
    }
}

pub(crate) fn rewrite_object_query(
    q: &mut ObjectQuery,
    map_rp: &impl Fn(&mut ResolvingPlayer),
    map_ap: &impl Fn(&mut AffectedPlayers),
    map_ve: &impl Fn(&mut ValueExpr),
) {
    match q {
        ObjectQuery::ZoneScan { zone, slice, .. } => {
            rewrite_query_zone(zone, map_rp, map_ap, map_ve);
            match slice {
                QuerySlice::All => {}
                QuerySlice::Top(v) | QuerySlice::Bottom(v) => map_ve(v),
                QuerySlice::UntilMatch { count, .. } => map_ve(count),
            }
        }
        ObjectQuery::Filtered { source, .. } => {
            rewrite_object_query(source, map_rp, map_ap, map_ve);
        }
        ObjectQuery::Take { source, count } => {
            rewrite_object_query(source, map_rp, map_ap, map_ve);
            map_ve(count);
        }
        ObjectQuery::Tagged { zone: Some(z), .. } => {
            rewrite_query_zone(z, map_rp, map_ap, map_ve);
        }
        ObjectQuery::Tagged { zone: None, .. } => {}
        ObjectQuery::Union(qs) => {
            for sub in qs {
                rewrite_object_query(sub, map_rp, map_ap, map_ve);
            }
        }
        ObjectQuery::Difference(a, b) | ObjectQuery::Intersect(a, b) => {
            rewrite_object_query(a, map_rp, map_ap, map_ve);
            rewrite_object_query(b, map_rp, map_ap, map_ve);
        }
        _ => {}
    }
}

pub(crate) fn rewrite_zone_op(
    spec: &mut ZoneOpSpec,
    map_rp: &impl Fn(&mut ResolvingPlayer),
    map_ap: &impl Fn(&mut AffectedPlayers),
    map_ve: &impl Fn(&mut ValueExpr),
) {
    rewrite_object_query(&mut spec.source, map_rp, map_ap, map_ve);
    match &mut spec.selection {
        ZoneSelection::All => {}
        ZoneSelection::Random { count, chooser } => {
            map_ve(count);
            rewrite_player_selector(chooser, map_rp, map_ap, map_ve);
        }
        ZoneSelection::Prompt {
            chooser, min, max, ..
        } => {
            rewrite_player_selector(chooser, map_rp, map_ap, map_ve);
            map_ve(min);
            map_ve(max);
        }
    }
    match &mut spec.destination {
        ZoneDest::Battlefield {
            controller,
            entering_counters,
            attach_to,
            ..
        } => {
            if let ZoneController::Player(p) = controller {
                rewrite_player_selector(p, map_rp, map_ap, map_ve);
            }
            for (_, v) in entering_counters {
                map_ve(v);
            }
            if let Some(q) = attach_to {
                rewrite_object_query(q, map_rp, map_ap, map_ve);
            }
        }
        ZoneDest::Exile { viewer, .. } => {
            if let Some(v) = viewer {
                rewrite_player_selector(v, map_rp, map_ap, map_ve);
            }
        }
        ZoneDest::Library { position } => match position {
            LibraryOrderPos::NthFromTop(v) => map_ve(v),
            LibraryOrderPos::ChoiceTopOrBottom { chooser } => {
                rewrite_player_selector(chooser, map_rp, map_ap, map_ve);
            }
            _ => {}
        },
        ZoneDest::LookOnly { viewer } => rewrite_player_selector(viewer, map_rp, map_ap, map_ve),
        ZoneDest::None | ZoneDest::Graveyard { .. } | ZoneDest::Hand | ZoneDest::RevealOnly => {}
    }
    if let Some(shuf) = &mut spec.shuffle_library {
        rewrite_player_selector(shuf, map_rp, map_ap, map_ve);
    }
}

#[must_use]
fn permanent_type_filter_query(zone: QueryZone, filter: PermanentTypeFilter) -> ObjectQuery {
    let mut clause = ObjectClause::any();
    if !filter.any_types.is_empty() {
        let mut mask = CardTypeMask::EMPTY;
        for ct in &filter.any_types {
            mask = mask.with(*ct);
        }
        clause = clause.any_types(mask);
    }
    if !filter.required_types.is_empty() {
        let mut mask = CardTypeMask::EMPTY;
        for ct in &filter.required_types {
            mask = mask.with(*ct);
        }
        clause = clause.required_types(mask);
    }
    if !filter.forbidden_types.is_empty() {
        let mut mask = CardTypeMask::EMPTY;
        for ct in &filter.forbidden_types {
            mask = mask.with(*ct);
        }
        clause = clause.forbid_types(mask);
    }
    let base = ObjectQuery::zone(zone, ObjectFilter::from_clause(clause));
    match filter.object_filter {
        Some(obj) => ObjectQuery::filtered(base, obj),
        None => base,
    }
}

#[must_use]
pub(crate) fn exile_target() -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Targets,
        ZoneDest::exile(),
    ))
}

#[must_use]
pub(crate) fn exile_until_next_end_step() -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Targets,
        ZoneDest::Exile {
            face_down: false,
            viewer: None,
            link_to_source: false,
            delayed_return: Some(DelayedReturnTiming::NextEndStep),
            link_group: None,
        },
    ))
}

#[must_use]
pub(crate) fn exile_top_face_down() -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::ZoneScan {
            zone: QueryZone::Library(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::Controller,
            ))),
            filter: Box::new(ObjectFilter::any()),
            slice: QuerySlice::Top(Box::new(ValueExpr::literal(1))),
        },
        ZoneDest::Exile {
            face_down: true,
            viewer: Some(PlayerSelector::Resolving(ResolvingPlayer::Controller)),
            link_to_source: true,
            delayed_return: None,
            link_group: None,
        },
    ))
}

#[must_use]
pub(crate) fn destroy_group(filter: ObjectFilter, allow_regenerate: bool) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::zone(QueryZone::Battlefield, filter),
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Destroy { allow_regenerate },
        },
    ))
}

#[must_use]
pub(crate) fn return_target_to_hand() -> Instruction {
    zone_op(ZoneOpSpec::move_all(ObjectQuery::Targets, ZoneDest::Hand))
}

#[must_use]
pub(crate) fn return_target_to_battlefield(tapped: bool) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Targets,
        ZoneDest::Battlefield {
            controller: ZoneController::Player(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::Controller,
            ))),
            tapped,
            attacking: false,
            transformed: false,
            face_down: false,
            entering_counters: Vec::new(),
            attach_to: None,
        },
    ))
}

#[must_use]
pub(crate) fn move_group(group: ObjectFilter, destination: PublicZoneDestination) -> Instruction {
    let dest = match destination {
        PublicZoneDestination::Hand | PublicZoneDestination::Command => ZoneDest::Hand,
        PublicZoneDestination::Graveyard => ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Move,
        },
        PublicZoneDestination::Exile => ZoneDest::exile(),
        PublicZoneDestination::Library => ZoneDest::Library {
            position: LibraryOrderPos::Top,
        },
        PublicZoneDestination::Battlefield => ZoneDest::battlefield(ZoneController::Owner),
    };
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::zone(QueryZone::Battlefield, group),
        dest,
    ))
}

#[must_use]
pub(crate) fn shuffle_each_hand_and_graveyard() -> Instruction {
    const SEAT_BIND: BindingId = BindingId(901);
    let inner = zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Union(vec![
            ObjectQuery::zone(
                QueryZone::Hand(Box::new(PlayerSelector::Bound(SEAT_BIND))),
                ObjectFilter::any(),
            ),
            ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Bound(SEAT_BIND))),
                ObjectFilter::any(),
            ),
        ]),
        ZoneDest::Library {
            position: LibraryOrderPos::ShuffleInto,
        },
    ));
    Instruction::ForEachPlayerQuery {
        selector: Box::new(PlayerSelector::Group(AffectedPlayers::EachPlayer)),
        bind_player: SEAT_BIND,
        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
    }
}

#[must_use]
pub(crate) fn select_zone_cards_to_zone(
    player: ResolvingPlayer,
    source_zone: SourceZoneFilter,
    filter: ObjectFilter,
    min: ValueExpr,
    max: ValueExpr,
    destination: SubjectDestination,
    reveal: bool,
) -> Instruction {
    zone_op(ZoneOpSpec::simple(
        source_zone_query(AffectedPlayers::One(player), source_zone, filter),
        ZoneSelection::Prompt {
            chooser: PlayerSelector::Resolving(player),
            min: Box::new(min),
            max: Box::new(max),
            constraint: None,
            category: ChoiceCategory::Generic,
            hidden: !reveal,
        },
        subject_dest_to_zone_dest(destination),
    ))
}

#[must_use]
pub(crate) fn exile_from_zone(
    players: AffectedPlayers,
    zone: CostExileZone,
    amount: ValueExpr,
    filter: ObjectFilter,
) -> Instruction {
    let hidden = matches!(zone, CostExileZone::Hand);
    per_player_prompt_zone_op(
        players,
        move |p| match zone {
            CostExileZone::Battlefield => QueryZone::BattlefieldControlledBy(Box::new(p)),
            CostExileZone::Graveyard => QueryZone::Graveyard(Box::new(p)),
            CostExileZone::Hand => QueryZone::Hand(Box::new(p)),
        },
        filter,
        amount.clone(),
        amount,
        ZoneDest::exile(),
        hidden,
    )
}

#[must_use]
pub(crate) fn exile_cards_from_graveyard(
    player: ResolvingPlayer,
    count: std::num::NonZeroU8,
) -> Instruction {
    let n = ValueExpr::literal(i64::from(count.get()));
    zone_op(ZoneOpSpec::simple(
        ObjectQuery::zone(
            QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(player))),
            ObjectFilter::any(),
        ),
        ZoneSelection::Prompt {
            chooser: PlayerSelector::Resolving(player),
            min: Box::new(n.clone()),
            max: Box::new(n),
            constraint: None,
            category: ChoiceCategory::Generic,
            hidden: false,
        },
        ZoneDest::exile(),
    ))
}

#[must_use]
pub(crate) fn exile_chosen_permanents(
    players: AffectedPlayers,
    amount: ValueExpr,
    filter: PermanentTypeFilter,
) -> Instruction {
    const SEAT_BIND: BindingId = BindingId(902);
    let inner = zone_op(ZoneOpSpec::simple(
        permanent_type_filter_query(
            QueryZone::BattlefieldControlledBy(Box::new(PlayerSelector::Bound(SEAT_BIND))),
            filter,
        ),
        ZoneSelection::Prompt {
            chooser: PlayerSelector::Bound(SEAT_BIND),
            min: Box::new(amount.clone()),
            max: Box::new(amount),
            constraint: None,
            category: ChoiceCategory::Generic,
            hidden: false,
        },
        ZoneDest::exile(),
    ));
    Instruction::ForEachPlayerQuery {
        selector: Box::new(PlayerSelector::Group(players)),
        bind_player: SEAT_BIND,
        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
    }
}

#[must_use]
pub(crate) fn return_controlled_permanents(
    players: AffectedPlayers,
    amount: ValueExpr,
    filter: PermanentTypeFilter,
) -> Instruction {
    const SEAT_BIND: BindingId = BindingId(903);
    let inner = zone_op(ZoneOpSpec::simple(
        permanent_type_filter_query(
            QueryZone::BattlefieldControlledBy(Box::new(PlayerSelector::Bound(SEAT_BIND))),
            filter,
        ),
        ZoneSelection::Prompt {
            chooser: PlayerSelector::Bound(SEAT_BIND),
            min: Box::new(amount.clone()),
            max: Box::new(amount),
            constraint: None,
            category: ChoiceCategory::Generic,
            hidden: false,
        },
        ZoneDest::Hand,
    ));
    Instruction::ForEachPlayerQuery {
        selector: Box::new(PlayerSelector::Group(players)),
        bind_player: SEAT_BIND,
        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
    }
}

#[must_use]
pub(crate) fn destroy_chosen(
    chooser: ResolvingPlayer,
    filter: ObjectFilter,
    count: ValueExpr,
    allow_regenerate: bool,
) -> Instruction {
    zone_op(ZoneOpSpec::simple(
        ObjectQuery::zone(QueryZone::Battlefield, filter),
        ZoneSelection::Prompt {
            chooser: PlayerSelector::Resolving(chooser),
            min: Box::new(count.clone()),
            max: Box::new(count),
            constraint: None,
            category: ChoiceCategory::Generic,
            hidden: false,
        },
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Destroy { allow_regenerate },
        },
    ))
}

#[must_use]
pub(crate) fn discard_filtered(
    players: AffectedPlayers,
    amount: ValueExpr,
    filter: ObjectFilter,
) -> Instruction {
    per_player_prompt_zone_op(
        players,
        |p| QueryZone::Hand(Box::new(p)),
        filter,
        amount.clone(),
        amount,
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Discard,
        },
        true,
    )
}

#[must_use]
pub(crate) fn discard_up_to_ext(
    players: AffectedPlayers,
    max_count: ValueExpr,
    filter: Option<ObjectFilter>,
    any_number: bool,
) -> Instruction {
    const SEAT_BIND: BindingId = BindingId(904);
    let flt = filter.unwrap_or_else(ObjectFilter::any);
    let hand_q = ObjectQuery::zone(
        QueryZone::Hand(Box::new(PlayerSelector::Bound(SEAT_BIND))),
        flt,
    );
    let max_expr = if any_number {
        ValueExpr::QueryCount(Box::new(hand_q.clone()))
    } else {
        max_count
    };
    let inner = zone_op(ZoneOpSpec::simple(
        hand_q,
        ZoneSelection::Prompt {
            chooser: PlayerSelector::Bound(SEAT_BIND),
            min: Box::new(ValueExpr::literal(0)),
            max: Box::new(max_expr),
            constraint: None,
            category: ChoiceCategory::Generic,
            hidden: true,
        },
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Discard,
        },
    ));
    Instruction::ForEachPlayerQuery {
        selector: Box::new(PlayerSelector::Group(players)),
        bind_player: SEAT_BIND,
        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
    }
}

#[must_use]
pub(crate) fn discard_up_to(maximum: u16) -> Instruction {
    discard_up_to_ext(
        AffectedPlayers::One(ResolvingPlayer::Controller),
        ValueExpr::literal(i64::from(maximum)),
        None,
        false,
    )
}

#[must_use]
pub(crate) fn discard_one_at_random(player: ResolvingPlayer) -> Instruction {
    zone_op(ZoneOpSpec::simple(
        ObjectQuery::zone(
            QueryZone::Hand(Box::new(PlayerSelector::Resolving(player))),
            ObjectFilter::any(),
        ),
        ZoneSelection::Random {
            count: Box::new(ValueExpr::literal(1)),
            chooser: PlayerSelector::Resolving(player),
        },
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Discard,
        },
    ))
}

#[must_use]
pub(crate) fn sacrifice_up_to(
    players: AffectedPlayers,
    amount: ValueExpr,
    filter: PermanentTypeFilter,
) -> Instruction {
    const SEAT_BIND: BindingId = BindingId(905);
    let inner = zone_op(ZoneOpSpec::simple(
        permanent_type_filter_query(
            QueryZone::BattlefieldControlledBy(Box::new(PlayerSelector::Bound(SEAT_BIND))),
            filter,
        ),
        ZoneSelection::Prompt {
            chooser: PlayerSelector::Bound(SEAT_BIND),
            min: Box::new(ValueExpr::literal(0)),
            max: Box::new(amount),
            constraint: None,
            category: ChoiceCategory::Generic,
            hidden: false,
        },
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Sacrifice,
        },
    ));
    Instruction::ForEachPlayerQuery {
        selector: Box::new(PlayerSelector::Group(players)),
        bind_player: SEAT_BIND,
        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
    }
}

#[must_use]
pub(crate) fn put_hand_permanent_filtered(
    player: ResolvingPlayer,
    filter: ObjectFilter,
    maximum: u8,
    tapped: bool,
) -> Instruction {
    zone_op(ZoneOpSpec::simple(
        ObjectQuery::zone(
            QueryZone::Hand(Box::new(PlayerSelector::Resolving(player))),
            filter,
        ),
        ZoneSelection::Prompt {
            chooser: PlayerSelector::Resolving(player),
            min: Box::new(ValueExpr::literal(0)),
            max: Box::new(ValueExpr::literal(i64::from(maximum))),
            constraint: None,
            category: ChoiceCategory::Generic,
            hidden: true,
        },
        ZoneDest::Battlefield {
            controller: ZoneController::Player(Box::new(PlayerSelector::Resolving(player))),
            tapped,
            attacking: false,
            transformed: false,
            face_down: false,
            entering_counters: Vec::new(),
            attach_to: None,
        },
    ))
}

#[must_use]
pub(crate) fn return_random_graveyard_cards(
    filter: Option<ObjectFilter>,
    count: std::num::NonZeroU8,
    to_battlefield: bool,
) -> Instruction {
    let dest = if to_battlefield {
        ZoneDest::battlefield(ZoneController::Player(Box::new(PlayerSelector::Resolving(
            ResolvingPlayer::Controller,
        ))))
    } else {
        ZoneDest::Hand
    };
    zone_op(ZoneOpSpec::simple(
        ObjectQuery::zone(
            QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::Controller,
            ))),
            filter.unwrap_or_else(ObjectFilter::any),
        ),
        ZoneSelection::Random {
            count: Box::new(ValueExpr::literal(i64::from(count.get()))),
            chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
        },
        dest,
    ))
}

#[must_use]
pub(crate) fn unwrap_zone_op(op: &Instruction) -> Option<&ZoneOpSpec> {
    match op {
        Instruction::ZoneOp(spec) => Some(spec),
        Instruction::ForEachPlayerQuery { body, .. } => match body.instructions.as_slice() {
            [Instruction::ZoneOp(spec)] => Some(spec),
            _ => None,
        },
        _ => None,
    }
}

#[must_use]
pub(crate) fn zone_op_any_value_expr(
    spec: &ZoneOpSpec,
    pred: &impl Fn(&ValueExpr) -> bool,
) -> bool {
    let query_uses = |q: &ObjectQuery| -> bool {
        fn walk(q: &ObjectQuery, pred: &impl Fn(&ValueExpr) -> bool) -> bool {
            match q {
                ObjectQuery::ZoneScan { slice, .. } => match slice {
                    QuerySlice::All => false,
                    QuerySlice::Top(v) | QuerySlice::Bottom(v) => pred(v),
                    QuerySlice::UntilMatch { count, .. } => pred(count),
                },
                ObjectQuery::Filtered { source, .. } => walk(source, pred),
                ObjectQuery::Take { source, count } => walk(source, pred) || pred(count),
                ObjectQuery::Union(qs) => qs.iter().any(|s| walk(s, pred)),
                ObjectQuery::Difference(a, b) | ObjectQuery::Intersect(a, b) => {
                    walk(a, pred) || walk(b, pred)
                }
                _ => false,
            }
        }
        walk(q, pred)
    };
    if query_uses(&spec.source) {
        return true;
    }
    let sel_uses = match &spec.selection {
        ZoneSelection::All => false,
        ZoneSelection::Random { count, .. } => pred(count),
        ZoneSelection::Prompt { min, max, .. } => pred(min) || pred(max),
    };
    if sel_uses {
        return true;
    }
    match &spec.destination {
        ZoneDest::Battlefield {
            entering_counters,
            attach_to,
            ..
        } => {
            entering_counters.iter().any(|(_, v)| pred(v))
                || attach_to.as_ref().is_some_and(|q| query_uses(q))
        }
        ZoneDest::Library {
            position: LibraryOrderPos::NthFromTop(v),
        } => pred(v),
        _ => false,
    }
}

// ============================================================================
// Phase 2B + Phase 3 Composable Builders: DrawCards, MutateObject,
// ApplyContinuous, and remaining ZoneOp helpers
// ============================================================================

#[must_use]
pub(crate) fn draw_cards(players: PlayerSelector, amount: ValueExpr) -> Instruction {
    Instruction::DrawCards(Box::new(spellwake_engine::toolbox::DrawCardsSpec::simple(
        players, amount,
    )))
}

#[must_use]
pub(crate) fn draw_controller(amount: ValueExpr) -> Instruction {
    draw_cards(
        PlayerSelector::Resolving(ResolvingPlayer::Controller),
        amount,
    )
}

#[must_use]
pub(crate) fn draw_target_player(amount: ValueExpr) -> Instruction {
    draw_cards(
        PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer),
        amount,
    )
}

#[must_use]
pub(crate) fn draw_each_player(amount: ValueExpr) -> Instruction {
    draw_cards(PlayerSelector::Group(AffectedPlayers::EachPlayer), amount)
}

#[must_use]
pub(crate) fn draw_player(players: AffectedPlayers, amount: ValueExpr) -> Instruction {
    draw_cards(PlayerSelector::Group(players), amount)
}

#[must_use]
pub(crate) fn draw_resolving_player(player: ResolvingPlayer, amount: ValueExpr) -> Instruction {
    draw_cards(PlayerSelector::Resolving(player), amount)
}

#[must_use]
pub(crate) fn mutate_all(
    source: ObjectQuery,
    op: spellwake_engine::toolbox::ObjectMutation,
) -> Instruction {
    Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(
        source, op,
    )))
}

#[must_use]
pub(crate) fn mutate_subject(
    subject: EffectSubject,
    op: spellwake_engine::toolbox::ObjectMutation,
) -> Instruction {
    mutate_all(ObjectQuery::EffectSubject(Box::new(subject)), op)
}

#[must_use]
pub(crate) fn set_subject_tapped(subject: EffectSubject, tapped: bool) -> Instruction {
    mutate_subject(
        subject,
        spellwake_engine::toolbox::ObjectMutation::SetTapped(tapped),
    )
}

#[must_use]
pub(crate) fn set_group_tapped(filter: ObjectFilter, tapped: bool) -> Instruction {
    mutate_all(
        ObjectQuery::zone(QueryZone::Battlefield, filter),
        spellwake_engine::toolbox::ObjectMutation::SetTapped(tapped),
    )
}

#[must_use]
pub(crate) fn tap_ability_source() -> Instruction {
    mutate_all(
        ObjectQuery::Source,
        spellwake_engine::toolbox::ObjectMutation::SetTapped(true),
    )
}

#[must_use]
pub(crate) fn untap_ability_source() -> Instruction {
    mutate_all(
        ObjectQuery::Source,
        spellwake_engine::toolbox::ObjectMutation::SetTapped(false),
    )
}

#[must_use]
pub(crate) fn phase_out_query(source: ObjectQuery) -> Instruction {
    mutate_all(source, spellwake_engine::toolbox::ObjectMutation::PhaseOut)
}

#[must_use]
pub(crate) fn transform_source() -> Instruction {
    mutate_all(
        ObjectQuery::Source,
        spellwake_engine::toolbox::ObjectMutation::Transform,
    )
}

#[must_use]
pub(crate) fn transform_subject(subject: EffectSubject) -> Instruction {
    mutate_subject(
        subject,
        spellwake_engine::toolbox::ObjectMutation::Transform,
    )
}

#[must_use]
pub(crate) fn turn_face_up_subject(subject: EffectSubject) -> Instruction {
    mutate_subject(
        subject,
        spellwake_engine::toolbox::ObjectMutation::TurnFaceUp,
    )
}

#[must_use]
pub(crate) fn turn_face_down_subject(subject: EffectSubject) -> Instruction {
    mutate_subject(
        subject,
        spellwake_engine::toolbox::ObjectMutation::TurnFaceDown,
    )
}

#[must_use]
pub(crate) fn regenerate_subject(subject: EffectSubject) -> Instruction {
    mutate_subject(
        subject,
        spellwake_engine::toolbox::ObjectMutation::Regenerate,
    )
}

#[must_use]
pub(crate) fn regenerate_source() -> Instruction {
    mutate_all(
        ObjectQuery::Source,
        spellwake_engine::toolbox::ObjectMutation::Regenerate,
    )
}

#[must_use]
pub(crate) fn regenerate_target() -> Instruction {
    mutate_all(
        ObjectQuery::Targets,
        spellwake_engine::toolbox::ObjectMutation::Regenerate,
    )
}

#[must_use]
pub(crate) fn goad_target() -> Instruction {
    mutate_all(
        ObjectQuery::Targets,
        spellwake_engine::toolbox::ObjectMutation::Goad {
            by: PlayerSelector::Resolving(ResolvingPlayer::Controller),
        },
    )
}

#[must_use]
pub(crate) fn goad_other_players_creatures() -> Instruction {
    mutate_all(
        ObjectQuery::zone(
            QueryZone::Battlefield,
            ObjectFilter::creature().not_controlled(),
        ),
        spellwake_engine::toolbox::ObjectMutation::Goad {
            by: PlayerSelector::Resolving(ResolvingPlayer::Controller),
        },
    )
}

#[must_use]
pub(crate) fn exert_source() -> Instruction {
    mutate_all(
        ObjectQuery::Source,
        spellwake_engine::toolbox::ObjectMutation::Exert,
    )
}

#[must_use]
pub(crate) fn skip_next_untap(subject: EffectSubject) -> Instruction {
    mutate_subject(
        subject,
        spellwake_engine::toolbox::ObjectMutation::SkipNextUntap,
    )
}

#[must_use]
pub(crate) fn attach_query(attachment: ObjectQuery, host: ObjectQuery) -> Instruction {
    mutate_all(
        attachment,
        spellwake_engine::toolbox::ObjectMutation::Attach { host },
    )
}

#[must_use]
pub(crate) fn attach_to_player(attachment: ObjectQuery, player: PlayerSelector) -> Instruction {
    mutate_all(
        attachment,
        spellwake_engine::toolbox::ObjectMutation::AttachToPlayer { player },
    )
}

#[must_use]
pub(crate) fn unattach_query(attachment: ObjectQuery) -> Instruction {
    mutate_all(
        attachment,
        spellwake_engine::toolbox::ObjectMutation::Unattach,
    )
}

#[must_use]
pub(crate) fn put_counters_on_query(
    source: ObjectQuery,
    kind: CounterKind,
    amount: ValueExpr,
) -> Instruction {
    mutate_all(
        source,
        spellwake_engine::toolbox::ObjectMutation::AddCounters { kind, amount },
    )
}

#[must_use]
pub(crate) fn put_counters_on_subject(
    subject: EffectSubject,
    kind: CounterKind,
    amount: ValueExpr,
) -> Instruction {
    put_counters_on_query(ObjectQuery::EffectSubject(Box::new(subject)), kind, amount)
}

#[must_use]
pub(crate) fn put_counters_on_source(kind: CounterKind, amount: ValueExpr) -> Instruction {
    put_counters_on_query(ObjectQuery::Source, kind, amount)
}

#[must_use]
pub(crate) fn put_counters_on_target(kind: CounterKind, amount: ValueExpr) -> Instruction {
    put_counters_on_query(ObjectQuery::Targets, kind, amount)
}

#[must_use]
pub(crate) fn remove_counters_from_subject(
    subject: EffectSubject,
    kind: Option<CounterKind>,
    amount: ValueExpr,
) -> Instruction {
    mutate_subject(
        subject,
        spellwake_engine::toolbox::ObjectMutation::RemoveCounters { kind, amount },
    )
}

#[must_use]
pub(crate) fn clear_source_counters(kind: Option<CounterKind>) -> Instruction {
    mutate_all(
        ObjectQuery::Source,
        spellwake_engine::toolbox::ObjectMutation::ClearCounters { kind },
    )
}

#[must_use]
pub(crate) fn double_counters_on_query(
    source: ObjectQuery,
    kind: Option<CounterKind>,
) -> Instruction {
    mutate_all(
        source,
        spellwake_engine::toolbox::ObjectMutation::DoubleCounters { kind },
    )
}

#[must_use]
pub(crate) fn add_one_of_each_counter(subject: EffectSubject) -> Instruction {
    mutate_subject(
        subject,
        spellwake_engine::toolbox::ObjectMutation::AddOneOfEachCounterKind,
    )
}

#[must_use]
pub(crate) fn gain_control_of_subject(
    subject: EffectSubject,
    controller: PlayerSelector,
    duration: spellwake_engine::characteristics::ResolvedDuration,
) -> Instruction {
    mutate_subject(
        subject,
        spellwake_engine::toolbox::ObjectMutation::GainControl {
            controller,
            duration,
        },
    )
}

#[must_use]
pub(crate) fn apply_temporary_rule(
    rule: spellwake_engine::program::TemporaryRuleKind,
    duration: spellwake_engine::characteristics::ResolvedDuration,
) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::temporary_rule(rule, duration),
    ))
}

#[must_use]
pub(crate) fn apply_continuous_parts(
    selector: ObjectQuery,
    parts: spellwake_engine::characteristics::ResolvedPartsSpec,
    duration: spellwake_engine::characteristics::ResolvedDuration,
) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::resolved_parts(selector, parts, duration),
    ))
}

#[must_use]
pub(crate) fn modify_group_eot(
    group: EffectSubject,
    power: ValueExpr,
    toughness: ValueExpr,
    keywords: std::collections::BTreeSet<spellwake_engine::characteristics::KeywordAbility>,
    suppress_keywords: std::collections::BTreeSet<
        spellwake_engine::characteristics::KeywordAbility,
    >,
) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::dynamic_boost(
            ObjectQuery::EffectSubject(Box::new(group)),
            power,
            toughness,
            keywords,
            suppress_keywords,
            spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
        ),
    ))
}

#[must_use]
pub(crate) fn double_pt_subject(
    subject: EffectSubject,
    power: bool,
    toughness: bool,
    duration: spellwake_engine::characteristics::ResolvedDuration,
) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::double_pt(
            ObjectQuery::EffectSubject(Box::new(subject)),
            power,
            toughness,
            duration,
        ),
    ))
}

#[must_use]
pub(crate) fn require_combat_action(
    subject: EffectSubject,
    kind: spellwake_engine::program::CombatActionKind,
) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::combat_action(
            ObjectQuery::EffectSubject(Box::new(subject)),
            kind,
        ),
    ))
}

#[must_use]
pub(crate) fn grant_play_permission(
    selector: ObjectQuery,
    player: PlayerSelector,
    duration: spellwake_engine::characteristics::ResolvedDuration,
    cast_only: bool,
    free: bool,
    any_color: bool,
) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::play_permission(
            selector, player, duration, cast_only, free, any_color,
        ),
    ))
}

#[must_use]
pub(crate) fn prevent_damage_spec(
    spec: spellwake_engine::damage::TurnPreventionSpec,
) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::prevent_damage(spec),
    ))
}

#[must_use]
pub(crate) fn skip_next_turns(player: ResolvingPlayer, count: u8) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::skip_turns(
            PlayerSelector::Resolving(player),
            ValueExpr::literal(i64::from(count)),
        ),
    ))
}

#[must_use]
pub(crate) fn skip_next_step(
    player: ResolvingPlayer,
    step: spellwake_engine::turn::Step,
) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::skip_step(
            PlayerSelector::Resolving(player),
            step,
        ),
    ))
}

#[must_use]
pub(crate) fn return_to_owners_hand() -> Instruction {
    zone_op(ZoneOpSpec::move_all(ObjectQuery::Targets, ZoneDest::Hand))
}

#[must_use]
pub(crate) fn return_source_to_hand() -> Instruction {
    zone_op(ZoneOpSpec::move_all(ObjectQuery::Source, ZoneDest::Hand))
}

#[must_use]
pub(crate) fn return_source_graveyard_to_battlefield(tapped: bool) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Source,
        ZoneDest::Battlefield {
            controller: ZoneController::Owner,
            tapped,
            attacking: false,
            transformed: false,
            face_down: false,
            entering_counters: Vec::new(),
            attach_to: None,
        },
    ))
}

#[must_use]
pub(crate) fn return_source_graveyard_to_hand() -> Instruction {
    zone_op(ZoneOpSpec::move_all(ObjectQuery::Source, ZoneDest::Hand))
}

#[must_use]
pub(crate) fn exile_target_until_source_leaves() -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Targets,
        ZoneDest::Exile {
            face_down: false,
            viewer: None,
            link_to_source: true,
            delayed_return: Some(DelayedReturnTiming::UntilSourceLeaves),
            link_group: None,
        },
    ))
}

#[must_use]
pub(crate) fn exile_library_top(
    player: ResolvingPlayer,
    count: ValueExpr,
    face_down: bool,
) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::ZoneScan {
            zone: QueryZone::Library(Box::new(PlayerSelector::Resolving(player))),
            filter: Box::new(ObjectFilter::any()),
            slice: QuerySlice::Top(Box::new(count)),
        },
        ZoneDest::Exile {
            face_down,
            viewer: None,
            link_to_source: false,
            delayed_return: None,
            link_group: None,
        },
    ))
}

#[must_use]
pub(crate) fn mill_players(players: AffectedPlayers, amount: ValueExpr) -> Instruction {
    const SEAT_BIND: BindingId = BindingId(905);
    let inner = zone_op(ZoneOpSpec::move_all(
        ObjectQuery::ZoneScan {
            zone: QueryZone::Library(Box::new(PlayerSelector::Bound(SEAT_BIND))),
            filter: Box::new(ObjectFilter::any()),
            slice: QuerySlice::Top(Box::new(amount)),
        },
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Mill,
        },
    ));
    Instruction::ForEachPlayerQuery {
        selector: Box::new(PlayerSelector::Group(players)),
        bind_player: SEAT_BIND,
        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner])),
    }
}
