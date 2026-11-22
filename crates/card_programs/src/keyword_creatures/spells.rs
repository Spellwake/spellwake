//! Compile supported single-face instant and sorcery text, including casting keywords.
use super::*;
mod cast_copies;
mod modes;
pub(super) mod ordinary_effects;
mod rebound;
use spellwake_engine::program::{
    AdditionalCastCost, BodyTarget, Comparison, ExecutableBody, Instruction, ResolutionCondition,
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || face.name != card.name {
        return None;
    }
    let kind = face.type_line.as_deref()?;
    if profile == Profile::Current
        && let Some(rules) = face
            .oracle_text
            .as_deref()
            .and_then(super::without_reminder)
        && let Some((effect, keyword)) = rules.trim().rsplit_once('\n')
        && let Some(price) = keyword.trim().strip_prefix("Splice onto ")
    {
        use spellwake_engine::program::{SpliceAbility, SpliceQuality};
        let (quality, symbols) = if let Some(price) = price.strip_prefix("Arcane ") {
            (SpliceQuality::Arcane, price)
        } else {
            (
                SpliceQuality::InstantOrSorcery,
                price.strip_prefix("instant or sorcery ")?,
            )
        };
        // Preserve an absent mana cost as unpayable until that printed cost is represented.
        if face.mana_cost.as_deref().is_none_or(str::is_empty) || effect.trim().contains('\n') {
            return None;
        }
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(effect.trim().to_owned());
        let mut program = compile(&reduced, vocabulary, profile)?;
        if !program.supports_splice_text() {
            return None;
        }
        program.splice = Some(SpliceAbility {
            quality,
            cost: fixed_cost(symbols.trim())?,
        });
        return Some(program);
    }
    if profile == Profile::Current
        && let Some(kind) = kind.strip_prefix("Legendary ")
        && matches!(kind, "Instant" | "Sorcery")
    {
        let mut ordinary = card.clone();
        ordinary.faces[0].type_line = Some(kind.to_owned());
        let mut program = compile(&ordinary, vocabulary, profile)?;
        program.supertypes.insert(Supertype::Legendary);
        return Some(program);
    }
    if profile==Profile::Current && let Some(subtype)=kind.strip_prefix("Kindred Instant — ")
        && let Some(text)=face.oracle_text.as_deref().and_then(super::without_reminder)
        && let Some((effect,reduction))=text.trim().split_once('\n')
        && effect=="Counter target spell or ability that targets a permanent you control."
        && let Some((cost,power))=reduction.strip_prefix("This spell costs {").and_then(|line|line.strip_suffix(" or greater.")).and_then(|line|line.split_once("} less to cast if it targets a spell or ability that targets a creature you control with power "))
    {
        let mut program=Program::instant(fixed_cost(face.mana_cost.as_deref()?)?,ExecutableBody::new(BodyTarget::StackEntryTargetingControlledPermanent,vec![Instruction::CounterSpell]));
        program.card_types.insert(CardType::Kindred);
        program.creature_subtypes.insert(vocabulary.resolve_label(subtype)?);
        program.cast_reductions.push(spellwake_engine::program::CastReduction::StackTargetThreatensCreature {minimum_power:power.parse().ok()?,discount:cost.parse().ok()?});
        return Some(program);
    }
    let kindred_subtype = if profile == Profile::Current {
        kind.strip_prefix("Kindred Sorcery — ")
    } else {
        (profile >= Profile::GameplayV6 && kind == "Kindred Sorcery — Rogue").then_some("Rogue")
    };
    let kindred = kindred_subtype.is_some();
    let arcane =
        profile == Profile::Current && matches!(kind, "Instant — Arcane" | "Sorcery — Arcane");
    if !kindred && !arcane && !matches!(kind, "Instant" | "Sorcery") {
        return None;
    }
    if profile == Profile::Current
        && card.name == "Decimate"
        && kind == "Sorcery"
        && face.mana_cost.as_deref() == Some("{2}{R}{G}")
        && super::without_reminder(face.oracle_text.as_deref()?)?.trim()
            == "Destroy target artifact, target creature, target enchantment, and target land."
    {
        use spellwake_engine::program::ObjectFilter;
        let targets = [
            BodyTarget::FilteredPermanent(ObjectFilter::artifact()),
            BodyTarget::SingleCreature,
            BodyTarget::FilteredPermanent(ObjectFilter::enchantment()),
            BodyTarget::FilteredPermanent(ObjectFilter::land()),
        ];
        let mut instructions = targets
            .into_iter()
            .map(|target| {
                Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                    target,
                    vec![Instruction::DestroyTarget],
                )))
            })
            .collect::<Vec<_>>();
        instructions.push(Instruction::DestroyFixedTargets);
        return Some(Program::sorcery(
            fixed_cost("{2}{R}{G}")?,
            ExecutableBody::new(BodyTarget::None, instructions),
        ));
    }
    if profile == Profile::Current
        && card.name == "Rise of the Eldrazi"
        && kind == "Sorcery"
        && face.mana_cost.as_deref() == Some("{9}{C}{C}{C}")
        && super::without_reminder(face.oracle_text.as_deref()?)?.trim()
            == "This spell can't be countered.\nDestroy target permanent. Target player draws four cards. Take an extra turn after this one.\nExile Rise of the Eldrazi."
    {
        let mut program = Program::sorcery(
            fixed_cost("{9}{C}{C}{C}")?,
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                        BodyTarget::SinglePermanent,
                        vec![Instruction::DestroyTarget],
                    ))),
                    Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                        BodyTarget::SinglePlayer,
                        vec![Instruction::DrawTargetPlayer {
                            amount: spellwake_engine::program::ValueExpr::Literal(4.into()),
                        }],
                    ))),
                    Instruction::TakeExtraTurn,
                    Instruction::ExileResolvingSpell,
                ],
            ),
        );
        program.cannot_be_countered = true;
        return Some(program);
    }
    let rules = super::without_reminder(face.oracle_text.as_deref()?)?;
    if profile == Profile::Current
        && kind == "Instant"
        && rules.trim()
            == "When you next cast an instant spell, cast a sorcery spell, or activate a loyalty ability this turn, copy that spell or ability twice. You may choose new targets for the copies."
    {
        return Some(Program::instant(
            fixed_cost(face.mana_cost.as_deref()?)?,
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::WatchNextSpellOrLoyaltyTwice],
            ),
        ));
    }
    if profile == Profile::Current && let Some(effect) = rules.trim().strip_prefix("As an additional cost to cast this spell, reveal a colorless creature card from your hand.\n") {
        let (target,operation) = match effect {
            "Exile target creature if its power is less than or equal to the revealed card's power." => (BodyTarget::SingleCreature,Instruction::ExileCreatureAtMostRevealedPower),
            "Exile each creature and planeswalker with mana value less than the revealed card's mana value." => (BodyTarget::None,Instruction::ExileCreaturesAndPlaneswalkersBelowRevealedManaValue),
            _ => return None,
        };
        let body=ExecutableBody::new(target,vec![operation]);
        let cost=fixed_cost(face.mana_cost.as_deref()?)?;
        let mut program = match kind {
            "Instant" => Program::instant(cost,body), "Sorcery" => Program::sorcery(cost,body), _ => return None,
        };
        program.reveal_colorless_creature_cast_cost=true;
        return Some(program);
    }
    if profile == Profile::Current
        && let Some((text, price)) = rules.trim().split_once("\nOverload ")
    {
        use spellwake_engine::program::{ConditionalCastCost, ObjectFilter, PublicZoneDestination};
        let each = match text {
            "Destroy target artifact you don't control." => {
                Instruction::DestroyGroup(Box::new(ObjectFilter::artifact().not_controlled()))
            }
            "Return target nonland permanent you don't control to its owner's hand." => {
                Instruction::MoveGroup {
                    group: Box::new(ObjectFilter::nonland().not_controlled()),
                    destination: PublicZoneDestination::Hand,
                }
            }
            _ => return None,
        };
        let normal = ordinary_effects::spell_body(text, &card.name)?;
        let cost = fixed_cost(face.mana_cost.as_deref()?)?;
        let mut program = match kind {
            "Instant" => Program::instant(cost, normal),
            "Sorcery" => Program::sorcery(cost, normal),
            _ => return None,
        };
        program.conditional_cost = Some(ConditionalCastCost::Overload {
            mana: fixed_cost(price.trim())?,
            each: Box::new(ExecutableBody::new(BodyTarget::None, vec![each])),
        });
        return Some(program);
    }
    if profile == Profile::Current
        && (rules.trim_start().starts_with("Choose ")
            || rules
                .trim_start()
                .starts_with("Cast this spell only during combat on your turn.\nChoose "))
        && rules.contains("• ")
    {
        return modes::compile(
            &rules,
            &card.name,
            kind,
            fixed_cost(face.mana_cost.as_deref()?)?,
            vocabulary,
        );
    }
    let mut cleave = None;
    let mut flashback = None;
    let mut flashback_x_symbols = 0;
    let mut flashback_sacrifice_creatures = 0;
    let mut flashback_tap_white_creatures = 0;
    let mut flashback_life = 0u16;
    let mut discard_cast_cost = false;
    let mut sacrifice_creature_cast_cost = false;
    let mut madness = None;
    let mut madness_cycling = None;
    let mut exile_action = None;
    let (rules, mut conditional) = escape::extract(rules, profile)?;
    let mut discounts = Vec::new();
    let mut keywords = BTreeSet::new();
    let mut additional = Vec::new();
    let mut copy_triggers = Vec::new();
    let mut effect_lines = Vec::new();
    let mut cycling_triggers = Vec::new();
    let mut cascades = Vec::new();
    for line in rules.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if profile == Profile::Current
            && line == "As an additional cost to cast this spell, sacrifice a creature."
        {
            if sacrifice_creature_cast_cost {
                return None;
            }
            sacrifice_creature_cast_cost = true;
            continue;
        }
        if profile == Profile::Current
            && line == "As an additional cost to cast this spell, discard a card."
        {
            if discard_cast_cost {
                return None;
            }
            discard_cast_cost = true;
            continue;
        }
        if profile >= Profile::GameplayV4
            && let Some(action) = exile_hand::parse(&line.to_ascii_lowercase())
        {
            if exile_action.replace(action).is_some() {
                return None;
            }
        } else if profile == Profile::Current
            && (matches!(line, "Storm" | "Gravestorm") || line.starts_with("Replicate "))
        {
            use spellwake_engine::program::CastCopyKeyword;
            let kind = match line {
                "Storm" => CastCopyKeyword::Storm,
                "Gravestorm" => CastCopyKeyword::Gravestorm,
                _ => {
                    let index = additional.len();
                    additional.push(AdditionalCastCost::Replicate(fixed_cost(
                        line.strip_prefix("Replicate ")?,
                    )?));
                    CastCopyKeyword::Replicate(index)
                }
            };
            copy_triggers.push(TriggeredAbility::spell_copies(
                TriggeredAbilityId(u64::try_from(copy_triggers.len() + cascades.len()).ok()?),
                kind,
            ));
        } else if profile >= Profile::GameplayV5
            && let Some(price) = line.strip_prefix("Madness ")
        {
            if madness.replace(fixed_cost(price)?).is_some() {
                return None;
            }
        } else if profile >= Profile::GameplayV5
            && let Some(price) = line.strip_prefix("Sneak ")
        {
            if conditional
                .replace(spellwake_engine::program::ConditionalCastCost::Sneak(
                    fixed_cost(price)?,
                ))
                .is_some()
            {
                return None;
            }
        } else if profile >= Profile::GameplayV5 && line.starts_with("Cleave ") {
            if cleave
                .replace(fixed_cost(line.strip_prefix("Cleave ")?)?)
                .is_some()
            {
                return None;
            }
        } else if profile >= Profile::GameplayV5
            && let Some(cost) = line.strip_prefix("Cycling ")
        {
            if madness_cycling
                .replace(ActivatedAbility::cycling(
                    ActivatedAbilityId(0),
                    fixed_cost(cost)?,
                ))
                .is_some()
            {
                return None;
            }
        } else if profile == Profile::Current
            && let Some(effect) = line.strip_prefix("When you cycle this card, ")
        {
            let body = ordinary_effects::spell_body(effect, &card.name)?;
            if body.target != BodyTarget::None {
                return None;
            }
            cycling_triggers.push(body);
        } else if profile >= Profile::GameplayV5 && line == "Bargain" {
            keywords.insert(KeywordAbility::Bargain);
        } else if profile >= Profile::GameplayV6 && line == "Cascade" {
            cascades.push(TriggeredAbility::cascade(TriggeredAbilityId(
                u64::try_from(cascades.len() + copy_triggers.len()).ok()?,
            )));
        } else if profile >= Profile::GameplayV5 && line == "Rebound" {
            keywords.insert(KeywordAbility::Rebound);
        } else if profile == Profile::Current
            && line == "Flashback—Tap three untapped white creatures you control."
        {
            if flashback.replace(ManaCost::free()).is_some() {
                return None;
            }
            flashback_tap_white_creatures = 3;
        } else if profile == Profile::Current && line == "Flashback—Sacrifice three creatures." {
            if flashback.replace(ManaCost::free()).is_some() {
                return None;
            }
            flashback_sacrifice_creatures = 3;
        } else if profile == Profile::Current
            && let Some((cost, life)) = line
                .strip_prefix("Flashback—")
                .and_then(|s| s.split_once(", Pay "))
        {
            if flashback.replace(fixed_cost(cost)?).is_some() {
                return None;
            }
            flashback_life = life.strip_suffix(" life.")?.parse().ok()?;
            if flashback_life == 0 {
                return None;
            }
        } else if let Some(cost) = line.strip_prefix("Flashback ") {
            let (price, symbols) = if profile == Profile::Current {
                super::avacyn_restored::cost_with_x(cost)?
            } else {
                (fixed_cost(cost)?, 0)
            };
            if flashback.replace(price).is_some() {
                return None;
            }
            flashback_x_symbols = symbols;
        } else if let Some(cost) = line.strip_prefix("Buyback ") {
            additional.push(AdditionalCastCost::Buyback(fixed_cost(cost)?));
        } else if let Some(cost) = line.strip_prefix("Kicker ") {
            additional.push(AdditionalCastCost::Kicker(fixed_cost(cost)?));
        } else if matches!(
            line,
            "Convoke" | "Delve" | "Improvise" | "Retrace" | "Jump-start"
        ) {
            keywords.insert(match line {
                "Convoke" => KeywordAbility::Convoke,
                "Delve" => KeywordAbility::Delve,
                "Retrace" => KeywordAbility::Retrace,
                "Jump-start" => KeywordAbility::JumpStart,
                _ => KeywordAbility::Improvise,
            });
        } else if let Some(price) = reductions::conditional(&line.to_ascii_lowercase(), profile) {
            if conditional.replace(price).is_some() {
                return None;
            }
        } else if let Some(reduction) =
            reductions::parse(&line.to_ascii_lowercase(), vocabulary, profile)
        {
            discounts.push(reduction);
        } else {
            effect_lines.push(line);
        }
    }
    // Older profiles admitted instruction spells only alongside a casting keyword.
    if profile != Profile::Current
        && copy_triggers.is_empty()
        && cascades.is_empty()
        && cleave.is_none()
        && madness.is_none()
        && exile_action.is_none()
        && flashback.is_none()
        && discounts.is_empty()
        && keywords.is_empty()
        && additional.is_empty()
        && conditional.is_none()
    {
        return None;
    }
    if profile != Profile::Current && madness_cycling.is_some() && madness.is_none() {
        return None;
    }
    let kicker = additional
        .iter()
        .position(|cost| matches!(cost, AdditionalCastCost::Kicker(_)));
    let escaped = matches!(
        conditional,
        Some(spellwake_engine::program::ConditionalCastCost::Escape { .. })
    );
    use spellwake_engine::program::ConditionalCastCost;
    if (!cycling_triggers.is_empty() && madness_cycling.is_none())
        || (effect_lines.is_empty() && cascades.is_empty())
        || (profile != Profile::Current && effect_lines.len() > 1)
    {
        return None;
    }
    let combined = effect_lines.join(" ");
    let text = combined.as_str();
    let ordinary = || {
        spell_effect(text, &card.name, kicker)
            .or_else(|| {
                (profile == Profile::Current)
                    .then(|| super::innistrad::spell(text, &card.name, vocabulary))?
            })
            .or_else(|| {
                (profile == Profile::Current)
                    .then(|| ordinary_effects::combat_defense(text, vocabulary))?
            })
            .or_else(|| {
                (profile == Profile::Current)
                    .then(|| ordinary_effects::exile_followup(text, &card.name, vocabulary))?
            })
            .or_else(|| {
                (profile == Profile::Current)
                    .then(|| ordinary_effects::threshold_destruction(text, vocabulary))?
            })
            .or_else(|| {
                (profile == Profile::Current).then(|| super::basic_land_search::body(text))?
            })
            .or_else(|| {
                (profile == Profile::Current)
                    .then(|| ordinary_effects::spell_body(text, &card.name))?
            })
            .or_else(|| {
                if profile != Profile::Current {
                    return None;
                }
                let tokens = text.strip_prefix("Create ")?.strip_suffix('.')?;
                let create = ordinary_text::create_tokens(tokens, vocabulary)?;
                Some(ExecutableBody::new(BodyTarget::None, vec![create]))
            })
    };
    let body = match text {
        "For each creature your opponents control, create a 4/4 green Phyrexian Beast creature token. Each of those tokens fights a different one of those creatures."
            if profile == Profile::Current =>
        {
            let Instruction::CreateTokens { mut creations } = ordinary_text::create_tokens(
                "a 4/4 green Phyrexian Beast creature token",
                vocabulary,
            )?
            else {
                return None;
            };
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokensToFight(
                    creations.pop()?.definition,
                )],
            )
        }
        "Tempting offer — Create X 1/1 red Elemental creature tokens with haste. Each opponent may create X 1/1 red Elemental creature tokens with haste. For each opponent who does, create X 1/1 red Elemental creature tokens with haste."
            if profile == Profile::Current =>
        {
            let Instruction::CreateTokens { mut creations } = ordinary_text::create_tokens(
                "a 1/1 red Elemental creature token with haste",
                vocabulary,
            )?
            else {
                return None;
            };
            let mut creation = creations.pop()?;
            creation.amount = spellwake_engine::program::ValueExpr::chosen_x();
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::TemptWithTokens {
                    creation,
                    phase: spellwake_engine::token::TemptingTokenPhase::Initial,
                }],
            )
        }
        // A spell whose only printed ability is cascade resolves without doing anything.
        "" if !cascades.is_empty() => {
            ExecutableBody::new(BodyTarget::None, vec![Instruction::Jump { destination: 1 }])
        }
        "You gain 4 life." if !cascades.is_empty() => ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Gain(4.into()),
            }],
        ),
        text if !cascades.is_empty()
            && text.strip_prefix(&card.name) == Some(" deals 4 damage to target creature.") =>
        {
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 4.into() }],
            )
        }

        "Target player loses 3 life and you gain 3 life. If this spell's prowl cost was paid, draw a card."
            if kindred && matches!(conditional, Some(ConditionalCastCost::Prowl(_))) =>
        {
            ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![
                    Instruction::AdjustTargetLife {
                        adjustment: spellwake_engine::life::LifeAdjustment::Lose(3.into()),
                    },
                    Instruction::AdjustControllerLife {
                        adjustment: spellwake_engine::life::LifeAdjustment::Gain(3.into()),
                    },
                    Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::event(
                                spellwake_engine::program::EventValueKind::PaidAlternative,
                            ),
                            comparison: Comparison::Equal,
                            right: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        },
                        destination: 4,
                    },
                    draw(1),
                ],
            )
        }
        "Target creature gets -5/-5 until end of turn."
            if matches!(conditional, Some(ConditionalCastCost::Mayhem(_))) =>
        {
            ExecutableBody::new(BodyTarget::SingleCreature, vec![pump(-5, -5, None)])
        }
        line if matches!(conditional, Some(ConditionalCastCost::Mayhem(_)))
            && line.strip_prefix(&card.name) == Some(" deals 4 damage to target creature.") =>
        {
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 4.into() }],
            )
        }
        "Counter target spell [that wasn't cast from its owner's hand]." if cleave.is_some() => {
            use spellwake_engine::program::ConditionalCastCost;
            let counter = |filter| {
                ExecutableBody::new(
                    BodyTarget::SingleSpell(filter),
                    vec![Instruction::CounterSpell],
                )
            };
            if conditional
                .replace(ConditionalCastCost::Cleave {
                    mana: cleave.take()?,
                    unbracketed: Box::new(counter(spellwake_engine::program::SpellFilter::any())),
                })
                .is_some()
            {
                return None;
            }
            counter(spellwake_engine::program::SpellFilter::any().not_from_owners_hand())
        }
        _ if cleave.is_some() => return None,
        "You gain 20 life." if exile_action.is_some() => ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Gain(20.into()),
            }],
        ),
        "Target creature gets +1/+1 and gains double strike until end of turn."
            if exile_action.is_some() =>
        {
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![pump(1, 1, Some(KeywordAbility::DoubleStrike))],
            )
        }
        "Counter target spell." if !cascades.is_empty() || exile_action.is_some() => {
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::CounterSpell],
            )
        }
        "Scry 2, then draw two cards." if exile_action.is_some() => ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::Scry {
                    player: spellwake_engine::program::ResolvingPlayer::Controller,
                    amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                },
                draw(2),
            ],
        ),
        line if exile_action.is_some()
            && line.strip_prefix(&card.name) == Some(" deals 13 damage to target creature.") =>
        {
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 13.into() }],
            )
        }
        "Target player loses 2 life." if escaped => ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![Instruction::AdjustTargetLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Lose(2.into()),
            }],
        ),
        "Target player mills four cards." if escaped => ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![Instruction::Mill {
                player: spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                amount: spellwake_engine::program::ValueExpr::Literal(4.into()),
            }],
        ),
        "Target creature gets -3/-3 until end of turn. If this spell was bargained, that creature gets -5/-5 until end of turn instead."
            if keywords.contains(&KeywordAbility::Bargain) =>
        {
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    if_not_bargained(3),
                    pump(-5, -5, None),
                    Instruction::Jump { destination: 4 },
                    pump(-3, -3, None),
                ],
            )
        }
        "Target creature gets +2/+2 until end of turn. If this spell was bargained, that creature also gains flying and lifelink until end of turn."
            if keywords.contains(&KeywordAbility::Bargain) =>
        {
            use spellwake_engine::characteristics::{
                AbilityChange, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
            };
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    pump(2, 2, None),
                    if_not_bargained(3),
                    Instruction::CreateContinuousEffect {
                        duration: ResolvedDuration::UntilEndOfTurn,
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                            AbilityChange::grant_keywords([
                                KeywordAbility::Flying,
                                KeywordAbility::Lifelink,
                            ]),
                        )]),
                    },
                ],
            )
        }
        "Target creature gets -3/-3 until end of turn." if madness.is_some() => {
            ExecutableBody::new(BodyTarget::SingleCreature, vec![pump(-3, -3, None)])
        }
        "Create two 2/2 black Zombie creature tokens." if madness.is_some() => {
            let mut create = rebound::creature_token(
                vocabulary,
                &["Zombie"],
                2,
                spellwake_engine::characteristics::Color::Black,
                None,
            )?;
            let Instruction::CreateTokens { creations } = &mut create else {
                return None;
            };
            creations.first_mut()?.amount = spellwake_engine::program::ValueExpr::Literal(2.into());
            ExecutableBody::new(BodyTarget::None, vec![create])
        }
        "Counter target spell." if madness.is_some() => ExecutableBody::new(
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
            vec![Instruction::CounterSpell],
        ),
        "Target creature gets +2/+2 and gains trample until end of turn." if madness.is_some() => {
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![pump(2, 2, Some(KeywordAbility::Trample))],
            )
        }
        text if madness.is_some()
            && text.strip_prefix(&card.name) == Some(" deals 4 damage to target creature.") =>
        {
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 4.into() }],
            )
        }
        source if !copy_triggers.is_empty() => {
            cast_copies::body(source, &card.name, vocabulary).or_else(ordinary)?
        }
        source if keywords.contains(&KeywordAbility::Rebound) => {
            rebound::body(source, vocabulary).or_else(ordinary)?
        }
        _ => ordinary()?,
    };
    let (cost, x_symbols) = if profile == Profile::Current {
        super::avacyn_restored::cost_with_x(face.mana_cost.as_deref()?)?
    } else {
        (fixed_cost(face.mana_cost.as_deref()?)?, 0)
    };
    let mut spell = match kind {
        "Instant" | "Instant — Arcane" => Program::instant(cost, body),
        _ => Program::sorcery(cost, body),
    };
    if arcane {
        spell
            .spell_subtypes
            .insert(spellwake_engine::program::SpellSubtype::Arcane);
    }
    if let Some(subtype) = kindred_subtype {
        spell.card_types.insert(CardType::Kindred);
        spell
            .creature_subtypes
            .insert(vocabulary.resolve_label(subtype)?);
    }
    spell.printed_keywords = keywords;
    spell.additional_cast_costs = additional;
    spell.conditional_cost = conditional;
    spell.flashback = flashback;
    spell.flashback_x_mana_symbols = flashback_x_symbols;
    spell.x_mana_symbols = x_symbols;
    spell.flashback_sacrifice_creatures = flashback_sacrifice_creatures;
    spell.flashback_tap_white_creatures = flashback_tap_white_creatures;
    spell.flashback_life = flashback_life;
    spell.discard_cast_cost = discard_cast_cost;
    spell.sacrifice_creature_cast_cost = sacrifice_creature_cast_cost;
    spell.exile_action = exile_action;
    spell.triggered.extend(cascades);
    spell.madness = madness;
    spell.activated.extend(madness_cycling);
    spell.triggered.extend(copy_triggers);
    for body in cycling_triggers {
        use spellwake_engine::program::TriggerGrouping;
        spell.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(u64::try_from(spell.triggered.len()).ok()?),
            condition: spellwake_engine::program::TriggerSpec::player_action(
                spellwake_engine::program::PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::Cycled { self_only: true },
            )
            .in_zone(spellwake_engine::program::TriggerZone::Any),
            grouping: TriggerGrouping::EachOccurrence,
            body,
        });
    }
    spell.cast_reductions = discounts;
    Some(spell)
}

fn if_not_bargained(destination: usize) -> Instruction {
    Instruction::BranchIfFalse {
        condition: ResolutionCondition {
            left: spellwake_engine::program::ValueExpr::event(
                spellwake_engine::program::EventValueKind::Bargained,
            ),
            comparison: Comparison::Equal,
            right: spellwake_engine::program::ValueExpr::Literal(1.into()),
        },
        destination,
    }
}

fn draw(amount: i64) -> Instruction {
    Instruction::DrawController {
        amount: spellwake_engine::program::ValueExpr::Literal(amount.into()),
    }
}

fn spell_effect(line: &str, name: &str, kicker: Option<usize>) -> Option<ExecutableBody> {
    let if_kicked = |index, destination| Instruction::BranchIfFalse {
        condition: ResolutionCondition {
            left: spellwake_engine::program::ValueExpr::event(
                spellwake_engine::program::EventValueKind::KickerCount(index),
            ),
            comparison: Comparison::Greater,
            right: spellwake_engine::program::ValueExpr::Literal(0.into()),
        },
        destination,
    };
    let (target, effects) = match line {
        "Return target permanent card with mana value 3 or less from your graveyard to the battlefield. If this spell was cast from a graveyard, you may copy this spell and may choose a new target for the copy." => {
            (
                BodyTarget::FilteredOwnGraveyardCard(
                    spellwake_engine::program::ObjectFilter::permanent()
                        .with_mana_value(spellwake_engine::program::ManaValueFilter::AtMost(3)),
                ),
                vec![
                    Instruction::ReturnOwnGraveyardPermanentToBattlefield { tapped: false },
                    Instruction::RequireCondition(
                        spellwake_engine::program::InterveningCondition::CastFromGraveyard,
                    ),
                    Instruction::OptionalEffect {
                        chooser: spellwake_engine::program::ResolvingPlayer::Controller,
                    },
                    Instruction::CopyResolvingSpell,
                ],
            )
        }
        "Target creature gets +4/+4 until end of turn." => {
            (BodyTarget::SingleCreature, vec![pump(4, 4, None)])
        }
        "Target creature gets +1/+1 and gains flying until end of turn." => (
            BodyTarget::SingleCreature,
            vec![pump(1, 1, Some(KeywordAbility::Flying))],
        ),
        "Target creature gets +1/+1 and gains haste until end of turn." => (
            BodyTarget::SingleCreature,
            vec![pump(1, 1, Some(KeywordAbility::Haste))],
        ),
        "Draw a card." => (BodyTarget::None, vec![draw(1)]),
        "Draw two cards." => (BodyTarget::None, vec![draw(2)]),
        "Draw three cards." => (BodyTarget::None, vec![draw(3)]),
        "Return target creature to its owner's hand." => (
            BodyTarget::SingleCreature,
            vec![Instruction::ReturnToOwnersHand],
        ),
        "Return target permanent to its owner's hand." => (
            BodyTarget::SinglePermanent,
            vec![Instruction::ReturnToOwnersHand],
        ),
        "Destroy target creature." => {
            (BodyTarget::SingleCreature, vec![Instruction::DestroyTarget])
        }
        "Destroy target permanent." => (
            BodyTarget::SinglePermanent,
            vec![Instruction::DestroyTarget],
        ),
        "Return target creature to its owner's hand. Draw a card. If this spell was kicked, you gain 3 life." => {
            (
                BodyTarget::SingleCreature,
                vec![
                    Instruction::ReturnToOwnersHand,
                    draw(1),
                    if_kicked(kicker?, 4),
                    Instruction::AdjustControllerLife {
                        adjustment: spellwake_engine::life::LifeAdjustment::Gain(3.into()),
                    },
                ],
            )
        }
        _ if line.strip_prefix(name)
            == Some(
                " deals 2 damage to target creature. If this spell was kicked, it deals 4 damage instead.",
            ) =>
        {
            (
                BodyTarget::SingleCreature,
                vec![
                    if_kicked(kicker?, 3),
                    Instruction::Damage { amount: 4.into() },
                    Instruction::Jump { destination: 4 },
                    Instruction::Damage { amount: 2.into() },
                ],
            )
        }
        _ => return None,
    };
    Some(ExecutableBody::new(target, effects))
}

fn pump(power: i64, toughness: i64, keyword: Option<KeywordAbility>) -> Instruction {
    use spellwake_engine::characteristics::{
        AbilityChange, PowerToughness, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    };
    let mut parts = vec![ResolvedPartSpec::ModifyPT(PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    })];
    if let Some(keyword) = keyword {
        parts.push(ResolvedPartSpec::Abilities(AbilityChange::grant_keywords(
            [keyword],
        )));
    }
    Instruction::CreateContinuousEffect {
        parts: ResolvedPartsSpec(parts),
        duration: ResolvedDuration::UntilEndOfTurn,
    }
}
