//! Clean, composable builders emitting universal `Instruction::ZoneOp` / `ForEachPlayerQuery`
//! programs for all zone-movement, exile, return, destroy, sacrifice, discard, shuffle, and
//! reveal operations.

use spellwake_engine::{
    effects::CounterKind,
    program::{
        AffectedPlayers, BodyTarget, DelayedReturnTiming, EffectSubject, ExecutableBody,
        Instruction, LibraryPlacement, LinkedExileGroup, MoveSubjectRef, ObjectFilter,
        PermanentTypeFilter, PublicZoneDestination, ResolvingPlayer, ValueExpr,
    },
    toolbox::{
        BindingId, ChoiceCategory, GraveyardSemantics, LibraryOrderPos, ObjectQuery,
        PlayerSelector, QuerySlice, QueryZone, ZoneController, ZoneDest, ZoneOpSpec, ZoneSelection,
    },
};

#[must_use]
pub(crate) fn zone_op(spec: ZoneOpSpec) -> Instruction {
    Instruction::ZoneOp(Box::new(spec))
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
pub(crate) fn put_target_in_library(placement: LibraryPlacement) -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Targets,
        ZoneDest::Library {
            position: LibraryOrderPos::from_placement(placement),
        },
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
            face_down: None,
            entering_counters: Vec::new(),
            attach_to: None,
        },
    ))
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
            face_down: None,
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
        ZoneOpSpec::permanent_type_filter_query(
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
        ZoneOpSpec::permanent_type_filter_query(
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
pub(crate) fn set_group_tapped(filter: ObjectFilter, tapped: bool) -> Instruction {
    mutate_all(
        ObjectQuery::zone(QueryZone::Battlefield, filter),
        spellwake_engine::toolbox::ObjectMutation::TapState(
            spellwake_engine::toolbox::TapStateOp::Set(tapped),
        ),
    )
}

#[must_use]
pub(crate) fn tap_ability_source() -> Instruction {
    mutate_all(
        ObjectQuery::Source,
        spellwake_engine::toolbox::ObjectMutation::TapState(
            spellwake_engine::toolbox::TapStateOp::Set(true),
        ),
    )
}

#[must_use]
pub(crate) fn untap_ability_source() -> Instruction {
    mutate_all(
        ObjectQuery::Source,
        spellwake_engine::toolbox::ObjectMutation::TapState(
            spellwake_engine::toolbox::TapStateOp::Set(false),
        ),
    )
}

#[must_use]
pub(crate) fn phase_out_query(source: ObjectQuery) -> Instruction {
    mutate_all(
        source,
        spellwake_engine::toolbox::ObjectMutation::Orientation(
            spellwake_engine::toolbox::OrientationOp::PhaseOut,
        ),
    )
}

#[must_use]
pub(crate) fn transform_source() -> Instruction {
    mutate_all(
        ObjectQuery::Source,
        spellwake_engine::toolbox::ObjectMutation::Orientation(
            spellwake_engine::toolbox::OrientationOp::Transform,
        ),
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
            permanent: false,
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
            permanent: false,
        },
    )
}

#[must_use]
pub(crate) fn skip_next_untap(subject: EffectSubject) -> Instruction {
    mutate_subject(
        subject,
        spellwake_engine::toolbox::ObjectMutation::TapState(
            spellwake_engine::toolbox::TapStateOp::SkipNextUntap,
        ),
    )
}

#[must_use]
pub(crate) fn unattach_query(attachment: ObjectQuery) -> Instruction {
    mutate_all(
        attachment,
        spellwake_engine::toolbox::ObjectMutation::Attach(
            spellwake_engine::toolbox::AttachOp::Unattach,
        ),
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
        spellwake_engine::toolbox::ObjectMutation::Counters(
            spellwake_engine::toolbox::CounterOp::Add { kind, amount },
        ),
    )
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
pub(crate) fn clear_source_counters(kind: Option<CounterKind>) -> Instruction {
    mutate_all(
        ObjectQuery::Source,
        spellwake_engine::toolbox::ObjectMutation::Counters(
            spellwake_engine::toolbox::CounterOp::Clear { kind },
        ),
    )
}

#[must_use]
pub(crate) fn double_counters_on_query(
    source: ObjectQuery,
    kind: Option<CounterKind>,
) -> Instruction {
    mutate_all(
        source,
        spellwake_engine::toolbox::ObjectMutation::Counters(
            spellwake_engine::toolbox::CounterOp::Double { kind },
        ),
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
            face_down: None,
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

#[must_use]
pub(crate) fn gain_life(players: PlayerSelector, amount: ValueExpr) -> Instruction {
    Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(
        players, amount,
    )))
}

#[must_use]
pub(crate) fn lose_life(players: PlayerSelector, amount: ValueExpr) -> Instruction {
    Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::lose(
        players, amount,
    )))
}

#[must_use]
pub(crate) fn pay_life(players: PlayerSelector, amount: ValueExpr) -> Instruction {
    Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::pay(
        players, amount,
    )))
}

#[must_use]
pub(crate) fn drain_life(
    losers: PlayerSelector,
    amount: ValueExpr,
    gain_to: PlayerSelector,
    per_loss: bool,
) -> Instruction {
    Instruction::AdjustLife(Box::new(spellwake_engine::toolbox::AdjustLifeSpec::drain(
        losers, amount, gain_to, per_loss,
    )))
}

#[must_use]
pub(crate) fn lose_half_life(players: PlayerSelector, round_up: bool) -> Instruction {
    Instruction::AdjustLife(Box::new(
        spellwake_engine::toolbox::AdjustLifeSpec::lose_half(players, round_up),
    ))
}

#[must_use]
pub(crate) fn deal_damage(
    recipients: spellwake_engine::toolbox::DamageTargetQuery,
    amount: ValueExpr,
    exile_if_dies: bool,
) -> Instruction {
    Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec {
        source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
        recipients,
        amount: spellwake_engine::program::DamageAmountSpec::Expr(amount),
        exile_if_dies,
    }))
}

#[must_use]
pub(crate) fn deal_damage_from(
    source: spellwake_engine::toolbox::DamageSourceQuery,
    recipients: spellwake_engine::toolbox::DamageTargetQuery,
    amount: spellwake_engine::program::DamageAmountSpec,
    exile_if_dies: bool,
) -> Instruction {
    Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec {
        source,
        recipients,
        amount,
        exile_if_dies,
    }))
}

#[must_use]
pub(crate) fn add_player_counter(
    players: PlayerSelector,
    kind: spellwake_engine::effects::CounterKind,
    amount: ValueExpr,
) -> Instruction {
    Instruction::MutatePlayer(Box::new(
        spellwake_engine::toolbox::MutatePlayerSpec::add_counter(players, kind, amount),
    ))
}

#[must_use]
pub(crate) fn take_extra_turn(players: PlayerSelector) -> Instruction {
    Instruction::MutatePlayer(Box::new(
        spellwake_engine::toolbox::MutatePlayerSpec::take_extra_turn(players),
    ))
}

#[must_use]
pub(crate) fn end_turn() -> Instruction {
    Instruction::MutatePlayer(Box::new(
        spellwake_engine::toolbox::MutatePlayerSpec::end_turn(),
    ))
}

#[must_use]
pub(crate) fn become_monarch(players: PlayerSelector) -> Instruction {
    Instruction::MutatePlayer(Box::new(
        spellwake_engine::toolbox::MutatePlayerSpec::become_monarch(players),
    ))
}

#[must_use]
pub(crate) fn create_emblem(
    players: PlayerSelector,
    definition: spellwake_engine::emblem::EmblemDefinition,
) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::emblem(players, definition),
    ))
}

#[must_use]
pub(crate) fn remove_from_combat(source: ObjectQuery) -> Instruction {
    Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(
        source,
        spellwake_engine::toolbox::ObjectMutation::Combat(
            spellwake_engine::toolbox::ObjectCombatOp::RemoveFromCombat,
        ),
    )))
}

#[must_use]
pub(crate) fn move_counters(
    source: ObjectQuery,
    to: ObjectQuery,
    kind: Option<spellwake_engine::effects::CounterKind>,
    amount: Option<ValueExpr>,
) -> Instruction {
    Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(
        source,
        spellwake_engine::toolbox::ObjectMutation::Counters(
            spellwake_engine::toolbox::CounterOp::Move { to, kind, amount },
        ),
    )))
}

#[must_use]
pub(crate) fn gain_control(
    source: ObjectQuery,
    controller: PlayerSelector,
    duration: spellwake_engine::characteristics::ResolvedDuration,
) -> Instruction {
    Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(
        source,
        spellwake_engine::toolbox::ObjectMutation::Control(
            spellwake_engine::toolbox::ControlOp::Gain {
                controller,
                duration,
            },
        ),
    )))
}

#[must_use]
pub(crate) fn destroy_targets() -> Instruction {
    zone_op(ZoneOpSpec::move_all(
        ObjectQuery::Targets,
        ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Destroy {
                allow_regenerate: true,
            },
        },
    ))
}

#[must_use]
pub(crate) fn set_targets_tapped(tapped: bool) -> Instruction {
    mutate_all(
        ObjectQuery::Targets,
        spellwake_engine::toolbox::ObjectMutation::TapState(
            spellwake_engine::toolbox::TapStateOp::Set(tapped),
        ),
    )
}

#[must_use]
pub(crate) fn toggle_targets_tapped() -> Instruction {
    mutate_all(
        ObjectQuery::Targets,
        spellwake_engine::toolbox::ObjectMutation::TapState(
            spellwake_engine::toolbox::TapStateOp::Toggle,
        ),
    )
}

#[must_use]
pub(crate) fn skip_next_untap_targets() -> Instruction {
    mutate_all(
        ObjectQuery::Targets,
        spellwake_engine::toolbox::ObjectMutation::TapState(
            spellwake_engine::toolbox::TapStateOp::SkipNextUntap,
        ),
    )
}

#[must_use]
pub(crate) fn exile_top_library_group(
    owners: AffectedPlayers,
    count: ValueExpr,
    face_down: bool,
) -> Instruction {
    zone_op(ZoneOpSpec::exile_top_library(
        PlayerSelector::Group(owners),
        count,
        face_down,
    ))
}

#[must_use]
pub(crate) fn impulse_play_last_moved(
    duration: spellwake_engine::program::ImpulsePlayDuration,
    cast_only: bool,
    free: bool,
    any_color: bool,
) -> Instruction {
    Instruction::ApplyContinuous(Box::new(
        spellwake_engine::toolbox::ApplyContinuousSpec::impulse_play_permission(
            ObjectQuery::from_move_subject(MoveSubjectRef::LastMoved),
            PlayerSelector::Resolving(ResolvingPlayer::Controller),
            duration,
            cast_only,
            free,
            any_color,
        ),
    ))
}

#[must_use]
pub(crate) fn choose_creature_type(
    into: spellwake_engine::program::CreatureTypeSlot,
    excluding: std::collections::BTreeSet<spellwake_contracts::creature_types::CreatureTypeId>,
) -> Instruction {
    Instruction::PromptChoice(Box::new(
        spellwake_engine::toolbox::PromptChoiceSpec::choose_creature_type(into, excluding),
    ))
}

#[must_use]
pub(crate) fn put_counters_on_targets_by_current_type() -> [Instruction; 2] {
    [
        put_counters_on_query(
            ObjectQuery::filtered(ObjectQuery::Targets, ObjectFilter::creature()),
            CounterKind::PlusOne,
            ValueExpr::literal(1),
        ),
        put_counters_on_query(
            ObjectQuery::filtered(ObjectQuery::Targets, ObjectFilter::planeswalker()),
            CounterKind::Loyalty,
            ValueExpr::literal(1),
        ),
    ]
}
