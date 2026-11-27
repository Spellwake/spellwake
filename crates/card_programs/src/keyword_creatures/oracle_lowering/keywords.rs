use super::*;

pub(super) fn lower_keyword_ability_name(name: &str) -> Option<KeywordAbility> {
    match name.to_ascii_lowercase().as_str() {
        "flying" => Some(KeywordAbility::Flying),
        "reach" => Some(KeywordAbility::Reach),
        "first strike" => Some(KeywordAbility::FirstStrike),
        "double strike" => Some(KeywordAbility::DoubleStrike),
        "deathtouch" => Some(KeywordAbility::Deathtouch),
        "lifelink" => Some(KeywordAbility::Lifelink),
        "vigilance" => Some(KeywordAbility::Vigilance),
        "trample" => Some(KeywordAbility::Trample),
        "haste" => Some(KeywordAbility::Haste),
        "menace" => Some(KeywordAbility::Menace),
        "defender" => Some(KeywordAbility::Defender),
        "indestructible" => Some(KeywordAbility::Indestructible),
        "hexproof" => Some(KeywordAbility::Hexproof),
        "umbra armor" | "totem armor" => Some(KeywordAbility::UmbraArmor),
        "shroud" => Some(KeywordAbility::Shroud),
        "flash" => Some(KeywordAbility::Flash),
        "fear" => Some(KeywordAbility::Fear),
        "intimidate" => Some(KeywordAbility::Intimidate),
        "skulk" => Some(KeywordAbility::Skulk),
        "shadow" => Some(KeywordAbility::Shadow),
        "horsemanship" => Some(KeywordAbility::Horsemanship),
        "phasing" => Some(KeywordAbility::Phasing),
        "wither" => Some(KeywordAbility::Wither),
        "infect" => Some(KeywordAbility::Infect),
        "split second" => Some(KeywordAbility::SplitSecond),
        "changeling" => Some(KeywordAbility::Changeling),
        "devoid" => Some(KeywordAbility::Devoid),
        "partner" => Some(KeywordAbility::Partner),
        "choose a background" => Some(KeywordAbility::ChooseABackground),
        "doctor's companion" => Some(KeywordAbility::DoctorsCompanion),
        "convoke" => Some(KeywordAbility::Convoke),
        "improvise" => Some(KeywordAbility::Improvise),
        "delve" => Some(KeywordAbility::Delve),
        "bargain" => Some(KeywordAbility::Bargain),
        "rebound" => Some(KeywordAbility::Rebound),
        "retrace" => Some(KeywordAbility::Retrace),
        "jump-start" => Some(KeywordAbility::JumpStart),
        "banding" => Some(KeywordAbility::Banding),
        "decayed" => Some(KeywordAbility::Decayed),
        "plainswalk" => Some(KeywordAbility::Plainswalk),
        "islandwalk" => Some(KeywordAbility::Islandwalk),
        "swampwalk" => Some(KeywordAbility::Swampwalk),
        "mountainwalk" => Some(KeywordAbility::Mountainwalk),
        "forestwalk" => Some(KeywordAbility::Forestwalk),
        "desertwalk" => Some(KeywordAbility::Desertwalk),
        "nonbasic landwalk" => Some(KeywordAbility::NonbasicLandwalk),
        "legendary landwalk" => Some(KeywordAbility::LegendaryLandwalk),
        "snow landwalk" => Some(KeywordAbility::SnowLandwalk),
        "snow forestwalk" => Some(KeywordAbility::SnowForestwalk),
        "snow swampwalk" => Some(KeywordAbility::SnowSwampwalk),
        "artifact landwalk" => Some(KeywordAbility::ArtifactLandwalk),
        "start your engines!" => Some(KeywordAbility::StartYourEngines),
        "daybound" => Some(KeywordAbility::Daybound),
        "nightbound" => Some(KeywordAbility::Nightbound),
        _ => None,
    }
}

pub(super) fn lower_keyword(
    kw: &KeywordAst,
    kw_idx: usize,
    face_ast: &FaceAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    is_spell: bool,
) -> Result<(), &'static str> {
    let lower = kw.name.to_ascii_lowercase();
    let param = kw.parameter.as_deref().map(str::trim);

    // Standalone static keyword abilities
    if param.is_none()
        && let Some(ka) = lower_keyword_ability_name(&lower)
    {
        if ka == KeywordAbility::Decayed {
            program.printed_keywords.insert(KeywordAbility::Decayed);
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Decayed,
            ));
            return Ok(());
        }
        program.printed_keywords.insert(ka);
        return Ok(());
    }

    match lower.as_str() {
        "landwalk" => {
            let p = param.ok_or("oracle:kw-param")?.to_ascii_lowercase();
            let ka = match p.as_str() {
                "plains" => KeywordAbility::Plainswalk,
                "island" => KeywordAbility::Islandwalk,
                "swamp" => KeywordAbility::Swampwalk,
                "mountain" => KeywordAbility::Mountainwalk,
                "forest" => KeywordAbility::Forestwalk,
                "desert" => KeywordAbility::Desertwalk,
                "nonbasic land" => KeywordAbility::NonbasicLandwalk,
                "legendary land" => KeywordAbility::LegendaryLandwalk,
                "snow land" => KeywordAbility::SnowLandwalk,
                "snow forest" => KeywordAbility::SnowForestwalk,
                "snow swamp" => KeywordAbility::SnowSwampwalk,
                "artifact land" => KeywordAbility::ArtifactLandwalk,
                _ => return Err("oracle:unsupported-keyword"),
            };
            program.printed_keywords.insert(ka);
            Ok(())
        }
        "protection" => {
            let p = param.ok_or("oracle:protection-no-param")?;
            let stripped = p.strip_prefix("from ").unwrap_or(p);
            let low = stripped.trim().to_ascii_lowercase();
            if matches!(
                low.as_str(),
                "the chosen color" | "the color of your choice"
            ) {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            ..PermanentSelector::default()
                        },
                        parts: StaticParts(vec![StaticPart::GrantProtectionChosenColor]),
                    },
                );
            }
            if low == "the chosen player" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            ..PermanentSelector::default()
                        },
                        parts: StaticParts(vec![StaticPart::GrantProtectionChosenPlayer]),
                    },
                );
            }
            let quals = parse_protection_keywords(stripped, vocabulary)
                .ok_or("oracle:unsupported-protection")?;
            program.printed_keywords.extend(quals);
            Ok(())
        }
        "prowess" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Prowess,
            ));
            Ok(())
        }
        "exploit" => {
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::self_enters(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 1,
                        },
                        Instruction::ZoneOp(Box::new(
                            spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                                AffectedPlayers::One(ResolvingPlayer::Controller),
                                ValueExpr::literal(1),
                                PermanentTypeFilter {
                                    any_types: BTreeSet::new(),
                                    required_types: BTreeSet::from([CardType::Creature]),
                                    forbidden_types: BTreeSet::new(),
                                    object_filter: None,
                                },
                            ),
                        )),
                    ],
                ),
            });
            Ok(())
        }
        "exalted" => {
            program
                .triggered
                .push(TriggeredAbility::exalted(ids.triggered()));
            Ok(())
        }
        "extort" => {
            program
                .triggered
                .push(TriggeredAbility::extort(ids.triggered()));
            Ok(())
        }
        "cascade" => {
            program
                .triggered
                .push(TriggeredAbility::cascade(ids.triggered()));
            Ok(())
        }
        "storm" if param.is_none() && !program.has_type(CardType::Land) => {
            program.triggered.push(TriggeredAbility::spell_copies(
                ids.triggered(),
                CastCopyKeyword::Storm,
            ));
            Ok(())
        }
        "gravestorm" if is_spell => {
            program.triggered.push(TriggeredAbility::spell_copies(
                ids.triggered(),
                CastCopyKeyword::Gravestorm,
            ));
            Ok(())
        }
        "flanking" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Flanking,
            ));
            Ok(())
        }
        "battle cry" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::BattleCry,
            ));
            Ok(())
        }
        "melee" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Melee,
            ));
            Ok(())
        }
        "dethrone" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Dethrone,
            ));
            Ok(())
        }
        "training" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Training,
            ));
            Ok(())
        }
        "ingest" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Ingest,
            ));
            Ok(())
        }
        "afflict" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Afflict(n.into()),
            ));
            Ok(())
        }
        "annihilator" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program
                .triggered
                .push(TriggeredAbility::annihilator(ids.triggered(), n.into()));
            Ok(())
        }
        "bushido" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Bushido(n.into()),
            ));
            Ok(())
        }
        "frenzy" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Frenzy(n.into()),
            ));
            Ok(())
        }
        "mobilize" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Mobilize(n.into()),
            ));
            Ok(())
        }
        "poisonous" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Poisonous(n.into()),
            ));
            Ok(())
        }
        "rampage" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Rampage(n.into()),
            ));
            Ok(())
        }
        "renown" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Renown(n.into()),
            ));
            Ok(())
        }
        "evolve" => {
            program
                .triggered
                .push(GrowthKeyword::Evolve.ability(ids.triggered()));
            Ok(())
        }
        "mentor" => {
            program
                .triggered
                .push(GrowthKeyword::Mentor.ability(ids.triggered()));
            Ok(())
        }
        "modular\u{2014}sunburst" if param.is_none() => {
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::Sunburst);
            program
                .triggered
                .push(TransferKeyword::Modular(0).ability(ids.triggered()));
            Ok(())
        }
        "modular" => {
            if param.is_some_and(|s| s.trim().eq_ignore_ascii_case("sunburst")) {
                program
                    .entry_counter_abilities
                    .push(EntryCounterAbility::Sunburst);
                program
                    .triggered
                    .push(TransferKeyword::Modular(0).ability(ids.triggered()));
                return Ok(());
            }
            let n: u32 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            let tk = TransferKeyword::Modular(n);
            if let Some(entry) = tk.entry() {
                program.entry_counter_abilities.push(entry);
            }
            program.triggered.push(tk.ability(ids.triggered()));
            Ok(())
        }
        "graft" => {
            let n: u32 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            let tk = TransferKeyword::Graft(n);
            if let Some(entry) = tk.entry() {
                program.entry_counter_abilities.push(entry);
            }
            program.triggered.push(tk.ability(ids.triggered()));
            Ok(())
        }
        "soulshift" => {
            let n: u32 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program
                .triggered
                .push(TransferKeyword::Soulshift(n).ability(ids.triggered()));
            Ok(())
        }
        "soulbond" => {
            for entrant_only in [false, true] {
                program.triggered.push(TriggeredAbility {
                    id: ids.triggered(),
                    condition: TriggerSpec::keyword(TriggerKeywordEvent::Soulbond { entrant_only }),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::soulbond_pair(
                                entrant_only,
                            ),
                        ))],
                    ),
                });
            }
            Ok(())
        }
        "undying" => {
            program
                .triggered
                .push(CounterKeyword::Undying.ability(ids.triggered()));
            Ok(())
        }
        "persist" => {
            program
                .triggered
                .push(CounterKeyword::Persist.ability(ids.triggered()));
            Ok(())
        }
        "vanishing" => {
            let n = match param {
                None => None,
                Some(s) => Some(s.parse::<u32>().map_err(|_| "oracle:kw-param")?),
            };
            program
                .triggered
                .push(CounterKeyword::Vanishing(n).ability(ids.triggered()));
            program
                .triggered
                .push(CounterKeyword::VanishingLastCounter.ability(ids.triggered()));
            Ok(())
        }
        "fading" => {
            let n: u32 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program
                .triggered
                .push(CounterKeyword::Fading(n).ability(ids.triggered()));
            Ok(())
        }
        "echo" => {
            let cost = match param {
                Some(s) => parse_fixed_mana_cost(s).ok_or("oracle:echo-cost")?,
                None => program
                    .printed_mana_cost()
                    .cloned()
                    .ok_or("oracle:echo-no-cost")?,
            };
            program
                .triggered
                .push(UpkeepKeyword::Echo.ability(ids.triggered(), cost));
            Ok(())
        }
        "cumulative upkeep" => {
            let raw = param.ok_or("oracle:cum-upkeep-cost")?.trim();
            let clean = raw.trim_end_matches('.').trim();
            if let Some(cost) = parse_fixed_mana_cost(clean) {
                program
                    .triggered
                    .push(UpkeepKeyword::CumulativeUpkeep.ability(ids.triggered(), cost));
                return Ok(());
            }
            let ab = lower_non_mana_cumulative_upkeep(clean, vocabulary, ids)?;
            program.triggered.push(ab);
            Ok(())
        }
        "afterlife" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            let ab = spellwake_engine::program::TokenKeyword::Afterlife
                .ability(ids.triggered(), n.into(), vocabulary)
                .map_err(|_| "oracle:afterlife-failed")?;
            program.triggered.push(ab);
            Ok(())
        }
        "fabricate" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            let ab = spellwake_engine::program::TokenKeyword::Fabricate
                .ability(ids.triggered(), n.into(), vocabulary)
                .map_err(|_| "oracle:fabricate-failed")?;
            program.triggered.push(ab);
            Ok(())
        }
        "ward" => {
            let p_str = param.ok_or("oracle:ward-non-mana")?;
            if let Some(cost) = parse_fixed_mana_cost(p_str) {
                program
                    .triggered
                    .push(TriggeredAbility::ward(ids.triggered(), cost));
                return Ok(());
            }
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            let atoms = spellwake_oracle_compiler::clauses::parse_cost_list(p_str, &compiler_vocab)
                .map_err(|_| "oracle:ward-non-mana")?;
            if atoms.is_empty() {
                return Err("oracle:ward-non-mana");
            }
            let mut body = lower_ward_cost_body(&atoms, vocabulary)?;
            body.instructions.push(Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::counter_ward_source(),
            )));
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::new(TriggerEvent::Targeted {
                    subject: ObjectFilter::self_object(),
                    by_opponent: true,
                    spell_only: false,
                    instant_or_sorcery_only: false,
                }),
                grouping: TriggerGrouping::EachOccurrence,
                body,
            });
            Ok(())
        }
        "myriad" => {
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::attacks(
                    ObjectFilter::self_object(),
                    AttackTriggerMode::Individual {
                        capture_defender: true,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::myriad(),
                        )),
                        Instruction::ScheduleDelayedStepTrigger {
                            step: Step::EndCombat,
                            own_step_only: false,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![Instruction::ZoneOp(Box::new(
                                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                        spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                                        spellwake_engine::toolbox::ZoneDest::exile(),
                                    ),
                                ))],
                            )),
                        },
                    ],
                ),
            });
            Ok(())
        }
        "bloodthirst" => {
            let p = param.ok_or("oracle:kw-param")?;
            if p.eq_ignore_ascii_case("x") {
                program
                    .entry_counter_abilities
                    .push(EntryCounterAbility::BloodthirstX);
            } else {
                let n: i64 = p.parse().map_err(|_| "oracle:kw-param")?;
                program
                    .entry_counter_abilities
                    .push(EntryCounterAbility::Bloodthirst(n.into()));
            }
            Ok(())
        }
        "devour" => {
            let n: u8 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::Devour(n));
            Ok(())
        }
        "sunburst" => {
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::Sunburst);
            Ok(())
        }
        "toxic" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.printed_toxic = Some(match &program.printed_toxic {
                Some(prev) => prev.plus(&n.into()),
                None => n.into(),
            });
            Ok(())
        }
        "equip" => lower_equip_keyword(param, program, vocabulary, ids),
        "scavenge" => {
            let p_str = param.ok_or("oracle:kw-param")?;
            let (mana_cost, 0, hybrids, snow) =
                parse_mana_cost_with_snow(p_str).ok_or("oracle:kw-param")?
            else {
                return Err("oracle:kw-param");
            };
            let power = match program
                .permanent()
                .and_then(|p| p.printed_pt.as_ref())
                .map(|pt| &pt.power)
            {
                Some(PrintedStat::Number(n)) => n.clone(),
                _ => return Err("oracle:kw-param"),
            };
            let id = ids.activated();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz_snow) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz_snow)]);
            }
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost,
                source_cost: SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: true,
                },
                body: ExecutableBody::new(
                    BodyTarget::SINGLE_CREATURE,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                        CounterKind::PlusOne,
                        ValueExpr::Literal(power),
                    )],
                ),
            });
            Ok(())
        }
        "ravenous" => {
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::ChosenXPlusOne);
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::self_enters(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::BranchCondition {
                            condition: Box::new(
                                spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(
                                    ResolutionCondition {
                                        left: ValueExpr::EventValue(EventValueKind::EnteredKickerX),
                                        comparison: Comparison::GreaterOrEqual,
                                        right: ValueExpr::literal(5),
                                    },
                                )),
                            ),
                            destination: 2,
                        },
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                            ValueExpr::literal(1),
                        ),
                    ],
                ),
            });
            Ok(())
        }
        "reconfigure" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:reconfigure-cost")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost.clone(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SINGLE_CREATURE,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::attach_ability(
                            AttachAbility::Reconfigure,
                        ),
                    ))],
                ),
            });
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::unattach(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                        ),
                    ],
                ),
            });
            Ok(())
        }
        "fortify" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:fortify-cost")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SINGLE_PERMANENT,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::attach_ability(
                            AttachAbility::Fortify,
                        ),
                    ))],
                ),
            });
            Ok(())
        }
        "cycling" => {
            let (cost, 0, hybrids, snow) = param
                .and_then(parse_mana_cost_with_snow)
                .ok_or("oracle:cycling-non-mana")?
            else {
                return Err("oracle:cycling-non-mana");
            };
            let id = ids.activated();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz_snow) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz_snow)]);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                            ValueExpr::literal(1),
                        ),
                    ],
                ),
            });
            Ok(())
        }
        s if s.ends_with("cycling") && s != "cycling" => {
            lower_typecycling_keyword(s, param, program, vocabulary, ids)
        }
        "transmute" => {
            let (cost, 0, hybrids, snow) = param
                .and_then(parse_mana_cost_with_snow)
                .ok_or("oracle:transmute-cost")?
            else {
                return Err("oracle:transmute-cost");
            };
            let id = ids.activated();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz_snow) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz_snow)]);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Transmute),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::SearchLibrary {
                        filter: Box::new(
                            LibrarySearch::to_hand(SearchCardMatcher::Any)
                                .with_mana_value(SearchManaValue::Exact(program.mana_value())),
                        ),
                    }],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "transfigure" if !is_spell => {
            use spellwake_engine::program::SearchBattlefieldPostEntry;
            let (cost, 0, hybrids, snow) = param
                .and_then(parse_mana_cost_with_snow)
                .ok_or("oracle:transfigure-cost")?
            else {
                return Err("oracle:transfigure-cost");
            };
            let id = ids.activated();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz_snow) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz_snow)]);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::Sacrifice,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::SearchLibrary {
                        filter: Box::new(
                            LibrarySearch::to_battlefield(
                                SearchCardMatcher::CardTypes(BTreeSet::from([CardType::Creature])),
                                false,
                            )
                            .with_destination(SearchDestination::Battlefield {
                                tapped: false,
                                post_entry: SearchBattlefieldPostEntry::Transfigure,
                            })
                            .with_mana_value(SearchManaValue::Exact(program.mana_value())),
                        ),
                    }],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "unearth" => {
            let (cost, 0, hybrids, snow) = param
                .and_then(parse_mana_cost_with_snow)
                .ok_or("oracle:unearth-cost")?
            else {
                return Err("oracle:unearth-cost");
            };
            let id = ids.activated();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz_snow) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz_snow)]);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::GraveyardSelf { unearth: true },
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::unearth(),
                    ))],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "crew" => {
            let n: u64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.activated.push(ActivatedAbility::tap_for_power(
                ids.activated(),
                PowerAbility::Crew,
                n,
            ));
            Ok(())
        }
        "saddle" => {
            let n: u64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            let id = ids.activated();
            program
                .activated
                .push(ActivatedAbility::tap_for_power(id, PowerAbility::Saddle, n));
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "station" => {
            let id = ids.activated();
            program.activated.push(ActivatedAbility::tap_for_power(
                id,
                PowerAbility::Station,
                0,
            ));
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "spree" | "tiered" if is_spell => {
            if let ProgramKind::Spell(spell) = &mut program.kind
                && let Some(modes) = spell.modes.as_mut()
                && !modes.mode_costs.is_empty()
            {
                if lower == "tiered" {
                    modes.minimum = 1;
                    modes.maximum = 1;
                }
                Ok(())
            } else {
                Err("oracle:spree-without-modes")
            }
        }
        "exhaust" if param.is_none() => Ok(()),
        "escalate" if is_spell => {
            let mana = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:escalate-cost")?;
            let ProgramKind::Spell(spell) = &mut program.kind else {
                return Err("oracle:escalate-non-spell");
            };
            let Some(modes) = spell.modes.as_mut() else {
                return Err("oracle:escalate-no-modes");
            };
            modes.escalate_mana = Some(mana);
            Ok(())
        }
        "provoke" if !is_spell => {
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::attacks(
                    ObjectFilter::self_object(),
                    AttackTriggerMode::Individual {
                        capture_defender: true,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::filtered_permanent(ObjectFilter::creature().opposing()),
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 1,
                        },
                        super::toolbox_emit::set_targets_tapped(false),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                BlockingRestriction::MustBlockSource,
                            )]),
                            ResolvedDuration::UntilEndOfTurn,
                        ),
                    ],
                ),
            });
            Ok(())
        }
        "recover" => {
            let mana = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:recover-cost")?;
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec {
                    event: TriggerEvent::Dies {
                        subject: ObjectFilter::creature().controlled(),
                        mode: DiesTriggerMode::Creature,
                    },
                    zone: spellwake_engine::program::TriggerZone::Graveyard,
                    intervening_if: None,
                },
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::PromptChoice(Box::new(
                            spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                                ResolvingPlayer::Controller,
                                mana,
                                ValueExpr::Literal(0.into()),
                                1,
                                3,
                            ),
                        )),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::return_source_graveyard_to_hand(),
                        Instruction::Jump { destination: 4 },
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                            MoveSubjectRef::AbilitySource,
                            SubjectDestination::Exile {
                                delayed_return: None,
                            },
                        ),
                    ],
                ),
            });
            Ok(())
        }
        "reinforce" => {
            let p = param.ok_or("oracle:reinforce-param")?;
            let (count_raw, cost_raw) = p
                .split_once('\u{2014}')
                .or_else(|| p.split_once(" - "))
                .or_else(|| p.split_once(' '))
                .ok_or("oracle:reinforce-param")?;
            let n: i64 = count_raw
                .trim()
                .parse()
                .map_err(|_| "oracle:reinforce-count")?;
            if n <= 0 {
                return Err("oracle:reinforce-count");
            }
            let mana_cost =
                parse_fixed_mana_cost(cost_raw.trim()).ok_or("oracle:reinforce-cost")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Reinforce),
                body: ExecutableBody::new(
                    BodyTarget::SINGLE_CREATURE,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                        CounterKind::PlusOne,
                        ValueExpr::Literal(n.into()),
                    )],
                ),
            });
            Ok(())
        }
        "ninjutsu" | "commander ninjutsu" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:ninjutsu-cost")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::Ninjutsu {
                    from_command: lower == "commander ninjutsu",
                },
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::ninjutsu(),
                    ))],
                ),
            });
            Ok(())
        }
        "flashback" => lower_flashback_keyword(param, program),
        "madness" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:madness-cost")?;
            if program.madness.replace(cost).is_some() {
                return Err("oracle:duplicate-madness");
            }
            Ok(())
        }
        "plot" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:plot-cost")?;
            if program.plot.replace(cost).is_some() {
                return Err("oracle:duplicate-plot");
            }
            Ok(())
        }
        "foretell" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:foretell-cost")?;
            if program
                .exile_action
                .replace(ExileAction::Foretell(cost))
                .is_some()
            {
                return Err("oracle:duplicate-exile-action");
            }
            Ok(())
        }
        "suspend" => {
            let p = param.ok_or("oracle:suspend-param")?;
            let (n_str, cost_str) = p
                .split_once('\u{2014}')
                .or_else(|| p.split_once('-'))
                .ok_or("oracle:suspend-param")?;
            let counters: u16 = n_str.trim().parse().map_err(|_| "oracle:suspend-count")?;
            let price = parse_fixed_mana_cost(cost_str.trim()).ok_or("oracle:suspend-cost")?;
            if program
                .exile_action
                .replace(ExileAction::Suspend { counters, price })
                .is_some()
            {
                return Err("oracle:duplicate-exile-action");
            }
            Ok(())
        }
        "kicker" => {
            let p = param.ok_or("oracle:kicker-non-mana")?;
            let trimmed = p
                .trim()
                .trim_start_matches(['\u{2014}', '\u{2013}', '-', ':'])
                .trim()
                .trim_end_matches('.')
                .trim();
            if trimmed.is_empty() {
                return Err("oracle:kicker-non-mana");
            }
            let branches = split_kicker_and_or(trimmed);
            let mut lowered_costs = Vec::with_capacity(branches.len());
            for raw_branch in branches {
                let branch = raw_branch
                    .trim()
                    .trim_start_matches(['\u{2014}', '\u{2013}', '-', ':'])
                    .trim()
                    .trim_end_matches('.')
                    .trim();
                if branch.is_empty() {
                    return Err("oracle:kicker-non-mana");
                }
                lowered_costs.push(lower_single_kicker_branch(branch, vocabulary)?);
            }
            program.additional_cast_costs.extend(lowered_costs);
            Ok(())
        }
        "multikicker" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:multikicker-non-mana")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Multikicker(cost));
            Ok(())
        }
        "buyback" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:buyback-non-mana")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Buyback(cost));
            Ok(())
        }
        "entwine" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:entwine-non-mana")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Entwine(cost));
            Ok(())
        }
        "replicate" if !program.has_type(CardType::Land) => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:replicate-non-mana")?;
            let idx = program.additional_cast_costs.len();
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Replicate(cost));
            program.triggered.push(TriggeredAbility::spell_copies(
                ids.triggered(),
                CastCopyKeyword::Replicate(idx),
            ));
            Ok(())
        }
        "evoke" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:evoke-cost")?;
            if program.conditional_cost.is_some() {
                return Err("oracle:duplicate-conditional-cost");
            }
            let tid = ids.triggered();
            *program = std::mem::replace(program, Program::enchantment(None)).with_evoke(cost, tid);
            Ok(())
        }
        "dash" | "blitz" | "warp" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:endstep-cast-cost")?;
            if program.conditional_cost.is_some() {
                return Err("oracle:duplicate-conditional-cost");
            }
            let kind = match lower.as_str() {
                "dash" => EndStepCast::Dash,
                "blitz" => EndStepCast::Blitz,
                _ => EndStepCast::Warp,
            };
            let tid = ids.triggered();
            *program = std::mem::replace(program, Program::enchantment(None))
                .with_end_step_cast(kind, cost, tid);
            Ok(())
        }
        "bestow" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:bestow-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Bestow(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "surge" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:surge-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Surge(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "spectacle" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:spectacle-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Spectacle(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "prowl" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:prowl-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Prowl(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "freerunning" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:freerunning-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Freerunning(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "morph" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:morph-non-mana")?;
            program.face_up_abilities.push(FaceUpAbility::Morph(cost));
            Ok(())
        }
        "megamorph" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:megamorph-non-mana")?;
            program
                .face_up_abilities
                .push(FaceUpAbility::Megamorph(cost));
            Ok(())
        }
        "disguise" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:disguise-non-mana")?;
            program
                .face_up_abilities
                .push(FaceUpAbility::Disguise(cost));
            Ok(())
        }
        "undaunted" => {
            program.cast_reductions.push(CastReduction::Undaunted);
            Ok(())
        }
        "affinity" => lower_affinity_keyword(param, program, vocabulary),
        "ascend" => {
            if is_spell {
                let ProgramKind::Spell(spell) = &mut program.kind else {
                    return Err("oracle:ascend-not-spell");
                };
                if let Some(modes) = &mut spell.modes {
                    if let Some(prefix) = &mut modes.prefix {
                        prefix.instructions.insert(
                            0,
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::ascend(),
                        );
                        for op in &mut prefix.instructions[1..] {
                            op.shift_jumps(1, 0);
                        }
                    } else {
                        modes.prefix = Some(Box::new(ExecutableBody::new(
                            BodyTarget::None,
                            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::ascend()],
                        )));
                    }
                } else {
                    spell.body.instructions.insert(
                        0,
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::ascend(),
                    );
                    for op in &mut spell.body.instructions[1..] {
                        op.shift_jumps(1, 0);
                    }
                }
                Ok(())
            } else {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Modifier {
                        id: ids.static_id(),
                        rule: StaticModifierRule::Ascend,
                    },
                )?;
                Ok(())
            }
        }
        "increment" => {
            program
                .triggered
                .push(GrowthKeyword::Increment.ability(ids.triggered()));
            Ok(())
        }
        "backup" => {
            // CR 702.165: Backup grants only abilities printed below it. Admit cleanly when
            // no non-keyword abilities/statics/replacements exist and all following keywords
            // lower to supported static KeywordAbility values.
            if !face_ast.abilities.is_empty()
                || !face_ast.statics.is_empty()
                || !face_ast.replacements.is_empty()
                || !face_ast.spell_effects.is_empty()
            {
                return Err("oracle:unsupported-backup-abilities");
            }
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            if n <= 0 {
                return Err("oracle:kw-param");
            }
            let mut printed_below = BTreeSet::new();
            for next_kw in &face_ast.keywords[kw_idx + 1..] {
                if next_kw.name.eq_ignore_ascii_case("backup") {
                    continue;
                }
                if next_kw.parameter.is_some() {
                    return Err("oracle:unsupported-backup-abilities");
                }
                let ka = lower_keyword_ability_name(&next_kw.name)
                    .ok_or("oracle:unsupported-backup-abilities")?;
                printed_below.insert(ka);
            }
            program.triggered.push(
                GrowthKeyword::Backup {
                    amount: n.into(),
                    printed_below,
                }
                .ability(ids.triggered()),
            );
            Ok(())
        }
        "encore" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:encore-cost")?;
            let id = ids.activated();
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: true,
                },
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::encore(),
                    ))],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "escape" => {
            let p = param.ok_or("oracle:escape-param")?;
            let (mana_raw, rest) = p
                .split_once(", Exile ")
                .or_else(|| p.split_once(", exile "))
                .ok_or("oracle:escape-param")?;
            let mana = parse_fixed_mana_cost(mana_raw.trim()).ok_or("oracle:escape-cost")?;
            let (count_word, tail) = rest.trim().split_once(' ').ok_or("oracle:escape-param")?;
            let exile: u16 = match count_word.to_ascii_lowercase().as_str() {
                "a" | "one" => 1,
                "two" => 2,
                "three" => 3,
                "four" => 4,
                "five" => 5,
                "six" => 6,
                "seven" => 7,
                "eight" => 8,
                other => other.parse().map_err(|_| "oracle:escape-count")?,
            };
            let tail_low = tail.trim().trim_end_matches('.').to_ascii_lowercase();
            if tail_low != "other card from your graveyard"
                && tail_low != "other cards from your graveyard"
            {
                return Err("oracle:escape-param");
            }
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Escape {
                    mana,
                    exile,
                    entry_counters: 0,
                })
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "sneak" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:sneak-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Sneak(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "mayhem" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:mayhem-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Mayhem(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "mutate" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:mutate-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Mutate(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "web-slinging" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:webslinging-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::WebSlinging(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "emerge" => {
            let p = param.ok_or("oracle:emerge-param")?;
            let (from, mana_str) = if let Some(rest) = p.strip_prefix("from ") {
                let (kind_str, cost_str) = rest.split_once(' ').ok_or("oracle:emerge-param")?;
                let ct = match kind_str.to_ascii_lowercase().as_str() {
                    "creature" => CardType::Creature,
                    "artifact" => CardType::Artifact,
                    "enchantment" => CardType::Enchantment,
                    "land" => CardType::Land,
                    _ => return Err("oracle:emerge-type"),
                };
                (ct, cost_str.trim())
            } else {
                (CardType::Creature, p)
            };
            let mana = parse_fixed_mana_cost(mana_str).ok_or("oracle:emerge-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Emerge { mana, from })
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        kw_name @ ("living weapon" | "for mirrodin!" | "job select") if param.is_none() => {
            let (pt, color, labels): (i64, Option<Color>, &[&str]) = match kw_name {
                "living weapon" => (0, Some(Color::Black), &["Phyrexian", "Germ"]),
                "for mirrodin!" => (2, Some(Color::Red), &["Rebel"]),
                "job select" => (1, None, &["Hero"]),
                _ => return Err("oracle:unsupported-keyword"),
            };
            let types = labels
                .iter()
                .map(|word| vocabulary.resolve_label(word))
                .collect::<Option<Vec<_>>>()
                .ok_or("oracle:token-unknown-subtype")?;
            let mut token =
                Program::creature(None, pt.into(), pt.into(), types.iter().copied().collect());
            token.color_indicator = color.map(|c| BTreeSet::from([c]));
            let definition = spellwake_engine::token::TokenDefinition::new(
                token,
                spellwake_engine::token::TokenName::Default {
                    ordered_subtypes: types
                        .into_iter()
                        .map(spellwake_engine::token::TokenSubtype::Creature)
                        .collect(),
                },
                vocabulary,
            )
            .map_err(|_| "oracle:invalid-token-def")?;
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::enters(ObjectFilter::self_object()),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::create_token_and_attach(
                            Box::new(definition),
                        ),
                    ))],
                ),
            });
            Ok(())
        }
        "outlast" => {
            let (cost, 0, hybrids, snow) = param
                .and_then(parse_mana_cost_with_snow)
                .ok_or("oracle:outlast-cost")?
            else {
                return Err("oracle:outlast-cost");
            };
            let id = ids.activated();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz_snow) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz_snow)]);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                        CounterKind::PlusOne,
                        ValueExpr::literal(1),
                    )],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "level up" => {
            let (mana_cost, x, hybrids) = param
                .and_then(parse_mana_cost_full)
                .ok_or("oracle:level-up-cost")?;
            if x > 0 {
                return Err("oracle:level-up-cost");
            }
            let id = ids.activated();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                        CounterKind::Level,
                        ValueExpr::Literal(1.into()),
                    )],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "miracle" => {
            let (mana, x_symbols) = param
                .and_then(parse_mana_cost_with_x)
                .ok_or("oracle:miracle-cost")?;
            if program
                .miracle
                .replace(MiracleCost { mana, x_symbols })
                .is_some()
            {
                return Err("oracle:duplicate-miracle");
            }
            Ok(())
        }
        "splice onto" => {
            if !is_spell {
                return Err("oracle:splice-non-spell");
            }
            let p = param.ok_or("oracle:splice-param")?;
            let (quality, cost_raw) = if let Some(rest) = p
                .strip_prefix("Arcane ")
                .or_else(|| p.strip_prefix("arcane "))
            {
                (SpliceQuality::Arcane, rest)
            } else if let Some(rest) = p
                .strip_prefix("instant or sorcery ")
                .or_else(|| p.strip_prefix("Instant or Sorcery "))
            {
                (SpliceQuality::InstantOrSorcery, rest)
            } else {
                return Err("oracle:splice-quality");
            };
            let cost = parse_fixed_mana_cost(cost_raw.trim()).ok_or("oracle:splice-cost")?;
            if !program.supports_splice_text() {
                return Err("oracle:splice-unsupported-text");
            }
            if program
                .splice
                .replace(SpliceAbility { quality, cost })
                .is_some()
            {
                return Err("oracle:duplicate-splice");
            }
            Ok(())
        }
        "hexproof" | "hexproof_from" => {
            let p = param.ok_or("oracle:unsupported-keyword")?;
            let quals =
                parse_hexproof_from_qualities(p, vocabulary).ok_or("oracle:unsupported-keyword")?;
            for q in quals {
                program
                    .printed_keywords
                    .insert(KeywordAbility::HexproofFrom(q));
            }
            Ok(())
        }
        s if s.starts_with("hexproof from ") && param.is_none() => {
            let quals =
                parse_hexproof_from_qualities(s.strip_prefix("hexproof ").unwrap_or(s), vocabulary)
                    .ok_or("oracle:unsupported-keyword")?;
            for q in quals {
                program
                    .printed_keywords
                    .insert(KeywordAbility::HexproofFrom(q));
            }
            Ok(())
        }
        s if s == "friends forever" || s.starts_with("partner\u{2014}") => {
            program.printed_keywords.insert(KeywordAbility::Partner);
            Ok(())
        }
        "partner with" | "partner" => {
            let raw = param.ok_or("oracle:unsupported-keyword")?;
            let partner_raw = if lower == "partner" {
                raw.strip_prefix("with ")
                    .or_else(|| raw.strip_prefix("With "))
                    .ok_or("oracle:unsupported-keyword")?
            } else {
                raw
            };
            let partner_name = partner_raw
                .split('(')
                .next()
                .unwrap_or(partner_raw)
                .trim()
                .trim_end_matches('.');
            if partner_name.is_empty() {
                return Err("oracle:unsupported-keyword");
            }
            let partner = EnglishCardName::new(partner_name.to_string())
                .map_err(|_| "oracle:unsupported-keyword")?;
            program.printed_keywords.insert(KeywordAbility::Partner);
            let search = LibrarySearch::to_hand(SearchCardMatcher::AnyOfNames(vec![partner]))
                .with_searcher(ResolvingPlayer::TargetPlayer);
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::self_enters(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::SINGLE_PLAYER,
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::TargetPlayer,
                            instructions: 1,
                        },
                        Instruction::SearchLibrary {
                            filter: Box::new(search),
                        },
                    ],
                ),
            });
            Ok(())
        }
        "unleash" if param.is_none() && !is_spell => {
            let ProgramKind::Permanent(perm) = &mut program.kind else {
                return Err("oracle:unsupported-keyword");
            };
            if perm.entry_tap.replace(EntryTap::Unleash).is_some() {
                return Err("oracle:duplicate-entry-tap");
            }
            let selector = PermanentSelector {
                controller: ControllerSelector::SourceItself,
                plus_one_counter: true,
                ..PermanentSelector::default()
            };
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ids.static_id(),
                    selector,
                    parts: StaticParts(vec![StaticPart::BlockingRestriction(
                        BlockingRestriction::CannotBlock,
                    )]),
                },
            )
        }
        "riot" if param.is_none() && !is_spell => {
            let ProgramKind::Permanent(perm) = &mut program.kind else {
                return Err("oracle:unsupported-keyword");
            };
            if perm.entry_tap.replace(EntryTap::Riot).is_some() {
                return Err("oracle:duplicate-entry-tap");
            }
            Ok(())
        }
        "living metal" if param.is_none() && !is_spell => {
            let selector = PermanentSelector {
                controller: ControllerSelector::SourceItself,
                source_condition: Some(StaticSourceCondition::ControllersTurn),
                ..PermanentSelector::default()
            };
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ids.static_id(),
                    selector,
                    parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                        subtypes: SubtypeOperation::Keep,
                    })]),
                },
            )
        }
        "firebending" => {
            let p = param.ok_or("oracle:kw-param")?;
            let p_clean = p.trim().trim_end_matches('.');
            let amount = if p_clean.eq_ignore_ascii_case("x")
                || p_clean.to_ascii_lowercase().starts_with("x, where x is ")
                    && p_clean.to_ascii_lowercase().contains("power")
            {
                ValueExpr::ObjectStat {
                    subject: ValueObjectRef::AbilitySource,
                    stat: ObjectStatKind::Power,
                }
            } else {
                let n: i64 = p_clean.parse().map_err(|_| "oracle:kw-param")?;
                if n <= 0 {
                    return Err("oracle:kw-param");
                }
                ValueExpr::literal(n)
            };
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::attacks(
                    ObjectFilter::self_object(),
                    AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                            ResolvingPlayer::Controller,
                            EffectManaYield::RetainedUntilEndOfCombat {
                                kind: ManaType::Red,
                                amount,
                            },
                        ),
                    ))],
                ),
            });
            Ok(())
        }
        "offspring" if !is_spell => {
            let p_str = param.ok_or("oracle:offspring-cost")?;
            let mana = parse_fixed_mana_cost(p_str).ok_or("oracle:offspring-cost")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Kicker(mana));
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::new(TriggerEvent::Enters {
                    subject: ObjectFilter::self_object(),
                    from_zone: TriggerFromZone::Any,
                    cast_origin: Some(CastEntryKind::Kicked),
                    attached_to_creature: false,
                }),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::create_token_copy(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            Box::new(spellwake_engine::toolbox::TokenCopySpec {
                                count: ValueExpr::literal(1),
                                recipients: TokenRecipients::Controller,
                                enters_tapped: false,
                                enters_attacking: false,
                                haste: false,
                                sacrifice_at_end_step: false,
                                exile_at_end_step: false,
                                nonlegendary: false,
                                extra_types: BTreeSet::new(),
                                extra_subtypes: BTreeSet::new(),
                                extra_keywords: BTreeSet::new(),
                                override_pt: Some((1.into(), 1.into())),
                                extra_colors: BTreeSet::new(),
                                override_colors: None,
                                strip_mana_cost: false,
                                defender_scope:
                                    spellwake_engine::token::AttackingDefenderScope::AnyLegal,
                                extra_activated: Vec::new(),
                                extra_triggered: Vec::new(),
                                extra_static: Vec::new(),
                                extra_mana: Vec::new(),
                                extra_counters: Vec::new(),
                                keep_name: false,
                            }),
                        ),
                    ))],
                ),
            });
            Ok(())
        }
        "squad" if !is_spell => {
            let p_str = param.ok_or("oracle:squad-cost")?;
            let mana = parse_fixed_mana_cost(p_str).ok_or("oracle:squad-cost")?;
            let idx = program.additional_cast_costs.len();
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Multikicker(mana));
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::new(TriggerEvent::Enters {
                    subject: ObjectFilter::self_object(),
                    from_zone: TriggerFromZone::Any,
                    cast_origin: Some(CastEntryKind::Kicked),
                    attached_to_creature: false,
                }),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::create_token_copy(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            Box::new(spellwake_engine::toolbox::TokenCopySpec {
                                count: ValueExpr::event(EventValueKind::EnteredKickerCount(idx)),
                                recipients: TokenRecipients::Controller,
                                enters_tapped: false,
                                enters_attacking: false,
                                haste: false,
                                sacrifice_at_end_step: false,
                                exile_at_end_step: false,
                                nonlegendary: false,
                                extra_types: BTreeSet::new(),
                                extra_subtypes: BTreeSet::new(),
                                extra_keywords: BTreeSet::new(),
                                override_pt: None,
                                extra_colors: BTreeSet::new(),
                                override_colors: None,
                                strip_mana_cost: false,
                                defender_scope:
                                    spellwake_engine::token::AttackingDefenderScope::AnyLegal,
                                extra_activated: Vec::new(),
                                extra_triggered: Vec::new(),
                                extra_static: Vec::new(),
                                extra_mana: Vec::new(),
                                extra_counters: Vec::new(),
                                keep_name: false,
                            }),
                        ),
                    ))],
                ),
            });
            Ok(())
        }
        "embalm" | "eternalize" if !is_spell => {
            let p_str = param.ok_or("oracle:embalm-cost")?;
            let (mana_cost, hybrids, extra_costs) = if let Some((mc, 0, hybs)) =
                parse_mana_cost_full(p_str)
            {
                (mc, hybs, Vec::new())
            } else {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let atoms = spellwake_oracle_compiler::clauses::parse_cost_list(
                    p_str.trim_start_matches('\u{2014}').trim(),
                    &compiler_vocab,
                )
                .map_err(|_| "oracle:embalm-cost")?;
                let (mc, sc, hybs, extras) = lower_activation_cost_full(&atoms, None, vocabulary)
                    .map_err(|_| "oracle:embalm-cost")?;
                if sc != SourceCost::None {
                    return Err("oracle:embalm-cost");
                }
                (mc, hybs, extras)
            };
            let zombie = vocabulary
                .resolve_label("Zombie")
                .ok_or("oracle:token-unknown-subtype")?;
            let is_eternalize = lower == "eternalize";
            let id = ids.activated();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if !extra_costs.is_empty() {
                program.extra_activation_costs.insert(id, extra_costs);
            }
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost,
                source_cost: SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: true,
                },
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::create_token_copy(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            Box::new(spellwake_engine::toolbox::TokenCopySpec {
                                count: ValueExpr::literal(1),
                                recipients: TokenRecipients::Controller,
                                enters_tapped: false,
                                enters_attacking: false,
                                haste: false,
                                sacrifice_at_end_step: false,
                                exile_at_end_step: false,
                                nonlegendary: false,
                                extra_types: BTreeSet::new(),
                                extra_subtypes: BTreeSet::from([zombie]),
                                extra_keywords: BTreeSet::new(),
                                override_pt: is_eternalize.then_some((4.into(), 4.into())),
                                extra_colors: BTreeSet::new(),
                                override_colors: Some(BTreeSet::from([if is_eternalize {
                                    Color::Black
                                } else {
                                    Color::White
                                }])),
                                strip_mana_cost: true,
                                defender_scope:
                                    spellwake_engine::token::AttackingDefenderScope::AnyLegal,
                                extra_activated: Vec::new(),
                                extra_triggered: Vec::new(),
                                extra_static: Vec::new(),
                                extra_mana: Vec::new(),
                                extra_counters: Vec::new(),
                                keep_name: false,
                            }),
                        ),
                    ))],
                ),
            });
            Ok(())
        }
        "overload" => {
            if !is_spell || face_ast.spell_effects.is_empty() {
                return Err("oracle:overload-non-spell");
            }
            let mana = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:overload-cost")?;
            let mut transformed = Vec::with_capacity(face_ast.spell_effects.len());
            let mut changed_any = false;
            for ins in &face_ast.spell_effects {
                let (next, changed) = replace_targets_with_each_ins(ins)?;
                changed_any |= changed;
                transformed.push(next);
            }
            if !changed_any {
                return Err("oracle:overload-no-target");
            }
            let each_body = lower_effect_body(&transformed, vocabulary, ids, false, false)?;
            if each_body.target != BodyTarget::None {
                return Err("oracle:overload-remaining-target");
            }
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Overload {
                    mana,
                    each: Box::new(each_body),
                })
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "awaken" => {
            let p = param.ok_or("oracle:awaken-param")?;
            let (n_str, cost_str) = p
                .split_once('\u{2014}')
                .or_else(|| p.split_once(" - "))
                .or_else(|| p.split_once('-'))
                .or_else(|| p.split_once(' '))
                .ok_or("oracle:awaken-param")?;
            let counters: u8 = n_str.trim().parse().map_err(|_| "oracle:awaken-count")?;
            if counters == 0 {
                return Err("oracle:awaken-count");
            }
            let mana = parse_fixed_mana_cost(cost_str.trim()).ok_or("oracle:awaken-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Awaken { mana, counters })
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "impending" if !is_spell => {
            let p = param.ok_or("oracle:impending-param")?;
            let (n_str, cost_str) = p
                .split_once('\u{2014}')
                .or_else(|| p.split_once(" - "))
                .or_else(|| p.split_once('-'))
                .or_else(|| p.split_once(' '))
                .ok_or("oracle:impending-param")?;
            let counters: u8 = n_str.trim().parse().map_err(|_| "oracle:impending-count")?;
            if counters == 0 {
                return Err("oracle:impending-count");
            }
            let mana = parse_fixed_mana_cost(cost_str.trim()).ok_or("oracle:impending-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Impending { mana, counters })
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            let selector = PermanentSelector {
                controller: ControllerSelector::SourceItself,
                source_condition: Some(StaticSourceCondition::SourceCounterAtLeast {
                    kind: CounterKind::Time,
                    minimum: 1,
                }),
                ..PermanentSelector::default()
            };
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ids.static_id(),
                    selector,
                    parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
                        subtypes: SubtypeOperation::Keep,
                    })]),
                },
            )?;
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::step_began(Step::End, PlayerRelation::Controller).only_if(
                    TriggerCondition::SourceCounterAtLeast {
                        kind: CounterKind::Time,
                        minimum: 1,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            Some(CounterKind::Time),
                            ValueExpr::literal(1),
                        ),
                    ],
                ),
            });
            Ok(())
        }
        "harmonize" => {
            let mana = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:harmonize-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Harmonize(mana))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "offering" => {
            let p = param.ok_or("oracle:offering-param")?;
            let subtype = resolve_protection_subtype(p, vocabulary)
                .or_else(|| vocabulary.resolve_label(p))
                .or_else(|| {
                    let titled: Vec<String> = p
                        .split_whitespace()
                        .map(|word| {
                            let mut chars = word.chars();
                            match chars.next() {
                                Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
                                None => String::new(),
                            }
                        })
                        .collect();
                    vocabulary.resolve_label(&titled.join(" "))
                })
                .ok_or("oracle:offering-subtype")?;
            let mana = program
                .printed_mana_cost()
                .cloned()
                .ok_or("oracle:offering-no-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Offering { mana, subtype })
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "assist" if param.is_none() => {
            program.assist = true;
            Ok(())
        }
        "gift" | "gift a" | "gift an" => {
            let raw = param.ok_or("oracle:gift-param")?;
            let norm = raw.trim().trim_end_matches('.').to_ascii_lowercase();
            let norm = norm
                .strip_prefix("an ")
                .or_else(|| norm.strip_prefix("a "))
                .unwrap_or(&norm)
                .trim();
            let kind = match norm {
                "card" => spellwake_engine::program::GiftKind::Card,
                "fish" => spellwake_engine::program::GiftKind::Fish,
                "food" => spellwake_engine::program::GiftKind::Food,
                "treasure" => spellwake_engine::program::GiftKind::Treasure,
                "tapped fish" => spellwake_engine::program::GiftKind::TappedFish,
                "extra turn" => spellwake_engine::program::GiftKind::ExtraTurn,
                _ => return Err("oracle:unsupported-gift"),
            };
            if program.gift.replace(kind).is_some() {
                return Err("oracle:duplicate-gift");
            }
            Ok(())
        }
        "dredge" => {
            let n: u8 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            if n == 0 {
                return Err("oracle:kw-param");
            }
            if program.dredge.replace(n).is_some() {
                return Err("oracle:duplicate-dredge");
            }
            Ok(())
        }
        "enlist" if param.is_none() && !is_spell => {
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::self_attacks(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::enlist(),
                    ))],
                ),
            });
            Ok(())
        }
        "cipher" if param.is_none() => {
            program.cipher = true;
            Ok(())
        }
        "conspire" if param.is_none() => {
            program.conspire = true;
            Ok(())
        }
        "casualty" => {
            let n: u8 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            if n == 0 {
                return Err("oracle:kw-param");
            }
            if program.casualty.replace(n).is_some() {
                return Err("oracle:duplicate-casualty");
            }
            Ok(())
        }
        "tribute" if !is_spell => {
            let n: u8 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            if n == 0 {
                return Err("oracle:kw-param");
            }
            let ProgramKind::Permanent(def) = &mut program.kind else {
                return Err("oracle:unsupported-keyword");
            };
            if def.entry_tap.replace(EntryTap::Tribute(n)).is_some() {
                return Err("oracle:duplicate-entry-tap");
            }
            Ok(())
        }
        "amplify" if !is_spell => {
            let n: u8 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            if n == 0 {
                return Err("oracle:kw-param");
            }
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::Amplify(n));
            Ok(())
        }
        "ripple" => {
            let n: u8 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            if n == 0 {
                return Err("oracle:kw-param");
            }
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::self_cast(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::ripple(n),
                    ))],
                ),
            });
            Ok(())
        }
        "demonstrate" if param.is_none() => {
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::self_cast(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::demonstrate(),
                    ))],
                ),
            });
            Ok(())
        }
        "champion" | "champion a" | "champion an" if !is_spell => {
            lower_champion_keyword(param, program, vocabulary, ids)
        }
        "hideaway" if !is_spell => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            if n <= 0 {
                return Err("oracle:kw-param");
            }
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::self_enters(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    spellwake_engine::toolbox::ZoneOpSpec::look_top_choose_instructions(
                        ResolvingPlayer::Controller,
                        ResolvingPlayer::Controller,
                        ValueExpr::literal(n),
                        None,
                        1,
                        false,
                        spellwake_engine::program::LookTopKeepDest::ExileLinked {
                            face_down: true,
                            playable: false,
                            free: false,
                            cast_only: false,
                            any_color: false,
                            duration: ResolvedDuration::NoStatedDuration,
                        },
                        spellwake_engine::program::LookTopRestDest::BottomRandom,
                        false,
                        false,
                        false,
                        None,
                    ),
                ),
            });
            let has_look_at_exiled =
                matches!(
                    &program.continuous,
                    Some(StaticContinuousAbility::CastPermission {
                        permission: CastPermissionRule::LookAtSourceExiledCards,
                        ..
                    })
                ) || program.additional_battlefield_abilities.iter().any(|ab| {
                    matches!(
                        ab,
                        StaticContinuousAbility::CastPermission {
                            permission: CastPermissionRule::LookAtSourceExiledCards,
                            ..
                        }
                    )
                });
            if !has_look_at_exiled {
                push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::LookAtSourceExiledCards,
                    },
                )?;
            }
            Ok(())
        }
        "absorb" if !is_spell => {
            let n: u8 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            if n == 0 {
                return Err("oracle:kw-param");
            }
            push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::ModifyDamage {
                        source: None,
                        source_controller: PlayerRelation::AnyParticipant,
                        recipient: StaticDamageRecipient::SelfObject,
                        kind: StaticDamageKind::Any,
                        op: StaticDamageOp::PreventUpTo(n),
                    },
                },
            )
        }
        "bands with other" => {
            let raw = param
                .ok_or("oracle:unsupported-keyword")?
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            let quality = match raw.as_str() {
                "legendary creatures" => spellwake_engine::combat::BandQuality::LegendaryCreatures,
                "creatures named wolves of the hunt" => {
                    spellwake_engine::combat::BandQuality::CreaturesNamedWolvesOfTheHunt
                }
                _ => return Err("oracle:unsupported-keyword"),
            };
            program
                .printed_keywords
                .insert(KeywordAbility::BandsWithOther(quality));
            Ok(())
        }
        _ => Err("oracle:unsupported-keyword"),
    }
}

pub(super) fn replace_targets_with_each_subject(
    subj: &SubjectRefAst,
) -> Result<(SubjectRefAst, bool), &'static str> {
    match subj {
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if filter.zone.is_none() && !filter.any_spell => {
            Ok((SubjectRefAst::Each(filter.clone()), true))
        }
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => Ok((
            SubjectRefAst::Player(if *opponent_only {
                PlayerRefAst::EachOpponent
            } else {
                PlayerRefAst::EachPlayer
            }),
            true,
        )),
        SubjectRefAst::TargetObject { .. }
        | SubjectRefAst::TargetPlayer { .. }
        | SubjectRefAst::AnyTarget
        | SubjectRefAst::TargetPlayerOrPlaneswalker { .. } => {
            Err("oracle:overload-unsupported-target")
        }
        other => Ok((other.clone(), false)),
    }
}

pub(super) fn replace_targets_with_each_player(p: PlayerRefAst) -> (PlayerRefAst, bool) {
    match p {
        PlayerRefAst::TargetPlayer => (PlayerRefAst::EachPlayer, true),
        PlayerRefAst::TargetOpponent => (PlayerRefAst::EachOpponent, true),
        other => (other, false),
    }
}

pub(super) fn replace_targets_with_each_ins(
    ins: &InstructionAst,
) -> Result<(InstructionAst, bool), &'static str> {
    match ins {
        InstructionAst::Destroy {
            subject,
            random,
            except,
        } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::Destroy {
                    subject: Box::new(s),
                    random: *random,
                    except: except.clone(),
                },
                c,
            ))
        }
        InstructionAst::Exile {
            actor,
            subject,
            random,
            face_down,
            into_pile,
        } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::Exile {
                    actor: *actor,
                    subject: Box::new(s),
                    random: *random,
                    face_down: *face_down,
                    into_pile: *into_pile,
                },
                c,
            ))
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped,
            transformed,
            with_counter,
            attached_to,
            random,
            except,
            count,
            up_to,
            any_number,
            actor,
        } if attached_to.is_none() => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::ReturnZone {
                    subject: Box::new(s),
                    from_zone: from_zone.clone(),
                    to: to.clone(),
                    tapped: *tapped,
                    transformed: *transformed,
                    with_counter: with_counter.clone(),
                    attached_to: None,
                    random: *random,
                    except: except.clone(),
                    count: count.clone(),
                    up_to: *up_to,
                    any_number: *any_number,
                    actor: *actor,
                },
                c,
            ))
        }
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => {
            let (t, c) = replace_targets_with_each_subject(target)?;
            Ok((
                InstructionAst::DealDamage {
                    source: source.clone(),
                    target: Box::new(t),
                    amount: amount.clone(),
                },
                c,
            ))
        }
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale,
        } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::ModifyPT {
                    subject: Box::new(s),
                    dx: dx.clone(),
                    dy: dy.clone(),
                    duration: duration.clone(),
                    scale: scale.clone(),
                },
                c,
            ))
        }
        InstructionAst::GrantKeywords {
            subject,
            keywords,
            duration,
        } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::GrantKeywords {
                    subject: Box::new(s),
                    keywords: keywords.clone(),
                    duration: duration.clone(),
                },
                c,
            ))
        }
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora,
            random,
        } => {
            let (p, c) = replace_targets_with_each_player(*player);
            Ok((
                InstructionAst::Discard {
                    player: p,
                    count: count.clone(),
                    filter: filter.clone(),
                    anaphora: anaphora.clone(),
                    random: *random,
                },
                c,
            ))
        }
        InstructionAst::Draw {
            player,
            count,
            up_to,
        } => {
            let (p, c) = replace_targets_with_each_player(*player);
            Ok((
                InstructionAst::Draw {
                    player: p,
                    count: count.clone(),
                    up_to: *up_to,
                },
                c,
            ))
        }
        InstructionAst::Mill { player, count } => {
            let (p, c) = replace_targets_with_each_player(*player);
            Ok((
                InstructionAst::Mill {
                    player: p,
                    count: count.clone(),
                },
                c,
            ))
        }
        InstructionAst::TapPermanent {
            subject,
            any_number,
        } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::TapPermanent {
                    subject: Box::new(s),
                    any_number: *any_number,
                },
                c,
            ))
        }
        InstructionAst::UntapPermanent {
            subject,
            count,
            up_to,
        } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::UntapPermanent {
                    subject: Box::new(s),
                    count: count.clone(),
                    up_to: *up_to,
                },
                c,
            ))
        }
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::PutCounters {
                    subject: Box::new(s),
                    counter_kind: counter_kind.clone(),
                    count: count.clone(),
                },
                c,
            ))
        }
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } => {
            let (p, c) = replace_targets_with_each_player(*player);
            Ok((
                InstructionAst::ChangeLife {
                    player: p,
                    delta: delta.clone(),
                    gain: *gain,
                },
                c,
            ))
        }
        InstructionAst::SetSuspected { subject, remove } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::SetSuspected {
                    subject: Box::new(s),
                    remove: *remove,
                },
                c,
            ))
        }
        InstructionAst::TurnFaceDown { subject } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::TurnFaceDown {
                    subject: Box::new(s),
                },
                c,
            ))
        }
        InstructionAst::BindXY {
            x_expr,
            y_expr,
            body,
        } => {
            if overload_value_expr_has_target(x_expr) || overload_value_expr_has_target(y_expr) {
                return Err("oracle:overload-unsupported-target");
            }
            let (new_body, c) = replace_targets_with_each_body(body)?;
            Ok((
                InstructionAst::BindXY {
                    x_expr: x_expr.clone(),
                    y_expr: y_expr.clone(),
                    body: new_body,
                },
                c,
            ))
        }
        InstructionAst::ForAsLongAs { condition, body } => {
            if overload_condition_has_target(condition) {
                return Err("oracle:overload-unsupported-target");
            }
            let (new_body, c) = replace_targets_with_each_body(body)?;
            Ok((
                InstructionAst::ForAsLongAs {
                    condition: condition.clone(),
                    body: new_body,
                },
                c,
            ))
        }
        InstructionAst::ForEachSubjectPlayer { players, body } => {
            let (p, c_players) = replace_targets_with_each_subject(players)?;
            let (new_body, c_body) = replace_targets_with_each_body(body)?;
            Ok((
                InstructionAst::ForEachSubjectPlayer {
                    players: Box::new(p),
                    body: new_body,
                },
                c_players || c_body,
            ))
        }
        _ => Err("oracle:overload-unsupported-ins"),
    }
}

fn replace_targets_with_each_body(
    body: &[InstructionAst],
) -> Result<(Vec<InstructionAst>, bool), &'static str> {
    let mut out = Vec::with_capacity(body.len());
    let mut changed = false;
    for ins in body {
        let (next, c) = replace_targets_with_each_ins(ins)?;
        changed |= c;
        out.push(next);
    }
    Ok((out, changed))
}

fn overload_player_is_target(player: PlayerRefAst) -> bool {
    matches!(
        player,
        PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
    )
}

fn overload_pile_has_target(pile: &spellwake_oracle_compiler::ast::PileRefAst) -> bool {
    match pile {
        spellwake_oracle_compiler::ast::PileRefAst::FilteredInPile { filter, pile } => {
            overload_filter_has_target(filter) || overload_pile_has_target(pile)
        }
        _ => false,
    }
}

fn overload_filter_has_target(filter: &ObjectFilterAst) -> bool {
    if matches!(
        filter.controller,
        Some(ControllerRelation::TargetOpponent | ControllerRelation::TargetPlayer)
    ) {
        return true;
    }
    if matches!(
        filter.zone,
        Some(
            ZoneFilter::TargetOpponentGraveyard
                | ZoneFilter::TargetPlayerGraveyard
                | ZoneFilter::TargetPlayerHand
                | ZoneFilter::TargetOpponentHand
                | ZoneFilter::TargetPlayerLibrary
                | ZoneFilter::TargetOpponentLibrary
        )
    ) {
        return true;
    }
    if matches!(
        &filter.named,
        Some(spellwake_oracle_compiler::ast::NameFilterAst::SameAs(subject))
            if overload_subject_has_target(subject)
    ) {
        return true;
    }
    filter
        .stat_bounds
        .iter()
        .any(|b| overload_value_expr_has_target(&b.value))
        || filter.predicates.iter().any(|pred| match pred {
            spellwake_oracle_compiler::ast::FilterPredicateAst::CouldAttachTo(s)
            | spellwake_oracle_compiler::ast::FilterPredicateAst::DifferentNameFrom(s) => {
                overload_subject_has_target(s)
            }
            spellwake_oracle_compiler::ast::FilterPredicateAst::SharesPropertyWith {
                reference,
                ..
            } => overload_subject_has_target(reference),
            spellwake_oracle_compiler::ast::FilterPredicateAst::StatRelationTo { expr, .. } => {
                overload_value_expr_has_target(expr)
            }
            spellwake_oracle_compiler::ast::FilterPredicateAst::AttachedBy(f) => {
                overload_filter_has_target(f)
            }
            _ => false,
        })
}

fn overload_subject_has_target(subj: &SubjectRefAst) -> bool {
    match subj {
        SubjectRefAst::AnyTarget
        | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
        | SubjectRefAst::TargetPlayer { .. }
        | SubjectRefAst::TargetObject { .. }
        | SubjectRefAst::TargetOneOf { .. }
        | SubjectRefAst::DistinctTarget { .. } => true,
        SubjectRefAst::Player(p) => overload_player_is_target(*p),
        SubjectRefAst::PlayerMatching { player, condition } => {
            overload_player_is_target(*player) || overload_condition_has_target(condition)
        }
        SubjectRefAst::InZone { subject, .. }
        | SubjectRefAst::ControllerOf(subject)
        | SubjectRefAst::OwnerOf(subject) => overload_subject_has_target(subject),
        SubjectRefAst::Each(f) | SubjectRefAst::All(f) | SubjectRefAst::Group(f) => {
            overload_filter_has_target(f)
        }
        SubjectRefAst::Compound(items) => items.iter().any(overload_subject_has_target),
        SubjectRefAst::PileRef(pile) => overload_pile_has_target(pile),
        SubjectRefAst::ManualTabletopSelection { filter, .. } => {
            filter.as_deref().is_some_and(overload_filter_has_target)
        }
        SubjectRefAst::SelfObject
        | SubjectRefAst::Anaphora(_)
        | SubjectRefAst::AttachedHost(_)
        | SubjectRefAst::CraftExiledCards
        | SubjectRefAst::TriggerSubject
        | SubjectRefAst::TriggerCause => false,
    }
}

fn overload_value_expr_has_target(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ObjectStat { subject, .. }
        | ValueExprAst::CounterCount { subject, .. }
        | ValueExprAst::ColorsOfManaSpentOn(subject)
        | ValueExprAst::ColorPipsIn { subject, .. } => overload_subject_has_target(subject),
        ValueExprAst::PlayerStat { player, .. }
        | ValueExprAst::PlayerCounterCount { player, .. }
        | ValueExprAst::StartingLifeTotal(player)
        | ValueExprAst::LifeChangedThisTurn { player, .. } => overload_player_is_target(*player),
        ValueExprAst::EventCountThisTurn { player, .. } => {
            player.is_some_and(overload_player_is_target)
        }
        ValueExprAst::Count(f)
        | ValueExprAst::AggregateStat { filter: f, .. }
        | ValueExprAst::ColorsAmong(f)
        | ValueExprAst::CardTypesAmong(f)
        | ValueExprAst::SubtypesAmong(f)
        | ValueExprAst::ExtremeStatAmong { filter: f, .. }
        | ValueExprAst::DistinctCountAmong { filter: f, .. }
        | ValueExprAst::MaxSharedSubtypeCount { filter: f }
        | ValueExprAst::CounterCountAmong { filter: f, .. } => overload_filter_has_target(f),
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::AbsDiff(a, b) => {
            overload_value_expr_has_target(a) || overload_value_expr_has_target(b)
        }
        ValueExprAst::DivConst { expr, .. } => overload_value_expr_has_target(expr),
        ValueExprAst::AggregateOverPlayers { metric, .. } => overload_value_expr_has_target(metric),
        ValueExprAst::Pow { base, exp } => {
            overload_value_expr_has_target(base) || overload_value_expr_has_target(exp)
        }
        ValueExprAst::Shortfall { threshold, actual } => {
            overload_value_expr_has_target(threshold) || overload_value_expr_has_target(actual)
        }
        _ => false,
    }
}

fn overload_marker_has_target(marker: &GameMarker) -> bool {
    match marker {
        GameMarker::SameNameAsTarget | GameMarker::TargetsCommander => true,
        GameMarker::Monarch(p)
        | GameMarker::Initiative(p)
        | GameMarker::OwnerHasCardInExile { player: p }
        | GameMarker::PlayerIsPoisoned { player: p }
        | GameMarker::PlayerHasRadCounter { player: p }
        | GameMarker::AttackedWithMinCreaturesThisTurn { player: p, .. }
        | GameMarker::DrewCardsThisTurn { player: p, .. }
        | GameMarker::CastSpellsThisTurn { player: p, .. }
        | GameMarker::PlayerGainedOrLostLifeThisTurn { player: p }
        | GameMarker::LostLifeLastTurn { player: p }
        | GameMarker::DidNotAttackWithCreatureThisTurn { player: p }
        | GameMarker::CreatedTokenThisTurn { player: p }
        | GameMarker::LandOrSpellOutsideHandThisTurn { player: p } => overload_player_is_target(*p),
        GameMarker::PlayerHasMoreHandThan { player, than } => {
            overload_player_is_target(*player) || overload_player_is_target(*than)
        }
        GameMarker::AttackedThisTurn { subject }
        | GameMarker::InZone { subject, .. }
        | GameMarker::ControlledSubject(subject)
        | GameMarker::DidNotAttackThisTurn { subject, .. }
        | GameMarker::AttackedOrBlockedThisTurn { subject }
        | GameMarker::AttackedThisCombat { subject }
        | GameMarker::SubjectEnteredThisTurn(subject)
        | GameMarker::SubjectWasCast { subject, .. }
        | GameMarker::DamageDealtToSubjectThisTurn { subject, .. }
        | GameMarker::SubjectHarnessed(subject) => overload_subject_has_target(subject),
        GameMarker::GreaterPowerOrToughnessThan { subject, other } => {
            overload_subject_has_target(subject) || overload_subject_has_target(other)
        }
        GameMarker::PutCountersOnSubjectThisTurn {
            player, subject, ..
        } => overload_player_is_target(*player) || overload_subject_has_target(subject),
        GameMarker::SacrificedThisTurn { player, filter }
        | GameMarker::ControlsNamed { player, filter, .. }
        | GameMarker::ControlsGreatestStat { player, filter, .. } => {
            overload_player_is_target(*player) || overload_filter_has_target(filter)
        }
        GameMarker::DidNotCastThisTurn { player, filter } => {
            overload_player_is_target(*player)
                || filter.as_deref().is_some_and(overload_filter_has_target)
        }
        GameMarker::ControlsMoreThanOpponents { filter, .. }
        | GameMarker::ControlsFewerThanOpponent { filter }
        | GameMarker::CardPutIntoGraveyardThisTurn { filter }
        | GameMarker::ZoneActionHappenedThisWay { filter, .. } => {
            overload_filter_has_target(filter)
        }
        GameMarker::OnlyObjectInZone {
            subject, filter, ..
        } => overload_subject_has_target(subject) || overload_filter_has_target(filter),
        GameMarker::AllMatchingAre { filter, predicate } => {
            overload_filter_has_target(filter) || overload_filter_has_target(predicate)
        }
        _ => false,
    }
}

fn overload_condition_has_target(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Compare { lhs, rhs, .. } | ConditionAst::ExprCompare { lhs, rhs, .. } => {
            overload_value_expr_has_target(lhs) || overload_value_expr_has_target(rhs)
        }
        ConditionAst::PrimeNumberCheck { expr } => overload_value_expr_has_target(expr),
        ConditionAst::Marker(marker) => overload_marker_has_target(marker),
        ConditionAst::Not(inner) => overload_condition_has_target(inner),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            overload_condition_has_target(a) || overload_condition_has_target(b)
        }
        ConditionAst::HasKeyword { subject, .. }
        | ConditionAst::HasStatus { subject, .. }
        | ConditionAst::StatCompare { subject, .. }
        | ConditionAst::CounterCompare { subject, .. } => overload_subject_has_target(subject),
        ConditionAst::MatchesFilter { subject, filter } => {
            overload_subject_has_target(subject) || overload_filter_has_target(filter)
        }
        ConditionAst::AnyOfGroupMatches { group, .. } => overload_subject_has_target(group),
        ConditionAst::CastSpellThisTurn { player, filter }
        | ConditionAst::ControlsCount { player, filter, .. } => {
            overload_player_is_target(*player) || overload_filter_has_target(filter)
        }
        ConditionAst::PlayerCounterCompare { player, .. }
        | ConditionAst::PlayerActionThisTurn { player, .. }
        | ConditionAst::LifeCompare { player, .. }
        | ConditionAst::HandCount { player, .. }
        | ConditionAst::GraveyardCount { player, .. }
        | ConditionAst::LostLifeThisTurn { player, .. }
        | ConditionAst::UnspentManaAtLeast { player, .. } => overload_player_is_target(*player),
        ConditionAst::OpponentMoreThanYou { filter, .. } => {
            filter.as_deref().is_some_and(overload_filter_has_target)
        }
        ConditionAst::TotalStatCompare { filter, .. }
        | ConditionAst::TargetsFilter(filter)
        | ConditionAst::ZoneObjectCount { filter, .. }
        | ConditionAst::EnteredUnderYourControlThisTurn { filter, .. } => {
            overload_filter_has_target(filter)
        }
        _ => false,
    }
}

pub(super) fn parse_protection_keywords(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<KeywordAbility>> {
    use spellwake_engine::characteristics::ProtectionFromQuality;
    let trimmed = raw.trim().trim_end_matches('.');
    let stripped = trimmed
        .strip_prefix("from ")
        .or_else(|| trimmed.strip_prefix("From "))
        .unwrap_or(trimmed);
    let low_full = stripped.to_ascii_lowercase();
    if matches!(low_full.as_str(), "each color" | "all colors") {
        return Some(vec![
            KeywordAbility::ProtectionFromWhite,
            KeywordAbility::ProtectionFromBlue,
            KeywordAbility::ProtectionFromBlack,
            KeywordAbility::ProtectionFromRed,
            KeywordAbility::ProtectionFromGreen,
        ]);
    }
    if low_full == "everything" {
        return Some(vec![KeywordAbility::ProtectionFrom(
            ProtectionFromQuality::Everything,
        )]);
    }
    let normalized_full = low_full.replace(" /from ", " and from ");
    let mut out = Vec::new();
    for part in normalized_full.split(" and ") {
        let word = part
            .trim()
            .strip_prefix("from ")
            .unwrap_or(part.trim())
            .trim();
        let ka = match word {
            "white" => KeywordAbility::ProtectionFromWhite,
            "blue" => KeywordAbility::ProtectionFromBlue,
            "black" => KeywordAbility::ProtectionFromBlack,
            "red" => KeywordAbility::ProtectionFromRed,
            "green" => KeywordAbility::ProtectionFromGreen,
            "multicolored" => KeywordAbility::ProtectionFromMulticolored,
            "monocolored" => KeywordAbility::ProtectionFrom(ProtectionFromQuality::Monocolored),
            "colorless" => KeywordAbility::ProtectionFrom(ProtectionFromQuality::Colorless),
            "everything" => KeywordAbility::ProtectionFrom(ProtectionFromQuality::Everything),
            "artifact" | "artifacts" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Artifact))
            }
            "creature" | "creatures" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Creature))
            }
            "enchantment" | "enchantments" => KeywordAbility::ProtectionFrom(
                ProtectionFromQuality::CardType(CardType::Enchantment),
            ),
            "instant" | "instants" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Instant))
            }
            "sorcery" | "sorceries" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Sorcery))
            }
            "land" | "lands" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Land))
            }
            "planeswalker" | "planeswalkers" => KeywordAbility::ProtectionFrom(
                ProtectionFromQuality::CardType(CardType::Planeswalker),
            ),
            "battle" | "battles" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Battle))
            }
            _ => {
                let (base_word, creatures_only) =
                    if let Some(prefix) = word.strip_suffix(" creatures") {
                        (prefix.trim(), true)
                    } else {
                        (word, false)
                    };
                let resolved = resolve_protection_subtype(base_word, vocabulary)?;
                KeywordAbility::ProtectionFromSubtype(SubtypeProtection {
                    subtype: resolved,
                    creatures_only,
                    exclude_subtype: false,
                })
            }
        };
        out.push(ka);
    }
    (!out.is_empty()).then_some(out)
}

pub(super) fn resolve_protection_subtype(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<CreatureTypeId> {
    let trimmed = raw.trim();
    let title = |s: &str| {
        let mut chars = s.chars();
        match chars.next() {
            Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
            None => String::new(),
        }
    };
    if let Some(id) = vocabulary.resolve_label(&title(trimmed)) {
        return Some(id);
    }
    if let Some(stem) = trimmed.strip_suffix("es")
        && let Some(id) = vocabulary.resolve_label(&title(stem))
    {
        return Some(id);
    }
    if let Some(stem) = trimmed.strip_suffix('s')
        && let Some(id) = vocabulary.resolve_label(&title(stem))
    {
        return Some(id);
    }
    if let Some(stem) = trimmed.strip_suffix("ves")
        && let Some(id) = vocabulary.resolve_label(&title(&format!("{stem}f")))
    {
        return Some(id);
    }
    None
}

pub(super) fn parse_hexproof_from_qualities(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<HexproofQuality>> {
    let trimmed = raw.trim().trim_end_matches('.');
    let stripped = trimmed
        .strip_prefix("from ")
        .or_else(|| trimmed.strip_prefix("From "))
        .unwrap_or(trimmed);
    let low_full = stripped.to_ascii_lowercase();
    if matches!(low_full.as_str(), "each color" | "all colors") {
        return Some(vec![
            HexproofQuality::Color(Color::White),
            HexproofQuality::Color(Color::Blue),
            HexproofQuality::Color(Color::Black),
            HexproofQuality::Color(Color::Red),
            HexproofQuality::Color(Color::Green),
        ]);
    }
    if matches!(
        low_full.as_str(),
        "activated and triggered abilities" | "activated or triggered abilities"
    ) {
        return Some(vec![HexproofQuality::ActivatedOrTriggeredAbility]);
    }
    let mut out = Vec::new();
    for comma_part in low_full.split(',') {
        let seg = comma_part.trim();
        if seg.is_empty() {
            return None;
        }
        for and_part in seg.split(" and ") {
            for part in and_part.split(" or ") {
                let mut word = part.trim();
                word = word
                    .strip_prefix("and ")
                    .or_else(|| word.strip_prefix("or "))
                    .unwrap_or(word)
                    .trim();
                word = word.strip_prefix("from ").unwrap_or(word).trim();
                if word.is_empty() {
                    return None;
                }
                let q = match word {
                    "white" => HexproofQuality::Color(Color::White),
                    "blue" => HexproofQuality::Color(Color::Blue),
                    "black" => HexproofQuality::Color(Color::Black),
                    "red" => HexproofQuality::Color(Color::Red),
                    "green" => HexproofQuality::Color(Color::Green),
                    "monocolored" => HexproofQuality::Monocolored,
                    "multicolored" => HexproofQuality::Multicolored,
                    "artifact" | "artifacts" => HexproofQuality::CardType(CardType::Artifact),
                    "creature" | "creatures" => HexproofQuality::CardType(CardType::Creature),
                    "enchantment" | "enchantments" => {
                        HexproofQuality::CardType(CardType::Enchantment)
                    }
                    "instant" | "instants" => HexproofQuality::CardType(CardType::Instant),
                    "sorcery" | "sorceries" => HexproofQuality::CardType(CardType::Sorcery),
                    "land" | "lands" => HexproofQuality::CardType(CardType::Land),
                    "planeswalker" | "planeswalkers" => {
                        HexproofQuality::CardType(CardType::Planeswalker)
                    }
                    "battle" | "battles" => HexproofQuality::CardType(CardType::Battle),
                    other => {
                        let st = resolve_protection_subtype(other, vocabulary)?;
                        HexproofQuality::Subtype(st)
                    }
                };
                if !out.contains(&q) {
                    out.push(q);
                }
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn lower_equip_keyword(
    param: Option<&str>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    let raw = param.ok_or("oracle:equip-non-mana")?;
    let trimmed = trim_equip_param(raw);
    if trimmed.is_empty() {
        return Err("oracle:equip-non-mana");
    }
    let (core, restriction) = strip_equip_trailing_restrictions(trimmed);
    let core = trim_equip_param(core);
    if core.is_empty() {
        return Err("oracle:equip-non-mana");
    }

    // Fast path for plain, hybrid, or snow mana equip costs without loading the compiler vocabulary.
    if let Some((mana_cost, 0, hybrids, snow)) = parse_mana_cost_with_snow(core) {
        let extra = NonZeroU8::new(snow)
            .map(|nz| vec![ExtraActivationCost::PaySnowMana(nz)])
            .unwrap_or_default();
        emit_equip_ability(
            program,
            ids,
            (mana_cost, SourceCost::None, hybrids, extra),
            BodyTarget::SINGLE_CREATURE,
            restriction,
        );
        return Ok(());
    }

    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let (target, cost_branches) =
        if let Some(branches) = parse_equip_cost_branches(core, &compiler_vocab, vocabulary) {
            (BodyTarget::SINGLE_CREATURE, branches)
        } else if let Some((quality_raw, cost_raw)) = split_equip_quality_and_cost(core)
            && let Some(obj) = parse_equip_quality_filter(quality_raw, &compiler_vocab, vocabulary)
            && let Some(branches) = parse_equip_cost_branches(cost_raw, &compiler_vocab, vocabulary)
        {
            (BodyTarget::filtered_permanent(obj), branches)
        } else {
            return Err("oracle:equip-non-mana");
        };

    for branch in cost_branches {
        emit_equip_ability(program, ids, branch, target, restriction.clone());
    }
    Ok(())
}

fn emit_equip_ability(
    program: &mut Program,
    ids: &mut LoweringIds,
    lowered_cost: LoweredActivationCost,
    target: BodyTarget,
    restriction: Option<ActivationRestriction>,
) {
    let (mana_cost, source_cost, hybrids, extra_costs) = lowered_cost;
    let id = ids.activated();
    if !hybrids.is_empty() {
        program.activation_hybrid_symbols.insert(id, hybrids);
    }
    if !extra_costs.is_empty() {
        program.extra_activation_costs.insert(id, extra_costs);
    }
    if let Some(res) = restriction {
        program.activation_restrictions.insert(id, res);
    }
    program.activated.push(ActivatedAbility {
        id,
        mana_cost,
        source_cost,
        body: ExecutableBody::new(
            target,
            vec![Instruction::MutateObject(Box::new(
                spellwake_engine::toolbox::MutateObjectSpec::attach_ability(AttachAbility::Equip),
            ))],
        ),
    });
}

fn split_kicker_and_or(raw: &str) -> Vec<&str> {
    let low = raw.to_ascii_lowercase();
    let needle = " and/or ";
    let mut parts = Vec::new();
    let mut start = 0usize;
    while let Some(rel) = low[start..].find(needle) {
        let end = start + rel;
        parts.push(&raw[start..end]);
        start = end + needle.len();
    }
    parts.push(&raw[start..]);
    parts
}

fn lower_single_kicker_branch(
    branch: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<AdditionalCastCost, &'static str> {
    if let Some((cost, x, hybrids, snow)) = parse_mana_cost_with_snow(branch)
        && hybrids.is_empty()
    {
        if snow == 0 {
            return match x {
                0 => Ok(AdditionalCastCost::Kicker(cost)),
                1 => Ok(AdditionalCastCost::KickerX(cost)),
                _ => Err("oracle:kicker-multi-x"),
            };
        }
        if x == 0
            && let Some(nz_snow) = NonZeroU8::new(snow)
        {
            return Ok(AdditionalCastCost::KickerExtra {
                mana: cost,
                extra: vec![ExtraActivationCost::PaySnowMana(nz_snow)],
            });
        }
        return Err("oracle:kicker-non-mana");
    }

    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let atoms = spellwake_oracle_compiler::clauses::parse_cost_list(branch, &compiler_vocab)
        .map_err(|_| "oracle:kicker-non-mana")?;
    if atoms.is_empty() {
        return Err("oracle:kicker-non-mana");
    }
    let mut mana = ManaCost::free();
    let mut snow_mana: u8 = 0;
    let mut extra = Vec::new();
    for atom in &atoms {
        match atom {
            CostAtomAst::PayMana(raw) => {
                let (mc, x, hybs, snow) =
                    parse_mana_cost_with_snow(raw).ok_or("oracle:kicker-non-mana")?;
                if x > 0 || !hybs.is_empty() {
                    return Err("oracle:kicker-non-mana");
                }
                mana.generic = mana.generic.plus(&mc.generic);
                for (k, v) in mc.required {
                    let entry = mana.required.entry(k).or_insert_with(|| 0.into());
                    *entry = entry.plus(&v);
                }
                snow_mana = snow_mana
                    .checked_add(snow)
                    .ok_or("oracle:kicker-non-mana")?;
            }
            other => {
                let lowered = lower_extra_cost_atom(other, vocabulary)
                    .map_err(|_| "oracle:kicker-non-mana")?;
                if matches!(
                    lowered,
                    ExtraActivationCost::TapSource
                        | ExtraActivationCost::UntapSource
                        | ExtraActivationCost::SacrificeSource
                        | ExtraActivationCost::ExertSource
                        | ExtraActivationCost::ReturnSourceToHand
                        | ExtraActivationCost::SourceCounter { .. }
                        | ExtraActivationCost::DiscardSource
                        | ExtraActivationCost::ExileSource { .. }
                        | ExtraActivationCost::RevealSource
                ) {
                    return Err("oracle:kicker-non-mana");
                }
                extra.push(lowered);
            }
        }
    }
    if let Some(nz_snow) = NonZeroU8::new(snow_mana) {
        extra.push(ExtraActivationCost::PaySnowMana(nz_snow));
    }
    if extra.is_empty() {
        Ok(AdditionalCastCost::Kicker(mana))
    } else {
        Ok(AdditionalCastCost::KickerExtra { mana, extra })
    }
}

fn trim_equip_param(raw: &str) -> &str {
    raw.trim()
        .trim_start_matches(['\u{2014}', '-', ':'])
        .trim()
        .trim_end_matches('.')
        .trim()
}

fn strip_equip_trailing_restrictions(raw: &str) -> (&str, Option<ActivationRestriction>) {
    let mut core = raw.trim();
    let mut restriction: Option<ActivationRestriction> = None;
    while let Some((before, tail)) = core.rsplit_once(". ")
        && let Some(parsed) = parse_equip_trailing_restriction(tail)
    {
        restriction = Some(combine_activation_restrictions(restriction, parsed));
        core = before.trim().trim_end_matches('.').trim();
    }
    (core, restriction)
}

fn parse_equip_trailing_restriction(sentence: &str) -> Option<ActivationRestriction> {
    let trimmed = sentence.trim().trim_end_matches('.').trim();
    let low = trimmed.to_ascii_lowercase();
    let rest = low
        .strip_prefix("activate this ability only ")
        .or_else(|| low.strip_prefix("activate only "))?;
    match rest {
        "once each turn" | "once per turn" => Some(ActivationRestriction::OnceEachTurn),
        "as a sorcery" => Some(ActivationRestriction::Sorcery),
        "as a sorcery and only once each turn" | "once each turn and only as a sorcery" => {
            Some(ActivationRestriction::SorceryOnceEachTurn)
        }
        "during your turn" => Some(ActivationRestriction::DuringYourTurn),
        "during your turn and only once each turn" | "once each turn and only during your turn" => {
            Some(ActivationRestriction::DuringYourTurnOnceEachTurn)
        }
        "during your upkeep" => Some(ActivationRestriction::DuringUpkeep),
        "once" => Some(ActivationRestriction::Exhaust),
        _ => None,
    }
}

fn normalize_equip_cost_text(raw: &str) -> String {
    let mut s = trim_equip_param(raw).to_string();
    for phrase in [
        "this Equipment",
        "This Equipment",
        "this equipment",
        "This equipment",
        "this permanent",
        "This permanent",
        "this artifact",
        "This artifact",
    ] {
        if s.contains(phrase) {
            s = s.replace(phrase, "~");
        }
    }
    s
}

fn parse_single_equip_cost(
    raw: &str,
    compiler_vocab: &Vocabulary,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<LoweredActivationCost> {
    let cleaned = normalize_equip_cost_text(raw);
    if cleaned.is_empty() {
        return None;
    }
    if let Some((mc, 0, hybs, snow)) = parse_mana_cost_with_snow(&cleaned) {
        let extra = NonZeroU8::new(snow)
            .map(|nz| vec![ExtraActivationCost::PaySnowMana(nz)])
            .unwrap_or_default();
        return Some((mc, SourceCost::None, hybs, extra));
    }
    let low = cleaned.to_ascii_lowercase();
    if low.contains("where x is") {
        return None;
    }
    let atoms =
        spellwake_oracle_compiler::clauses::parse_cost_list(&cleaned, compiler_vocab).ok()?;
    if atoms.is_empty() {
        return None;
    }
    if let Ok((mana_cost, mut source_cost, hybrids, extra_costs)) =
        lower_activation_cost_full(&atoms, None, vocabulary)
    {
        if matches!(
            source_cost,
            SourceCost::HandDiscardSelf(_)
                | SourceCost::GraveyardSelf { .. }
                | SourceCost::GraveyardDiscard { .. }
                | SourceCost::GraveyardExileOther { .. }
                | SourceCost::GraveyardPayLife { .. }
                | SourceCost::Ninjutsu { .. }
                | SourceCost::GenericX { .. }
        ) {
            return None;
        }
        // Preserve "another" self-exclusion when `parse_cost_list` strips the leading
        // quantifier (for example, "Sacrifice another nonland permanent").
        if low.starts_with("sacrifice another ")
            && atoms.len() == 1
            && let SourceCost::SacrificeSelected { filter, .. } = &mut source_cost
        {
            match filter {
                SelectedSacrificeFilter::Creature => {
                    *filter = SelectedSacrificeFilter::OtherCreature;
                }
                SelectedSacrificeFilter::Artifact => {
                    *filter = SelectedSacrificeFilter::Matching(
                        ObjectFilter::artifact().other_than_ability_source(),
                    );
                }
                SelectedSacrificeFilter::Matching(obj) => {
                    *obj = obj.other_than_ability_source();
                }
                _ => {}
            }
        }
        return Some((mana_cost, source_cost, hybrids, extra_costs));
    }
    // Generic counter removal from self (for example, "remove a counter from ~") parses as
    // `RemoveCounters { counter_kind: "any", subject: SelfObject }`.
    if let [
        CostAtomAst::RemoveCounters {
            all,
            count,
            counter_kind,
            subject,
        },
    ] = atoms.as_slice()
        && matches!(subject.as_ref(), SubjectRefAst::SelfObject)
        && matches!(
            counter_kind.trim().to_ascii_lowercase().as_str(),
            "" | "a" | "an" | "any" | "counter" | "counters"
        )
    {
        let cnt = if *all {
            None
        } else if let Some(c) = count {
            match c.as_ref() {
                ValueExprAst::Const(n) if *n > 0 => {
                    let u = u8::try_from(*n).ok()?;
                    Some(NonZeroU8::new(u)?)
                }
                _ => return None,
            }
        } else {
            Some(NonZeroU8::MIN)
        };
        return Some((
            ManaCost::free(),
            SourceCost::None,
            Vec::new(),
            vec![ExtraActivationCost::RemovePermanentCounters {
                kind: None,
                count: cnt,
                all: *all,
                filter: ObjectFilter::self_object(),
            }],
        ));
    }
    None
}

fn parse_equip_cost_branches(
    raw: &str,
    compiler_vocab: &Vocabulary,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<LoweredActivationCost>> {
    let cleaned = normalize_equip_cost_text(raw);
    if cleaned.is_empty() {
        return None;
    }
    if let Some((left, right)) = cleaned.split_once(" or ")
        && let (Some(left_cost), Some(right_cost)) = (
            parse_single_equip_cost(left, compiler_vocab, vocabulary),
            parse_single_equip_cost(right, compiler_vocab, vocabulary),
        )
    {
        return Some(vec![left_cost, right_cost]);
    }
    parse_single_equip_cost(&cleaned, compiler_vocab, vocabulary).map(|cost| vec![cost])
}

fn split_equip_quality_and_cost(core: &str) -> Option<(&str, &str)> {
    if let Some((quality, cost)) = core.split_once('\u{2014}') {
        let (q, c) = (quality.trim(), cost.trim());
        if !q.is_empty() && !c.is_empty() {
            return Some((q, c));
        }
    }
    let brace_idx = core.find('{')?;
    if brace_idx == 0 {
        return None;
    }
    let q = core[..brace_idx].trim();
    let c = core[brace_idx..].trim();
    if q.is_empty() || c.is_empty() {
        return None;
    }
    Some((q, c))
}

fn parse_equip_quality_filter(
    quality_raw: &str,
    compiler_vocab: &Vocabulary,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let quality = quality_raw
        .trim()
        .trim_end_matches(['.', ',', ';', ':'])
        .trim();
    let quality = quality.strip_suffix(" -").unwrap_or(quality).trim();
    if quality.is_empty() || quality.contains(['.', ';']) {
        return None;
    }
    let low = quality.to_ascii_lowercase();
    // CR 702.6d: "Equip commander [cost]" targets a commander creature you control.
    // `parse_object_filter("commander")` maps "commander" to LEGENDARY, so preserve
    // exact commander identity via `ObjectFilterFlags::COMMANDER_ONLY`.
    if low == "commander" || low == "commander creature" {
        return Some(ObjectFilter::creature().commander().controlled());
    }
    let ast_filter =
        spellwake_oracle_compiler::filter::parse_object_filter(quality, compiler_vocab).ok()?;
    if ast_filter.zone.is_some()
        || ast_filter.any_spell
        || ast_filter.any_card
        || ast_filter.is_ability
        || !matches!(ast_filter.controller, None | Some(ControllerRelation::You))
    {
        return None;
    }
    let obj = lower_object_filter(&ast_filter, vocabulary).ok()?;
    let ensure_creature_when_untyped = |clause: ObjectClause| -> ObjectClause {
        if clause.any_types.is_empty() {
            clause.any_types(CardTypeMask::CREATURE)
        } else {
            clause
        }
    };
    let typed = if ast_filter.any_permanent {
        obj
    } else {
        match obj {
            ObjectFilter::Clause(c) => ObjectFilter::Clause(ensure_creature_when_untyped(c)),
            ObjectFilter::Either(a, b) => ObjectFilter::Either(
                ensure_creature_when_untyped(a),
                ensure_creature_when_untyped(b),
            ),
        }
    };
    Some(typed.controlled())
}

fn parse_cycling_land_subtype(prefix: &str) -> Option<LandType> {
    match prefix {
        "plains" => Some(LandType::Plains),
        "island" => Some(LandType::Island),
        "swamp" => Some(LandType::Swamp),
        "mountain" => Some(LandType::Mountain),
        "forest" => Some(LandType::Forest),
        "cave" => Some(LandType::Cave),
        "desert" => Some(LandType::Desert),
        "gate" => Some(LandType::Gate),
        "lair" => Some(LandType::Lair),
        "locus" => Some(LandType::Locus),
        "mine" => Some(LandType::Mine),
        "planet" => Some(LandType::Planet),
        "power-plant" => Some(LandType::PowerPlant),
        "sphere" => Some(LandType::Sphere),
        "tower" => Some(LandType::Tower),
        "town" => Some(LandType::Town),
        "urza's" => Some(LandType::Urzas),
        _ => None,
    }
}

fn lower_typecycling_keyword(
    lower_name: &str,
    param: Option<&str>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    let prefix = lower_name
        .strip_suffix("cycling")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("oracle:unsupported-keyword")?;
    let search = match prefix {
        "basic land" => LibrarySearch::to_hand(SearchCardMatcher::BasicLand),
        "land" => LibrarySearch::to_hand(SearchCardMatcher::CardTypes(BTreeSet::from([
            CardType::Land,
        ]))),
        "artifact land" => {
            LibrarySearch::to_hand(SearchCardMatcher::Filtered(Box::new(ObjectFilter::Clause(
                ObjectClause::any()
                    .required_type(CardType::Artifact)
                    .required_type(CardType::Land),
            ))))
        }
        "artifact" => LibrarySearch::to_hand(SearchCardMatcher::CardTypes(BTreeSet::from([
            CardType::Artifact,
        ]))),
        "creature" => LibrarySearch::to_hand(SearchCardMatcher::CardTypes(BTreeSet::from([
            CardType::Creature,
        ]))),
        "enchantment" => LibrarySearch::to_hand(SearchCardMatcher::CardTypes(BTreeSet::from([
            CardType::Enchantment,
        ]))),
        "instant" => LibrarySearch::to_hand(SearchCardMatcher::CardTypes(BTreeSet::from([
            CardType::Instant,
        ]))),
        "sorcery" => LibrarySearch::to_hand(SearchCardMatcher::CardTypes(BTreeSet::from([
            CardType::Sorcery,
        ]))),
        "planeswalker" => LibrarySearch::to_hand(SearchCardMatcher::CardTypes(BTreeSet::from([
            CardType::Planeswalker,
        ]))),
        "battle" => LibrarySearch::to_hand(SearchCardMatcher::CardTypes(BTreeSet::from([
            CardType::Battle,
        ]))),
        other => {
            if let Some(lt) = parse_cycling_land_subtype(other) {
                LibrarySearch::land_subtype_to_hand(lt)
            } else if let Some(st) = resolve_protection_subtype(other, vocabulary) {
                LibrarySearch::to_hand(SearchCardMatcher::CreatureSubtype(st))
            } else {
                return Err("oracle:unsupported-keyword");
            }
        }
    };
    let (cost, x_count, hybrids) = param
        .and_then(parse_mana_cost_full)
        .ok_or("oracle:cycling-non-mana")?;
    if x_count != 0 {
        return Err("oracle:cycling-non-mana");
    }
    let id = ids.activated();
    if !hybrids.is_empty() {
        program.activation_hybrid_symbols.insert(id, hybrids);
    }
    program.activated.push(ActivatedAbility {
        id,
        mana_cost: cost,
        source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::SearchLibrary {
                filter: Box::new(search),
            }],
        ),
    });
    Ok(())
}

fn lower_champion_keyword(
    param: Option<&str>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    fn strip_article(s: &str) -> &str {
        s.strip_prefix("a ")
            .or_else(|| s.strip_prefix("A "))
            .or_else(|| s.strip_prefix("an "))
            .or_else(|| s.strip_prefix("An "))
            .unwrap_or(s)
            .trim()
    }
    let raw = param.ok_or("oracle:kw-param")?;
    let trimmed = trim_equip_param(raw);
    let quality = strip_article(trimmed);
    if quality.is_empty() {
        return Err("oracle:kw-param");
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let parsed = if let Some((lhs_raw, rhs_raw)) = quality.split_once(" or ")
        && !rhs_raw.contains(" or ")
    {
        let lhs = strip_article(lhs_raw.trim());
        let rhs = strip_article(rhs_raw.trim());
        match (
            parse_equip_quality_filter(lhs, &compiler_vocab, vocabulary),
            parse_equip_quality_filter(rhs, &compiler_vocab, vocabulary),
        ) {
            (Some(ObjectFilter::Clause(a)), Some(ObjectFilter::Clause(b))) => {
                Some(ObjectFilter::Either(a, b))
            }
            _ => parse_equip_quality_filter(quality, &compiler_vocab, vocabulary),
        }
    } else {
        parse_equip_quality_filter(quality, &compiler_vocab, vocabulary)
    };
    let obj = parsed.ok_or("oracle:kw-param")?.other_than_ability_source();
    program.triggered.push(TriggeredAbility {
        id: ids.triggered(),
        condition: TriggerSpec::self_enters(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(
                    ResolvingPlayer::Controller,
                    SourceZoneFilter::Battlefield,
                    obj,
                    ValueExpr::literal(0),
                    ValueExpr::literal(1),
                    SubjectDestination::Exile {
                        delayed_return: Some(
                            spellwake_engine::program::DelayedReturnTiming::LinkedGroup(
                                spellwake_engine::program::LinkedExileGroup::SourceLinked,
                            ),
                        ),
                    },
                    false,
                ),
                Instruction::BranchCondition {
                    condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                        Box::new(ResolutionCondition {
                            left: ValueExpr::EventValue(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(0),
                        }),
                    )),
                    destination: 3,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::sacrifice_subject(
                    EffectSubject::AbilitySource,
                ),
            ],
        ),
    });
    program.triggered.push(TriggeredAbility {
        id: ids.triggered(),
        condition: TriggerSpec::leaves_battlefield(ObjectFilter::self_object()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_cards_exiled_with_link(spellwake_engine::program::LinkedExileGroup::SourceLinked, false, false)],
        ),
    });
    Ok(())
}

fn parse_cum_upkeep_positive_count(tok: &str) -> Option<u16> {
    if let Ok(n) = tok.parse::<u16>()
        && n > 0
    {
        return Some(n);
    }
    match tok {
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        _ => None,
    }
}

fn lower_non_mana_cumulative_upkeep(
    clean_cost: &str,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<TriggeredAbility, &'static str> {
    let low = clean_cost.trim().to_ascii_lowercase();
    let age_count = ValueExpr::CounterCount {
        subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
        kind: Some(CounterKind::Age),
    };
    let mut pre_guard: Option<ResolutionCondition> = None;
    let cost_ops: Vec<Instruction> = if let Some(life_part) = low
        .strip_prefix("pay ")
        .and_then(|rest| rest.strip_suffix(" life"))
    {
        let n =
            parse_cum_upkeep_positive_count(life_part.trim()).ok_or("oracle:cum-upkeep-cost")?;
        let amount = if n == 1 {
            age_count
        } else {
            scale_value_expr(age_count, i64::from(n))
        };
        vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::pay_life(
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                amount,
            ),
        ]
    } else if let Some(after_put) = low
        .strip_prefix("put a ")
        .or_else(|| low.strip_prefix("put an "))
        && let Some((kind_raw, subj_raw)) = after_put.split_once(" counter on ")
        && matches!(
            subj_raw.trim(),
            "~" | "this creature"
                | "this permanent"
                | "this artifact"
                | "this enchantment"
                | "this land"
        )
    {
        let ck = parse_counter_kind(kind_raw.trim())
            .filter(|k| k.is_permanent())
            .ok_or("oracle:cum-upkeep-cost")?;
        vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                ck, age_count,
            ),
        ]
    } else if let Some(sym) = low.strip_prefix("add ") {
        let mana_type = match sym.trim() {
            "{w}" => ManaType::White,
            "{u}" => ManaType::Blue,
            "{b}" => ManaType::Black,
            "{r}" => ManaType::Red,
            "{g}" => ManaType::Green,
            "{c}" => ManaType::Colourless,
            _ => return Err("oracle:cum-upkeep-cost"),
        };
        vec![Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                ResolvingPlayer::Controller,
                EffectManaYield::Counted {
                    kind: mana_type,
                    amount: age_count,
                },
            ),
        ))]
    } else if matches!(
        low.as_str(),
        "sacrifice a creature"
            | "sacrifice a land"
            | "sacrifice an artifact"
            | "sacrifice an enchantment"
            | "sacrifice a permanent"
    ) {
        let required_types = match low.as_str() {
            "sacrifice a creature" => BTreeSet::from([CardType::Creature]),
            "sacrifice a land" => BTreeSet::from([CardType::Land]),
            "sacrifice an artifact" => BTreeSet::from([CardType::Artifact]),
            "sacrifice an enchantment" => BTreeSet::from([CardType::Enchantment]),
            _ => BTreeSet::new(),
        };
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                AffectedPlayers::One(ResolvingPlayer::Controller),
                age_count,
                PermanentTypeFilter {
                    any_types: BTreeSet::new(),
                    required_types,
                    forbidden_types: BTreeSet::new(),
                    object_filter: None,
                },
            ),
        ))]
    } else if low == "draw a card" {
        vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(age_count)]
    } else if low == "discard a card" {
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::discard(
                AffectedPlayers::One(ResolvingPlayer::Controller),
                age_count,
            ),
        ))]
    } else if low == "flip a coin" {
        vec![Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::flip_coins(
                ResolvingPlayer::Controller,
                spellwake_engine::program::CoinFlipMode::Count(age_count),
                None,
                None,
                None,
            ),
        ))]
    } else if low == "exile the top card of your library" {
        pre_guard = Some(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLibrarySize(
                ResolvingPlayer::Controller,
            )),
            comparison: Comparison::GreaterOrEqual,
            right: age_count.clone(),
        });
        vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_library_top_linked(
                ResolvingPlayer::Controller,
                age_count,
                spellwake_engine::program::LinkedExileGroup::SourceLinked,
                false,
            ),
        ]
    } else if let Some(life_part) = low
        .strip_prefix("an opponent gains ")
        .and_then(|rest| rest.strip_suffix(" life"))
    {
        let n =
            parse_cum_upkeep_positive_count(life_part.trim()).ok_or("oracle:cum-upkeep-cost")?;
        let loop_body = ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::PromptChoice(Box::new(
                    spellwake_engine::toolbox::PromptChoiceSpec::choose_value(
                        ResolvingPlayer::Controller,
                        spellwake_engine::toolbox::PromptDomain::player(true),
                    ),
                )),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::ChosenPlayer,
                    ),
                    ValueExpr::literal(i64::from(n)),
                ),
            ],
        );
        vec![Instruction::RepeatLoop {
            count: age_count,
            body: Box::new(loop_body),
        }]
    } else if low == "have an opponent create a 1/1 red survivor creature token" {
        let survivor = vocabulary
            .resolve_label("Survivor")
            .ok_or("oracle:cum-upkeep-cost")?;
        let mut tok_prog = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([survivor]));
        tok_prog.effect_defined_colors = Some(BTreeSet::from([Color::Red]));
        let tok_def = TokenDefinition::new(
            tok_prog,
            TokenName::Default {
                ordered_subtypes: vec![TokenSubtype::Creature(survivor)],
            },
            vocabulary,
        )
        .map_err(|_| "oracle:cum-upkeep-cost")?;
        let loop_body = ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::PromptChoice(Box::new(
                    spellwake_engine::toolbox::PromptChoiceSpec::choose_value(
                        ResolvingPlayer::Controller,
                        spellwake_engine::toolbox::PromptDomain::player(true),
                    ),
                )),
                Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        enters_tapped: false,
                        definition: tok_def,
                        amount: ValueExpr::literal(1),
                        base_size: None,
                        recipients: TokenRecipients::Player(ResolvingPlayer::ChosenPlayer),
                        attacking_defender: None,
                    }],
                },
            ],
        );
        vec![Instruction::RepeatLoop {
            count: age_count,
            body: Box::new(loop_body),
        }]
    } else if low == "put a +1/+1 counter on a creature an opponent controls" {
        let opp_creature = ObjectFilter::Clause(ObjectClause::creature().opposing());
        pre_guard = Some(ResolutionCondition {
            left: ValueExpr::count_battlefield(opp_creature),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        });
        let mut ops = spellwake_engine::toolbox::PromptChoiceSpec::choose_objects_instructions(
            AffectedPlayers::One(ResolvingPlayer::Controller),
            SourceZoneFilter::AnyBattlefield,
            opp_creature,
            ValueExpr::literal(1),
            ValueExpr::literal(1),
            false,
            None,
            None,
        );
        ops.push(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(
                EffectSubject::LastMoved,
                CounterKind::PlusOne,
                ValueExpr::literal(1),
            ),
        );
        let loop_body = ExecutableBody::new(BodyTarget::None, ops);
        vec![Instruction::RepeatLoop {
            count: age_count,
            body: Box::new(loop_body),
        }]
    } else if low == "gain control of a land you don't control" {
        let other_land = ObjectFilter::Clause(ObjectClause::land().not_controlled());
        pre_guard = Some(ResolutionCondition {
            left: ValueExpr::count_battlefield(other_land),
            comparison: Comparison::GreaterOrEqual,
            right: age_count.clone(),
        });
        let mut ops = spellwake_engine::toolbox::PromptChoiceSpec::choose_objects_instructions(
            AffectedPlayers::One(ResolvingPlayer::Controller),
            SourceZoneFilter::AnyBattlefield,
            other_land,
            ValueExpr::literal(1),
            ValueExpr::literal(1),
            false,
            None,
            None,
        );
        ops.push(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_control_of_subject(
                EffectSubject::LastMoved,
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                ResolvedDuration::NoStatedDuration,
            ),
        );
        let loop_body = ExecutableBody::new(BodyTarget::None, ops);
        vec![Instruction::RepeatLoop {
            count: age_count,
            body: Box::new(loop_body),
        }]
    } else {
        return Err("oracle:cum-upkeep-cost");
    };

    let mut instructions = Vec::with_capacity(cost_ops.len() + 5);
    instructions.push(
        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
            CounterKind::Age,
            ValueExpr::literal(1),
        ),
    );
    let sac_idx = 1 + usize::from(pre_guard.is_some()) + 1 + cost_ops.len() + 1;
    if let Some(cond) = pre_guard {
        instructions.push(Instruction::BranchCondition {
            condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                Box::new(cond),
            )),
            destination: sac_idx,
        });
    }
    instructions.push(Instruction::OptionalSequence {
        chooser: ResolvingPlayer::Controller,
        instructions: cost_ops.len() + 1,
    });
    instructions.extend(cost_ops);
    instructions.push(Instruction::Jump {
        destination: sac_idx + 1,
    });
    instructions.push(Instruction::MutateObject(Box::new(
        spellwake_engine::toolbox::MutateObjectSpec::sacrifice_state_trigger_source(),
    )));

    Ok(TriggeredAbility {
        id: ids.triggered(),
        condition: TriggerSpec::new(TriggerEvent::StepBegan {
            step: Step::Upkeep,
            player: TriggerPlayerScope::Relation(PlayerRelation::Controller),
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, instructions),
    })
}

fn lower_affinity_keyword(
    param: Option<&str>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), &'static str> {
    let raw = param.ok_or("oracle:affinity-param")?.trim();
    let stripped = raw
        .strip_prefix("for ")
        .or_else(|| raw.strip_prefix("For "))
        .unwrap_or(raw)
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
    if stripped.is_empty() {
        return Err("oracle:affinity-param");
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let ast_filter =
        spellwake_oracle_compiler::filter::parse_object_filter(&stripped, &compiler_vocab)
            .map_err(|_| "oracle:unsupported-affinity")?;
    if ast_filter.any_card || ast_filter.any_spell || ast_filter.is_ability {
        return Err("oracle:unsupported-affinity");
    }
    let mut sel = lower_filter_to_permanent_selector(&ast_filter, vocabulary)
        .map_err(|_| "oracle:unsupported-affinity")?;
    if sel.controller != ControllerSelector::Any || sel.owner.is_some() {
        return Err("oracle:unsupported-affinity");
    }
    sel.controller = ControllerSelector::SourceController;
    let empty_controlled = PermanentSelector {
        controller: ControllerSelector::SourceController,
        ..PermanentSelector::default()
    };
    if sel == empty_controlled && stripped != "permanents" {
        return Err("oracle:unsupported-affinity");
    }
    program
        .cast_reductions
        .push(CastReduction::Affinity(Box::new(sel)));
    Ok(())
}

fn parse_flashback_positive_u8(tok: &str) -> Option<u8> {
    if matches!(tok, "a" | "an") {
        return Some(1);
    }
    let n = parse_cum_upkeep_positive_count(tok)?;
    u8::try_from(n).ok()
}

fn lower_flashback_keyword(param: Option<&str>, program: &mut Program) -> Result<(), &'static str> {
    let raw = param.ok_or("oracle:flashback-non-mana")?.trim();
    if let Some((cost, x)) = parse_mana_cost_with_x(raw) {
        if program.flashback.replace(cost).is_some() {
            return Err("oracle:duplicate-flashback");
        }
        program.flashback_x_mana_symbols = x;
        return Ok(());
    }

    let cleaned = raw
        .trim_start_matches(['\u{2014}', '\u{2013}', '-', ':'])
        .trim()
        .trim_end_matches('.')
        .trim();
    if cleaned.is_empty() {
        return Err("oracle:flashback-non-mana");
    }

    let mut segments = cleaned
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .peekable();
    let (mana_cost, x_symbols) = match segments.peek().and_then(|seg| parse_mana_cost_with_x(seg)) {
        Some(parsed) => {
            segments.next();
            parsed
        }
        None => (ManaCost::free(), 0),
    };

    let mut life: u16 = 0;
    let mut sacrifice_creatures: u8 = 0;
    let mut tap_white_creatures: u8 = 0;
    let mut saw_non_mana = false;

    for seg in segments {
        let low = seg.trim_end_matches('.').trim().to_ascii_lowercase();
        if let Some(rest) = low
            .strip_prefix("pay ")
            .and_then(|s| s.strip_suffix(" life"))
        {
            let amount: u16 = rest
                .trim()
                .parse()
                .map_err(|_| "oracle:flashback-non-mana")?;
            if amount == 0 || life != 0 {
                return Err("oracle:flashback-non-mana");
            }
            life = amount;
            saw_non_mana = true;
        } else if let Some(rest) = low.strip_prefix("sacrifice ").and_then(|s| {
            s.strip_suffix(" creatures")
                .or_else(|| s.strip_suffix(" creature"))
        }) {
            let count =
                parse_flashback_positive_u8(rest.trim()).ok_or("oracle:flashback-non-mana")?;
            if sacrifice_creatures != 0 {
                return Err("oracle:flashback-non-mana");
            }
            sacrifice_creatures = count;
            saw_non_mana = true;
        } else if let Some(rest) = low.strip_prefix("tap ").and_then(|s| {
            s.strip_suffix(" untapped white creatures you control")
                .or_else(|| s.strip_suffix(" untapped white creature you control"))
        }) {
            let count =
                parse_flashback_positive_u8(rest.trim()).ok_or("oracle:flashback-non-mana")?;
            if tap_white_creatures != 0 {
                return Err("oracle:flashback-non-mana");
            }
            tap_white_creatures = count;
            saw_non_mana = true;
        } else {
            return Err("oracle:flashback-non-mana");
        }
    }

    if !saw_non_mana {
        return Err("oracle:flashback-non-mana");
    }
    if program.flashback.replace(mana_cost).is_some() {
        return Err("oracle:duplicate-flashback");
    }
    program.flashback_x_mana_symbols = x_symbols;
    program.flashback_life = life;
    program.flashback_sacrifice_creatures = sacrifice_creatures;
    program.flashback_tap_white_creatures = tap_white_creatures;
    Ok(())
}
