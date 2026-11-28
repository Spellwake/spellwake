//! Complete single-face Aura and Equipment definitions for continuous host bonuses.
use super::*;
use spellwake_engine::program::{ResolvingPlayer, TriggerGrouping};
use spellwake_engine::{
    attachment::EnchantRestriction,
    characteristics::{
        AbilityChange, NumericExpression, NumericPair, PermanentSelector, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts,
    },
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    compile_rules(
        card,
        vocabulary,
        without_reminder(face.oracle_text.as_deref()?)?,
        profile,
    )
}

pub(super) fn compile_rules(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    text: String,
    profile: Profile,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || card.name != face.name {
        return None;
    }
    let cost = Some(fixed_cost(face.mana_cost.as_deref()?)?);
    let lines = text.lines().map(str::trim).collect::<Vec<_>>();
    if face.type_line.as_deref() == Some("Enchantment — Aura")
        && let Some(program) = compound_aura(cost.clone(), &lines, vocabulary)
    {
        return Some(program);
    }
    if face.type_line.as_deref() == Some("Legendary Artifact — Equipment")
        && let ["Living weapon", "Indestructible", effect, equip] = lines.as_slice()
        && *effect
            == "Equipped creature gets +5/+5 and has first strike, trample, indestructible, haste, and \"Whenever this creature deals combat damage to a creature, exile that creature.\""
    {
        let mut program = Program::equipment(cost, fixed_cost(equip.strip_prefix("Equip ")?)?);
        program.supertypes.insert(Supertype::Legendary);
        program
            .printed_keywords
            .insert(KeywordAbility::Indestructible);
        program
            .triggered
            .push(bearer_trigger("Living weapon", vocabulary)?);
        let mut continuous = host_effect(
            "gets +5/+5 and has first strike, trample, indestructible, and haste.",
            vocabulary,
        )?;
        let StaticContinuousAbility::Battlefield { parts, .. } = &mut continuous else {
            return None;
        };
        let abilities = parts.0.iter_mut().find_map(|part| match part {
            StaticPart::Abilities(abilities) => Some(abilities),
            _ => None,
        })?;
        abilities.granted_triggers.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::deals_damage(
                spellwake_engine::program::ObjectFilter::self_object(),
                spellwake_engine::program::DamageTriggerRecipient::Creature,
                true,
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![crate::keyword_creatures::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Intersect(
                            Box::new(spellwake_engine::toolbox::ObjectQuery::TriggerSubject),
                            Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                spellwake_engine::program::ObjectFilter::any(),
                            )),
                        ),
                        spellwake_engine::toolbox::ZoneDest::exile(),
                    ),
                )],
            ),
        });
        program.continuous = Some(continuous);
        return Some(program);
    }
    if face.type_line.as_deref() == Some("Enchantment — Aura")
        && let Some((restriction, parts)) = type_changing_aura(&lines, vocabulary)
    {
        let mut program = Program::aura(cost, restriction);
        super::army_grants::add_battlefield_ability(
            &mut program,
            PermanentSelector {
                attached_to_source: true,
                ..Default::default()
            },
            StaticParts(parts),
        )?;
        return Some(program);
    }
    if face.type_line.as_deref() == Some("Artifact — Fortification")
        && lines.as_slice()
            == [
                "Fortified land has indestructible.",
                "Whenever fortified land becomes tapped, target creature gets +1/+1 until end of turn.",
                "Fortify {3}",
            ]
    {
        use spellwake_engine::{
            characteristics::{
                PowerToughness, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
            },
            program::TriggerGrouping,
        };
        let mut program = Program::fortification(cost, ManaCost::generic(3.into()));
        program.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: PermanentSelector {
                attached_to_source: true,
                ..Default::default()
            },
            parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
                [KeywordAbility::Indestructible],
            ))]),
        });
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: spellwake_engine::program::TriggerSpec::becomes_tapped(
                spellwake_engine::program::ObjectFilter::any().attached_to_self(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![
                    crate::keyword_creatures::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(PowerToughness {
                            power: 1.into(),
                            toughness: 1.into(),
                        })]),
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                ],
            ),
        });
        return Some(program);
    }
    // Cascade is a cast trigger, independent of the later Equipment attachment.
    let (cascade, lines) = if let [rest @ .., "Cascade"] = lines.as_slice() {
        (true, rest)
    } else {
        (false, lines.as_slice())
    };
    let printed_kind = face.type_line.as_deref()?;
    let kind = printed_kind
        .strip_prefix("Legendary ")
        .unwrap_or(printed_kind);
    let (bearer, main_lines) = match lines {
        [first, rest @ ..]
            if kind == "Artifact — Equipment"
                && (matches!(*first, "Living weapon" | "For Mirrodin!" | "Job select")
                    || first.starts_with("When this Equipment enters, ")) =>
        {
            (Some(*first), rest)
        }
        rest => (None, rest),
    };
    let mut conditional = Vec::new();
    let mut triggered = Vec::new();
    let main = main_lines.iter().copied().filter(|line| {
        if line.starts_with("As long as equipped creature is ") {
            conditional.push(*line);
            false
        } else if *line=="When you cast this spell, copy it." || line.starts_with("Whenever equipped creature ") ||
            *line == "Whenever a legendary creature you control enters, you may attach this Equipment to it." ||
            line.strip_prefix("Whenever ").and_then(|s|s.strip_prefix(&face.name)) == Some(" or another Equipment you control enters, you may attach that Equipment to target creature you control.") || *line == "Whenever a creature you control enters, put a charge counter on this Equipment." {
            triggered.push(*line);
            false
        } else {
            true
        }
    }).collect::<Vec<_>>();
    let main_lines = main.as_slice();
    let (mut program, effect) = match (kind, main_lines) {
        ("Enchantment — Aura", [enchant, effect]) => {
            let target = match *enchant {
                "Enchant creature" => EnchantRestriction::Creature,
                "Enchant creature you control" => EnchantRestriction::CreatureYouControl,
                "Enchant creature an opponent controls" => {
                    EnchantRestriction::CreatureAnOpponentControls
                }
                _ => return None,
            };
            (
                Program::aura(cost, target),
                effect.strip_prefix("Enchanted creature ")?,
            )
        }
        (kind, [own_keyword, effect, reconfigure])
            if kind.starts_with("Artifact Creature — Equipment ") =>
        {
            let subtypes = kind
                .strip_prefix("Artifact Creature — Equipment ")?
                .split_whitespace()
                .map(|label| vocabulary.resolve_label(label))
                .collect::<Option<BTreeSet<_>>>()?;
            let parsed = parse_keywords(own_keyword, Some(vocabulary), None)?;
            let grants = host_abilities(parsed)?;
            if !grants.granted_combat_keywords.is_empty() || grants.granted_exalted != 0 {
                return None;
            }
            let mut program = Program::reconfigure(
                cost,
                number(face.power.as_deref())?.into(),
                number(face.toughness.as_deref())?.into(),
                subtypes,
                fixed_cost(reconfigure.strip_prefix("Reconfigure ")?)?,
            );
            program.printed_keywords = grants.granted_keywords;
            program.printed_toxic = grants.granted_toxic;
            (program, effect.strip_prefix("Equipped creature ")?)
        }
        ("Artifact — Equipment", [equip]) if !triggered.is_empty() => (
            Program::equipment(cost, fixed_cost(equip.strip_prefix("Equip ")?)?),
            "",
        ),
        ("Artifact — Equipment", [effect, equip]) => (
            Program::equipment(cost, fixed_cost(equip.strip_prefix("Equip ")?)?),
            effect.strip_prefix("Equipped creature ")?,
        ),
        _ => return None,
    };
    if printed_kind.starts_with("Legendary ") {
        program
            .supertypes
            .insert(spellwake_engine::program::Supertype::Legendary);
    }
    if let Some(bearer) = bearer {
        program.triggered.push(bearer_trigger(bearer, vocabulary)?);
    }
    for line in triggered {
        if kind != "Artifact — Equipment" {
            return None;
        }
        if line == "When you cast this spell, copy it." {
            let id = program
                .triggered
                .iter()
                .map(|ability| ability.id.0)
                .max()
                .map_or(Some(0), |id| id.checked_add(1))?;
            program.triggered.push(TriggeredAbility::spell_copies(
                TriggeredAbilityId(id),
                spellwake_engine::program::CastCopyKeyword::CopyThisPermanentSpell,
            ));
            continue;
        }
        let (condition, target, instructions) = match line {
            "Whenever a legendary creature you control enters, you may attach this Equipment to it." => {
                (
                    spellwake_engine::program::TriggerSpec::enters(
                        spellwake_engine::program::ObjectFilter::creature()
                            .controlled()
                            .legendary(),
                    ),
                    BodyTarget::None,
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 1,
                        },
                        Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::all(
                                spellwake_engine::toolbox::ObjectQuery::Source,
                                spellwake_engine::toolbox::ObjectMutation::Attach(
                                    spellwake_engine::toolbox::AttachOp::ToObject {
                                        host: Box::new(
                                            spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                                        ),
                                        ability: None,
                                    },
                                ),
                            ),
                        )),
                    ],
                )
            }
            line if line
                .strip_prefix("Whenever ")
                .and_then(|s| s.strip_prefix(&face.name))
                == Some(
                    " or another Equipment you control enters, you may attach that Equipment to target creature you control.",
                ) =>
            {
                (
                    spellwake_engine::program::TriggerSpec::enters(
                        spellwake_engine::program::ObjectFilter::any()
                            .controlled()
                            .with_artifact_subtype(
                                spellwake_engine::program::ArtifactSubtype::Equipment,
                            )
                            .or_self(),
                    ),
                    BodyTarget::filtered_permanent(
                        spellwake_engine::program::ObjectFilter::creature().controlled(),
                    ),
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 1,
                        },
                        Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::all(
                                spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                                spellwake_engine::toolbox::ObjectMutation::Attach(
                                    spellwake_engine::toolbox::AttachOp::ToObject {
                                        host: Box::new(
                                            spellwake_engine::toolbox::ObjectQuery::Targets,
                                        ),
                                        ability: None,
                                    },
                                ),
                            ),
                        )),
                    ],
                )
            }
            "Whenever a creature you control enters, put a charge counter on this Equipment." => (
                spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::any_types(
                        spellwake_engine::program::CardTypeMask::from_card_type(CardType::Creature),
                    )
                    .controlled(),
                ),
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::toolbox_emit::put_counters_on_source(
                        spellwake_engine::effects::CounterKind::Charge,
                        spellwake_engine::program::ValueExpr::literal(1),
                    ),
                ],
            ),
            "Whenever equipped creature deals combat damage to a player, that player loses half their life, rounded up." => {
                (
                    spellwake_engine::program::TriggerSpec::deals_damage(
                        spellwake_engine::program::ObjectFilter::any().attached_to_self(),
                        spellwake_engine::program::DamageTriggerRecipient::AnyPlayer,
                        true,
                    ),
                    BodyTarget::None,
                    vec![crate::keyword_creatures::toolbox_emit::lose_half_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::UniqueRelatedPlayer,
                        ),
                        true,
                    )],
                )
            }
            "Whenever equipped creature is dealt damage, it deals damage equal to its power to target player or planeswalker." => {
                (
                    spellwake_engine::program::TriggerSpec::new(
                        spellwake_engine::program::TriggerEvent::DealtDamage {
                            subject: spellwake_engine::program::ObjectFilter::any()
                                .attached_to_self(),
                            linked_player: false,
                        },
                    ),
                    BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER,
                    vec![Instruction::DealDamage(Box::new(
                        spellwake_engine::toolbox::DealDamageSpec::from_power(
                            spellwake_engine::program::PowerDamageSource::CreatureDealtDamage,
                        ),
                    ))],
                )
            }
            "Whenever equipped creature dies, attach this Equipment to target creature you control." => {
                (
                    spellwake_engine::program::TriggerSpec::dies(
                        spellwake_engine::program::ObjectFilter::any().attached_to_self(),
                    ),
                    BodyTarget::filtered_permanent(
                        spellwake_engine::program::ObjectFilter::creature().controlled(),
                    ),
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::attach_ability(
                            spellwake_engine::attachment::AttachAbility::Equip,
                        ),
                    ))],
                )
            }
            "Whenever equipped creature attacks, this Equipment deals 1 damage to target creature defending player controls." => {
                (
                    spellwake_engine::program::TriggerSpec::attacks(
                        spellwake_engine::program::ObjectFilter::any().attached_to_self(),
                        spellwake_engine::program::AttackTriggerMode::Individual {
                            capture_defender: false,
                        },
                    ),
                    BodyTarget::filtered_permanent(
                        spellwake_engine::program::ObjectFilter::creature().of_related_player(),
                    ),
                    vec![crate::keyword_creatures::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(1),
                        false,
                    )],
                )
            }
            "Whenever equipped creature attacks, you may search your library for a basic land card, put it onto the battlefield tapped, then shuffle." => {
                (
                    spellwake_engine::program::TriggerSpec::attacks(
                        spellwake_engine::program::ObjectFilter::any().attached_to_self(),
                        spellwake_engine::program::AttackTriggerMode::Individual {
                            capture_defender: false,
                        },
                    ),
                    BodyTarget::None,
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 1,
                        },
                        Instruction::SearchLibrary {
                            filter: Box::new(
                                spellwake_engine::program::LibrarySearch::to_battlefield(
                                    spellwake_engine::program::SearchCardMatcher::BasicLand,
                                    true,
                                ),
                            ),
                        },
                    ],
                )
            }
            _ => return None,
        };
        let next = program
            .triggered
            .iter()
            .map(|ability| ability.id.0)
            .max()
            .map_or(Some(0), |id| id.checked_add(1))?;
        let grouping = if condition
            == spellwake_engine::program::TriggerSpec::new(
                spellwake_engine::program::TriggerEvent::DealtDamage {
                    subject: spellwake_engine::program::ObjectFilter::any().attached_to_self(),
                    linked_player: false,
                },
            ) {
            TriggerGrouping::OneOrMoreTogether
        } else {
            TriggerGrouping::EachOccurrence
        };
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(next),
            condition,
            grouping,
            body: ExecutableBody::new(target, instructions),
        });
    }
    if cascade {
        if kind != "Artifact — Equipment" {
            return None;
        }
        let next = program
            .triggered
            .iter()
            .map(|ability| ability.id.0)
            .max()
            .map_or(Some(0), |id| id.checked_add(1))?;
        program
            .triggered
            .push(TriggeredAbility::cascade(TriggeredAbilityId(next)));
    }
    if !effect.is_empty() {
        program.continuous = Some(host_effect(effect, vocabulary)?);
    }
    for line in conditional {
        if kind != "Artifact — Equipment" {
            return None;
        }
        let (quality, grant) = line
            .strip_prefix("As long as equipped creature is ")?
            .split_once(", it ")?;
        let grant = if profile == Profile::Current {
            grant
                .strip_prefix("gets an additional ")
                .map(|modifier| format!("gets {modifier}"))
                .unwrap_or_else(|| grant.to_owned())
        } else {
            grant.to_owned()
        };
        let StaticContinuousAbility::Battlefield {
            mut selector,
            parts,
            ..
        } = host_effect(&grant, vocabulary)?
        else {
            return None;
        };
        if quality == "legendary" {
            selector
                .required_supertypes
                .insert(spellwake_engine::program::Supertype::Legendary);
        } else if profile == Profile::Current {
            selector.creature_subtype = Some(
                vocabulary.resolve_label(
                    quality
                        .strip_prefix("a ")
                        .or_else(|| quality.strip_prefix("an "))?,
                )?,
            );
        } else {
            return None;
        }
        super::army_grants::add_battlefield_ability(&mut program, selector, parts)?;
    }
    Some(program)
}

fn compound_aura(
    cost: Option<spellwake_engine::mana::ManaCost>,
    lines: &[&str],
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let selector = || PermanentSelector {
        attached_to_source: true,
        ..Default::default()
    };
    match lines {
        ["Enchant creature", "You control enchanted creature.", bonus] => {
            let StaticContinuousAbility::Battlefield {
                selector,
                mut parts,
                ..
            } = host_effect(bonus.strip_prefix("Enchanted creature ")?, vocabulary)?
            else {
                return None;
            };
            parts.0.insert(0, StaticPart::ControlSourceController);
            let mut program = Program::aura(cost, EnchantRestriction::Creature);
            super::army_grants::add_battlefield_ability(&mut program, selector, parts)?;
            Some(program)
        }
        ["Enchant creature", grant] => {
            let (keywords, ability) =
                if let Some(quoted) = grant.strip_prefix("Enchanted creature has \"") {
                    (None, quoted)
                } else {
                    let (keywords, quoted) = grant
                        .strip_prefix("Enchanted creature has ")?
                        .split_once(" and \"")?;
                    (
                        Some(host_effect(&format!("has {keywords}."), vocabulary)?),
                        quoted,
                    )
                };
            let part = super::army_grants::granted_creature_ability(
                ability.strip_suffix('"')?,
                vocabulary,
            )?;
            let mut program = Program::aura(cost, EnchantRestriction::Creature);
            program.continuous = keywords;
            super::army_grants::add_battlefield_ability(
                &mut program,
                selector(),
                StaticParts(vec![part]),
            )?;
            Some(program)
        }
        [
            "Enchant land",
            enters,
            "Enchanted land has \"{T}: Add one mana of any color.\"",
        ] => {
            let effect = enters.strip_prefix("When this Aura enters, ")?;
            let body = super::ordinary_triggers::body(effect, "this Aura", vocabulary)?;
            let mut program = Program::aura(cost, EnchantRestriction::Land);
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(0),
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body,
            });
            super::army_grants::add_battlefield_ability(
                &mut program,
                selector(),
                StaticParts(vec![StaticPart::GrantAnyColorMana]),
            )?;
            Some(program)
        }
        _ => None,
    }
}

fn type_changing_aura(
    lines: &[&str],
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(EnchantRestriction, Vec<StaticPart>)> {
    use spellwake_engine::characteristics::{
        AbilityRemoval, ColorOperation, StaticColorOperand, StaticLandSubtypeOperand,
        StaticSubtypeOperand, StaticTypeChange, SubtypeOperation, TypeOperation,
    };
    match lines {
        [
            "Enchant permanent",
            "Enchanted permanent is a colorless Forest land.",
        ] => Some((
            EnchantRestriction::Permanent,
            vec![
                StaticPart::Type(StaticTypeChange {
                    types: TypeOperation::Set(BTreeSet::from([CardType::Land])),
                    supertypes: TypeOperation::Keep,
                    subtypes: SubtypeOperation::Keep,
                    land_subtypes: SubtypeOperation::Set(StaticLandSubtypeOperand::Literal(
                        BTreeSet::from([spellwake_engine::land::LandType::Forest]),
                    )),
                }),
                StaticPart::Color(ColorOperation::Set(StaticColorOperand::Literal(
                    BTreeSet::new(),
                ))),
            ],
        )),
        [
            "Enchant creature",
            "Enchanted creature is an Insect artifact creature with base power and toughness 0/1 and has indestructible, and it loses all other abilities, card types, and creature types.",
        ] => {
            let mut abilities = AbilityChange::grant_keywords([KeywordAbility::Indestructible]);
            abilities.removal = Some(AbilityRemoval::All);
            Some((
                EnchantRestriction::Creature,
                vec![
                    StaticPart::Type(StaticTypeChange {
                        types: TypeOperation::Set(BTreeSet::from([
                            CardType::Artifact,
                            CardType::Creature,
                        ])),
                        supertypes: TypeOperation::Keep,
                        land_subtypes: SubtypeOperation::Keep,
                        subtypes: SubtypeOperation::Set(StaticSubtypeOperand::Literal(
                            BTreeSet::from([vocabulary.resolve_label("Insect")?]),
                        )),
                    }),
                    StaticPart::Abilities(abilities),
                    StaticPart::SetPT(NumericPair {
                        power: NumericExpression::Literal(0.into()),
                        toughness: NumericExpression::Literal(1.into()),
                    }),
                ],
            ))
        }
        _ => None,
    }
}

pub(super) fn host_effect(
    effect: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticContinuousAbility> {
    let effect = effect.strip_suffix('.')?;
    let (effect, equipped_by_at_least) = effect
        .strip_suffix(" as long as two or more Equipment are attached to it")
        .map_or((effect, 0), |prefix| (prefix, 2));
    let mut parts = Vec::new();
    let mut effect = effect;
    if let Some(prefix) = effect.strip_suffix(" in addition to its other types") {
        use spellwake_engine::characteristics::{
            StaticSubtypeOperand, StaticTypeChange, SubtypeOperation, TypeOperation,
        };
        let (rest, kind) = prefix.rsplit_once(" and is a ")?;
        let subtype = vocabulary.resolve_label(kind)?;
        parts.push(StaticPart::Type(StaticTypeChange {
            types: TypeOperation::Keep,
            supertypes: TypeOperation::Keep,
            land_subtypes: SubtypeOperation::Keep,
            subtypes: SubtypeOperation::Add(StaticSubtypeOperand::Literal(BTreeSet::from([
                subtype,
            ]))),
        }));
        effect = rest.trim_end_matches(',');
    }
    let effect = effect.replace(", has ", " and has ");
    let grant = if let Some(stats) = effect.strip_prefix("gets ") {
        let (pair, grant) = stats
            .split_once(" and has ")
            .map_or((stats, None), |(pair, grant)| (pair, Some(grant)));
        let modifier = match pair {
            "+1/+1 for each artifact and/or enchantment you control" => {
                Some(NumericExpression::ZoneCount {
                    zone: spellwake_engine::characteristics::StaticZone::ControlledBattlefield,
                    types: spellwake_engine::program::CardTypeMask::ARTIFACT
                        .union(spellwake_engine::program::CardTypeMask::ENCHANTMENT),
                })
            }
            "+1/+1 for each charge counter on this Equipment" => {
                Some(NumericExpression::Counters {
                    source: None,
                    kind: spellwake_engine::effects::CounterKind::Charge,
                })
            }
            _ => None,
        };
        if let Some(modifier) = modifier {
            parts.push(StaticPart::ModifyPT(NumericPair {
                power: modifier.clone(),
                toughness: modifier,
            }));
        } else {
            let (p, t) = pair.split_once('/')?;
            parts.push(StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(p.parse::<i64>().ok()?.into()),
                toughness: NumericExpression::Literal(t.parse::<i64>().ok()?.into()),
            }));
        }
        grant
    } else {
        Some(effect.strip_prefix("has ")?)
    };
    if let Some(words) = grant {
        let normalized = words.replace(", and ", ", ").replace(" and ", ", ");
        parts.push(StaticPart::Abilities(host_abilities(parse_keywords(
            &normalized,
            Some(vocabulary),
            None,
        )?)?));
    }
    Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            attached_to_source: true,
            equipped_by_at_least,
            ..Default::default()
        },
        parts: StaticParts(parts),
    })
}

/// Only admit a host bonus when every parsed ability has an executable grant.
fn host_abilities(parsed: ParsedKeywords) -> Option<AbilityChange> {
    let ParsedKeywords {
        madness,
        exile_action,
        entry_counters,
        plot,
        conditional,
        toxic,
        continuous,
        keywords,
        triggers,
        activated,
        reductions,
    } = parsed;
    if madness.is_some()
        || exile_action.is_some()
        || !entry_counters.is_empty()
        || plot.is_some()
        || conditional.is_some()
        || continuous.is_some()
        || !activated.is_empty()
        || !reductions.is_empty()
        || (keywords.is_empty() && triggers.is_empty() && toxic.is_none())
        || keywords
            .iter()
            .any(|key| matches!(key, KeywordAbility::Changeling | KeywordAbility::Devoid))
    {
        return None;
    }
    let mut change = AbilityChange::grant_keywords(keywords);
    change.granted_toxic = toxic;
    for trigger in triggers {
        if trigger.is_exalted() {
            change.granted_exalted = change.granted_exalted.checked_add(1)?;
        } else {
            change
                .granted_combat_keywords
                .push(trigger.as_combat_keyword()?.clone());
        }
    }
    Some(change)
}

/// Exact token characteristics and default name are part of each keyword definition.
fn bearer_trigger(keyword: &str, vocabulary: &CreatureTypeVocabulary) -> Option<TriggeredAbility> {
    use spellwake_engine::{
        characteristics::Color,
        program::TriggerGrouping,
        token::{TokenDefinition, TokenName, TokenSubtype},
    };
    if let Some(text) = keyword.strip_prefix("When this Equipment enters, ") {
        let (price, action) = if let Some(paid) = text.strip_prefix("you may pay ") {
            let (price, action) = paid.split_once(". If you do, ")?;
            (Some(fixed_cost(price)?), action)
        } else {
            (None, text)
        };
        let description = action
            .strip_prefix("create ")?
            .strip_suffix(", then attach this Equipment to it.")?;
        let Instruction::CreateTokens { creations } =
            ordinary_text::create_tokens(description, vocabulary)?
        else {
            return None;
        };
        let [creation] = <[_; 1]>::try_from(creations).ok()?;
        if creation.amount != spellwake_engine::program::ValueExpr::Literal(1.into())
            || creation.recipients != spellwake_engine::token::TokenRecipients::Controller
        {
            return None;
        }
        let mut instructions = Vec::new();
        if let Some(cost) = price {
            instructions.push(Instruction::PromptChoice(Box::new(
                spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                    ResolvingPlayer::Controller,
                    cost,
                    spellwake_engine::program::ValueExpr::Literal(0.into()),
                    1,
                    2,
                ),
            )));
        }
        instructions.push(Instruction::MutateObject(Box::new(
            spellwake_engine::toolbox::MutateObjectSpec::create_token_and_attach(Box::new(
                creation.definition,
            )),
        )));
        return Some(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, instructions),
        });
    }
    let (pt, color, labels): (i64, Option<Color>, &[&str]) = match keyword {
        "Living weapon" => (0, Some(Color::Black), &["Phyrexian", "Germ"]),
        "For Mirrodin!" => (2, Some(Color::Red), &["Rebel"]),
        "Job select" => (1, None, &["Hero"]),
        _ => return None,
    };
    let types = labels
        .iter()
        .map(|word| vocabulary.resolve_label(word))
        .collect::<Option<Vec<_>>>()?;
    let mut token = Program::creature(None, pt.into(), pt.into(), types.iter().copied().collect());
    token.color_indicator = color.map(|color| BTreeSet::from([color]));
    let definition = TokenDefinition::new(
        token,
        TokenName::Default {
            ordered_subtypes: types.into_iter().map(TokenSubtype::Creature).collect(),
        },
        vocabulary,
    )
    .ok()?;
    Some(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MutateObject(Box::new(
                spellwake_engine::toolbox::MutateObjectSpec::create_token_and_attach(Box::new(
                    definition,
                )),
            ))],
        ),
    })
}

/// Complete Bestow rules boxes with one supported continuous grant to the enchanted creature.
pub(super) fn compile_bestow(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::program::ConditionalCastCost;
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let raw = face.oracle_text.as_deref()?;
    if !face
        .type_line
        .as_deref()?
        .starts_with("Enchantment Creature — ")
        || !raw.lines().any(|line| line.starts_with("Bestow "))
    {
        return None;
    }
    let rules = without_reminder(raw)?;
    let mut cost = None;
    let mut host = None;
    let mut creature = Vec::new();
    for row in rules.lines().map(str::trim).filter(|row| !row.is_empty()) {
        if let Some(price) = row.strip_prefix("Bestow ") {
            if cost.replace(fixed_cost(price)?).is_some() {
                return None;
            }
        } else if let Some(text) = row.strip_prefix("Enchanted creature ") {
            if host.replace(host_effect(text, vocabulary)?).is_some() {
                return None;
            }
        } else {
            creature.push(row);
        }
    }
    let mut program = ordinary_creature(
        card,
        vocabulary,
        Profile::Current,
        Some(&creature.join("\n")),
    )
    .ok()?;
    if program.conditional_cost.is_some() || program.continuous.is_some() {
        return None;
    }
    program.conditional_cost = Some(ConditionalCastCost::Bestow(cost?));
    program.continuous = Some(host?);
    Some(program)
}
