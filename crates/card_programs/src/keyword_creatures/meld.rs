//! Source-backed meld sets. Both components and the joined face are independent Oracle rows.
use super::*;
use spellwake_engine::activated::{
    ActivatedAbility, ActivatedAbilityId, ActivationRestriction, SourceCost,
};
use spellwake_engine::mana::ManaCost;
use spellwake_engine::program::{
    BodyTarget, CardType, ExecutableBody, Instruction, PlayerRelation, ProgramKind,
    TriggerCondition, TriggerGrouping, TriggerSpec,
};
use spellwake_engine::turn::Step;

// Single-use local parser output for meld trigger vs activated clause; never stored in collections.
#[allow(clippy::large_enum_variant)]
enum MeldClauseKind {
    StepTrigger {
        step: Step,
        extra_condition: Option<TriggerCondition>,
    },
    Activated {
        mana_cost: ManaCost,
        tap: bool,
        sorcery: bool,
    },
}

fn parse_meld_clause(line: &str, partner: &str, result_name: &str) -> Option<MeldClauseKind> {
    let trimmed = line.trim();
    if !trimmed.contains(partner) || !trimmed.contains(&format!("meld them into {result_name}")) {
        return None;
    }
    if trimmed.starts_with("At the beginning of combat on your turn, if you both own and control ")
    {
        return Some(MeldClauseKind::StepTrigger {
            step: Step::BeginningCombat,
            extra_condition: None,
        });
    }
    if trimmed.starts_with("At the beginning of your end step, if you both own and control ") {
        return Some(MeldClauseKind::StepTrigger {
            step: Step::End,
            extra_condition: None,
        });
    }
    if trimmed.starts_with(
        "At the beginning of your upkeep, if there are four or more land cards in your graveyard and you both own and control ",
    ) {
        return Some(MeldClauseKind::StepTrigger {
            step: Step::Upkeep,
            extra_condition: Some(TriggerCondition::GraveyardCardTypeCount {
                kind: Some(CardType::Land),
                minimum: 4,
            }),
        });
    }
    if let Some((cost_raw, eff_raw)) = trimmed.split_once(": If you both own and control ") {
        let sorcery = eff_raw.ends_with("Activate only as a sorcery.");
        let mut tap = false;
        let mut mana_part = None;
        for atom in cost_raw.split(',').map(str::trim) {
            if atom == "{T}" {
                tap = true;
            } else if atom.starts_with('{') && atom.ends_with('}') && mana_part.is_none() {
                mana_part = Some(atom);
            } else {
                return None;
            }
        }
        let mana_cost = parse_simple_meld_mana(mana_part?)?;
        return Some(MeldClauseKind::Activated {
            mana_cost,
            tap,
            sorcery,
        });
    }
    None
}

fn parse_simple_meld_mana(raw: &str) -> Option<ManaCost> {
    let inner = raw.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut cost = ManaCost::free();
    for sym in inner.split("}{") {
        let add = |cost: &mut ManaCost, mt: spellwake_engine::mana::ManaType| {
            let entry = cost.required.entry(mt).or_insert_with(|| 0.into());
            *entry = entry.plus(&1.into());
        };
        match sym {
            "W" => add(&mut cost, spellwake_engine::mana::ManaType::White),
            "U" => add(&mut cost, spellwake_engine::mana::ManaType::Blue),
            "B" => add(&mut cost, spellwake_engine::mana::ManaType::Black),
            "R" => add(&mut cost, spellwake_engine::mana::ManaType::Red),
            "G" => add(&mut cost, spellwake_engine::mana::ManaType::Green),
            "C" => add(&mut cost, spellwake_engine::mana::ManaType::Colourless),
            s if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
                let n: i64 = s.parse().ok()?;
                cost.generic = cost.generic.plus(&n.into());
            }
            _ => return None,
        }
    }
    Some(cost)
}

pub(super) fn compile(
    card: &InventoryCard,
    inventory: &Inventory,
    vocabulary: &CreatureTypeVocabulary,
    loyalty_source_verified: bool,
) -> Option<Program> {
    let family = inventory.meld_set(card.oracle_id.as_deref()?)?;
    let [first, second] = &family.parts;
    let partner = if &card.name == first {
        second
    } else if &card.name == second {
        first
    } else {
        return None;
    };
    let other = inventory
        .cards
        .iter()
        .find(|row| row.layout == "meld" && &row.name == partner)?;
    let result = inventory
        .cards
        .iter()
        .find(|row| row.layout == "meld" && row.name == family.result)?;
    if [other, result]
        .iter()
        .any(|r| r.oracle_id.as_deref().and_then(|id| inventory.meld_set(id)) != Some(family))
    {
        return None;
    }
    let normalize = |row: &InventoryCard| {
        let [face] = row.faces.as_slice() else {
            return None;
        };
        let text = without_reminder(face.oracle_text.as_deref()?)?;
        let mut single = row.clone();
        single.layout = "normal".into();
        single.faces[0].oracle_text = Some(text.trim().to_owned());
        Some(single)
    };
    let mut joined = normalize(result)?;
    if !joined.faces[0]
        .mana_cost
        .as_deref()
        .unwrap_or("")
        .is_empty()
    {
        return None;
    }
    joined.faces[0].mana_cost = Some("{0}".into());
    let mut back = if joined.faces[0]
        .type_line
        .as_deref()
        .is_some_and(|tl| tl.contains("Planeswalker"))
    {
        planeswalkers::compile(&joined, inventory, vocabulary, loyalty_source_verified).or_else(
            || {
                oracle_lowering::compile_planeswalker(
                    &joined,
                    inventory,
                    vocabulary,
                    loyalty_source_verified,
                )
            },
        )?
    } else {
        super::compile(&joined, vocabulary, Profile::Current).ok()?
    };
    let ProgramKind::Permanent(p) = &mut back.kind else {
        return None;
    };
    p.casting_cost = None;
    back.names = ObjectNames::single(EnglishCardName::new(result.name.clone()).ok()?);
    let mut front = normalize(card)?;
    let other = normalize(other)?;
    let mut remaining = Vec::new();
    let mut meld_clause: Option<MeldClauseKind> = None;
    for line in front.faces[0].oracle_text.as_deref()?.lines() {
        if let Some(kind) = parse_meld_clause(line, partner, &result.name) {
            if meld_clause.is_some() {
                return None;
            }
            meld_clause = Some(kind);
        } else {
            remaining.push(line);
        }
    }
    if meld_clause.is_none()
        && !other.faces[0]
            .oracle_text
            .as_deref()?
            .lines()
            .any(|line| parse_meld_clause(line, &card.name, &result.name).is_some())
    {
        return None;
    }
    front.faces[0].oracle_text = Some(remaining.join("\n"));
    let mut program = super::compile(&front, vocabulary, Profile::Current).ok()?;
    let mate = EnglishCardName::new(partner.clone()).ok()?;
    program.meld_partner = Some(mate.clone());
    if let Some(clause) = meld_clause {
        let meld_body = ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::ChooseMeldPartner(mate),
                Instruction::ExileMeldPair,
                Instruction::MeldExiledPair(Box::new(back)),
            ],
        );
        match clause {
            MeldClauseKind::StepTrigger {
                step,
                extra_condition,
            } => {
                let cond = match extra_condition {
                    None => TriggerCondition::ControlsMeldPair,
                    Some(extra) => TriggerCondition::And(
                        Box::new(TriggerCondition::ControlsMeldPair),
                        Box::new(extra),
                    ),
                };
                let next_id = program
                    .triggered
                    .iter()
                    .map(|t| t.id.0)
                    .max()
                    .map_or(0, |n| n + 1);
                program.triggered.push(TriggeredAbility {
                    id: TriggeredAbilityId(next_id),
                    condition: TriggerSpec::step_began(step, PlayerRelation::Controller)
                        .only_if(cond),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: meld_body,
                });
            }
            MeldClauseKind::Activated {
                mana_cost,
                tap,
                sorcery,
            } => {
                let next_id = program
                    .activated
                    .iter()
                    .map(|a| a.id.0)
                    .max()
                    .map_or(0, |n| n + 1);
                let id = ActivatedAbilityId(next_id);
                if sorcery {
                    program
                        .activation_restrictions
                        .insert(id, ActivationRestriction::Sorcery);
                }
                program.activated.push(ActivatedAbility {
                    id,
                    mana_cost,
                    source_cost: if tap {
                        SourceCost::Tap
                    } else {
                        SourceCost::None
                    },
                    body: meld_body,
                });
            }
        }
    }
    Some(program)
}
