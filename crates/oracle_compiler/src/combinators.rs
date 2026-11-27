//! Shared compositional parsing combinators and lightweight AST constructors used across
//! `filter/` and `clauses/`.
//!
//! Centralizes punctuation trimming, balanced-quote extraction, recursion/operation budgeting,
//! common noun/step/rounding/visibility/color parsers, and small AST filter constructors so
//! submodules stay focused on Magic grammar rules rather than repeating boilerplate.

use std::cell::Cell;

use crate::ast::{
    CardTypeMask, CompareOp, ConditionAst, CostAtomAst, DelayedStepScope, DurationAst,
    ExchangeKind, FaceAst, GameMarker, InstructionAst, ManaSpecAst, ObjectFilterAst, PileRefAst,
    PileVisibilityAst, PlayerRefAst, PlayerStatKind, ReplacementEffectAst, Rounding, StatKind,
    StaticAbilityAst, SubjectRefAst, TokenSpecAst, TriggerSpecAst, TurnStepAst, ValueExprAst,
    ZoneFilter,
};
use crate::filter::parse_player_ref;

/// Trim surrounding whitespace and trailing clause punctuation (`.`, `,`, `;`).
#[inline]
pub(crate) fn trim_punct(s: &str) -> &str {
    s.trim().trim_end_matches(['.', ',', ';'])
}

thread_local! {
    static PARSE_DEPTH: Cell<u32> = const { Cell::new(0) };
    static PARSE_OPS: Cell<u32> = const { Cell::new(0) };
}

/// RAII recursion-depth and operation-budget guard preventing runaway backtracking in recursive
/// clause combinators.
pub(crate) struct DepthGuard;

impl DepthGuard {
    /// Current recursion depth on this thread.
    #[inline]
    pub(crate) fn current_depth() -> u32 {
        PARSE_DEPTH.with(Cell::get)
    }

    /// Enter one recursive parsing frame; returns `None` when depth or op budget is exhausted.
    #[inline]
    pub(crate) fn enter() -> Option<Self> {
        PARSE_DEPTH.with(|d| {
            let cur = d.get();
            if cur == 0 {
                PARSE_OPS.with(|ops| ops.set(1));
                d.set(1);
                return Some(Self);
            }
            if cur >= 7 {
                return None;
            }
            let allowed = PARSE_OPS.with(|ops| {
                let n = ops.get();
                if n >= 800 {
                    false
                } else {
                    ops.set(n + 1);
                    true
                }
            });
            if !allowed {
                return None;
            }
            d.set(cur + 1);
            Some(Self)
        })
    }
}

impl Drop for DepthGuard {
    #[inline]
    fn drop(&mut self) {
        PARSE_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Extract all balanced double-quoted spans from `text`, plus any non-quoted prefix before the
/// first quote and trailing suffix after the last quote. Returns `None` if quotes are unbalanced
/// or absent.
pub(crate) fn extract_quoted_spans(text: &str) -> Option<(&str, Vec<&str>, &str)> {
    let mut spans = Vec::new();
    let mut first_open: Option<usize> = None;
    let mut open: Option<usize> = None;
    let mut last_close: Option<usize> = None;
    for (idx, ch) in text.char_indices() {
        if ch == '"' {
            if let Some(start) = open.take() {
                spans.push(&text[start + 1..idx]);
                last_close = Some(idx);
            } else {
                if first_open.is_none() {
                    first_open = Some(idx);
                }
                open = Some(idx);
            }
        }
    }
    if open.is_some() || spans.is_empty() {
        return None;
    }
    let before = &text[..first_open?];
    let after = &text[last_close? + 1..];
    Some((before, spans, after))
}

/// Convenience constructor for an [`ObjectFilterAst`] matching one or more card types.
#[inline]
pub(crate) fn obj_type(mask: CardTypeMask) -> ObjectFilterAst {
    ObjectFilterAst {
        any_types: mask,
        ..Default::default()
    }
}

/// Convenience constructor for an [`ObjectFilterAst`] matching any permanent.
#[inline]
pub(crate) fn any_perm() -> ObjectFilterAst {
    ObjectFilterAst {
        any_permanent: true,
        ..Default::default()
    }
}

/// Convenience constructor for an [`ObjectFilterAst`] matching any card.
#[inline]
pub(crate) fn any_card_flt() -> ObjectFilterAst {
    ObjectFilterAst {
        any_card: true,
        ..Default::default()
    }
}

/// Convenience constructor for a card-form [`ObjectFilterAst`] matching one or more card types.
#[inline]
pub(crate) fn card_flt(mask: CardTypeMask) -> ObjectFilterAst {
    ObjectFilterAst {
        any_types: mask,
        any_card: true,
        ..Default::default()
    }
}

/// Convenience constructor for a boxed [`SubjectRefAst::Anaphora`].
#[inline]
pub(crate) fn box_anaphora(text: impl Into<String>) -> Box<SubjectRefAst> {
    Box::new(SubjectRefAst::Anaphora(text.into()))
}

/// Convenience constructor for a boxed [`SubjectRefAst::SelfObject`].
#[inline]
pub(crate) fn self_obj() -> Box<SubjectRefAst> {
    Box::new(SubjectRefAst::SelfObject)
}

/// Wrap a [`GameMarker`] in [`ConditionAst::Marker`].
#[inline]
pub(crate) fn cond_marker(marker: GameMarker) -> ConditionAst {
    ConditionAst::Marker(marker)
}

/// Wrap a [`ConditionAst`] in [`ConditionAst::Not`].
#[inline]
pub(crate) fn cond_not(cond: ConditionAst) -> ConditionAst {
    ConditionAst::Not(Box::new(cond))
}

/// Build a [`ConditionAst::HasStatus`].
#[inline]
pub(crate) fn cond_status(subject: SubjectRefAst, status: impl Into<String>) -> ConditionAst {
    ConditionAst::HasStatus {
        subject: Box::new(subject),
        status: status.into(),
    }
}

/// Build a [`ConditionAst::Compare`].
#[inline]
pub(crate) fn cond_cmp(lhs: ValueExprAst, op: CompareOp, rhs: ValueExprAst) -> ConditionAst {
    ConditionAst::Compare {
        lhs: Box::new(lhs),
        op,
        rhs: Box::new(rhs),
    }
}

/// Build a [`ConditionAst::ControlsCount`].
#[inline]
pub(crate) fn cond_controls(
    player: PlayerRefAst,
    filter: ObjectFilterAst,
    op: CompareOp,
    value: u16,
) -> ConditionAst {
    ConditionAst::ControlsCount {
        player,
        filter: Box::new(filter),
        op,
        value,
    }
}

/// Build a [`ConditionAst::ZoneObjectCount`].
#[inline]
pub(crate) fn cond_zone_count(filter: ObjectFilterAst, op: CompareOp, value: u16) -> ConditionAst {
    ConditionAst::ZoneObjectCount {
        filter: Box::new(filter),
        op,
        value,
    }
}

/// Build an [`InstructionAst::IfCondition`].
#[inline]
pub(crate) fn ins_if(condition: ConditionAst, then_body: Vec<InstructionAst>) -> InstructionAst {
    InstructionAst::IfCondition {
        condition: Box::new(condition),
        then_body,
    }
}

/// Build an [`InstructionAst::ForEachPlayer`].
#[inline]
pub(crate) fn ins_for_each_player(
    player: PlayerRefAst,
    body: Vec<InstructionAst>,
) -> InstructionAst {
    InstructionAst::ForEachPlayer { player, body }
}

/// Build an [`InstructionAst::AbilityRestriction`] with no extra condition.
#[inline]
pub(crate) fn ins_ability_restr(restriction: impl Into<String>) -> InstructionAst {
    InstructionAst::AbilityRestriction {
        restriction: restriction.into(),
        condition: None,
    }
}

/// Build an [`InstructionAst::CombatRuleMod`].
#[inline]
pub(crate) fn ins_combat_rule(
    subject: SubjectRefAst,
    rule: impl Into<String>,
    duration: DurationAst,
) -> InstructionAst {
    InstructionAst::CombatRuleMod {
        subject: Box::new(subject),
        rule: rule.into(),
        duration,
    }
}

/// Build an [`InstructionAst::TemporaryStatic`].
#[inline]
pub(crate) fn ins_temp_static(
    static_ability: StaticAbilityAst,
    duration: DurationAst,
) -> InstructionAst {
    InstructionAst::TemporaryStatic {
        static_ability: Box::new(static_ability),
        duration,
    }
}

/// Build a standard [`InstructionAst::Exile`] with default flags.
#[inline]
pub(crate) fn ins_exile(actor: Option<PlayerRefAst>, subject: SubjectRefAst) -> InstructionAst {
    InstructionAst::Exile {
        actor,
        subject: Box::new(subject),
        random: false,
        face_down: false,
        into_pile: false,
    }
}

/// Build a standard [`InstructionAst::MoveZone`].
#[inline]
pub(crate) fn ins_move(subject: SubjectRefAst, to: impl Into<String>) -> InstructionAst {
    InstructionAst::MoveZone {
        subject: Box::new(subject),
        to: to.into(),
    }
}

/// Build a standard [`InstructionAst::Draw`].
#[inline]
pub(crate) fn ins_draw(player: PlayerRefAst, count: ValueExprAst) -> InstructionAst {
    InstructionAst::Draw {
        player,
        count: Box::new(count),
        up_to: false,
    }
}

/// Build a standard [`InstructionAst::ChangeLife`].
#[inline]
pub(crate) fn ins_life(player: PlayerRefAst, delta: ValueExprAst, gain: bool) -> InstructionAst {
    InstructionAst::ChangeLife {
        player,
        delta: Box::new(delta),
        gain,
    }
}

/// Build a standard [`InstructionAst::DealDamage`].
#[inline]
pub(crate) fn ins_dmg(
    source: SubjectRefAst,
    target: SubjectRefAst,
    amount: ValueExprAst,
) -> InstructionAst {
    InstructionAst::DealDamage {
        source: Box::new(source),
        target: Box::new(target),
        amount: Box::new(amount),
    }
}

/// Build a standard [`InstructionAst::PutCounters`].
#[inline]
pub(crate) fn ins_counters(
    subject: SubjectRefAst,
    counter_kind: impl Into<String>,
    count: ValueExprAst,
) -> InstructionAst {
    InstructionAst::PutCounters {
        subject: Box::new(subject),
        counter_kind: counter_kind.into(),
        count: Box::new(count),
    }
}

/// Build a standard [`InstructionAst::Sacrifice`] with default optional modifiers.
#[inline]
pub(crate) fn ins_sac(player: PlayerRefAst, subject: SubjectRefAst) -> InstructionAst {
    InstructionAst::Sacrifice {
        player,
        count: None,
        subject: Box::new(subject),
        except: None,
        half: false,
        rounding: None,
        chooser: None,
    }
}

/// Build a standard [`InstructionAst::Discard`] with default optional modifiers.
#[inline]
pub(crate) fn ins_discard(player: PlayerRefAst, count: Option<ValueExprAst>) -> InstructionAst {
    InstructionAst::Discard {
        player,
        count: count.map(Box::new),
        filter: None,
        anaphora: None,
        random: false,
    }
}

/// Build a standard [`InstructionAst::GainControl`].
#[inline]
pub(crate) fn ins_gain_control(
    controller: PlayerRefAst,
    subject: SubjectRefAst,
    duration: DurationAst,
) -> InstructionAst {
    InstructionAst::GainControl {
        controller,
        subject: Box::new(subject),
        duration,
    }
}

/// Build a standard [`InstructionAst::GrantCastPermission`] with default optional modifiers.
#[inline]
pub(crate) fn ins_grant_cast(
    player: PlayerRefAst,
    cast_only: bool,
    subject: SubjectRefAst,
    free: bool,
    duration: impl Into<String>,
) -> InstructionAst {
    InstructionAst::GrantCastPermission {
        player,
        cast_only,
        subject: Box::new(subject),
        free,
        duration: duration.into(),
        count_limit: None,
        alt_cost: None,
    }
}

/// Build a [`ValueExprAst::ObjectStat`].
#[inline]
pub(crate) fn val_obj_stat(subject: SubjectRefAst, stat: StatKind) -> ValueExprAst {
    ValueExprAst::ObjectStat {
        subject: Box::new(subject),
        stat,
    }
}

/// Build a [`ValueExprAst::PlayerStat`].
#[inline]
pub(crate) fn val_player_stat(player: PlayerRefAst, stat: PlayerStatKind) -> ValueExprAst {
    ValueExprAst::PlayerStat { player, stat }
}

/// Build a [`ValueExprAst::DivConst`].
#[inline]
pub(crate) fn val_div(expr: ValueExprAst, denom: i32, rounding: Rounding) -> ValueExprAst {
    ValueExprAst::DivConst {
        expr: Box::new(expr),
        denom,
        rounding,
    }
}

/// Build a [`ConditionAst::PlayerActionThisTurn`].
#[inline]
pub(crate) fn cond_action(player: PlayerRefAst, action: impl Into<String>) -> ConditionAst {
    ConditionAst::PlayerActionThisTurn {
        player,
        action: action.into(),
    }
}

/// Build a [`ConditionAst::MatchesFilter`].
#[inline]
pub(crate) fn cond_matches(subject: SubjectRefAst, filter: ObjectFilterAst) -> ConditionAst {
    ConditionAst::MatchesFilter {
        subject: Box::new(subject),
        filter: Box::new(filter),
    }
}

/// Build a [`ConditionAst::PriorActionOccurred`].
#[inline]
pub(crate) fn cond_prior(
    action: impl Into<String>,
    subject_filter: Option<ObjectFilterAst>,
    min_count: u8,
) -> ConditionAst {
    ConditionAst::PriorActionOccurred {
        action: action.into(),
        subject_filter: subject_filter.map(Box::new),
        min_count,
    }
}

/// Build a [`ConditionAst::CounterCompare`].
#[inline]
pub(crate) fn cond_counter_cmp(
    subject: SubjectRefAst,
    counter_kind: impl Into<String>,
    op: CompareOp,
    value: u16,
) -> ConditionAst {
    ConditionAst::CounterCompare {
        subject: Box::new(subject),
        counter_kind: counter_kind.into(),
        op,
        value,
    }
}

/// Build an [`InstructionAst::GrantKeywords`].
#[inline]
pub(crate) fn ins_grant_kw(
    subject: SubjectRefAst,
    keywords: Vec<String>,
    duration: DurationAst,
) -> InstructionAst {
    InstructionAst::GrantKeywords {
        subject: Box::new(subject),
        keywords,
        duration,
    }
}

/// Build an [`InstructionAst::GrantAbilityText`].
#[inline]
pub(crate) fn ins_grant_ab(
    subject: SubjectRefAst,
    ability_text: impl Into<String>,
    compiled: Option<Box<FaceAst>>,
    duration: DurationAst,
) -> InstructionAst {
    InstructionAst::GrantAbilityText {
        subject: Box::new(subject),
        ability_text: ability_text.into(),
        compiled,
        duration,
    }
}

/// Build an [`InstructionAst::LookTopLibrary`].
#[inline]
pub(crate) fn ins_look_top(
    reveal: bool,
    library: impl Into<String>,
    count: ValueExprAst,
) -> InstructionAst {
    InstructionAst::LookTopLibrary {
        reveal,
        library: library.into(),
        count: Box::new(count),
    }
}

/// Build an [`InstructionAst::ExileTopLibrary`].
#[inline]
pub(crate) fn ins_exile_top(
    actor: Option<PlayerRefAst>,
    whose: impl Into<String>,
    count: ValueExprAst,
    face_down: bool,
) -> InstructionAst {
    InstructionAst::ExileTopLibrary {
        actor,
        whose: whose.into(),
        count: Box::new(count),
        face_down,
    }
}

/// Build an [`InstructionAst::ChooseFromGroup`].
#[inline]
pub(crate) fn ins_choose_group(
    count: ValueExprAst,
    random: bool,
    group: SubjectRefAst,
) -> InstructionAst {
    InstructionAst::ChooseFromGroup {
        count: Box::new(count),
        random,
        group: Box::new(group),
    }
}

/// Build an [`InstructionAst::DelayedTrigger`].
#[inline]
pub(crate) fn ins_delayed(
    step: TurnStepAst,
    scope: DelayedStepScope,
    body: Vec<InstructionAst>,
) -> InstructionAst {
    InstructionAst::DelayedTrigger { step, scope, body }
}

/// Build an [`InstructionAst::TemporaryTrigger`].
#[inline]
pub(crate) fn ins_temp_trig(
    trigger: TriggerSpecAst,
    body: Vec<InstructionAst>,
    duration: DurationAst,
) -> InstructionAst {
    InstructionAst::TemporaryTrigger {
        trigger,
        body,
        duration,
    }
}

/// Build an [`InstructionAst::ForEachCount`].
#[inline]
pub(crate) fn ins_for_each_count(count: ValueExprAst, body: Vec<InstructionAst>) -> InstructionAst {
    InstructionAst::ForEachCount {
        count: Box::new(count),
        body,
    }
}

/// Build an [`InstructionAst::RemoveCounters`].
#[inline]
pub(crate) fn ins_rm_counters(
    subject: SubjectRefAst,
    counter_kind: impl Into<String>,
    all: bool,
    count: Option<ValueExprAst>,
) -> InstructionAst {
    InstructionAst::RemoveCounters {
        subject: Box::new(subject),
        counter_kind: counter_kind.into(),
        all,
        count: count.map(Box::new),
    }
}

/// Build an [`InstructionAst::GainPlayerCounters`].
#[inline]
pub(crate) fn ins_player_counters(
    player: PlayerRefAst,
    counter_kind: impl Into<String>,
    count: ValueExprAst,
) -> InstructionAst {
    InstructionAst::GainPlayerCounters {
        player,
        counter_kind: counter_kind.into(),
        count: Box::new(count),
    }
}

/// Build an [`InstructionAst::Copy`].
#[inline]
pub(crate) fn ins_copy(
    subject: SubjectRefAst,
    exception: Option<String>,
    count: Option<ValueExprAst>,
) -> InstructionAst {
    InstructionAst::Copy {
        subject: Box::new(subject),
        exception,
        count: count.map(Box::new),
    }
}

/// Build an [`InstructionAst::ModifyPT`] with no scaling expression.
#[inline]
pub(crate) fn ins_mod_pt(
    subject: SubjectRefAst,
    dx: impl Into<String>,
    dy: impl Into<String>,
    duration: DurationAst,
) -> InstructionAst {
    InstructionAst::ModifyPT {
        subject: Box::new(subject),
        dx: dx.into(),
        dy: dy.into(),
        duration,
        scale: None,
    }
}

/// Build an [`InstructionAst::AnimateOrSetBasePT`].
#[inline]
pub(crate) fn ins_animate(
    subject: SubjectRefAst,
    power: impl Into<String>,
    toughness: impl Into<String>,
    description: Option<String>,
    keywords: Vec<String>,
    duration: DurationAst,
) -> InstructionAst {
    InstructionAst::AnimateOrSetBasePT {
        subject: Box::new(subject),
        power: power.into(),
        toughness: toughness.into(),
        description,
        keywords,
        duration,
    }
}

/// Build an [`InstructionAst::OptionalPlayer`].
#[inline]
pub(crate) fn ins_opt_player(player: PlayerRefAst, body: Vec<InstructionAst>) -> InstructionAst {
    InstructionAst::OptionalPlayer { player, body }
}

/// Build an [`InstructionAst::AddMana`].
#[inline]
pub(crate) fn ins_add_mana(player: PlayerRefAst, mana: ManaSpecAst) -> InstructionAst {
    InstructionAst::AddMana { player, mana }
}

/// Build an [`InstructionAst::CreateToken`].
#[inline]
pub(crate) fn ins_create_token(controller: PlayerRefAst, token: TokenSpecAst) -> InstructionAst {
    InstructionAst::CreateToken { controller, token }
}

/// Build a simple [`InstructionAst::ReturnZone`] with default optional modifiers.
#[inline]
pub(crate) fn ins_return_simple(
    subject: SubjectRefAst,
    to: impl Into<String>,
    tapped: bool,
) -> InstructionAst {
    InstructionAst::ReturnZone {
        subject: Box::new(subject),
        from_zone: None,
        to: to.into(),
        tapped,
        transformed: false,
        with_counter: None,
        attached_to: None,
        random: false,
        except: None,
        count: None,
        up_to: false,
        any_number: false,
        actor: None,
    }
}

/// Convenience constructors for single-instruction `Result`/`Option` returns and common AST nodes.
#[inline]
pub(crate) fn ok1(ins: InstructionAst) -> Result<Vec<InstructionAst>, String> {
    Ok(vec![ins])
}

#[inline]
pub(crate) fn some1(ins: InstructionAst) -> Option<Vec<InstructionAst>> {
    Some(vec![ins])
}

#[inline]
pub(crate) fn anaphora(text: impl Into<String>) -> SubjectRefAst {
    SubjectRefAst::Anaphora(text.into())
}

#[inline]
pub(crate) fn grp(filter: ObjectFilterAst) -> SubjectRefAst {
    SubjectRefAst::Group(Box::new(filter))
}

#[inline]
pub(crate) fn vconst(n: i32) -> Box<ValueExprAst> {
    Box::new(ValueExprAst::Const(n))
}

#[inline]
pub(crate) fn any_spell_flt() -> ObjectFilterAst {
    ObjectFilterAst {
        any_spell: true,
        ..Default::default()
    }
}

#[inline]
pub(crate) fn ins_opt1(ins: InstructionAst) -> InstructionAst {
    InstructionAst::Optional(vec![ins])
}

#[inline]
pub(crate) fn ins_shuffle(player: PlayerRefAst) -> InstructionAst {
    InstructionAst::ShuffleLibrary { player }
}

#[inline]
pub(crate) fn ins_shuffle_into(subject: SubjectRefAst) -> InstructionAst {
    InstructionAst::ShuffleIntoLibrary {
        subject: Box::new(subject),
    }
}

#[inline]
pub(crate) fn ins_unless(
    body: Vec<InstructionAst>,
    unless_player: Option<PlayerRefAst>,
    unless_cost: Option<CostAtomAst>,
    unless_action: Option<Vec<InstructionAst>>,
    unless_condition: Option<ConditionAst>,
) -> InstructionAst {
    InstructionAst::Unless {
        body,
        unless_player,
        unless_cost,
        unless_action,
        unless_condition: unless_condition.map(Box::new),
    }
}

#[inline]
pub(crate) fn ins_fe_subj(players: SubjectRefAst, body: Vec<InstructionAst>) -> InstructionAst {
    InstructionAst::ForEachSubjectPlayer {
        players: Box::new(players),
        body,
    }
}

#[inline]
pub(crate) fn ins_move_cnt(
    actor: Option<PlayerRefAst>,
    count: Option<ValueExprAst>,
    up_to: bool,
    any_number: bool,
    subject: SubjectRefAst,
    to: impl Into<String>,
) -> InstructionAst {
    InstructionAst::MoveCounted {
        actor,
        count: count.map(Box::new),
        up_to,
        any_number,
        subject: Box::new(subject),
        to: to.into(),
    }
}

#[inline]
pub(crate) fn ins_exchange(
    kind: ExchangeKind,
    left: SubjectRefAst,
    right: Option<SubjectRefAst>,
) -> InstructionAst {
    InstructionAst::Exchange {
        kind,
        left: Box::new(left),
        right: right.map(Box::new),
    }
}

#[inline]
pub(crate) fn ins_repeat(
    times: Option<ValueExprAst>,
    for_list: Vec<String>,
    until: Option<String>,
) -> InstructionAst {
    InstructionAst::RepeatProcessExt {
        times: times.map(Box::new),
        for_list,
        until,
    }
}

#[inline]
pub(crate) fn ins_temp_rep(
    replacement: ReplacementEffectAst,
    duration: DurationAst,
) -> InstructionAst {
    InstructionAst::TemporaryReplacement {
        replacement: Box::new(replacement),
        duration,
    }
}

#[inline]
pub(crate) fn ins_pile_action(
    action: impl Into<String>,
    pile: PileRefAst,
    filter: Option<ObjectFilterAst>,
) -> InstructionAst {
    InstructionAst::PileAction {
        action: action.into(),
        pile,
        filter: filter.map(Box::new),
    }
}

#[inline]
pub(crate) fn ins_manifest(
    count: ValueExprAst,
    dread: bool,
    subject: Option<SubjectRefAst>,
) -> InstructionAst {
    InstructionAst::Manifest {
        count: Box::new(count),
        dread,
        subject: subject.map(Box::new),
    }
}

#[inline]
pub(crate) fn ins_delayed_ev(trigger: TriggerSpecAst, body: Vec<InstructionAst>) -> InstructionAst {
    InstructionAst::DelayedEventTrigger { trigger, body }
}

#[inline]
pub(crate) fn cond_and(a: ConditionAst, b: ConditionAst) -> ConditionAst {
    ConditionAst::And(Box::new(a), Box::new(b))
}

#[inline]
pub(crate) fn cond_or(a: ConditionAst, b: ConditionAst) -> ConditionAst {
    ConditionAst::Or(Box::new(a), Box::new(b))
}

#[inline]
pub(crate) fn cond_has_kw(subject: SubjectRefAst, keyword: impl Into<String>) -> ConditionAst {
    ConditionAst::HasKeyword {
        subject: Box::new(subject),
        keyword: keyword.into(),
    }
}

#[inline]
pub(crate) fn cond_zone_ch(
    filter: ObjectFilterAst,
    from_zone: Option<ZoneFilter>,
    to_zone: Option<ZoneFilter>,
    controller: Option<PlayerRefAst>,
    min_count: u16,
) -> ConditionAst {
    ConditionAst::ZoneChangeThisTurn {
        filter: Box::new(filter),
        from_zone,
        to_zone,
        controller,
        min_count,
    }
}

/// Split on the last occurrence of the first matching delimiter from `needles`.
#[inline]
pub(crate) fn rsplit_once_any<'a, S: AsRef<str> + ?Sized>(
    s: &'a S,
    needles: &[&str],
) -> Option<(&'a str, &'a str)> {
    let s = s.as_ref();
    needles.iter().find_map(|needle| s.rsplit_once(needle))
}

/// Strip the first matching prefix from a slice of candidates.
#[inline]
pub(crate) fn strip_prefix_any<'a, S: AsRef<str> + ?Sized>(
    s: &'a S,
    prefixes: &[&str],
) -> Option<&'a str> {
    let s = s.as_ref();
    prefixes.iter().find_map(|p| s.strip_prefix(p))
}

/// Strip the first matching suffix from a slice of candidates.
#[inline]
pub(crate) fn strip_suffix_any<'a, S: AsRef<str> + ?Sized>(
    s: &'a S,
    suffixes: &[&str],
) -> Option<&'a str> {
    let s = s.as_ref();
    suffixes.iter().find_map(|p| s.strip_suffix(p))
}

/// Parse a rounding direction (`up` / `down`) into [`Rounding`].
#[inline]
pub(crate) fn parse_rounding_word(word: &str) -> Option<Rounding> {
    match word.trim() {
        "up" => Some(Rounding::Up),
        "down" => Some(Rounding::Down),
        _ => None,
    }
}

/// Parse a pile visibility modifier (`face-up` / `face-down`) into [`PileVisibilityAst`].
#[inline]
pub(crate) fn parse_pile_visibility(word: &str) -> Option<PileVisibilityAst> {
    match word.trim() {
        "face-down" | "face down" => Some(PileVisibilityAst::FaceDown),
        "face-up" | "face up" => Some(PileVisibilityAst::FaceUp),
        _ => None,
    }
}

/// Parse `a "<l1>" pile and a "<l2>" pile` into `[l1, l2]`.
pub(crate) fn parse_two_quoted_piles(raw: &str) -> Option<Vec<String>> {
    let (p1, p2) = raw.split_once(" pile and ")?;
    let l1 = p1
        .trim()
        .strip_prefix("a \"")
        .or_else(|| p1.trim().strip_prefix("an \""))?
        .strip_suffix('"')?;
    let l2 = p2
        .trim()
        .strip_prefix("a \"")
        .or_else(|| p2.trim().strip_prefix("an \""))?
        .strip_suffix("\" pile")?;
    Some(vec![l1.to_string(), l2.to_string()])
}

/// Normalize sticker kind wording into canonical AST tags.
#[inline]
pub(crate) fn normalize_sticker_kind(raw: &str) -> String {
    match raw.trim() {
        "name" => "name".to_string(),
        "art" => "art".to_string(),
        "ability" => "ability".to_string(),
        "power and toughness" => "power_and_toughness".to_string(),
        other => other.to_string(),
    }
}

/// Parse a player reference including anaphoric `"the player"`.
#[inline]
pub(crate) fn parse_player_ref_or_the_player(raw: &str) -> Option<PlayerRefAst> {
    match raw.trim() {
        "the player" => Some(PlayerRefAst::ThatPlayer),
        other => parse_player_ref(other),
    }
}

/// Split on the first matching delimiter from a slice of candidate delimiters.
#[inline]
pub(crate) fn split_once_any<'a, S: AsRef<str> + ?Sized>(
    s: &'a S,
    needles: &[&str],
) -> Option<(&'a str, &'a str)> {
    let s = s.as_ref();
    needles.iter().find_map(|needle| s.split_once(needle))
}
