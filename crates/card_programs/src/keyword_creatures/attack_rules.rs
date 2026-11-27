//! Attack requirements can coexist with a creature's other printed rules.
use super::{InventoryCard, Profile, without_reminder};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::program::{AttackRequirement, Program};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    if let Some(remainder) = text.trim().strip_prefix(concat!(
        "If an effect would create one or more tokens under your control, ",
        "it creates twice that many of those tokens instead."
    )) && face.type_line.as_deref() == Some("Enchantment")
    {
        use spellwake_engine::characteristics::StaticAbilityId;
        let mut program =
            Program::enchantment(Some(super::fixed_cost(face.mana_cost.as_deref()?)?));
        program.continuous = Some(
            spellwake_engine::characteristics::StaticContinuousAbility::Modifier {
                id: StaticAbilityId(0),
                rule: spellwake_engine::characteristics::StaticModifierRule::DoubleCreatedTokens,
            },
        );
        match remainder.trim() {
            "" => {}
            concat!(
                "If an effect would put one or more counters on a permanent you control, ",
                "it puts twice that many of those counters on that permanent instead."
            ) => program.additional_battlefield_abilities.push(
                spellwake_engine::characteristics::StaticContinuousAbility::Modifier {
                    id: StaticAbilityId(1),
                    rule:
                        spellwake_engine::characteristics::StaticModifierRule::DoublePlacedCounters,
                },
            ),
            _ => return None,
        }
        return Some(program);
    }

    if let [retain, bonus] = text
        .trim()
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .as_slice()
        && let Some(color) = retain
            .strip_prefix("You don't lose unspent ")
            .and_then(|line| line.strip_suffix(" mana as steps and phases end."))
        && let Some((subject, amount)) = bonus.split_once(" gets +1/+1 for each unspent ")
        && (subject == "This creature"
            || subject == face.name
            || face
                .name
                .split_once(',')
                .is_some_and(|(name, _)| name == subject))
        && amount.strip_suffix(" mana you have.") == Some(color)
    {
        use spellwake_engine::characteristics::{
            ControllerSelector, NumericExpression, NumericPair, PermanentSelector, StaticAbilityId,
            StaticPart, StaticParts,
        };
        use spellwake_engine::mana::ManaType;
        let kind = match color {
            "white" => ManaType::White,
            "blue" => ManaType::Blue,
            "black" => ManaType::Black,
            "red" => ManaType::Red,
            "green" => ManaType::Green,
            "colorless" => ManaType::Colourless,
            _ => return None,
        };
        let mut program =
            super::ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
        program.continuous = Some(
            spellwake_engine::characteristics::StaticContinuousAbility::Modifier {
                id: StaticAbilityId(0),
                rule: spellwake_engine::characteristics::StaticModifierRule::RetainMana(kind),
            },
        );
        super::army_grants::add_battlefield_ability(
            &mut program,
            PermanentSelector {
                controller: ControllerSelector::SourceItself,
                ..Default::default()
            },
            StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::PlayerStat(
                    spellwake_engine::characteristics::StaticPlayerStat::ManaInPool(kind),
                ),
                toughness: NumericExpression::PlayerStat(
                    spellwake_engine::characteristics::StaticPlayerStat::ManaInPool(kind),
                ),
            })]),
        )?;
        return Some(program);
    }

    let entry_copy = match text.trim() {
        "You may have this creature enter as a copy of any creature card in a graveyard." => {
            Some(spellwake_engine::program::EntryCopyRule::GraveyardCreature)
        }
        "You may have this creature enter as a copy of a creature or planeswalker you control, except it enters with an additional +1/+1 counter on it if it's a creature, it enters with an additional loyalty counter on it if it's a planeswalker, and it isn't legendary." => {
            Some(spellwake_engine::program::EntryCopyRule::ControlledCreatureOrPlaneswalker)
        }
        _ => None,
    };
    if let Some(rule) = entry_copy {
        let mut base =
            super::ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
        base.entry_copy = Some(rule);
        return Some(base);
    }
    if let Some((heading, body))=text.trim().split_once('\n')
        && let Some(grant)=heading.strip_prefix(&format!("Eminence — As long as {} is in the command zone or on the battlefield, other ",face.name))
        && let Some((subtype,amount))=grant.strip_suffix("} less to cast.").and_then(|line|line.split_once(" spells you cast cost {"))
        && let Some((other,trigger))=body.rsplit_once('\n')
        && let Some(attacker_type)=trigger.strip_prefix("Whenever one or more ").and_then(|line|line.strip_suffix("s you control attack, draw that many cards, then you may put a permanent card from your hand onto the battlefield.")) {
        use spellwake_engine::{characteristics::{StaticAbilityId}, program::{BodyTarget, CardType, ExecutableBody, Instruction, ResolvingPlayer, TriggerGrouping, TriggeredAbility, TriggeredAbilityId}};
        let mut reduced=card.clone(); reduced.faces[0].oracle_text=Some(other.to_owned());
        let mut base=super::compile(&reduced,vocabulary,Profile::Current).ok()?;
        if base.continuous.is_some() || !base.triggered.is_empty() {return None;}
        base.continuous=Some(spellwake_engine::characteristics::StaticContinuousAbility::CostModifier { id: StaticAbilityId(0), modifier: spellwake_engine::characteristics::SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { subtype: vocabulary.resolve_label(subtype)?, amount: std::num::NonZeroU8::new(amount.parse().ok()?)? } });
        base.triggered.push(TriggeredAbility {id:TriggeredAbilityId(0),condition:spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::any(), spellwake_engine::program::AttackTriggerMode::GroupSubtype(vocabulary.resolve_label(attacker_type)?)),grouping:TriggerGrouping::EachOccurrence,
            body:ExecutableBody::new(BodyTarget::None,vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::DeclaredSubtypeAttackers)),
                Instruction::PutHandPermanentsOntoBattlefield {player:ResolvingPlayer::Controller,types:[CardType::Land,CardType::Artifact,CardType::Creature,CardType::Enchantment,CardType::Planeswalker].into(), maximum:1,tapped:false}])});
        return Some(base);
    }
    if let Some((minimum, keyword_list)) = text
        .trim()
        .strip_prefix("Colorless spells you cast from your hand with mana value ")
        .and_then(|s| s.strip_suffix(".\""))
        .and_then(|s| s.split_once(" or greater have \""))
    {
        let mut keywords = keyword_list.split(", ");
        if keywords.next()? != "Cascade" {
            return None;
        }
        let mut count = 1u8;
        for keyword in keywords {
            if keyword != "cascade" {
                return None;
            }
            count = count.checked_add(1)?;
        }
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(String::new());
        let mut base = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        base.continuous=Some(spellwake_engine::characteristics::StaticContinuousAbility::CastPermission { id: spellwake_engine::characteristics::StaticAbilityId(0), permission: spellwake_engine::characteristics::CastPermissionRule::ColorlessHandSpellsHaveCascade { minimum_mana_value: minimum.parse().ok()?, count: std::num::NonZeroU8::new(count)? } });
        return Some(base);
    }
    if let Some((replicate, grant)) = text.trim().split_once('\n')
        && let Some(price) = replicate.trim().strip_prefix("Replicate ")
        && let Some(subtype) = grant.strip_prefix("Each ").and_then(|line| {
            line.strip_suffix(
                " spell you cast has replicate. The replicate cost is equal to its mana cost.",
            )
        })
    {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(String::new());
        let mut base = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        use spellwake_engine::{
            characteristics::StaticAbilityId,
            program::{AdditionalCastCost, CastCopyKeyword, TriggeredAbility, TriggeredAbilityId},
        };
        let index = base.additional_cast_costs.len();
        base.additional_cast_costs
            .push(AdditionalCastCost::Replicate(super::fixed_cost(price)?));
        base.triggered.push(TriggeredAbility::spell_copies(
            TriggeredAbilityId(0),
            CastCopyKeyword::Replicate(index),
        ));
        base.continuous=Some(spellwake_engine::characteristics::StaticContinuousAbility::CastPermission { id: StaticAbilityId(0), permission: spellwake_engine::characteristics::CastPermissionRule::SubtypeSpellsHaveReplicate(vocabulary.resolve_label(subtype)?) });
        return Some(base);
    }
    if text.trim() == "Nonartifact spells you cast have improvise."
        && face.type_line.as_deref() == Some("Artifact")
        && card.name == face.name
        && card.layout == "normal"
    {
        let mut base =
            Program::artifact(Some(super::fixed_cost(face.mana_cost.as_deref()?)?), vec![]);
        base.continuous=Some(spellwake_engine::characteristics::StaticContinuousAbility::CostModifier { id: spellwake_engine::characteristics::StaticAbilityId(0), modifier: spellwake_engine::characteristics::SpellCostModifier::NonartifactSpellsHaveImprovise });
        return Some(base);
    }
    if let Some(printed) = text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("Metalcraft — Equipment you control have equip "))
    {
        let (price, threshold) = printed
            .strip_prefix("Metalcraft — Equipment you control have equip ")?
            .split_once(" as long as you control ")?;
        let count = super::ordinary_text::amount(threshold.strip_suffix(" or more artifacts.")?)?
            .try_into()
            .ok()?;
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(
            text.lines()
                .filter(|line| line.trim() != printed)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        let ability = spellwake_engine::attachment::AttachAbility::Equip;
        let grant = spellwake_engine::activated::ActivatedAbility {
            id: spellwake_engine::activated::ActivatedAbilityId(0),
            source_cost: spellwake_engine::activated::SourceCost::None,
            mana_cost: super::fixed_cost(price)?,
            body: spellwake_engine::program::ExecutableBody::new(
                spellwake_engine::program::BodyTarget::SingleCreature,
                vec![spellwake_engine::program::Instruction::Attach(ability)],
            ),
        };
        use spellwake_engine::characteristics::{
            ControllerSelector, PermanentSelector, StaticPart, StaticParts,
        };
        super::army_grants::add_battlefield_ability(
            &mut program,
            PermanentSelector {
                controller: ControllerSelector::SourceController,
                artifact_subtype: Some(spellwake_engine::program::ArtifactSubtype::Equipment),
                required_types: [spellwake_engine::program::CardType::Artifact].into(),
                controller_type_threshold: Some((
                    spellwake_engine::program::CardType::Artifact,
                    std::num::NonZeroU16::new(count)?,
                )),
                ..Default::default()
            },
            StaticParts(vec![StaticPart::GrantActivated(Box::new(grant))]),
        )?;
        return Some(program);
    }
    if let Some(line) = text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("If one or more +1/+1 counters would be put on "))
    {
        let (subject, change) = line
            .strip_prefix("If one or more +1/+1 counters would be put on ")?
            .split_once(", ")?;
        if (subject != face.name
            && Some(subject) != face.name.split(',').next()
            && subject != "this creature")
            || change.strip_prefix("that many plus one +1/+1 counters ")
                != Some("are put on it instead.")
        {
            return None;
        }
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(
            text.lines()
                .filter(|part| part.trim() != line)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        if program.permanent().is_none() || program.continuous.is_some() {
            return None;
        }
        program.continuous=Some(spellwake_engine::characteristics::StaticContinuousAbility::Modifier { id: spellwake_engine::characteristics::StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticModifierRule::AdditionalSelfPlusOneCounter });
        return Some(program);
    }
    let opponent_rule = "During your turn, your opponents can't cast spells or activate abilities of artifacts, creatures, or enchantments.";
    if text.lines().any(|line| line.trim() == opponent_rule) {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(
            text.lines()
                .filter(|line| line.trim() != opponent_rule)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        if program.permanent().is_none() || program.continuous.is_some() {
            return None;
        }
        program.continuous=Some(spellwake_engine::characteristics::StaticContinuousAbility::Restriction { id: spellwake_engine::characteristics::StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticRestriction::RestrictOpponentsDuringOwnTurn });
        return Some(program);
    }
    let death_rule = "If a creature dying causes a triggered ability of a permanent you control to trigger, that ability triggers an additional time.";
    if text.lines().any(|line| line.trim() == death_rule) {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(
            text.lines()
                .filter(|line| line.trim() != death_rule)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let mut base = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        base.permanent()?;
        let id = base
            .continuous
            .iter()
            .chain(&base.additional_battlefield_abilities)
            .map(|a| a.id().0)
            .max()
            .unwrap_or(0)
            .checked_add(1)?;
        let death=spellwake_engine::characteristics::StaticContinuousAbility::Modifier { id: spellwake_engine::characteristics::StaticAbilityId(id), rule: spellwake_engine::characteristics::StaticModifierRule::AdditionalCreatureDeathTrigger };
        if let Some(existing) = base.continuous.replace(death) {
            base.additional_battlefield_abilities.push(existing);
        }
        return Some(base);
    }
    if text
        .lines()
        .any(|line| line.trim() == "Your opponents can't gain life.")
    {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(
            text.lines()
                .filter(|line| line.trim() != "Your opponents can't gain life.")
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        if program.permanent().is_none() || program.continuous.is_some() {
            return None;
        }
        program.continuous = Some(
            spellwake_engine::characteristics::StaticContinuousAbility::Restriction {
                id: spellwake_engine::characteristics::StaticAbilityId(0),
                rule: spellwake_engine::characteristics::StaticRestriction::OpponentsCannotGainLife,
            },
        );
        return Some(program);
    }
    if let Some(remainder)=text.trim().strip_suffix("Lethal damage dealt to creatures you control is determined by their power rather than their toughness.") {
        let mut reduced=card.clone();reduced.faces[0].oracle_text=Some(remainder.trim_end().to_owned());
        let mut program=super::compile(&reduced,vocabulary,Profile::Current).ok()?;
        if program.continuous.is_some() || program.permanent().is_none() {return None;}
        program.continuous=Some(spellwake_engine::characteristics::StaticContinuousAbility::DamageRule { id: spellwake_engine::characteristics::StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticDamageRule::LethalUsesPower });
        return Some(program);
    }
    const DOUBLE_DAMAGE: &str = concat!(
        "If a source would deal damage to an opponent or a permanent an opponent controls, ",
        "that source deals double that damage to that player or permanent instead."
    );
    const PREVENT_HALF: &str = "If a source would deal damage to you or a permanent you control, prevent half that damage, rounded up.";
    if let Some(remaining) = text
        .trim()
        .strip_suffix(PREVENT_HALF)
        .and_then(|text| text.trim_end().strip_suffix(DOUBLE_DAMAGE))
    {
        use spellwake_engine::characteristics::StaticAbilityId;
        let mut program =
            super::ordinary_creature(card, vocabulary, Profile::Current, Some(remaining.trim()))
                .ok()?;
        if program.continuous.is_some() || !program.additional_battlefield_abilities.is_empty() {
            return None;
        }
        program.continuous = Some(
            spellwake_engine::characteristics::StaticContinuousAbility::DamageRule {
                id: StaticAbilityId(0),
                rule: spellwake_engine::characteristics::StaticDamageRule::DoubleToOpponents,
            },
        );
        program.additional_battlefield_abilities.push(
            spellwake_engine::characteristics::StaticContinuousAbility::DamageRule {
                id: StaticAbilityId(1),
                rule: spellwake_engine::characteristics::StaticDamageRule::PreventHalfIncoming,
            },
        );
        return Some(program);
    }
    let combat_prevention = "Prevent all combat damage that would be dealt to attacking artifact creatures you control.";
    if text.lines().any(|line| line.trim() == combat_prevention) {
        use spellwake_engine::characteristics::StaticAbilityId;
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(
            text.lines()
                .filter(|line| line.trim() != combat_prevention)
                .collect::<Vec<_>>()
                .join(
                    "
",
                ),
        );
        let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        if program.continuous.is_some() || program.permanent().is_none() {
            return None;
        }
        program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::DamageRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticDamageRule::PreventCombatToAttackingArtifacts });
        return Some(program);
    }
    if text.lines().any(|line| {
        line.trim()
            == "Prevent all combat damage that would be dealt to and dealt by this creature."
    }) {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text=Some(text.lines().filter(|line|line.trim()!="Prevent all combat damage that would be dealt to and dealt by this creature.").collect::<Vec<_>>().join("\n"));
        let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        if program.continuous.is_some()
            || !program.has_type(spellwake_engine::program::CardType::Creature)
        {
            return None;
        }
        program.continuous = Some(
            spellwake_engine::characteristics::StaticContinuousAbility::DamageRule {
                id: spellwake_engine::characteristics::StaticAbilityId(0),
                rule: spellwake_engine::characteristics::StaticDamageRule::PreventSourceCombat,
            },
        );
        return Some(program);
    }
    if card.layout == "normal"
        && card.name == face.name
        && face.type_line.as_deref() == Some("Enchantment — Aura")
        && text.lines().map(str::trim).eq([
            "Enchant creature",
            "Enchanted creature can't attack you or planeswalkers you control.",
            "Enchanted creature can't block creatures you control.",
        ])
    {
        let mut aura = Program::aura(
            Some(super::fixed_cost(face.mana_cost.as_deref()?)?),
            spellwake_engine::attachment::EnchantRestriction::Creature,
        );
        aura.attack_requirement = Some(AttackRequirement::EnchantedCannotAttackOrBlockController);
        return Some(aura);
    }
    let mut requirement = None;
    use spellwake_engine::program::{
        BlockDeclarationRule, BodyTarget, ExecutableBody, Instruction, ObjectFilter,
    };
    let mut combat_trigger = None;
    let mut entry_search = None;
    let mut limits = spellwake_engine::program::CombatDeclarationLimits::default();
    let mut remaining = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let short = card.name.split(',').next()?;
        if let Some(search) = line.strip_prefix(&format!("When {short} enters, you may "))
            && let Some(mut body) = super::basic_land_search::body(search)
        {
            body.instructions.insert(
                0,
                Instruction::OptionalEffect {
                    chooser: spellwake_engine::program::ResolvingPlayer::Controller,
                },
            );
            if entry_search.replace(body).is_some() {
                return None;
            }
            continue;
        }
        let declaration_rule = if let Some(subtype) = line
            .strip_prefix(&format!(
                "Whenever {short} attacks for the first time each turn, untap it and all "
            ))
            .and_then(|s| {
                s.strip_suffix(
                    " you control. After this phase, there is an additional combat phase.",
                )
            }) {
            Some((
                spellwake_engine::program::TriggerSpec::attacks(
                    spellwake_engine::program::ObjectFilter::self_object(),
                    spellwake_engine::program::AttackTriggerMode::FirstThisTurn {
                        highest_life: false,
                    },
                ),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::additional_combat(
                            spellwake_engine::program::CombatUntapGroup::SourceAndControlledSubtype(
                                vocabulary.resolve_label(subtype)?,
                            ),
                        ),
                    ))],
                ),
            ))
        } else if line
            == "Whenever this creature attacks for the first time each turn, if it's attacking the player with the most life or tied for most life, untap all attacking creatures. After this phase, there is an additional combat phase."
        {
            Some((spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::self_object(), spellwake_engine::program::AttackTriggerMode::FirstThisTurn { highest_life: true }),ExecutableBody::new(BodyTarget::None,vec![Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::additional_combat(spellwake_engine::program::CombatUntapGroup::AttackersIfSourceAttacksHighestLife)))])))
        } else if line
            == format!(
                "Whenever {short} attacks, each opponent can't block with more than one creature this combat."
            )
        {
            Some((
                spellwake_engine::program::TriggerSpec::self_attacks(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::block_declarations(
                            BlockDeclarationRule::EachOpponentOneBlocker,
                        ),
                    ))],
                ),
            ))
        } else if line
            == format!(
                "Whenever {short} and at least three other creatures attack, you choose which creatures block this combat and how those creatures block."
            )
        {
            Some((
                spellwake_engine::program::TriggerSpec::attacks(
                    spellwake_engine::program::ObjectFilter::self_object(),
                    spellwake_engine::program::AttackTriggerMode::WithAtLeastOthers(3),
                ),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::block_declarations(
                            BlockDeclarationRule::ControllerChooses,
                        ),
                    ))],
                ),
            ))
        } else if line
            == "When this creature enters during the declare attackers step, you may reselect which player or permanent target attacking creature is attacking."
        {
            Some((
                spellwake_engine::program::TriggerSpec::self_enters().only_if(
                    spellwake_engine::program::TriggerCondition::DuringStep(
                        spellwake_engine::turn::Step::DeclareAttackers,
                    ),
                ),
                ExecutableBody::new(
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().attacking()),
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::reselect_attack_defender(),
                    ))],
                ),
            ))
        } else {
            None
        };
        if let Some(trigger) = declaration_rule {
            if combat_trigger.replace(trigger).is_some() {
                return None;
            }
            continue;
        }

        if line == "No more than one creature can attack each combat." {
            limits.one_attacker = true;
            continue;
        }
        if line == "No more than one creature can block each combat." {
            limits.one_blocker = true;
            continue;
        }
        use spellwake_engine::program::AttackCostSymbol;
        let rule = if line == format!("As long as {short} is tapped, no more than one creature can attack you each combat.") {
            AttackRequirement::LimitAttacksAtTappedController
        } else if line == "Creatures can't attack planeswalkers you control unless their controller pays {1} for each creature they control that's attacking a planeswalker you control." {
            AttackRequirement::PayPerAttacker { protect_player: false, symbol: AttackCostSymbol::OneGeneric }
        } else if let Some(symbol) = line.strip_prefix("Creatures can't attack you or planeswalkers you control unless their controller pays ")
            .and_then(|tail| tail.strip_suffix(" for each of those creatures.")) {
            let symbol = match symbol { "{1}" => AttackCostSymbol::OneGeneric, "{W/P}" => AttackCostSymbol::WhitePhyrexian, _ => return None };
            AttackRequirement::PayPerAttacker { protect_player: true, symbol }
        } else if line == "Creatures with flying can't attack you or planeswalkers you control." {
            AttackRequirement::FlyingCannotAttackController
        } else if line == "This creature attacks each combat if able." {
            AttackRequirement::SourceAttacks
        } else if line.strip_suffix(" attacks each combat if able and can't attack its owner or planeswalkers its owner controls.")
            .is_some_and(|s| s == face.name || face.name.split_once(',').is_some_and(|(short,_)| s == short)) {
            AttackRequirement::SourceAttacksExceptOwner
        } else if line == "All creatures have double strike and attack each combat if able." {
            remaining.push("All creatures have double strike.");
            AttackRequirement::AllCreaturesAttack
        } else { remaining.push(line); continue; };
        if requirement.replace(rule).is_some() {
            return None;
        }
    }
    if requirement.is_none() && combat_trigger.is_none() && limits == Default::default() {
        return None;
    }
    let mut reduced = card.clone();
    let remaining = remaining.join("\n");
    let mut base = if remaining.trim().is_empty()
        && card.layout == "normal"
        && face.name == card.name
        && face.type_line.as_deref() == Some("Artifact")
    {
        Program::artifact(
            Some(super::fixed_cost(face.mana_cost.as_deref()?)?),
            Vec::new(),
        )
    } else {
        reduced.faces[0].oracle_text = Some(remaining);
        super::compile(&reduced, vocabulary, Profile::Current).ok()?
    };
    if base.permanent().is_none() || base.attack_requirement.is_some() {
        return None;
    }
    for (condition, body) in entry_search
        .into_iter()
        .map(|body| {
            (
                spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                body,
            )
        })
        .chain(combat_trigger)
    {
        use spellwake_engine::program::{TriggerGrouping, TriggeredAbility, TriggeredAbilityId};
        let id = base
            .triggered
            .iter()
            .map(|trigger| trigger.id.0)
            .max()
            .map_or(Some(0), |n| n.checked_add(1))?;
        base.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition,
            grouping: TriggerGrouping::EachOccurrence,
            body,
        });
    }
    base.attack_requirement = requirement;
    base.combat_limits = limits;
    Some(base)
}
