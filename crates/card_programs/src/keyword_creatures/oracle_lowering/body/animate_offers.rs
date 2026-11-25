use super::*;

pub(super) struct MilledZoneRefPick {
    pub(super) selector: Option<PlayerRefAst>,
    pub(super) filter: ObjectFilter,
    pub(super) min: ValueExpr,
    pub(super) max: ValueExpr,
    pub(super) destination: SubjectDestination,
    pub(super) reveal: bool,
}

pub(super) fn is_milled_zone_ref_predicate(
    pred: &spellwake_oracle_compiler::ast::FilterPredicateAst,
) -> bool {
    match pred {
        spellwake_oracle_compiler::ast::FilterPredicateAst::PriorActionZoneRef { kind } => {
            let low = kind.trim().to_ascii_lowercase();
            matches!(
                low.as_str(),
                "milled_this_way"
                    | "among_the_cards_milled_this_way"
                    | "among the cards milled this way"
                    | "cards_milled_this_way"
                    | "from_among_the_cards_milled_this_way"
                    | "from among the cards milled this way"
                    | "from_among_those"
                    | "from_among_those_cards"
                    | "among_them"
                    | "among them"
                    | "from_among_them"
                    | "from among them"
                    | "in_referenced_zone"
            )
        }
        spellwake_oracle_compiler::ast::FilterPredicateAst::AffectedByPriorAction(
            spellwake_oracle_compiler::ast::PriorActionRefKind::PutIntoGraveyardThisWay,
        ) => true,
        spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "milled this way"
                | "they milled this way"
                | "that player milled this way"
                | "you milled this way"
        ),
        _ => false,
    }
}

pub(super) fn clean_milled_object_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(bool, ObjectFilter)> {
    let mut cleaned = f.clone();
    let mut had_milled = cleaned.predicates.iter().any(is_milled_zone_ref_predicate);
    cleaned
        .predicates
        .retain(|p| !is_milled_zone_ref_predicate(p));
    for pred in &mut cleaned.predicates {
        if let spellwake_oracle_compiler::ast::FilterPredicateAst::AnyOf(items) = pred {
            for item in items.iter_mut() {
                if item.predicates.iter().any(is_milled_zone_ref_predicate) {
                    had_milled = true;
                    item.predicates.retain(|p| !is_milled_zone_ref_predicate(p));
                }
                if matches!(
                    item.zone,
                    Some(
                        ZoneFilter::YourGraveyard
                            | ZoneFilter::ThatPlayerGraveyard
                            | ZoneFilter::AnyGraveyard
                    )
                ) {
                    item.zone = None;
                }
                if item.zone.is_some() || !item.extra_zones.is_empty() {
                    return None;
                }
            }
        }
    }
    if matches!(
        cleaned.zone,
        Some(
            ZoneFilter::YourGraveyard | ZoneFilter::ThatPlayerGraveyard | ZoneFilter::AnyGraveyard
        )
    ) {
        cleaned.zone = None;
    }
    if cleaned.zone.is_some() || !cleaned.extra_zones.is_empty() {
        return None;
    }
    let obj = if is_unrestricted_filter(&cleaned) {
        ObjectFilter::any()
    } else {
        lower_object_filter(&cleaned, vocabulary).ok()?
    };
    Some((had_milled, obj))
}

pub(super) fn lower_milled_zone_ref_subject(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    match subject {
        SubjectRefAst::Group(f) => {
            let (had_milled, obj) = clean_milled_object_filter(f, vocabulary)?;
            had_milled.then_some(obj)
        }
        SubjectRefAst::Compound(items) => {
            let [SubjectRefAst::Group(f1), SubjectRefAst::Group(f2)] = items.as_slice() else {
                return None;
            };
            let (had1, obj1) = clean_milled_object_filter(f1, vocabulary)?;
            let (had2, obj2) = clean_milled_object_filter(f2, vocabulary)?;
            if !(had1 || had2) {
                return None;
            }
            match (obj1, obj2) {
                (ObjectFilter::Clause(c1), ObjectFilter::Clause(c2)) => {
                    Some(ObjectFilter::Either(c1, c2))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn parse_milled_move_dest(to: &str, tapped_flag: bool) -> Option<SubjectDestination> {
    let low = to.trim().trim_end_matches('.').to_ascii_lowercase();
    if low.contains("attacking")
        || low.contains("face down")
        || low.contains("transformed")
        || low.contains("with a ")
        || low.contains("under an opponent")
    {
        return None;
    }
    if low.contains("hand") {
        Some(SubjectDestination::Hand)
    } else if low.contains("battlefield") {
        Some(SubjectDestination::Battlefield {
            tapped: tapped_flag || low.contains("tapped"),
            attacking: false,
            transformed: false,
            under_owner_control: low.contains("owner's control") || low.contains("owners control"),
            with_counter: None,
            attach_to_self: false,
        })
    } else if low.contains("exile") {
        Some(SubjectDestination::Exile {
            delayed_return: None,
        })
    } else if low.contains("bottom") && low.contains("library") {
        Some(SubjectDestination::Library(LibraryPlacement::Bottom))
    } else if low.contains("top") && low.contains("library") && !low.contains("bottom") {
        Some(SubjectDestination::Library(LibraryPlacement::Top))
    } else {
        None
    }
}

pub(super) fn parse_milled_zone_ref_move(
    op: &InstructionAst,
    vocabulary: &CreatureTypeVocabulary,
    bound_x: Option<&ValueExprAst>,
) -> Option<MilledZoneRefPick> {
    let (selector, is_optional, reveal, mv_op): (
        Option<PlayerRefAst>,
        bool,
        bool,
        &InstructionAst,
    ) = match op {
        InstructionAst::MoveZone { .. } | InstructionAst::ReturnZone { .. } => {
            (None, false, false, op)
        }
        InstructionAst::Optional(inner) => match inner.as_slice() {
            [single] => (None, true, false, single),
            [
                InstructionAst::RevealCards { subject: rev_subj }
                | InstructionAst::RevealByPlayer {
                    player: PlayerRefAst::You,
                    subject: rev_subj,
                },
                mv,
            ] => {
                let (to_str, tapped_flag) = match mv {
                    InstructionAst::MoveZone { subject, to }
                        if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) =>
                    {
                        (to.as_str(), false)
                    }
                    InstructionAst::ReturnZone {
                        subject,
                        from_zone: None,
                        to,
                        tapped,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        random: false,
                        except: None,
                        ..
                    } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                        (to.as_str(), *tapped)
                    }
                    _ => return None,
                };
                let obj = lower_milled_zone_ref_subject(rev_subj, vocabulary)?;
                let destination = parse_milled_move_dest(to_str, tapped_flag)?;
                return Some(MilledZoneRefPick {
                    selector: None,
                    filter: obj,
                    min: ValueExpr::literal(0),
                    max: ValueExpr::literal(1),
                    destination,
                    reveal: true,
                });
            }
            _ => return None,
        },
        InstructionAst::OptionalPlayer { player, body }
            if !matches!(
                player,
                PlayerRefAst::AnyPlayer
                    | PlayerRefAst::EachPlayer
                    | PlayerRefAst::EachOtherPlayer
                    | PlayerRefAst::AnyOpponent
                    | PlayerRefAst::EachOpponent
            ) =>
        {
            match body.as_slice() {
                [single] => (Some(*player), true, false, single),
                _ => return None,
            }
        }
        _ => return None,
    };

    let (subject, to_str, tapped_flag, count_opt, up_to, any_number) = match mv_op {
        InstructionAst::MoveZone { subject, to } => {
            (subject.as_ref(), to.as_str(), false, None, false, false)
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped,
            transformed: false,
            with_counter: None,
            attached_to: None,
            random: false,
            except: None,
            count,
            up_to,
            any_number,
            ..
        } if from_zone
            .as_deref()
            .is_none_or(|z| z.to_ascii_lowercase().contains("graveyard")) =>
        {
            (
                subject.as_ref(),
                to.as_str(),
                *tapped,
                count.as_deref(),
                *up_to,
                *any_number,
            )
        }
        _ => return None,
    };

    let obj = lower_milled_zone_ref_subject(subject, vocabulary)?;
    let destination = parse_milled_move_dest(to_str, tapped_flag)?;
    let (min, max) = if any_number {
        (
            ValueExpr::literal(0),
            ValueExpr::literal(i64::from(u16::MAX)),
        )
    } else if let Some(cnt_ast) = count_opt {
        let c = lower_value_expr(resolve_x(cnt_ast, bound_x), vocabulary).ok()?;
        let min_v = if is_optional || up_to {
            ValueExpr::literal(0)
        } else {
            c.clone()
        };
        (min_v, c)
    } else {
        (
            ValueExpr::literal(if is_optional || up_to { 0 } else { 1 }),
            ValueExpr::literal(1),
        )
    };
    Some(MilledZoneRefPick {
        selector,
        filter: obj,
        min,
        max,
        destination,
        reveal,
    })
}

pub(super) fn is_offer_any_player_paid_cond(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::PlayerActionThisTurn { action, .. }
        | ConditionAst::EventOccurredThisTurn { event: action } => matches!(
            action
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase()
                .as_str(),
            "a player does"
                | "any player does"
                | "that player does"
                | "the player does"
                | "they do"
                | "an opponent does"
                | "any opponent does"
                | "a player did"
                | "that player did"
                | "a player pays"
                | "any player pays"
                | "an opponent pays"
                | "a player paid"
        ),
        ConditionAst::PriorActionOccurred {
            action,
            subject_filter: None,
            min_count: 1,
        } => matches!(
            action.trim().to_ascii_lowercase().as_str(),
            "a_player_does"
                | "any_player_does"
                | "that_player_does"
                | "did"
                | "paid"
                | "paid_this_way"
        ),
        _ => false,
    }
}

pub(super) fn is_offer_any_player_declined_cond(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Not(inner) => is_offer_any_player_paid_cond(inner),
        ConditionAst::PlayerActionThisTurn { action, .. }
        | ConditionAst::EventOccurredThisTurn { event: action } => matches!(
            action
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase()
                .as_str(),
            "no player does"
                | "no one does"
                | "no opponent does"
                | "neither player does"
                | "they don't"
                | "that player doesn't"
                | "the player doesn't"
                | "no player pays"
                | "no opponent pays"
        ),
        _ => false,
    }
}

pub(super) fn rewrite_offer_chosen_to_loop_rp(rp: &mut ResolvingPlayer) {
    if *rp == ResolvingPlayer::ChosenPlayer {
        *rp = ResolvingPlayer::LoopPlayer;
    }
}

pub(super) fn rewrite_offer_chosen_to_loop_ap(ap: &mut AffectedPlayers) {
    if let AffectedPlayers::One(rp) = ap {
        rewrite_offer_chosen_to_loop_rp(rp);
    }
}

pub(super) fn rewrite_offer_chosen_to_loop_pl(pl: &mut LibraryPlacement) {
    if let LibraryPlacement::ChoiceTopOrBottom { chooser } = pl {
        rewrite_offer_chosen_to_loop_rp(chooser);
    }
}

pub(super) fn rewrite_offer_chosen_to_loop_dest(dest: &mut SubjectDestination) {
    if let SubjectDestination::Library(pl) = dest {
        rewrite_offer_chosen_to_loop_pl(pl);
    }
}

pub(super) fn rewrite_offer_chosen_to_loop_dg(dg: &mut DamageGroup) {
    match dg {
        DamageGroup::AffectedPlayers(ap) | DamageGroup::FilteredAndPlayers { players: ap, .. } => {
            rewrite_offer_chosen_to_loop_ap(ap);
        }
        _ => {}
    }
}

pub(super) fn rewrite_offer_chosen_to_loop_ve(ve: &mut ValueExpr) {
    match ve {
        ValueExpr::Count {
            zone,
            controller_scope,
            ..
        } => {
            match zone {
                ValueZone::Graveyard(rp) | ValueZone::Hand(rp) | ValueZone::Library(rp) => {
                    rewrite_offer_chosen_to_loop_rp(rp);
                }
                ValueZone::Exile(Some(rp)) => rewrite_offer_chosen_to_loop_rp(rp),
                _ => {}
            }
            if let Some(rp) = controller_scope {
                rewrite_offer_chosen_to_loop_rp(rp);
            }
        }
        ValueExpr::OpponentMaxCount { zone, .. } | ValueExpr::OpponentMinCount { zone, .. } => {
            match zone {
                ValueZone::Graveyard(rp) | ValueZone::Hand(rp) | ValueZone::Library(rp) => {
                    rewrite_offer_chosen_to_loop_rp(rp);
                }
                ValueZone::Exile(Some(rp)) => rewrite_offer_chosen_to_loop_rp(rp),
                _ => {}
            }
        }
        ValueExpr::DistinctManaValuesInGraveyard(rp) => rewrite_offer_chosen_to_loop_rp(rp),
        ValueExpr::CounterCount {
            subject: spellwake_engine::program::CounterSubjectRef::PlayerCounter(rp, _),
            ..
        } => rewrite_offer_chosen_to_loop_rp(rp),
        ValueExpr::ObjectMatches {
            subject: MatchObjectRef::LibraryTop(rp),
            ..
        } => rewrite_offer_chosen_to_loop_rp(rp),
        ValueExpr::PlayerStat(ps) => match ps {
            PlayerStatExpr::PlayerLife(rp)
            | PlayerStatExpr::PlayerHandSize(rp)
            | PlayerStatExpr::PlayerLibrarySize(rp)
            | PlayerStatExpr::PlayerGraveyardSize(rp)
            | PlayerStatExpr::LandsEnteredThisTurn(rp)
            | PlayerStatExpr::ActivatedGraveyardAbilityThisTurn(rp)
            | PlayerStatExpr::PlayerLifeLostLastTurn(rp)
            | PlayerStatExpr::ControlsGreatestStat { player: rp, .. }
            | PlayerStatExpr::PlayerHasMostLife { player: rp, .. }
            | PlayerStatExpr::PlayerControlsMost { player: rp, .. } => {
                rewrite_offer_chosen_to_loop_rp(rp);
            }
            PlayerStatExpr::IsInStep {
                player: Some(rp), ..
            }
            | PlayerStatExpr::IsInPhase {
                player: Some(rp), ..
            } => rewrite_offer_chosen_to_loop_rp(rp),
            PlayerStatExpr::SpellsCastMatchingThisTurn { players, .. }
            | PlayerStatExpr::SacrificedMatchingThisTurn { players, .. }
            | PlayerStatExpr::AttackedMatchingThisTurn { players, .. } => {
                rewrite_offer_chosen_to_loop_ap(players);
            }
            PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                controller: Some(rp),
                ..
            } => rewrite_offer_chosen_to_loop_rp(rp),
            _ => {}
        },
        ValueExpr::Scale { expr, .. } | ValueExpr::DivConst { expr, .. } => {
            rewrite_offer_chosen_to_loop_ve(expr);
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            rewrite_offer_chosen_to_loop_ve(a);
            rewrite_offer_chosen_to_loop_ve(b);
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            rewrite_offer_chosen_to_loop_ve(left);
            rewrite_offer_chosen_to_loop_ve(right);
            rewrite_offer_chosen_to_loop_ve(then_value);
            rewrite_offer_chosen_to_loop_ve(else_value);
        }
        _ => {}
    }
}

pub(super) fn rewrite_offer_chosen_to_loop_rule(rule: &mut TemporaryRuleKind) {
    match rule {
        TemporaryRuleKind::PlayerCantGainLife { players }
        | TemporaryRuleKind::CantCastSpells { players, .. }
        | TemporaryRuleKind::CantActivateAbilities { players, .. }
        | TemporaryRuleKind::CantPlayLands { players }
        | TemporaryRuleKind::PlayerGrantKeyword { players, .. }
        | TemporaryRuleKind::SpellCostIncrease { players, .. } => {
            rewrite_offer_chosen_to_loop_ap(players);
        }
        TemporaryRuleKind::AdditionalLandPlay { player, .. }
        | TemporaryRuleKind::FilteredFlash { player, .. }
        | TemporaryRuleKind::ZoneCastPermission { player, .. } => {
            rewrite_offer_chosen_to_loop_rp(player);
        }
        TemporaryRuleKind::SpellCostReductionExpr { amount, .. } => {
            rewrite_offer_chosen_to_loop_ve(amount);
        }
        _ => {}
    }
}

pub(super) fn rewrite_offer_chosen_to_loop_ops(ops: &mut [Instruction]) {
    for op in ops {
        match op {
            Instruction::DrawPlayer { players, amount }
            | Instruction::AdjustPlayersLife {
                players, amount, ..
            }
            | Instruction::Discard { players, amount }
            | Instruction::DiscardFiltered {
                players, amount, ..
            }
            | Instruction::DiscardUpToExt {
                players,
                max_count: amount,
                ..
            }
            | Instruction::Sacrifice {
                players, amount, ..
            }
            | Instruction::ExileChosenPermanents {
                players, amount, ..
            }
            | Instruction::ReturnControlledPermanents {
                players, amount, ..
            }
            | Instruction::ExileFromZone {
                players, amount, ..
            }
            | Instruction::TapOrUntapControlled {
                players, amount, ..
            }
            | Instruction::AddPlayerCounter {
                players, amount, ..
            } => {
                rewrite_offer_chosen_to_loop_ap(players);
                rewrite_offer_chosen_to_loop_ve(amount);
            }
            Instruction::ExileTopPlay {
                count,
                library_owner,
                ..
            } => {
                rewrite_offer_chosen_to_loop_ve(count);
                rewrite_offer_chosen_to_loop_ap(library_owner);
            }
            Instruction::ExilePlayerGraveyards(players)
            | Instruction::LoseGame(players)
            | Instruction::ExileGroupFromZone { players, .. } => {
                rewrite_offer_chosen_to_loop_ap(players);
            }
            Instruction::MoveGraveyardGroupOwned {
                players,
                destination,
                ..
            } => {
                rewrite_offer_chosen_to_loop_ap(players);
                rewrite_offer_chosen_to_loop_dest(destination);
            }
            Instruction::ChooseOptionForPlayers { players, options } => {
                rewrite_offer_chosen_to_loop_ap(players);
                for br in options {
                    rewrite_offer_chosen_to_loop_ops(&mut br.body.instructions);
                }
            }
            Instruction::Scry { player, amount }
            | Instruction::Surveil { player, amount }
            | Instruction::Mill { player, amount }
            | Instruction::PayLife { player, amount }
            | Instruction::PayEnergy { player, amount }
            | Instruction::ManifestTop { player, amount }
            | Instruction::Blight { player, amount }
            | Instruction::ExileLibraryTopLinked { player, amount, .. } => {
                rewrite_offer_chosen_to_loop_rp(player);
                rewrite_offer_chosen_to_loop_ve(amount);
            }
            Instruction::CollectEvidence { player, minimum } => {
                rewrite_offer_chosen_to_loop_rp(player);
                rewrite_offer_chosen_to_loop_ve(minimum);
            }
            Instruction::UntapOrTapSelect { chooser, count, .. }
            | Instruction::DestroyChosen { chooser, count, .. } => {
                rewrite_offer_chosen_to_loop_rp(chooser);
                rewrite_offer_chosen_to_loop_ve(count);
            }
            Instruction::SelectZoneCardsToZone {
                player,
                min,
                max,
                destination,
                ..
            } => {
                rewrite_offer_chosen_to_loop_rp(player);
                rewrite_offer_chosen_to_loop_ve(min);
                rewrite_offer_chosen_to_loop_ve(max);
                rewrite_offer_chosen_to_loop_dest(destination);
            }
            Instruction::LookTopChoose {
                count,
                library_owner,
                ..
            } => {
                rewrite_offer_chosen_to_loop_ve(count);
                rewrite_offer_chosen_to_loop_rp(library_owner);
            }
            Instruction::LookTopChooseExt {
                library_owner,
                chooser,
                count,
                ..
            }
            | Instruction::RevealUntilMatchExt {
                library_owner,
                chooser,
                count,
                ..
            } => {
                rewrite_offer_chosen_to_loop_rp(library_owner);
                rewrite_offer_chosen_to_loop_rp(chooser);
                rewrite_offer_chosen_to_loop_ve(count);
            }
            Instruction::OfferManaPayment { payer, .. } => {
                rewrite_offer_chosen_to_loop_rp(payer);
            }
            Instruction::OfferDynamicManaPayment {
                payer,
                extra_generic,
                ..
            } => {
                rewrite_offer_chosen_to_loop_rp(payer);
                rewrite_offer_chosen_to_loop_ve(extra_generic);
            }
            Instruction::OptionalEffect { chooser }
            | Instruction::OptionalSequence { chooser, .. }
            | Instruction::Clash { chooser }
            | Instruction::ChooseValue { chooser, .. }
            | Instruction::AttachChoosingPermanent { chooser, .. } => {
                rewrite_offer_chosen_to_loop_rp(chooser);
            }
            Instruction::ChooseOption { chooser, options } => {
                rewrite_offer_chosen_to_loop_rp(chooser);
                for br in options {
                    rewrite_offer_chosen_to_loop_ops(&mut br.body.instructions);
                }
            }
            Instruction::AddMana(yield_kind) => {
                if let Some(amount) = mana_yield_amount_mut(yield_kind) {
                    rewrite_offer_chosen_to_loop_ve(amount);
                }
            }
            Instruction::AddManaForPlayer { player, yield_kind } => {
                rewrite_offer_chosen_to_loop_rp(player);
                if let Some(amount) = mana_yield_amount_mut(yield_kind) {
                    rewrite_offer_chosen_to_loop_ve(amount);
                }
            }
            Instruction::ShufflePlayerLibrary(rp)
            | Instruction::RevealPlayerHand(rp)
            | Instruction::DiscardOneAtRandom(rp)
            | Instruction::WinGame(rp)
            | Instruction::SkipNextTurns { player: rp, .. }
            | Instruction::AttachPermanentToPlayer { player: rp, .. }
            | Instruction::ExileLibraryTop { player: rp, .. }
            | Instruction::ExileCardsFromGraveyard { player: rp, .. }
            | Instruction::PutHandPermanentFiltered { player: rp, .. }
            | Instruction::PutHandPermanentsOntoBattlefield { player: rp, .. }
            | Instruction::GrantCardPlayPermission { player: rp, .. }
            | Instruction::VentureIntoDungeon { player: rp, .. }
            | Instruction::TakeInitiative { player: rp }
            | Instruction::GainControlOfPermanent { controller: rp, .. } => {
                rewrite_offer_chosen_to_loop_rp(rp);
            }
            Instruction::CreateTokens { creations }
            | Instruction::CreateAttackingTokens { creations, .. } => {
                for c in creations {
                    rewrite_offer_chosen_to_loop_ve(&mut c.amount);
                    if let TokenRecipients::Player(rp) = &mut c.recipients {
                        rewrite_offer_chosen_to_loop_rp(rp);
                    }
                }
            }
            Instruction::CreateTokenCopy {
                count, recipients, ..
            }
            | Instruction::CreateTokenCopyExt {
                count, recipients, ..
            } => {
                rewrite_offer_chosen_to_loop_ve(count);
                if let TokenRecipients::Player(rp) = recipients {
                    rewrite_offer_chosen_to_loop_rp(rp);
                }
            }
            Instruction::DamageGroup { group, .. } => {
                rewrite_offer_chosen_to_loop_dg(group);
            }
            Instruction::DamageGroupAmount { group, amount } => {
                rewrite_offer_chosen_to_loop_dg(group);
                rewrite_offer_chosen_to_loop_ve(amount);
            }
            Instruction::DealDamageFrom {
                recipients, amount, ..
            } => {
                if let spellwake_engine::program::DamageRecipientsRef::Group(g) =
                    recipients.as_mut()
                {
                    rewrite_offer_chosen_to_loop_dg(g);
                }
                if let spellwake_engine::program::DamageAmountSpec::Expr(e) = amount.as_mut() {
                    rewrite_offer_chosen_to_loop_ve(e);
                }
            }
            Instruction::MoveSubject { destination, .. } => {
                rewrite_offer_chosen_to_loop_dest(destination);
            }
            Instruction::PutTargetInLibrary(pl)
            | Instruction::PutResolvingSpellInLibrary(pl)
            | Instruction::MoveGroupToLibrary { placement: pl, .. } => {
                rewrite_offer_chosen_to_loop_pl(pl);
            }
            Instruction::CreateTemporaryRule { rule, .. } => {
                rewrite_offer_chosen_to_loop_rule(rule);
            }
            Instruction::BranchIfFalse { condition, .. } => {
                rewrite_offer_chosen_to_loop_ve(&mut condition.left);
                rewrite_offer_chosen_to_loop_ve(&mut condition.right);
            }
            Instruction::RepeatLoop { count, body } => {
                rewrite_offer_chosen_to_loop_ve(count);
                rewrite_offer_chosen_to_loop_ops(&mut body.instructions);
            }
            Instruction::ForEachPlayer { players, body } => {
                rewrite_offer_chosen_to_loop_ap(players);
                rewrite_offer_chosen_to_loop_ops(&mut body.instructions);
            }
            Instruction::OfferAnyPlayer {
                players,
                cost_body,
                paid_body,
                declined_body,
            } => {
                rewrite_offer_chosen_to_loop_ap(players);
                rewrite_offer_chosen_to_loop_ops(&mut cost_body.instructions);
                rewrite_offer_chosen_to_loop_ops(&mut paid_body.instructions);
                if let Some(dec) = declined_body {
                    rewrite_offer_chosen_to_loop_ops(&mut dec.instructions);
                }
            }
            Instruction::Exchange(spec) => match spec {
                spellwake_engine::program::ExchangeSpec::LifeTotals { first, second } => {
                    rewrite_offer_chosen_to_loop_rp(first);
                    rewrite_offer_chosen_to_loop_rp(second);
                }
                spellwake_engine::program::ExchangeSpec::LifeWithStat { player, .. }
                | spellwake_engine::program::ExchangeSpec::Zones { player, .. } => {
                    rewrite_offer_chosen_to_loop_rp(player);
                }
                spellwake_engine::program::ExchangeSpec::Control { .. } => {}
            },
            Instruction::RollDie { player, count, .. } => {
                rewrite_offer_chosen_to_loop_rp(player);
                rewrite_offer_chosen_to_loop_ve(count);
            }
            Instruction::FlipCoins {
                player,
                win_body,
                lose_body,
                ..
            } => {
                rewrite_offer_chosen_to_loop_rp(player);
                if let Some(wb) = win_body {
                    rewrite_offer_chosen_to_loop_ops(&mut wb.instructions);
                }
                if let Some(lb) = lose_body {
                    rewrite_offer_chosen_to_loop_ops(&mut lb.instructions);
                }
            }
            Instruction::DrawController { amount }
            | Instruction::DrawTargetPlayer { amount }
            | Instruction::DrawEachPlayer { amount }
            | Instruction::DrawControllerAndTarget { amount }
            | Instruction::GainControllerLife { amount }
            | Instruction::DamageAmount(amount)
            | Instruction::DamageOpponentsAmount(amount)
            | Instruction::DamageRelatedPlayerAmount(amount)
            | Instruction::DrainOpponents(amount)
            | Instruction::DrainTargetAndGain(amount)
            | Instruction::OpponentsLoseAndControllerGains(amount)
            | Instruction::PutCountersOnSubject { amount, .. }
            | Instruction::PutNamedCounterOnTarget { amount, .. }
            | Instruction::RemoveCountersFromPermanent { amount, .. }
            | Instruction::DealDamageToAnnouncedTargets { amount, .. }
            | Instruction::Earthbend { amount }
            | Instruction::BindResolutionX(amount) => {
                rewrite_offer_chosen_to_loop_ve(amount);
            }
            Instruction::ReturnEarthbentCard
            | Instruction::RegenerateSubject { .. }
            | Instruction::SelectExtremeLifePlayer { .. }
            | Instruction::AttachFixedTargets => {}
            Instruction::RemoveCountersFromSubject {
                amount: Some(amt), ..
            } => {
                rewrite_offer_chosen_to_loop_ve(amt);
            }
            Instruction::SearchLibrary { filter } => {
                rewrite_offer_chosen_to_loop_rp(&mut filter.searcher);
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_offer_loop_player_sub_body(
    body: &[InstructionAst],
    outer_builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<ExecutableBody, &'static str> {
    if body.is_empty() {
        return Err("oracle:empty-loop-body");
    }
    let mut sub = BodyBuilder::with_trigger_hint(outer_builder.trigger_hint);
    sub.has_source = outer_builder.has_source;
    sub.last_group_filter = outer_builder.last_group_filter;
    sub.instructions.push(Instruction::ChooseValue {
        chooser: ResolvingPlayer::Controller,
        domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
            opponents_only: false,
        },
    });
    lower_instruction_seq(
        body,
        &mut sub,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        bound_x,
    )?;
    if sub.instructions.len() <= 1 {
        return Err("oracle:empty-loop-body");
    }
    sub.instructions.remove(0);
    for op in &mut sub.instructions {
        op.shift_jumps(-1, 1);
    }
    if sub.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::ChooseValue {
                domain: spellwake_engine::program::ResolutionChoiceDomain::Player { .. },
                ..
            } | Instruction::SelectExtremeLifePlayer { .. }
        )
    }) {
        return Err("oracle:loop-body-chooses-player");
    }
    rewrite_offer_chosen_to_loop_ops(&mut sub.instructions);
    if sub.target != BodyTarget::None {
        if outer_builder.target == BodyTarget::None {
            outer_builder.bind_target(sub.target)?;
        } else if outer_builder.target != sub.target {
            return Err("oracle:multiple-targets");
        }
        sub.target = BodyTarget::None;
    }
    if outer_builder.graveyard_target_bounds.is_none() {
        outer_builder.graveyard_target_bounds = sub.graveyard_target_bounds;
    }
    outer_builder.x_target_count |= sub.x_target_count;
    Ok(ExecutableBody::new(BodyTarget::None, sub.instructions))
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn lower_animate_or_set_pt(
    subject: &SubjectRefAst,
    power: &str,
    toughness: &str,
    description: Option<&str>,
    keywords: &[String],
    duration: &DurationAst,
    keep_types: bool,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    if let Some(eb_raw) = description.and_then(|d| d.strip_prefix("earthbend:")) {
        let trimmed = eb_raw.trim();
        let (const_n, n_expr) = if trimmed == "ChosenX" {
            let expr = if let Some(bx) = bound_x {
                lower_value_expr(bx, vocabulary)?
            } else {
                ValueExpr::chosen_x()
            };
            (None, expr)
        } else if let Some(inner) = trimmed
            .strip_prefix("Const(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let n = inner
                .trim()
                .parse::<i64>()
                .map_err(|_| "oracle:unsupported-earthbend-count")?;
            (Some(n), ValueExpr::literal(n))
        } else if let Ok(n) = trimmed.parse::<i64>() {
            (Some(n), ValueExpr::literal(n))
        } else {
            return Err("oracle:unsupported-earthbend-count");
        };
        let watch_land = ObjectFilter::land().controlled();
        builder.bind_target(BodyTarget::FilteredPermanent(watch_land))?;
        builder
            .instructions
            .push(Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(vec![
                    ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                        subtypes: SubtypeOperation::Keep,
                    }),
                    ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                        KeywordAbility::Haste,
                    ])),
                    ResolvedPartSpec::SetPT(PowerToughness {
                        power: 0.into(),
                        toughness: 0.into(),
                    }),
                ]),
                duration: ResolvedDuration::NoStatedDuration,
            });
        if let Some(n) = const_n
            && n > 0
        {
            builder
                .instructions
                .push(Instruction::PutPowerToughnessCounters {
                    counters: PowerToughnessCounters {
                        plus_one: n.into(),
                        minus_one: 0.into(),
                    },
                });
        } else {
            builder
                .instructions
                .push(Instruction::PutNamedCounterOnTarget {
                    kind: CounterKind::PlusOne,
                    amount: n_expr,
                });
        }
        let return_body = ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MoveSubject {
                subject: MoveSubjectRef::TriggeringObject,
                destination: SubjectDestination::Battlefield {
                    tapped: true,
                    attacking: false,
                    transformed: false,
                    under_owner_control: false,
                    with_counter: None,
                    attach_to_self: false,
                },
            }],
        );
        builder
            .instructions
            .push(Instruction::ScheduleDelayedEventTrigger {
                trigger: Box::new(TriggerSpec::new(TriggerEvent::AnyOf(vec![
                    TriggerEvent::Dies {
                        subject: watch_land,
                        mode: DiesTriggerMode::Permanent,
                    },
                    TriggerEvent::Exiled {
                        subject: watch_land,
                        from_zone: TriggerFromZone::Battlefield,
                    },
                ]))),
                body: Box::new(return_body),
                expires: ResolvedDuration::NoStatedDuration,
                once: true,
            });
        return Ok(());
    }
    if let SubjectRefAst::Compound(items) = subject
        && !items.is_empty()
    {
        for it in items {
            lower_animate_or_set_pt(
                it,
                power,
                toughness,
                description,
                keywords,
                duration,
                keep_types,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
        }
        return Ok(());
    }
    let parse_pt_expr = |raw: &str| -> Result<ValueExpr, &'static str> {
        let s = raw.trim();
        if let Ok(n) = s.parse::<i64>() {
            return Ok(ValueExpr::literal(n));
        }
        if s.eq_ignore_ascii_case("x") {
            return if let Some(bx) = bound_x {
                lower_value_expr(bx, vocabulary)
            } else {
                Ok(ValueExpr::chosen_x())
            };
        }
        if let Ok(ast_e) = spellwake_oracle_compiler::filter::parse_value_expr(
            s,
            &cached_compiler_vocab(vocabulary),
        ) {
            return lower_value_expr(resolve_x(&ast_e, bound_x), vocabulary);
        }
        Err("oracle:animate-non-const-pt")
    };
    let mut inline_pt_expr: Option<ValueExpr> = None;
    let mut inline_kws: Vec<String> = Vec::new();
    let mut effective_desc = description;
    if let Some(desc) = description {
        let desc_low = desc.to_ascii_lowercase();
        for pat in [
            " with base power and toughness each equal to ",
            " with base power and toughness equal to ",
            " with power and toughness each equal to ",
            " with power and toughness equal to ",
        ] {
            if let Some(idx) = desc_low.find(pat) {
                let ty_part = desc[..idx].trim();
                let tail = desc[idx + pat.len()..].trim().trim_end_matches('.');
                let tail_low = tail.to_ascii_lowercase();
                let (val_raw, kw_tail) = [" and gains ", " and gain ", " and has ", " and have "]
                    .iter()
                    .find_map(|sep| {
                        tail_low.find(sep).map(|k_idx| {
                            (tail[..k_idx].trim(), Some(tail[k_idx + sep.len()..].trim()))
                        })
                    })
                    .unwrap_or((tail, None));
                let val_expr = parse_pt_expr(val_raw)?;
                if let Some(kw_raw) = kw_tail {
                    for piece in kw_raw.split(',').flat_map(|seg| seg.split(" and ")) {
                        let k = piece.trim();
                        if !k.is_empty() {
                            inline_kws.push(k.to_string());
                        }
                    }
                }
                inline_pt_expr = Some(val_expr);
                effective_desc = (!ty_part.is_empty()).then_some(ty_part);
                break;
            }
        }
    }
    let p_trim = power.trim();
    let t_trim = toughness.trim();
    let omit_pt = (p_trim.is_empty() || p_trim.eq_ignore_ascii_case("keep"))
        && (t_trim.is_empty() || t_trim.eq_ignore_ascii_case("keep"));
    let pt_part: Option<ResolvedPartSpec> = if let Some(val) = inline_pt_expr {
        Some(ResolvedPartSpec::SetPTExpr {
            power: Box::new(val.clone()),
            toughness: Box::new(val),
        })
    } else if omit_pt {
        None
    } else {
        Some(match (p_trim.parse::<i64>(), t_trim.parse::<i64>()) {
            (Ok(p), Ok(t)) => ResolvedPartSpec::SetPT(PowerToughness {
                power: p.into(),
                toughness: t.into(),
            }),
            _ => ResolvedPartSpec::SetPTExpr {
                power: Box::new(parse_pt_expr(power)?),
                toughness: Box::new(parse_pt_expr(toughness)?),
            },
        })
    };
    let dur = lower_duration(duration)?;
    let mut parts = Vec::new();
    let strip_phrase_ci = |buf: &mut String, phrase: &str| -> bool {
        let mut found = false;
        while let Some(idx) = buf.to_ascii_lowercase().find(phrase) {
            buf.replace_range(idx..idx + phrase.len(), " ");
            found = true;
        }
        found
    };
    if let Some(desc) = effective_desc {
        let mut colors = BTreeSet::new();
        let mut explicit_colorless = false;
        let mut add_colors = false;
        let mut loses_all_abilities = false;
        let mut supertypes = BTreeSet::new();
        let mut card_types = BTreeSet::new();
        let mut land_subtypes = BTreeSet::new();
        let mut artifact_subtypes = BTreeSet::new();
        let mut enchantment_subtypes = BTreeSet::new();
        let mut subtypes = BTreeSet::new();
        let mut extra_kws = std::mem::take(&mut inline_kws);
        let lower_desc = desc.to_ascii_lowercase();
        let mut add_types = keep_types
            || lower_desc.contains("in addition to its other types")
            || lower_desc.contains("in addition to their other types")
            || lower_desc.contains("in addition to its other creature types")
            || lower_desc.contains("in addition to their other creature types")
            || lower_desc.contains("in addition to its other land types")
            || lower_desc.contains("in addition to their other land types")
            || lower_desc.contains("in addition to its other colors and types")
            || lower_desc.contains("in addition to their other colors and types")
            || lower_desc.contains("still a ")
            || lower_desc.contains("still an ")
            || lower_desc.contains("still lands")
            || lower_desc.contains("still artifacts")
            || lower_desc.contains("still enchantments")
            || lower_desc.contains("still creatures");
        if lower_desc.contains("in addition to its other colors")
            || lower_desc.contains("in addition to their other colors")
        {
            add_colors = true;
        }
        let mut clean = desc.to_string();
        for phrase in [
            "in addition to its other colors and types",
            "in addition to their other colors and types",
            "in addition to its other creature types",
            "in addition to their other creature types",
            "in addition to its other land types",
            "in addition to their other land types",
            "in addition to its other types",
            "in addition to their other types",
            "in addition to its other colors",
            "in addition to their other colors",
        ] {
            strip_phrase_ci(&mut clean, phrase);
        }
        let mut all_creature_types = false;
        for phrase in [
            "gains all creature types",
            "gain all creature types",
            "has all creature types",
            "have all creature types",
            "gains every creature type",
            "gain every creature type",
            "has every creature type",
            "have every creature type",
        ] {
            if strip_phrase_ci(&mut clean, phrase) {
                all_creature_types = true;
                add_types = true;
            }
        }
        for phrase in [
            "becomes all creature types",
            "become all creature types",
            "becomes every creature type",
            "become every creature type",
            "is all creature types",
            "are all creature types",
            "is every creature type",
            "are every creature type",
            "all creature types",
            "every creature type",
        ] {
            if strip_phrase_ci(&mut clean, phrase) {
                all_creature_types = true;
            }
        }
        if all_creature_types {
            for ord in 0..vocabulary.len() {
                if let Ok(ordinal) = u16::try_from(ord)
                    && let Some(ct) = vocabulary.resolve_member(vocabulary.id(), ordinal)
                {
                    subtypes.insert(ct);
                }
            }
        }
        let mut all_basic_land_types = false;
        for phrase in [
            "gains all basic land types",
            "gain all basic land types",
            "has all basic land types",
            "have all basic land types",
            "gains every basic land type",
            "gain every basic land type",
            "has every basic land type",
            "have every basic land type",
        ] {
            if strip_phrase_ci(&mut clean, phrase) {
                all_basic_land_types = true;
                add_types = true;
            }
        }
        for phrase in [
            "becomes all basic land types",
            "become all basic land types",
            "becomes every basic land type",
            "become every basic land type",
            "is all basic land types",
            "are all basic land types",
            "is every basic land type",
            "are every basic land type",
            "all basic land types",
            "every basic land type",
        ] {
            if strip_phrase_ci(&mut clean, phrase) {
                all_basic_land_types = true;
            }
        }
        if all_basic_land_types {
            land_subtypes.extend([
                LandType::Plains,
                LandType::Island,
                LandType::Swamp,
                LandType::Mountain,
                LandType::Forest,
            ]);
        }
        if strip_phrase_ci(&mut clean, "all colors") {
            colors.extend([
                Color::White,
                Color::Blue,
                Color::Black,
                Color::Red,
                Color::Green,
            ]);
        }
        if strip_phrase_ci(&mut clean, "loses all abilities")
            || strip_phrase_ci(&mut clean, "lose all abilities")
        {
            loses_all_abilities = true;
        }
        for phrase in [
            "until end of turn",
            "until your next turn",
            "it's still a land.",
            "it's still a land",
            "they're still lands.",
            "they're still lands",
            "it's still an artifact.",
            "it's still an artifact",
            "they're still artifacts.",
            "they're still artifacts",
            "it's still an enchantment.",
            "it's still an enchantment",
            "they're still enchantments.",
            "they're still enchantments",
            "it's still a creature.",
            "it's still a creature",
            "they're still creatures.",
            "they're still creatures",
        ] {
            strip_phrase_ci(&mut clean, phrase);
        }
        clean = clean.replace(" and ", " ");
        for (phrase, kw_label) in [
            ("first strike", "first strike"),
            ("double strike", "double strike"),
        ] {
            if strip_phrase_ci(&mut clean, phrase) {
                extra_kws.push(kw_label.to_string());
            }
        }
        for raw_tok in clean.split_whitespace() {
            let tok = raw_tok
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'' && c != '/');
            if tok.is_empty() {
                continue;
            }
            if tok.contains('/')
                && tok.split('/').all(|seg| {
                    seg.parse::<i64>().is_ok() || seg.eq_ignore_ascii_case("x") || seg == "*"
                })
            {
                continue;
            }
            let tok = tok.trim_matches('/');
            if tok.is_empty() {
                continue;
            }
            match tok.to_ascii_lowercase().as_str() {
                "a" | "an" | "with" | "still" | "that's" | "it's" | "they're" | "also" => {}
                "white" => {
                    colors.insert(Color::White);
                }
                "blue" => {
                    colors.insert(Color::Blue);
                }
                "black" => {
                    colors.insert(Color::Black);
                }
                "red" => {
                    colors.insert(Color::Red);
                }
                "green" => {
                    colors.insert(Color::Green);
                }
                "colorless" => {
                    explicit_colorless = true;
                }
                "basic" => {
                    supertypes.insert(Supertype::Basic);
                }
                "legendary" => {
                    supertypes.insert(Supertype::Legendary);
                }
                "snow" => {
                    supertypes.insert(Supertype::Snow);
                }
                "creature" | "creatures" => {
                    card_types.insert(CardType::Creature);
                }
                "artifact" | "artifacts" => {
                    card_types.insert(CardType::Artifact);
                }
                "enchantment" | "enchantments" => {
                    card_types.insert(CardType::Enchantment);
                }
                "land" | "lands" => {
                    card_types.insert(CardType::Land);
                }
                "planeswalker" | "planeswalkers" => {
                    card_types.insert(CardType::Planeswalker);
                }
                "battle" | "battles" => {
                    card_types.insert(CardType::Battle);
                }
                "plains" => {
                    land_subtypes.insert(LandType::Plains);
                }
                "island" | "islands" => {
                    land_subtypes.insert(LandType::Island);
                }
                "swamp" | "swamps" => {
                    land_subtypes.insert(LandType::Swamp);
                }
                "mountain" | "mountains" => {
                    land_subtypes.insert(LandType::Mountain);
                }
                "forest" | "forests" => {
                    land_subtypes.insert(LandType::Forest);
                }
                "treasure" | "treasures" => {
                    artifact_subtypes.insert(ArtifactSubtype::Treasure);
                }
                "food" | "foods" => {
                    artifact_subtypes.insert(ArtifactSubtype::Food);
                }
                "clue" | "clues" => {
                    artifact_subtypes.insert(ArtifactSubtype::Clue);
                }
                "blood" => {
                    artifact_subtypes.insert(ArtifactSubtype::Blood);
                }
                "gold" => {
                    artifact_subtypes.insert(ArtifactSubtype::Gold);
                }
                "vehicle" | "vehicles" => {
                    artifact_subtypes.insert(ArtifactSubtype::Vehicle);
                }
                "equipment" => {
                    artifact_subtypes.insert(ArtifactSubtype::Equipment);
                }
                "junk" => {
                    artifact_subtypes.insert(ArtifactSubtype::Junk);
                }
                "map" | "maps" => {
                    artifact_subtypes.insert(ArtifactSubtype::Map);
                }
                "powerstone" | "powerstones" => {
                    artifact_subtypes.insert(ArtifactSubtype::Powerstone);
                }
                "cartouche" | "cartouches" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Cartouche);
                }
                "shard" | "shards" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Shard);
                }
                "saga" | "sagas" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Saga);
                }
                "shrine" | "shrines" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Shrine);
                }
                "curse" | "curses" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Curse);
                }
                "role" | "roles" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Role);
                }
                "room" | "rooms" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Room);
                }
                "class" | "classes" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Class);
                }
                "case" | "cases" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Case);
                }
                "rune" | "runes" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Rune);
                }
                "background" | "backgrounds" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Background);
                }
                kw_cand if lower_keyword_ability_name(kw_cand).is_some() => {
                    extra_kws.push(kw_cand.to_string());
                }
                _ => {
                    let resolve_ci = |w: &str| -> Option<CreatureTypeId> {
                        if let Some(ct) = vocabulary.resolve_label(w) {
                            return Some(ct);
                        }
                        let title = w
                            .split('-')
                            .map(|seg| {
                                let mut ch = seg.chars();
                                ch.next()
                                    .map(|c| c.to_uppercase().collect::<String>() + ch.as_str())
                                    .unwrap_or_default()
                            })
                            .collect::<Vec<_>>()
                            .join("-");
                        vocabulary.resolve_label(&title)
                    };
                    if let Some(ct) = resolve_ci(tok) {
                        subtypes.insert(ct);
                    } else if let Some(singular) = tok.strip_suffix("ves")
                        && let Some(ct) = resolve_ci(&format!("{singular}f"))
                    {
                        subtypes.insert(ct);
                    } else if let Some(singular) = tok.strip_suffix("es")
                        && let Some(ct) = resolve_ci(singular)
                    {
                        subtypes.insert(ct);
                    } else if let Some(singular) = tok.strip_suffix('s')
                        && let Some(ct) = resolve_ci(singular)
                    {
                        subtypes.insert(ct);
                    } else {
                        return Err("oracle:unsupported-animate-desc");
                    }
                }
            }
        }
        if !supertypes.is_empty()
            || !card_types.is_empty()
            || !subtypes.is_empty()
            || !land_subtypes.is_empty()
            || !artifact_subtypes.is_empty()
            || !enchantment_subtypes.is_empty()
        {
            let super_op = if supertypes.is_empty() {
                TypeOperation::Keep
            } else if add_types {
                TypeOperation::Add(supertypes)
            } else {
                TypeOperation::Set(supertypes)
            };
            let types_op = if card_types.is_empty() {
                TypeOperation::Keep
            } else if add_types {
                TypeOperation::Add(card_types)
            } else {
                TypeOperation::Set(card_types)
            };
            let sub_op = if subtypes.is_empty() {
                SubtypeOperation::Keep
            } else if add_types {
                SubtypeOperation::Add(InstructionSubtypeOperand::Literal(subtypes))
            } else {
                SubtypeOperation::Set(InstructionSubtypeOperand::Literal(subtypes))
            };
            let land_op = if land_subtypes.is_empty() {
                SubtypeOperation::Keep
            } else if add_types {
                SubtypeOperation::Add(land_subtypes)
            } else {
                SubtypeOperation::Set(land_subtypes)
            };
            let base = TypeChangeSpec {
                land_subtypes: land_op,
                supertypes: super_op,
                types: types_op,
                subtypes: sub_op,
            };
            if artifact_subtypes.is_empty() && enchantment_subtypes.is_empty() {
                parts.push(ResolvedPartSpec::Type(base));
            } else {
                let art_op = if artifact_subtypes.is_empty() {
                    SubtypeOperation::Keep
                } else if add_types {
                    SubtypeOperation::Add(artifact_subtypes)
                } else {
                    SubtypeOperation::Set(artifact_subtypes)
                };
                let ench_op = if enchantment_subtypes.is_empty() {
                    SubtypeOperation::Keep
                } else if add_types {
                    SubtypeOperation::Add(enchantment_subtypes)
                } else {
                    SubtypeOperation::Set(enchantment_subtypes)
                };
                parts.push(ResolvedPartSpec::TypeWithSubtypes {
                    base,
                    artifact_subtypes: art_op,
                    enchantment_subtypes: ench_op,
                });
            }
        }
        if !colors.is_empty() {
            parts.push(ResolvedPartSpec::Color(if add_colors {
                ColorOperation::Add(colors)
            } else {
                ColorOperation::Set(colors)
            }));
        } else if explicit_colorless {
            parts.push(ResolvedPartSpec::Color(
                ColorOperation::Set(BTreeSet::new()),
            ));
        }
        if loses_all_abilities || !extra_kws.is_empty() || !keywords.is_empty() {
            let mut merged: Vec<String> = keywords.to_vec();
            merged.extend(extra_kws);
            let mut ab_change = if !merged.is_empty() {
                lower_ability_change(&merged, vocabulary)?
            } else {
                AbilityChange::default()
            };
            if loses_all_abilities {
                ab_change.removal = Some(AbilityRemoval::All);
            }
            parts.push(ResolvedPartSpec::Abilities(ab_change));
        }
    } else if !keywords.is_empty() || !inline_kws.is_empty() {
        let mut merged: Vec<String> = keywords.to_vec();
        merged.extend(inline_kws);
        parts.push(ResolvedPartSpec::Abilities(lower_ability_change(
            &merged, vocabulary,
        )?));
    }
    if let Some(pt) = pt_part {
        parts.push(pt);
    }
    if parts.is_empty() {
        return Err("oracle:unsupported-animate-desc");
    }
    let spec = ResolvedPartsSpec(parts);
    let has_perm_target = matches!(
        builder.target,
        BodyTarget::SingleCreature
            | BodyTarget::SinglePermanent
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
    );
    let has_created_tokens = builder.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::CreateTokens { .. }
                | Instruction::CreateAttackingTokens { .. }
                | Instruction::CreateTokenCopy { .. }
                | Instruction::CreateTokenAndAttach { .. }
        )
    });
    let has_last_moved = builder.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::MoveSubject {
                destination: SubjectDestination::Battlefield { .. },
                ..
            }
        )
    });
    match subject {
        SubjectRefAst::SelfObject => {
            builder.instructions.push(Instruction::ChangeAbilitySource {
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(a)
            if has_created_tokens
                && (a.to_ascii_lowercase().contains("token") || !has_perm_target) =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::CreatedTokens),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
            builder.instructions.push(Instruction::ChangeAbilitySource {
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::AttachedHost(_) => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                    ObjectClause::any().attached_to_self(),
                ))),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
            let obj = lower_object_filter(f, vocabulary)?;
            bind_filter_player_target(builder, &obj)?;
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(obj)),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let bt = lower_battlefield_body_target(&inner, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: spec,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(n),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let bt = lower_battlefield_body_target(&inner, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: *n,
                    maximum: *n,
                    action: PermanentTargetAction::Modify {
                        parts: spec,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::UpTo(max),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *max > 0 => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let bt = lower_battlefield_body_target(&inner, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: *max,
                    action: PermanentTargetAction::Modify {
                        parts: spec,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::AnyNumber,
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let obj = lower_object_filter(&inner, vocabulary)?;
            builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
            builder
                .instructions
                .push(Instruction::ModifyAnyNumberOfPermanents {
                    parts: spec,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::ChosenX,
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let bt = lower_battlefield_body_target(&inner, vocabulary)?;
            builder.bind_target(bt)?;
            builder.x_target_count = true;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: u16::MAX,
                    action: PermanentTargetAction::Modify {
                        parts: spec,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if has_perm_target => {
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: spec,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TriggerSubject if in_trigger => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if builder.target == BodyTarget::None && in_trigger && !self_pronoun_valid =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if builder.target == BodyTarget::None && builder.last_group_filter.is_some() =>
        {
            let obj = builder
                .last_group_filter
                .ok_or("oracle:unsupported-animate-subject")?;
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(obj)),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if builder.target == BodyTarget::None && has_last_moved => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::LastMoved),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        _ => Err("oracle:unsupported-animate-subject"),
    }
}
