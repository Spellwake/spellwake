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
                        Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        },
                        Instruction::Sacrifice {
                            players: AffectedPlayers::One(ResolvingPlayer::Controller),
                            amount: ValueExpr::literal(1),
                            filter: PermanentTypeFilter {
                                any_types: BTreeSet::new(),
                                required_types: BTreeSet::from([CardType::Creature]),
                                forbidden_types: BTreeSet::new(),
                                object_filter: None,
                            },
                        },
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
        "storm" if is_spell => {
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
        "modular" => {
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
                        vec![Instruction::PairWithCreature { entrant_only }],
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
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:cum-upkeep-cost")?;
            program
                .triggered
                .push(UpkeepKeyword::CumulativeUpkeep.ability(ids.triggered(), cost));
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
            body.instructions.push(Instruction::CounterWardSource);
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
                        Instruction::Myriad,
                        Instruction::ScheduleCreatedTokenExile(Step::EndCombat),
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
        "equip" => {
            let p_str = param.ok_or("oracle:equip-non-mana")?;
            let (cost, source_cost, hybrids, extra_costs) =
                if let Some((mc, 0, hybs)) = parse_mana_cost_full(p_str) {
                    (mc, SourceCost::None, hybs, Vec::new())
                } else {
                    let compiler_vocab = cached_compiler_vocab(vocabulary);
                    let atoms =
                        spellwake_oracle_compiler::clauses::parse_cost_list(p_str, &compiler_vocab)
                            .map_err(|_| "oracle:equip-non-mana")?;
                    lower_activation_cost_full(&atoms, None, vocabulary)
                        .map_err(|_| "oracle:equip-non-mana")?
                };
            let id = ids.activated();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if !extra_costs.is_empty() {
                program.extra_activation_costs.insert(id, extra_costs);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost,
                body: ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Attach(AttachAbility::Equip)],
                ),
            });
            Ok(())
        }
        "scavenge" => {
            let p_str = param.ok_or("oracle:kw-param")?;
            let (mana_cost, 0, hybrids) = parse_mana_cost_full(p_str).ok_or("oracle:kw-param")?
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
                    BodyTarget::SingleCreature,
                    vec![Instruction::PutNamedCounterOnTarget {
                        kind: CounterKind::PlusOne,
                        amount: ValueExpr::Literal(power),
                    }],
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
                        Instruction::BranchIfFalse {
                            condition: ResolutionCondition {
                                left: ValueExpr::EventValue(EventValueKind::EnteredKickerX),
                                comparison: Comparison::GreaterOrEqual,
                                right: ValueExpr::literal(5),
                            },
                            destination: 2,
                        },
                        Instruction::DrawController {
                            amount: ValueExpr::literal(1),
                        },
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
                    BodyTarget::SingleCreature,
                    vec![Instruction::Attach(AttachAbility::Reconfigure)],
                ),
            });
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Unattach]),
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
                    BodyTarget::SinglePermanent,
                    vec![Instruction::Attach(AttachAbility::Fortify)],
                ),
            });
            Ok(())
        }
        "cycling" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:cycling-non-mana")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::DrawController {
                        amount: ValueExpr::literal(1),
                    }],
                ),
            });
            Ok(())
        }
        "basic landcycling" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:cycling-non-mana")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::SearchLibrary {
                        filter: Box::new(LibrarySearch::to_hand(SearchCardMatcher::BasicLand)),
                    }],
                ),
            });
            Ok(())
        }
        "plainscycling" | "islandcycling" | "swampcycling" | "mountaincycling"
        | "forestcycling" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:cycling-non-mana")?;
            let lt = match lower.as_str() {
                "plainscycling" => LandType::Plains,
                "islandcycling" => LandType::Island,
                "swampcycling" => LandType::Swamp,
                "mountaincycling" => LandType::Mountain,
                _ => LandType::Forest,
            };
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::SearchLibrary {
                        filter: Box::new(LibrarySearch::land_subtype_to_hand(lt)),
                    }],
                ),
            });
            Ok(())
        }
        "transmute" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:transmute-cost")?;
            let id = ids.activated();
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
        "unearth" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:unearth-cost")?;
            let id = ids.activated();
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::GraveyardSelf { unearth: true },
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Unearth]),
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
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().opposing()),
                    vec![
                        Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        },
                        Instruction::AffectPermanentTargets {
                            minimum: 1,
                            maximum: 1,
                            action: PermanentTargetAction::Untap,
                        },
                        Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                BlockingRestriction::MustBlockSource,
                            )]),
                            duration: ResolvedDuration::UntilEndOfTurn,
                        },
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
                        Instruction::OfferManaPayment {
                            payer: ResolvingPlayer::Controller,
                            cost: mana,
                            paid_destination: 1,
                            declined_destination: 3,
                        },
                        Instruction::ReturnSourceGraveyardToHand,
                        Instruction::Jump { destination: 4 },
                        Instruction::MoveSubject {
                            subject: MoveSubjectRef::AbilitySource,
                            destination: SubjectDestination::Exile {
                                delayed_return: None,
                            },
                        },
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
                    BodyTarget::SingleCreature,
                    vec![Instruction::PutPowerToughnessCounters {
                        counters: PowerToughnessCounters {
                            plus_one: n.into(),
                            minus_one: 0.into(),
                        },
                    }],
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
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Ninjutsu]),
            });
            Ok(())
        }
        "flashback" => {
            let (cost, x) = param
                .and_then(parse_mana_cost_with_x)
                .ok_or("oracle:flashback-non-mana")?;
            if program.flashback.replace(cost).is_some() {
                return Err("oracle:duplicate-flashback");
            }
            program.flashback_x_mana_symbols = x;
            Ok(())
        }
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
            let (cost, x) = param
                .and_then(parse_mana_cost_with_x)
                .ok_or("oracle:kicker-non-mana")?;
            if x == 0 {
                program
                    .additional_cast_costs
                    .push(AdditionalCastCost::Kicker(cost));
            } else if x == 1 {
                program
                    .additional_cast_costs
                    .push(AdditionalCastCost::KickerX(cost));
            } else {
                return Err("oracle:kicker-multi-x");
            }
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
        "replicate" if is_spell => {
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
        "affinity" => {
            let p = param.ok_or("oracle:affinity-param")?;
            let stripped = p
                .strip_prefix("for ")
                .unwrap_or(p)
                .trim()
                .to_ascii_lowercase();
            let mut sel = PermanentSelector {
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            };
            match stripped.as_str() {
                "artifacts" => {
                    sel.required_types.insert(CardType::Artifact);
                }
                "creatures" => {
                    sel.required_types.insert(CardType::Creature);
                }
                "enchantments" => {
                    sel.required_types.insert(CardType::Enchantment);
                }
                "lands" => {
                    sel.required_types.insert(CardType::Land);
                }
                "plains" => {
                    sel.land_subtype = Some(LandType::Plains);
                }
                "islands" => {
                    sel.land_subtype = Some(LandType::Island);
                }
                "swamps" => {
                    sel.land_subtype = Some(LandType::Swamp);
                }
                "mountains" => {
                    sel.land_subtype = Some(LandType::Mountain);
                }
                "forests" => {
                    sel.land_subtype = Some(LandType::Forest);
                }
                "equipment" => {
                    sel.artifact_subtype = Some(ArtifactSubtype::Equipment);
                }
                _ => return Err("oracle:unsupported-affinity"),
            }
            program
                .cast_reductions
                .push(CastReduction::Affinity(Box::new(sel)));
            Ok(())
        }
        "ascend" => {
            if is_spell {
                let ProgramKind::Spell(spell) = &mut program.kind else {
                    return Err("oracle:ascend-not-spell");
                };
                if let Some(modes) = &mut spell.modes {
                    if let Some(prefix) = &mut modes.prefix {
                        prefix.instructions.insert(0, Instruction::Ascend);
                        for op in &mut prefix.instructions[1..] {
                            op.shift_jumps(1, 0);
                        }
                    } else {
                        modes.prefix = Some(Box::new(ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::Ascend],
                        )));
                    }
                } else {
                    spell.body.instructions.insert(0, Instruction::Ascend);
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
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Encore]),
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
                    vec![Instruction::CreateTokenAndAttach {
                        definition: Box::new(definition),
                    }],
                ),
            });
            Ok(())
        }
        "outlast" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:outlast-cost")?;
            let id = ids.activated();
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::PutCountersOnSource { amount: 1.into() }],
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
                    vec![Instruction::PutCountersOnSubject {
                        subject: Box::new(EffectSubject::AbilitySource),
                        kind: CounterKind::Level,
                        amount: ValueExpr::Literal(1.into()),
                    }],
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
                    BodyTarget::SinglePlayer,
                    vec![
                        Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::TargetPlayer,
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
                    vec![Instruction::AddMana(
                        EffectManaYield::RetainedUntilEndOfCombat {
                            kind: ManaType::Red,
                            amount,
                        },
                    )],
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
                    vec![Instruction::CreateTokenCopy {
                        source: MoveSubjectRef::AbilitySource,
                        count: ValueExpr::literal(1),
                        tapped: false,
                        attacking: false,
                        sacrifice_at_end_step: false,
                        exile_at_end_step: false,
                        grant_haste: false,
                        nonlegendary: false,
                        override_pt: Some((1, 1)),
                        override_colors: None,
                        add_subtype: None,
                        strip_mana_cost: false,
                    }],
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
                    vec![Instruction::CreateTokenCopy {
                        source: MoveSubjectRef::AbilitySource,
                        count: ValueExpr::event(EventValueKind::EnteredKickerCount(idx)),
                        tapped: false,
                        attacking: false,
                        sacrifice_at_end_step: false,
                        exile_at_end_step: false,
                        grant_haste: false,
                        nonlegendary: false,
                        override_pt: None,
                        override_colors: None,
                        add_subtype: None,
                        strip_mana_cost: false,
                    }],
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
                    vec![Instruction::CreateTokenCopy {
                        source: MoveSubjectRef::AbilitySource,
                        count: ValueExpr::literal(1),
                        tapped: false,
                        attacking: false,
                        sacrifice_at_end_step: false,
                        exile_at_end_step: false,
                        grant_haste: false,
                        nonlegendary: false,
                        override_pt: is_eternalize.then_some((4, 4)),
                        override_colors: Some(BTreeSet::from([if is_eternalize {
                            Color::Black
                        } else {
                            Color::White
                        }])),
                        add_subtype: Some(zombie),
                        strip_mana_cost: true,
                    }],
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
                    vec![Instruction::RemoveCountersFromPermanent {
                        target_self: true,
                        kind: Some(CounterKind::Time),
                        amount: ValueExpr::literal(1),
                    }],
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
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Enlist]),
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
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Ripple(n)]),
            });
            Ok(())
        }
        "demonstrate" if param.is_none() => {
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::self_cast(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Demonstrate]),
            });
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
        InstructionAst::Destroy { subject } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::Destroy {
                    subject: Box::new(s),
                },
                c,
            ))
        }
        InstructionAst::Exile { actor, subject } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::Exile {
                    actor: *actor,
                    subject: Box::new(s),
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
        InstructionAst::Draw { player, count } => {
            let (p, c) = replace_targets_with_each_player(*player);
            Ok((
                InstructionAst::Draw {
                    player: p,
                    count: count.clone(),
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
        InstructionAst::TapPermanent { subject } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::TapPermanent {
                    subject: Box::new(s),
                },
                c,
            ))
        }
        InstructionAst::UntapPermanent { subject } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::UntapPermanent {
                    subject: Box::new(s),
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
        _ => Err("oracle:overload-unsupported-ins"),
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
    let mut out = Vec::new();
    for part in low_full.split(" and ") {
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
    if low_full == "activated and triggered abilities" {
        return Some(vec![HexproofQuality::ActivatedOrTriggeredAbility]);
    }
    let mut out = Vec::new();
    for part in low_full.split(" and ") {
        let word = part
            .trim()
            .strip_prefix("from ")
            .unwrap_or(part.trim())
            .trim();
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
            "enchantment" | "enchantments" => HexproofQuality::CardType(CardType::Enchantment),
            "instant" | "instants" => HexproofQuality::CardType(CardType::Instant),
            "sorcery" | "sorceries" => HexproofQuality::CardType(CardType::Sorcery),
            "land" | "lands" => HexproofQuality::CardType(CardType::Land),
            "planeswalker" | "planeswalkers" => HexproofQuality::CardType(CardType::Planeswalker),
            "battle" | "battles" => HexproofQuality::CardType(CardType::Battle),
            other => {
                let st = resolve_protection_subtype(other, vocabulary)?;
                HexproofQuality::Subtype(st)
            }
        };
        out.push(q);
    }
    (!out.is_empty()).then_some(out)
}
