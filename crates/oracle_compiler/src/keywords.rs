//! CR 702 keyword parser and structured IR desugarer.

use crate::ast::{KeywordAst, StaticAbilityAst};
use crate::filter::split_keyword_tokens;
use crate::preprocess::EM_DASH;
use crate::vocab::Vocabulary;

/// Attempt to parse an entire paragraph as a CR 702 keyword line.
#[must_use]
pub fn try_parse_keyword_line(
    line: &str,
    vocab: &Vocabulary,
) -> Option<(Vec<KeywordAst>, Vec<StaticAbilityAst>)> {
    if line.contains(':') {
        return None;
    }
    let low = line.to_ascii_lowercase();
    if low.starts_with("enchant ") {
        return None;
    }
    if low.starts_with("protection from ")
        || low.starts_with("hexproof from ")
        || low.starts_with("partner with ")
        || low.starts_with("firebending ")
        || low.starts_with("teamwork ")
        || low == "trample over planeswalkers"
        || low == "mayhem"
        || low == "vanishing"
        || low == "modular\u{2014}sunburst"
    {
        if let Some((kw, st)) = parse_keyword_token(line, vocab) {
            return Some((vec![kw], st.into_iter().collect()));
        }
        return Some((
            vec![KeywordAst {
                name: low,
                parameter: None,
                desugared_rule: Some("ParameterizedKeywordLine".to_string()),
            }],
            Vec::new(),
        ));
    }
    if low.starts_with("equip ")
        && line.contains('{')
        && let Some((kw, st)) = parse_keyword_token(line, vocab)
    {
        return Some((vec![kw], st.into_iter().collect()));
    }

    // Em-dash parameterized keywords often contain commas in their cost
    // (e.g. "Flashback—{1}{U}, Pay 3 life", "Ward—{2}, Discard a card", "Escape—{3}{B}, Exile four other cards").
    // Try parsing the entire line as one keyword token BEFORE splitting on commas!
    if line.contains(EM_DASH)
        && let Some((kw, st)) = parse_keyword_token(line, vocab)
    {
        return Some((vec![kw], st.into_iter().collect()));
    }

    let parts = split_keyword_tokens(line);
    if parts.is_empty() {
        return None;
    }
    let mut kws = Vec::with_capacity(parts.len());
    let mut statics = Vec::new();
    for part in parts {
        let (kw, st) = parse_keyword_token(part, vocab)?;
        kws.push(kw);
        if let Some(s) = st {
            statics.push(s);
        }
    }
    Some((kws, statics))
}

/// Parse one keyword token and attach its desugared rule tag where applicable.
#[must_use]
pub fn parse_keyword_token(
    token: &str,
    vocab: &Vocabulary,
) -> Option<(KeywordAst, Option<StaticAbilityAst>)> {
    let t = token.trim().trim_end_matches('.');
    if t.is_empty() {
        return None;
    }
    let low = t.to_ascii_lowercase();

    // Conspiracy agenda keywords
    if low == "hidden agenda" {
        return Some((
            KeywordAst {
                name: "hidden agenda".to_string(),
                parameter: None,
                desugared_rule: Some("CommandZoneSecretName1".to_string()),
            },
            Some(StaticAbilityAst::ConspiracyAgenda {
                hidden: true,
                named_slots: 1,
            }),
        ));
    }
    if low == "double agenda" {
        return Some((
            KeywordAst {
                name: "double agenda".to_string(),
                parameter: None,
                desugared_rule: Some("CommandZoneSecretName2".to_string()),
            },
            Some(StaticAbilityAst::ConspiracyAgenda {
                hidden: true,
                named_slots: 2,
            }),
        ));
    }

    // Landwalk family (including Un-card denimwalk)
    if low.ends_with("walk") && low.split_whitespace().count() <= 3 {
        return Some((
            KeywordAst {
                name: "landwalk".to_string(),
                parameter: Some(low.trim_end_matches("walk").trim().to_string()),
                desugared_rule: Some("UnblockableIfDefenderControls".to_string()),
            },
            None,
        ));
    }

    // Standalone evergreen / triggered CR 702 keywords
    if vocab.is_evergreen_keyword(&low) {
        let desug = match low.as_str() {
            "prowess" => Some("TriggerNoncreatureSpellSelfPlus1Plus1"),
            "exalted" => Some("TriggerAttacksAlonePlus1Plus1"),
            "extort" => Some("TriggerSpellCastOptionalWBDrain1"),
            "evolve" => Some("TriggerOtherCreatureETBGreaterPTPlus1Counter"),
            "undying" => Some("TriggerDiesWithoutPlus1ReturnWithPlus1"),
            "persist" => Some("TriggerDiesWithoutMinus1ReturnWithMinus1"),
            "battle cry" => Some("TriggerAttacksOtherAttackersPlus1Plus0"),
            "unleash" => Some("OptionalETBPlus1CounterCantBlock"),
            "riot" => Some("ETBChoicePlus1CounterOrHaste"),
            "living weapon" => Some("ETBCreate00GermAttachSelf"),
            "for mirrodin!" => Some("ETBCreate22RebelAttachSelf"),
            _ => None,
        };
        return Some((
            KeywordAst {
                name: low,
                parameter: None,
                desugared_rule: desug.map(ToString::to_string),
            },
            None,
        ));
    }

    // Protection / Hexproof from / Affinity for
    for (pref, name, rule) in [
        ("protection from ", "protection", "StaticProtection"),
        ("hexproof from ", "hexproof_from", "StaticHexproofFrom"),
        ("affinity for ", "affinity", "CostReducePerMatchingObject"),
    ] {
        if let Some(rest) = low.strip_prefix(pref) {
            return Some((
                KeywordAst {
                    name: name.to_string(),
                    parameter: Some(rest.trim().to_string()),
                    desugared_rule: Some(rule.to_string()),
                },
                None,
            ));
        }
    }

    // Typecycling / Cycling
    if let Some(idx) = low.find("cycling") {
        let after = t[idx + 7..].trim().trim_start_matches(EM_DASH).trim();
        if !after.is_empty() {
            let sub = low[..idx].trim();
            return Some((
                KeywordAst {
                    name: if sub.is_empty() {
                        "cycling".to_string()
                    } else {
                        format!("{sub}cycling")
                    },
                    parameter: Some(after.to_string()),
                    desugared_rule: Some(if sub.is_empty() {
                        "ActivatedHandDiscardDraw1".to_string()
                    } else {
                        "ActivatedHandDiscardSearchSubtype".to_string()
                    }),
                },
                None,
            ));
        }
    }

    // Offering ("<Subtype> offering")
    if let Some(sub) = low.strip_suffix(" offering")
        && !sub.trim().is_empty()
        && sub.split_whitespace().count() <= 2
    {
        return Some((
            KeywordAst {
                name: "offering".to_string(),
                parameter: Some(sub.trim().to_string()),
                desugared_rule: Some("FlashAltSacrificeDiscount".to_string()),
            },
            None,
        ));
    }

    // Parameterized cost keywords
    const COST_KEYWORDS: &[(&str, &str)] = &[
        ("morph", "FaceDownCastAndTurnUp"),
        ("megamorph", "FaceDownCastAndTurnUpPlus1Counter"),
        ("disguise", "FaceDownWard2CastAndTurnUp"),
        ("level up", "ActivatedSorceryPutLevelCounter"),
        ("craft with", "ActivatedExileCraftTransform"),
        ("prototype", "CastAltPrototypePT"),
        ("more than meets the eye", "CastAltConverted"),
        ("partner with", "PartnerWithNamedCard"),
        ("champion a", "ETBExileReturnOnLeave"),
        ("champion an", "ETBExileReturnOnLeave"),
        ("bands with other", "BandsWithOther"),
        ("gift a", "OptionalGiftOpponent"),
        ("gift an", "OptionalGiftOpponent"),
        ("renew", "ActivateFromGraveyardExilePutCounters"),
        ("recover", "DiesTriggerPayToReturnFromGraveyard"),
        ("aura swap", "ActivatedSwapAuraFromHand"),
        ("forecast", "ActivatedRevealFromHandUpkeep"),
        ("equip", "ActivatedSorceryAttachControlledCreature"),
        ("ward", "TriggerTargetedByOpponentCounterUnlessPay"),
        ("kicker", "OptionalAdditionalCost"),
        ("multikicker", "RepeatableAdditionalCost"),
        ("flashback", "CastFromGraveyardExile"),
        ("madness", "CastFromMadnessExile"),
        ("miracle", "CastWhenFirstDrawn"),
        ("foretell", "ExileFaceDownCastLater"),
        ("plot", "ExileFaceUpCastFreeLater"),
        ("escape", "CastFromGraveyardAlt"),
        ("evoke", "CastAltSacrificeOnETB"),
        ("dash", "CastAltHasteBounceEndStep"),
        ("blitz", "CastAltHasteDrawSacrificeEndStep"),
        ("warp", "CastAltExileEndStep"),
        ("overload", "CastAltReplaceTargetWithEach"),
        ("cleave", "CastAltOmitBracketedText"),
        ("emerge", "CastAltSacrificeDiscount"),
        ("bestow", "CastAsAuraAlt"),
        ("mutate", "CastAsMutateAlt"),
        ("prowl", "CastAltCombatDamageCondition"),
        ("surge", "CastAltSpellCastCondition"),
        ("spectacle", "CastAltOpponentLostLife"),
        ("buyback", "OptionalAdditionalCostReturnToHand"),
        ("entwine", "OptionalAdditionalCostChooseAllModes"),
        ("replicate", "RepeatableAdditionalCostCopySpell"),
        ("unearth", "ActivateFromGraveyardReturnHasteExile"),
        ("encore", "ActivateFromGraveyardTokenPerOpponent"),
        ("disturb", "CastTransformedFromGraveyard"),
        ("embalm", "ActivateFromGraveyardExileTokenCopy"),
        ("eternalize", "ActivateFromGraveyardExile44TokenCopy"),
        ("scavenge", "ActivateFromGraveyardExilePutCounters"),
        ("outlast", "ActivateTapSorceryPutPlusOneCounter"),
        (
            "commander ninjutsu",
            "ActivateFromHandOrCommandReturnUnblocked",
        ),
        ("ninjutsu", "ActivateFromHandReturnUnblocked"),
        ("reconfigure", "ActivateAttachOrUnattachCreatureEquipment"),
        ("fortify", "ActivateAttachToLand"),
        ("echo", "UpkeepPayOrSacrificeFirstTurn"),
        ("cumulative upkeep", "UpkeepAgeCounterPayOrSacrifice"),
        ("transmute", "ActivateHandDiscardSearchSameMV"),
        ("transfigure", "ActivateSacrificeSearchSameMVCreature"),
        ("squad", "RepeatableAdditionalCostCreateTokenCopy"),
        ("offspring", "OptionalAdditionalCostCreate11TokenCopy"),
        ("harmonize", "CastFromGraveyardTapCreatureDiscount"),
        ("freerunning", "CastAltCombatDamageCondition"),
        ("sneak", "CastAltReturnUnblocked"),
        ("mayhem", "CastFromGraveyardIfDiscardedThisTurn"),
        ("web-slinging", "CastAltReturnTappedCreature"),
        ("escalate", "AdditionalCostPerExtraMode"),
        ("splice onto", "SpliceOntoSpell"),
        ("augment", "UnCardAugmentHost"),
        ("saddle", "ActivatedTapCreaturesPowerNSaddleMount"),
        ("station", "ActivatedTapCreaturePutChargeCounters"),
    ];
    for (kw_name, rule) in COST_KEYWORDS {
        if let Some(after) = low.strip_prefix(kw_name)
            && (after.starts_with(' ') || after.starts_with(EM_DASH))
        {
            let param = t[kw_name.len()..].trim().trim_start_matches(EM_DASH).trim();
            if !param.is_empty() {
                return Some((
                    KeywordAst {
                        name: (*kw_name).to_string(),
                        parameter: Some(param.to_string()),
                        desugared_rule: Some((*rule).to_string()),
                    },
                    None,
                ));
            }
        }
    }

    // Parameterized numeric keywords ("Crew 3", "Toxic 1", "Afterlife 2", "Suspend 3 - {U}", etc.)
    const NUM_KEYWORDS: &[(&str, &str)] = &[
        ("crew", "ActivatedTapCreaturesPowerNAnimateVehicle"),
        ("saddle", "ActivatedTapCreaturesPowerNSaddleMount"),
        ("toxic", "CombatDamageGivesNPoisonCounters"),
        ("poisonous", "CombatDamageTriggerNPoisonCounters"),
        ("annihilator", "AttackTriggerDefenderSacrificesNPermanents"),
        ("bushido", "BlockOrBlockedTriggerGetsPlusNPlusN"),
        ("rampage", "BlockedBeyondFirstGetsPlusNPlusN"),
        ("frenzy", "UnblockedTriggerGetsPlusNPlus0"),
        ("afflict", "BecomesBlockedTriggerDefenderLosesNLife"),
        ("renown", "CombatDamagePlayerPutNPlusOneCounters"),
        ("afterlife", "DiesTriggerCreateN11SpiritTokens"),
        ("fabricate", "ETBChoiceNPlusOneCountersOrNServos"),
        ("backup", "ETBPutNPlusOneCountersGrantAbilities"),
        ("bloodthirst", "ETBWithNPlusOneCountersIfOpponentDamaged"),
        ("devour", "ETBSacrificeCreaturesEnterWithNPerSac"),
        ("graft", "ETBWithNPlusOneCountersMoveOnOtherETB"),
        ("modular", "ETBWithNPlusOneCountersMoveOnDeath"),
        ("soulshift", "DiesTriggerReturnSpiritMVLessOrEqualN"),
        ("vanishing", "ETBWithNTimeCountersUpkeepRemoveSacrifice"),
        ("fading", "ETBWithNFadeCountersUpkeepRemoveSacrifice"),
        ("hideaway", "ETBLookTopNExileOne"),
        (
            "mobilize",
            "AttackTriggerCreateN11WarriorTokensTappedAttacking",
        ),
        ("casualty", "OptionalAdditionalSacrificePowerNCopySpell"),
        ("tribute", "ETBOpponentChoiceNPlusOneCounters"),
        ("absorb", "PreventNDamageFromEachSource"),
        ("dredge", "ReplaceDrawWithMillNReturnSelf"),
        ("amplify", "ETBRevealMatchingCardsEnterWithNCounters"),
        ("suspend", "SuspendN"),
        ("reinforce", "ReinforceN"),
        ("impending", "ImpendingN"),
        ("awaken", "AwakenN"),
        ("ripple", "RippleN"),
        ("firebending", "AttackTriggerAddNRedMana"),
        ("earthbend", "EarthbendN"),
        ("airbend", "AirbendN"),
        ("waterbend", "WaterbendN"),
    ];
    for (kw_name, rule) in NUM_KEYWORDS {
        if let Some(after) = low.strip_prefix(kw_name)
            && after.starts_with(' ')
        {
            let param = t[kw_name.len()..].trim();
            if !param.is_empty() {
                return Some((
                    KeywordAst {
                        name: (*kw_name).to_string(),
                        parameter: Some(param.to_string()),
                        desugared_rule: Some((*rule).to_string()),
                    },
                    None,
                ));
            }
        }
    }

    None
}
