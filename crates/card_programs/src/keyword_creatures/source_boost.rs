//! Fixed-cost permanent abilities, including their exact source payments and timing restrictions.
use super::*;
use spellwake_engine::{
    activated::{ActivationRestriction, SourceCost},
    characteristics::PowerToughness,
    program::{BodyTarget, EffectSubject, ExecutableBody, Instruction, ObjectFilter, PowerFilter},
};
pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    remaining: Option<&str>,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || face.name != card.name {
        return None;
    }
    let types = face.type_line.as_deref()?.split(" — ").next()?;
    let stripped = without_reminder(remaining.or(face.oracle_text.as_deref())?)?;
    let mut abilities = Vec::new();
    let mut other = Vec::new();
    for line in stripped
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(ability) = parse(line, &card.name, types, vocabulary) {
            abilities.push(ability);
        } else {
            other.push(line);
        }
    }
    if abilities.is_empty() {
        return None;
    }
    let mut encore_subtype = None;
    let mut legend_subtype = None;
    for line in &other {
        if let Some(kind) = line.strip_prefix("Each ").and_then(|s| {
            s.strip_suffix(
                " creature card in your graveyard has encore {X}, where X is its mana value.",
            )
        }) {
            encore_subtype = Some(vocabulary.resolve_label(kind)?);
        }
        if let Some(kind) = line
            .strip_prefix("The \"legend rule\" doesn't apply to ")
            .and_then(|s| s.strip_suffix("s you control."))
        {
            legend_subtype = Some(vocabulary.resolve_label(kind)?);
        }
    }
    if encore_subtype.is_some() {
        other.retain(|s| {
            !s.ends_with(
                " creature card in your graveyard has encore {X}, where X is its mana value.",
            )
        });
    }
    if legend_subtype.is_some() {
        other.retain(|s| !s.starts_with("The \"legend rule\" doesn't apply to "));
    }
    let untaps_on_others_turns =
        other.contains(&"Untap this creature during each other player's untap step.");
    if untaps_on_others_turns {
        other.retain(|line| *line != "Untap this creature during each other player's untap step.");
    }
    let mut artifact_subtypes = std::collections::BTreeSet::new();
    if matches!(types, "Artifact" | "Legendary Artifact" | "Snow Artifact")
        && let Some((_, subtypes)) = face.type_line.as_deref()?.split_once(" — ")
    {
        for label in subtypes.split_whitespace() {
            let kind = spellwake_engine::program::ArtifactSubtype::from_wire_label(label)?;
            if kind == spellwake_engine::program::ArtifactSubtype::Equipment {
                return None;
            }
            artifact_subtypes.insert(kind);
        }
    }
    let remainder = other.join("\n");
    let mut reduced = card.clone();
    reduced.faces[0].oracle_text = Some(remainder.clone());
    let mut program = if types.split_whitespace().any(|ty| ty == "Creature") {
        ordinary_creature(card, vocabulary, Profile::Current, Some(&remainder))
            .ok()
            .or_else(|| mana_permanents::compile(&reduced, vocabulary))
            .or_else(|| ordinary_triggers::compile(&reduced, vocabulary))
    } else if types.split_whitespace().any(|ty| ty == "Land") {
        basic_lands::compile(&reduced, Profile::Current)
    } else if (face.type_line.as_deref() == Some(types) || !artifact_subtypes.is_empty())
        && matches!(
            types,
            "Artifact"
                | "Legendary Artifact"
                | "Snow Artifact"
                | "Enchantment"
                | "Legendary Enchantment"
                | "Snow Enchantment"
        )
    {
        mana_permanents::compile(&reduced, vocabulary)
            .or_else(|| ordinary_triggers::compile(&reduced, vocabulary))
            .or_else(|| {
                let cost = Some(fixed_cost(face.mana_cost.as_deref()?)?);
                let artifact = types.split_whitespace().any(|ty| ty == "Artifact");
                let mut base = if artifact {
                    Program::artifact(cost, vec![])
                } else {
                    Program::enchantment(cost)
                };
                for line in other {
                    match line {
                        "This artifact enters tapped." if artifact => {
                            let spellwake_engine::program::ProgramKind::Permanent(def) =
                                &mut base.kind
                            else {
                                return None;
                            };
                            def.entry_tap = Some(spellwake_engine::land::EntryTap::Always);
                        }
                        keyword => {
                            base.printed_keywords
                                .insert(static_keyword(&keyword.to_ascii_lowercase())?);
                        }
                    }
                }
                if types.starts_with("Legendary ") {
                    base.supertypes.insert(Supertype::Legendary);
                }
                if types.starts_with("Snow ") {
                    base.supertypes.insert(Supertype::Snow);
                }
                Some(base)
            })
    } else {
        None
    }?;
    program.artifact_subtypes.extend(artifact_subtypes);
    if let (Some(encore), Some(exempt)) = (encore_subtype, legend_subtype) {
        use spellwake_engine::characteristics::StaticAbilityId;
        if program.continuous.is_some() || !program.additional_battlefield_abilities.is_empty() {
            return None;
        }
        program.continuous = Some(
            spellwake_engine::characteristics::StaticContinuousAbility::Modifier {
                id: StaticAbilityId(0),
                rule: spellwake_engine::characteristics::StaticModifierRule::IgnoreLegendForSubtype(
                    exempt,
                ),
            },
        );
        program.additional_battlefield_abilities.push(
            spellwake_engine::characteristics::StaticContinuousAbility::CastPermission {
                id: StaticAbilityId(1),
                permission:
                    spellwake_engine::characteristics::CastPermissionRule::GraveyardSubtypeEncore(
                        encore,
                    ),
            },
        );
    } else if encore_subtype.is_some() || legend_subtype.is_some() {
        return None;
    }
    if untaps_on_others_turns {
        use spellwake_engine::characteristics::StaticAbilityId;
        if program.continuous.is_some() || !program.additional_battlefield_abilities.is_empty() {
            return None;
        }
        program.continuous = Some(
            spellwake_engine::characteristics::StaticContinuousAbility::UntapRule {
                id: StaticAbilityId(0),
                rule:
                    spellwake_engine::characteristics::StaticUntapRule::UntapDuringOtherPlayersUntap,
            },
        );
    }
    let mut next = program
        .activated
        .iter()
        .map(|a| a.id.0)
        .max()
        .map_or(Some(0), |v| v.checked_add(1))?;
    for (mana_cost, source_cost, body, restriction) in abilities {
        let id = ActivatedAbilityId(next);
        program.activated.push(ActivatedAbility {
            id,
            mana_cost,
            source_cost,
            body,
        });
        if let Some(restriction) = restriction {
            program.activation_restrictions.insert(id, restriction);
        }
        next = next.checked_add(1)?;
    }
    Some(program)
}
pub(super) fn parse(
    line: &str,
    name: &str,
    types: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(
    ManaCost,
    SourceCost,
    ExecutableBody,
    Option<ActivationRestriction>,
)> {
    use spellwake_engine::activated::{
        CounterCostDelta, DiscardCostMode, GraveyardExileCost, LifeCostAmount,
        SelectedSacrificeFilter, TapCostCount, TapCostFilter,
    };
    if let Some(cost) = line.strip_prefix("Encore ") {
        return Some((
            fixed_cost(cost.trim())?,
            SourceCost::Exile {
                tap_source: false,
                from_graveyard: true,
            },
            ExecutableBody::new(BodyTarget::None, vec![Instruction::Encore]),
            Some(ActivationRestriction::Sorcery),
        ));
    }
    let (paid, rest) = line.split_once(": ")?;
    if let Some(price) = paid.strip_suffix(", {T}")
        && !price.is_empty()
        && price.replace("{X}", "").is_empty()
        && matches!(
            rest,
            "Put X charge counters on this land." | "Put X charge counters on this artifact."
        )
    {
        let symbols = u8::try_from(price.matches("{X}").count()).ok()?;
        return Some((
            ManaCost::free(),
            SourceCost::GenericX {
                symbols,
                tap_source: true,
            },
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                        spellwake_engine::effects::CounterKind::Charge,
                        spellwake_engine::program::ValueExpr::chosen_x(),
                    ),
                ],
            ),
            None,
        ));
    }
    let mut parts = paid.split(", ").peekable();
    let price = parts
        .next_if(|value| value.starts_with('{') && *value != "{T}")
        .map(fixed_cost)
        .unwrap_or(Some(ManaCost::free()))?;
    let tap = parts.next_if_eq(&"{T}").is_some();
    let source_reference = |reference: &str| {
        reference == name
            || (types.split_whitespace().any(|ty| ty == "Legendary")
                && name
                    .split_once(',')
                    .is_some_and(|(short, _)| short == reference))
            || reference.eq_ignore_ascii_case("this permanent")
            || reference
                .strip_prefix("this ")
                .or_else(|| reference.strip_prefix("This "))
                .is_some_and(|noun| {
                    types
                        .split_whitespace()
                        .any(|ty| ty.eq_ignore_ascii_case(noun))
                })
    };
    let parse_counter_cost = |action: &str| -> Option<SourceCost> {
        let (action, sacrifice_source) = match action.strip_suffix(" and sacrifice it") {
            Some(prefix) => (prefix, true),
            None => (action, false),
        };
        if let Some(body) = action.strip_prefix("Put ")
            && !sacrifice_source
            && let Some((qty_kind, target)) = body.split_once(" on ")
            && source_reference(target)
        {
            let (count, kind_str) = if let Some(rest) = qty_kind
                .strip_prefix("a ")
                .or_else(|| qty_kind.strip_prefix("an "))
            {
                (1u8, rest.strip_suffix(" counter")?)
            } else {
                let (word, rest) = qty_kind.split_once(' ')?;
                let n = super::ordinary_text::amount(word)?;
                (u8::try_from(n).ok()?, rest.strip_suffix(" counters")?)
            };
            let kind = spellwake_engine::effects::CounterKind::from_oracle_label(
                &kind_str.to_ascii_lowercase(),
            )?;
            return Some(SourceCost::Counter {
                kind,
                delta: CounterCostDelta::Add(std::num::NonZeroU8::new(count)?),
                tap_source: tap,
                sacrifice_source: false,
            });
        }
        if let Some(body) = action.strip_prefix("Remove ")
            && let Some((qty_kind, target)) = body.split_once(" from ")
            && source_reference(target)
        {
            let (delta, kind_str) = if let Some(rest) = qty_kind.strip_prefix("all ") {
                (CounterCostDelta::RemoveAll, rest.strip_suffix(" counters")?)
            } else if let Some(rest) = qty_kind
                .strip_prefix("a ")
                .or_else(|| qty_kind.strip_prefix("an "))
            {
                (
                    CounterCostDelta::Remove(std::num::NonZeroU8::MIN),
                    rest.strip_suffix(" counter")?,
                )
            } else {
                let (word, rest) = qty_kind.split_once(' ')?;
                let n = super::ordinary_text::amount(word)?;
                let count = std::num::NonZeroU8::new(u8::try_from(n).ok()?)?;
                (
                    CounterCostDelta::Remove(count),
                    rest.strip_suffix(" counters")?,
                )
            };
            let kind = spellwake_engine::effects::CounterKind::from_oracle_label(
                &kind_str.to_ascii_lowercase(),
            )?;
            return Some(SourceCost::Counter {
                kind,
                delta,
                tap_source: tap,
                sacrifice_source,
            });
        }
        None
    };
    let source_cost = match parts.next() {
        Some(action) if tap && action.starts_with("Tap X untapped ") => {
            let kind = action
                .strip_prefix("Tap X untapped ")?
                .strip_suffix(" you control")?
                .strip_suffix('s')?;
            SourceCost::TapPermanents {
                filter: TapCostFilter::Subtype(vocabulary.resolve_label(kind)?),
                count: TapCostCount::ChosenX,
                tap_source: true,
                exclude_source: true,
            }
        }
        Some("Tap two untapped artifacts you control") if !tap => SourceCost::TapPermanents {
            filter: TapCostFilter::Artifact,
            count: TapCostCount::Fixed(std::num::NonZeroU8::new(2)?),
            tap_source: false,
            exclude_source: false,
        },
        Some("Exile this card from your graveyard") if !tap => SourceCost::Exile {
            tap_source: false,
            from_graveyard: true,
        },
        Some("Discard your hand") if tap => SourceCost::Discard {
            mode: DiscardCostMode::WholeHand,
            tap_source: true,
            sacrifice_source: false,
        },
        Some("Exile X cards from your graveyard") if tap => SourceCost::ExileGraveyard {
            filter: GraveyardExileCost::AnyCountX,
            tap_source: true,
        },
        Some("Exile a creature card from your graveyard") => SourceCost::ExileGraveyard {
            filter: GraveyardExileCost::Creature {
                pay_mana_cost: false,
            },
            tap_source: tap,
        },
        Some("Tap an untapped legendary permanent you control") if !tap => {
            SourceCost::TapPermanents {
                filter: TapCostFilter::Legendary,
                count: TapCostCount::Fixed(std::num::NonZeroU8::MIN),
                tap_source: false,
                exclude_source: false,
            }
        }
        Some(action) if !tap && action.starts_with("Tap an untapped ") => {
            SourceCost::TapPermanents {
                filter: TapCostFilter::Subtype(
                    vocabulary.resolve_label(
                        action
                            .strip_prefix("Tap an untapped ")?
                            .strip_suffix(" you control")?,
                    )?,
                ),
                count: TapCostCount::Fixed(std::num::NonZeroU8::MIN),
                tap_source: false,
                exclude_source: false,
            }
        }
        Some(action) if !tap && action.starts_with("Pay ") => {
            let life = action
                .strip_prefix("Pay ")?
                .strip_suffix(" life")?
                .parse()
                .ok()?;
            let sacrifice = parts
                .next_if_eq(&"Sacrifice another creature")
                .map(|_| SelectedSacrificeFilter::OtherCreature);
            SourceCost::PayLife {
                amount: LifeCostAmount::Fixed(life),
                tap_source: false,
                sacrifice,
            }
        }
        None if tap => SourceCost::Tap,
        None => SourceCost::None,
        Some(action)
            if action
                .strip_prefix("Sacrifice ")
                .is_some_and(source_reference) =>
        {
            if tap {
                SourceCost::TapSacrifice
            } else {
                SourceCost::Sacrifice
            }
        }
        Some("Sacrifice another enchantment") => SourceCost::SacrificeSelected {
            filter: SelectedSacrificeFilter::OtherEnchantment,
            count: 1,
            tap_source: tap,
            from_graveyard: false,
        },
        Some("Sacrifice another creature") => SourceCost::SacrificeSelected {
            filter: SelectedSacrificeFilter::OtherCreature,
            count: 1,
            tap_source: tap,
            from_graveyard: false,
        },
        Some("Sacrifice two artifacts") => SourceCost::SacrificeSelected {
            filter: SelectedSacrificeFilter::Artifact,
            count: 2,
            tap_source: tap,
            from_graveyard: false,
        },
        Some("Sacrifice a creature") => SourceCost::SacrificeSelected {
            filter: SelectedSacrificeFilter::Creature,
            count: 1,
            tap_source: tap,
            from_graveyard: false,
        },
        Some("Sacrifice two creatures") => SourceCost::SacrificeSelected {
            filter: SelectedSacrificeFilter::Creature,
            count: 2,
            tap_source: tap,
            from_graveyard: false,
        },
        Some("Sacrifice a creature or enchantment") => SourceCost::SacrificeSelected {
            filter: SelectedSacrificeFilter::CreatureOrEnchantment,
            count: 1,
            tap_source: tap,
            from_graveyard: false,
        },
        Some("Sacrifice an artifact or creature") => SourceCost::SacrificeSelected {
            filter: SelectedSacrificeFilter::ArtifactOrCreature,
            count: 1,
            tap_source: tap,
            from_graveyard: false,
        },
        Some("Sacrifice a Desert") => SourceCost::SacrificeSelected {
            filter: SelectedSacrificeFilter::Desert,
            count: 1,
            tap_source: tap,
            from_graveyard: false,
        },
        Some(action)
            if action.starts_with("Sacrifice a ") || action.starts_with("Sacrifice two ") =>
        {
            let (count, kind) = if let Some(kind) = action.strip_prefix("Sacrifice a ") {
                (1, kind)
            } else {
                (2, action.strip_prefix("Sacrifice two ")?.strip_suffix('s')?)
            };
            SourceCost::SacrificeSelected {
                filter: SelectedSacrificeFilter::Subtype(vocabulary.resolve_label(kind)?),
                count,
                tap_source: tap,
                from_graveyard: false,
            }
        }
        Some(action) if parse_counter_cost(action).is_some() => parse_counter_cost(action)?,
        Some("Discard a card with mana value X") if !tap => SourceCost::Discard {
            mode: DiscardCostMode::MatchingTargetSpellManaValue,
            tap_source: false,
            sacrifice_source: false,
        },
        Some("Discard a card") => SourceCost::Discard {
            mode: DiscardCostMode::SelectedOne,
            tap_source: tap,
            sacrifice_source: false,
        },
        Some(action) if action.strip_prefix("Exile ").is_some_and(source_reference) => {
            SourceCost::Exile {
                tap_source: tap,
                from_graveyard: false,
            }
        }
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    let mana_cost = price;
    let (rest, restriction) = [
        (
            " Activate only if you created a token this turn.",
            ActivationRestriction::CreatedTokenThisTurn,
        ),
        (
            " Activate only as a sorcery.",
            ActivationRestriction::Sorcery,
        ),
        (
            " Activate only during your turn, before attackers are declared.",
            ActivationRestriction::OwnTurnBeforeAttackers,
        ),
        (
            " Activate only if you have no cards in hand.",
            ActivationRestriction::EmptyHand,
        ),
        (
            " Activate only if you control a creature with power 4 or greater.",
            ActivationRestriction::ControlsCreaturePowerFourOrGreater,
        ),
        (
            " Activate only if you control three or more lands with the same name.",
            ActivationRestriction::ControlsThreeLandsWithSameName,
        ),
    ]
    .into_iter()
    .find_map(|(suffix, rule)| rest.strip_suffix(suffix).map(|effect| (effect, Some(rule))))
    .unwrap_or((rest, None));
    if let Some(limit) = rest
        .strip_prefix("Another target creature with power ")
        .and_then(|s| s.strip_suffix(" or less can't be blocked this turn."))
        .and_then(|s| s.parse().ok())
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::FilteredPermanent(
                    ObjectFilter::creature()
                        .other_than_ability_source()
                        .with_power(PowerFilter::AtMost(limit)),
                ),
                vec![super::static_grants::unblockable_effect()],
            ),
            restriction,
        ));
    }
    if let Some(count) = rest
        .strip_prefix("Target player mills ")
        .and_then(|text| {
            text.strip_suffix(" cards.")
                .or_else(|| text.strip_suffix(" card."))
        })
        .and_then(super::ordinary_text::amount)
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::Mill {
                    player: spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                    amount: spellwake_engine::program::ValueExpr::Literal(count.into()),
                }],
            ),
            restriction,
        ));
    }
    if rest == "Put a +1/+1 counter on target creature." {
        let body = super::spells::ordinary_effects::body(rest, name)?;
        return Some((mana_cost, source_cost, body, restriction));
    }
    if rest
        == "Shuffle your library, then exile the top card. Until end of turn, you may play that card without paying its mana cost."
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library(),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_library_top(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                        spellwake_engine::program::ValueExpr::literal(1),
                        false,
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::grant_play_permission(
                        spellwake_engine::toolbox::ObjectQuery::LastMoved,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                        false,
                        true,
                        false,
                    ),
                ],
            ),
            restriction,
        ));
    }
    if rest == "Target creature gets -X/-X until end of turn."
        && matches!(
            source_cost,
            SourceCost::ExileGraveyard {
                filter: GraveyardExileCost::AnyCountX,
                tap_source: true,
            }
        )
    {
        let neg_x = spellwake_engine::program::ValueExpr::Scale {
            expr: Box::new(spellwake_engine::program::ValueExpr::chosen_x()),
            factor: (-1).into(),
        };
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        spellwake_engine::program::EffectSubject::TargetObject,
                        neg_x.clone(),
                        neg_x,
                        BTreeSet::new(),
                        BTreeSet::new(),
                    ),
                ],
            ),
            restriction,
        ));
    }
    if matches!(
        rest,
        "Untap this artifact." | "Untap this permanent." | "Untap this creature."
    ) {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::untap_ability_source(),
                ],
            ),
            restriction,
        ));
    }
    if matches!(
        rest,
        "Return this permanent to its owner's hand." | "Return this creature to its owner's hand."
    ) {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::return_source_to_hand(
                    ),
                ],
            ),
            restriction,
        ));
    }
    if rest == "This creature explores." {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ExploreSubject(
                    spellwake_engine::program::MoveSubjectRef::AbilitySource,
                )],
            ),
            restriction,
        ));
    }
    if let Some(kind) = rest
        .strip_prefix("Regenerate target ")
        .and_then(|s| s.strip_suffix('.'))
    {
        let target = BodyTarget::FilteredPermanent(
            ObjectFilter::any().with_subtype(vocabulary.resolve_label(kind)?),
        );
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                target,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_target()],
            ),
            restriction,
        ));
    }
    if matches!(
        rest,
        "Regenerate this creature." | "Regenerate this permanent."
    ) {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_source()],
            ),
            restriction,
        ));
    }
    if let Some(clause) = rest.strip_suffix(" That creature can't block this turn.") {
        let (subject, amount) = clause
            .strip_suffix(" damage to target creature.")?
            .split_once(" deals ")?;
        if !source_reference(subject) {
            return None;
        }
        let amount = i64::from(amount.parse::<u16>().ok()?);
        use spellwake_engine::characteristics::{
            BlockingRestriction, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        };
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(amount),
                        false,
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                            BlockingRestriction::CannotBlock,
                        )]),
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                ],
            ),
            restriction,
        ));
    }
    if let Some((kind, modifier)) = rest
        .strip_prefix("Target ")
        .and_then(|s| s.split_once(" creature gets "))
        && let Some(subtype) = vocabulary.resolve_label(kind)
    {
        let mut body = super::spells::ordinary_effects::body(
            &format!("Target creature gets {modifier}"),
            name,
        )?;
        if body.target != BodyTarget::SingleCreature
            || !matches!(
                body.instructions.as_slice(),
                [Instruction::ApplyContinuous(spec)]
                    if matches!(
                        spec.payload,
                        spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(_)
                    )
            )
        {
            return None;
        }
        body.target = BodyTarget::FilteredPermanent(ObjectFilter::creature().with_subtype(subtype));
        return Some((mana_cost, source_cost, body, restriction));
    }
    if let Some((subject, amount)) = rest
        .strip_suffix(" damage to target creature.")
        .and_then(|s| s.split_once(" deals "))
        && source_reference(subject)
    {
        let amount = amount.parse::<u16>().ok()?;
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(i64::from(amount)),
                        false,
                    ),
                ],
            ),
            restriction,
        ));
    }
    if let Some(consequence) = rest.strip_prefix("Flip a coin. If you win the flip, ") {
        let mut body = super::spells::ordinary_effects::body(consequence, name)?;
        if body.target != BodyTarget::None
            || !matches!(
                body.instructions.as_slice(),
                [Instruction::ZoneOp(spec)]
                    if matches!(
                        spec.destination,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { .. }
                        }
                    )
            )
        {
            return None;
        }
        body.instructions.insert(
            0,
            Instruction::FlipCoins {
                player: spellwake_engine::program::ResolvingPlayer::Controller,
                mode: spellwake_engine::program::CoinFlipMode::Single,
                win_body: None,
                lose_body: None,
                lose_destination: Some(2),
            },
        );
        return Some((mana_cost, source_cost, body, restriction));
    }
    if matches!(
        source_cost,
        SourceCost::TapPermanents {
            count: TapCostCount::ChosenX,
            ..
        }
    ) {
        if rest != "Destroy target creature with power X or less." {
            return None;
        }
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::FilteredPermanent(
                    ObjectFilter::creature().with_power(PowerFilter::AtMostX),
                ),
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                    allow_regenerate: true,
                                },
                            },
                        ),
                    ),
                ],
            ),
            restriction,
        ));
    }
    if rest == "Destroy target artifact or enchantment." {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::FilteredPermanent(ObjectFilter::artifact_or_enchantment()),
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                    allow_regenerate: true,
                                },
                            },
                        ),
                    ),
                ],
            ),
            restriction,
        ));
    }
    if rest == "Counter target spell with mana value X."
        && matches!(
            source_cost,
            SourceCost::Discard {
                mode: DiscardCostMode::MatchingTargetSpellManaValue,
                ..
            }
        )
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                        spellwake_engine::toolbox::CounterDestination::Graveyard,
                    ),
                ))],
            ),
            restriction,
        ));
    }
    if rest
        == "This creature gets +X/+0 until end of turn, where X is the sacrificed enchantment's mana value."
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        EffectSubject::AbilitySource,
                        spellwake_engine::program::ValueExpr::object_stat(
                            spellwake_engine::program::ValueObjectRef::SacrificedPermanent,
                            spellwake_engine::program::ObjectStatKind::ManaValue,
                        ),
                        spellwake_engine::program::ValueExpr::literal(0),
                        Default::default(),
                        Default::default(),
                    ),
                ],
            ),
            restriction,
        ));
    }
    if let Some(amount) = rest
        .strip_prefix("You gain ")
        .and_then(|s| s.strip_suffix(" life."))
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::literal(i64::from(
                            amount.parse::<u16>().ok()?,
                        )),
                    ),
                ],
            ),
            restriction,
        ));
    }
    if let Some(amount) = rest
        .strip_prefix("This creature deals ")
        .and_then(|effect| effect.strip_suffix(" damage to any target."))
        .and_then(super::ordinary_text::amount)
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::SingleAnyTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(amount),
                        false,
                    ),
                ],
            ),
            restriction,
        ));
    }
    if rest == "This creature deals damage equal to its power to any target." {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::SingleAnyTarget,
                vec![Instruction::DamageFromPower(
                    spellwake_engine::program::PowerDamageSource::AbilitySource,
                )],
            ),
            restriction,
        ));
    }
    if let Some((subject, amount)) = rest
        .strip_suffix(" life and you draw a card. Any player may activate this ability.")
        .and_then(|s| s.split_once("'s controller loses "))
        && source_reference(subject)
        && source_cost == SourceCost::None
        && restriction.is_none()
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                        spellwake_engine::toolbox::PlayerSelector::ControllersOf(Box::new(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                        )),
                        spellwake_engine::program::ValueExpr::literal(i64::from(
                            amount.parse::<u16>().ok()?,
                        )),
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                ],
            ),
            Some(ActivationRestriction::AnyPlayer),
        ));
    }
    if rest
        == "Put all commanders you own from the command zone and from your graveyard into your hand. Then shuffle your graveyard into your library."
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Union(vec![
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Command,
                                    spellwake_engine::program::ObjectFilter::any()
                                        .owned()
                                        .commander(),
                                ),
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                                            spellwake_engine::program::ResolvingPlayer::Controller,
                                        ),
                                    )),
                                    spellwake_engine::program::ObjectFilter::any()
                                        .owned()
                                        .commander(),
                                ),
                            ]),
                            spellwake_engine::toolbox::ZoneDest::Hand,
                        ),
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        spellwake_engine::program::ResolvingPlayer::Controller,
                                    ),
                                )),
                                spellwake_engine::program::ObjectFilter::any(),
                            ),
                            spellwake_engine::toolbox::ZoneDest::Library {
                                position: spellwake_engine::toolbox::LibraryOrderPos::ShuffleInto,
                            },
                        ),
                    ),
                ],
            ),
            restriction,
        ));
    }
    if let Some(restriction_text) = rest
        .strip_prefix("Copy target activated or triggered ability you control")
        .and_then(|s| s.strip_suffix(". You may choose new targets for the copy."))
    {
        let colorless_source = match restriction_text {
            "" => false,
            " from a colorless source" => true,
            _ => return None,
        };
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::ControlledAbility { colorless_source },
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_targeted_ability(),
                ))],
            ),
            restriction,
        ));
    }
    if rest
        == "Copy target instant or sorcery spell you control. You may choose new targets for the copy."
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::SingleSpell(
                    spellwake_engine::program::SpellFilter::instant_or_sorcery().controlled(),
                ),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                ))],
            ),
            restriction,
        ));
    }
    if let Some(subtype) = rest
        .strip_prefix("Target legendary creature becomes a ")
        .and_then(|remaining| {
            remaining.strip_suffix(" in addition to its other types. Put a +1/+1 counter on it.")
        })
    {
        use spellwake_engine::characteristics::{
            InstructionSubtypeOperand, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
            SubtypeOperation, TypeChangeSpec, TypeOperation,
        };
        let change = TypeChangeSpec {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Add(InstructionSubtypeOperand::Literal(
                [vocabulary.resolve_label(subtype)?].into(),
            )),
        };
        let body = ExecutableBody::new(
            BodyTarget::FilteredPermanent(ObjectFilter::creature().legendary()),
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Type(change)]),
                    ResolvedDuration::NoStatedDuration,
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                    spellwake_engine::effects::CounterKind::PlusOne,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ],
        );
        return Some((mana_cost, source_cost, body, restriction));
    }
    if let Some(subtype) = rest
        .strip_prefix("Put an indestructible counter on target ")
        .and_then(|remaining| remaining.strip_suffix('.'))
    {
        let target = ObjectFilter::any().with_subtype(vocabulary.resolve_label(subtype)?);
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::FilteredPermanent(target),
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                        spellwake_engine::effects::CounterKind::Indestructible,
                        spellwake_engine::program::ValueExpr::literal(1),
                    ),
                ],
            ),
            restriction,
        ));
    }
    if let Some((stats,tail))=rest.strip_prefix("This land becomes a ").and_then(|s|s.split_once(' '))
        && let Some((subtype,keyword))=tail.strip_suffix(" until end of turn. It's still a land. This ability costs {1} less to activate for each legendary creature and planeswalker you control.")
            .and_then(|s|s.split_once(" creature with ")) {
        use spellwake_engine::characteristics::{AbilityChange,InstructionSubtypeOperand,ResolvedDuration,ResolvedPartSpec,ResolvedPartsSpec,SubtypeOperation,TypeChangeSpec,TypeOperation};
        let (p,t)=stats.split_once('/')?;
        let parts=ResolvedPartsSpec(vec![
            ResolvedPartSpec::Type(TypeChangeSpec {land_subtypes:SubtypeOperation::Keep,supertypes:TypeOperation::Keep,types:TypeOperation::Add([CardType::Creature].into()),
                subtypes:SubtypeOperation::Add(InstructionSubtypeOperand::Literal([vocabulary.resolve_label(subtype)?].into()))}),
            ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([static_keyword(keyword)?])),
            ResolvedPartSpec::SetPT(PowerToughness {power:p.parse::<i64>().ok()?.into(),toughness:t.parse::<i64>().ok()?.into()}),
        ]);
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                        parts,
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                ],
            ),
            Some(ActivationRestriction::DiscountLegendaryCreaturesAndPlaneswalkers),
        ));
    }
    if rest == "Exile all graveyards." {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_all_graveyards(),
                ],
            ),
            restriction,
        ));
    }
    if let Some((kind_str, reference)) = rest
        .strip_prefix(
            "Search your library for a creature card with mana value equal to the number of ",
        )
        .and_then(|s| s.strip_suffix(", put it onto the battlefield, then shuffle."))
        .and_then(|s| s.split_once(" counters on "))
        && source_reference(reference)
        && let Some(kind) = spellwake_engine::effects::CounterKind::from_oracle_label(
            &kind_str.to_ascii_lowercase(),
        )
    {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SearchLibrary {
                    filter: Box::new(
                        spellwake_engine::program::LibrarySearch::to_battlefield(
                            spellwake_engine::program::SearchCardMatcher::CardTypes(
                                [spellwake_engine::program::CardType::Creature].into(),
                            ),
                            false,
                        )
                        .with_mana_value(
                            spellwake_engine::program::SearchManaValue::EqualToSourceCounters(kind),
                        ),
                    ),
                }],
            ),
            restriction,
        ));
    }
    if let Some(body) = super::basic_land_search::body(rest) {
        return Some((mana_cost, source_cost, body, restriction));
    }
    if rest == "Target creature can't be blocked this turn." {
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![super::static_grants::unblockable_effect()],
            ),
            restriction,
        ));
    }
    if let Some((kind, modifier)) = rest.split_once(" creatures get ")
        && let Some(kind) = vocabulary.resolve_label(kind)
    {
        let synthetic = format!("All creatures get {modifier}");
        let Instruction::ApplyContinuous(mut spec) =
            super::spells::ordinary_effects::group_modifier(synthetic.strip_suffix('.')?)?
        else {
            return None;
        };
        spec.selector = Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(
            Box::new(EffectSubject::Matching(
                ObjectFilter::creature().with_subtype(kind),
            )),
        ));
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(BodyTarget::None, vec![Instruction::ApplyContinuous(spec)]),
            restriction,
        ));
    }
    if let Some(number) = rest
        .strip_prefix("You gain ")
        .and_then(|s| s.strip_suffix(" life and draw a card."))
    {
        let amount = i64::from(number.parse::<u16>().ok()?);
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::literal(amount),
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                ],
            ),
            restriction,
        ));
    }
    if let Some(body) = super::spells::ordinary_effects::exile_followup(rest, name, vocabulary)
        .or_else(|| super::spells::ordinary_effects::body(rest, name))
    {
        return Some((mana_cost, source_cost, body, restriction));
    }
    if let Some(template) = rest
        .strip_prefix("Create ")
        .and_then(|s| s.strip_suffix('.'))
    {
        let create = super::ordinary_text::create_tokens(template, vocabulary)?;
        return Some((
            mana_cost,
            source_cost,
            ExecutableBody::new(BodyTarget::None, vec![create]),
            restriction,
        ));
    }
    let printed = rest.strip_suffix(" until end of turn.")?;
    let subject = printed
        .strip_prefix("This creature ")
        .or_else(|| printed.strip_prefix(name)?.strip_prefix(' '))
        .or_else(|| {
            let (short, _) = name.split_once(',')?;
            source_reference(short)
                .then(|| printed.strip_prefix(short)?.strip_prefix(' '))
                .flatten()
        })?;
    let (stats, gains) = if let Some(value) = subject.strip_prefix("gets ") {
        let (numbers, gains) = value
            .split_once(" and gains ")
            .map_or((value, None), |(s, k)| (s, Some(k)));
        let (power, toughness) = numbers.split_once('/')?;
        let signed = |n: &str| {
            let digits = n.strip_prefix(['+', '-'])?;
            (!digits.is_empty() && digits.bytes().all(|v| v.is_ascii_digit()))
                .then(|| n.parse::<i64>().ok())
                .flatten()
        };
        (
            Some(PowerToughness {
                power: signed(power)?.into(),
                toughness: signed(toughness)?.into(),
            }),
            gains,
        )
    } else {
        (None, Some(subject.strip_prefix("gains ")?))
    };
    let mut keywords = BTreeSet::new();
    if let Some(gains) = gains {
        for word in gains.split(" and ") {
            keywords.insert(static_keyword(word)?);
        }
    }
    Some((
        mana_cost,
        source_cost,
        ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                    EffectSubject::AbilitySource,
                    stats
                        .as_ref()
                        .map(|pt| spellwake_engine::program::ValueExpr::Literal(pt.power.clone()))
                        .unwrap_or_else(|| spellwake_engine::program::ValueExpr::literal(0)),
                    stats
                        .as_ref()
                        .map(|pt| {
                            spellwake_engine::program::ValueExpr::Literal(pt.toughness.clone())
                        })
                        .unwrap_or_else(|| spellwake_engine::program::ValueExpr::literal(0)),
                    keywords,
                    BTreeSet::new(),
                ),
            ],
        ),
        restriction,
    ))
}
