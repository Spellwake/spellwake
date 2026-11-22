//! Read expected results from the complete source independently of compiled instructions.
use super::*;
use spellwake_engine::{
    combat::CombatDecision,
    game::{DecisionReply, RuleFrame},
};
use std::io::Write;

// Overkill applies -9999 toughness. Keep this witness alive to measure the exact change and its expiry.
pub(super) const TARGET_TOUGHNESS: i64 = 10_000;
pub(super) enum Outcome {
    Draw(usize),
    Gain(i64),
    DamageCreature(i64),
    ChangeStats(i64, i64),
}
impl Outcome {
    pub(super) fn from_source(name: &str, text: &str) -> Option<Self> {
        match text.trim() {
            "Draw a card." => Some(Self::Draw(1)),
            "Draw two cards." => Some(Self::Draw(2)),
            "Draw three cards." => Some(Self::Draw(3)),
            "Draw four cards." => Some(Self::Draw(4)),
            line if line.starts_with("You gain ") => Some(Self::Gain(
                line.strip_prefix("You gain ")?
                    .strip_suffix(" life.")?
                    .parse()
                    .ok()?,
            )),
            line if line.starts_with("Target creature gets ") => {
                let change = line
                    .strip_prefix("Target creature gets ")?
                    .strip_suffix(" until end of turn.")?;
                let (power, toughness) = change.split_once('/')?;
                Some(Self::ChangeStats(
                    power.parse().ok()?,
                    toughness.parse().ok()?,
                ))
            }
            line => Some(Self::DamageCreature(
                line.strip_prefix(name)?
                    .strip_prefix(" deals ")?
                    .strip_suffix(" damage to target creature.")?
                    .parse()
                    .ok()?,
            )),
        }
    }

    pub(super) fn target(&self) -> BodyTarget {
        match self {
            Self::DamageCreature(_) | Self::ChangeStats(_, _) => BodyTarget::SingleCreature,
            _ => BodyTarget::None,
        }
    }

    fn evidence(&self) -> &'static str {
        match self {
            Self::Draw(_) => "Paid cast; exact library cards moved into hand",
            Self::Gain(_) => "Paid cast; exact controller life gain; opponent life unchanged",
            Self::DamageCreature(_) => "Paid cast; creature-only target; printed damage marked",
            Self::ChangeStats(_, _) => {
                "Paid cast; creature-only target; printed stat changes and cleanup expiry"
            }
        }
    }

    pub(super) fn verify(&self, state: &GameState, name: &str) {
        let (a, b) = (SeatId(0), SeatId(1));
        let execution = state.execution();
        let cards = execution.window().cards();
        match *self {
            Self::Gain(amount) => {
                assert_eq!(
                    execution.life_total(a).unwrap(),
                    &(20 + amount).into(),
                    "{name}"
                );
                assert_eq!(execution.life_total(b).unwrap(), &20.into(), "{name}");
            }
            Self::Draw(amount) => {
                assert_eq!(cards.library(a).len(), 4 - amount, "{name}");
                for index in 0..amount {
                    let object = cards.current_identity(CardId(3 + index as u64)).unwrap();
                    assert_eq!(
                        cards.get(object).unwrap().zone(),
                        ObjectZone::Hand(a),
                        "{name}"
                    );
                }
            }
            Self::DamageCreature(amount) => {
                let target = cards.current_identity(CardId(2)).unwrap();
                assert_eq!(
                    execution.battlefield_marked_damage(target),
                    Some(amount.into()),
                    "{name}"
                );
            }
            Self::ChangeStats(power, toughness) => {
                let target = cards.current_identity(CardId(2)).unwrap();
                let stats = execution.power_toughness(target).unwrap().unwrap().derived;
                assert_eq!(
                    (stats.power, stats.toughness),
                    ((2 + power).into(), (TARGET_TOUGHNESS + toughness).into()),
                    "{name}"
                );
                let mut game = state.clone();
                for _ in 0..32 {
                    if game.turn().unwrap().position().number > 1 {
                        break;
                    }
                    let (actor, command) = if let Some(d) = game.pending_decision() {
                        assert!(
                            matches!(d.frame(), RuleFrame::Combat(CombatDecision::Attackers(_))),
                            "{name}: {:?}",
                            d.frame()
                        );
                        (
                            d.chooser(),
                            GameCommand::Reply {
                                decision: d.id(),
                                choice: DecisionReply::DeclareAttackers(vec![]),
                            },
                        )
                    } else {
                        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                            game.execution().window().phase()
                        else {
                            panic!("{name}: no priority")
                        };
                        (holder, GameCommand::Pass)
                    };
                    game = advance(&game, actor, command);
                }
                assert_eq!(game.turn().unwrap().position().number, 2, "{name}");
                let stats = game
                    .execution()
                    .power_toughness(target)
                    .unwrap()
                    .unwrap()
                    .derived;
                assert_eq!(
                    (stats.power, stats.toughness),
                    (2.into(), TARGET_TOUGHNESS.into()),
                    "{name}"
                );
            }
        }
    }
}

use spellwake_engine::program::BodyTarget;

#[test]
#[ignore = "requires the dated Oracle catalogue"]
fn ordinary_spell_audit() {
    let (_, vocabulary, registry) = current_catalogue();
    let mut receipts = Vec::new();
    let source =
        MultiGzDecoder::new(std::fs::File::open(input("SPELLWAKE_CATALOGUE_INPUT")).unwrap());
    for line in BufReader::new(source).lines() {
        let raw: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if raw["layout"] != "normal"
            || !matches!(raw["type_line"].as_str(), Some("Instant" | "Sorcery"))
        {
            continue;
        }
        let id = OracleId::try_from(raw["oracle_id"].as_str().unwrap().to_owned()).unwrap();
        if !registry.bindings.contains_key(&id) {
            continue;
        }
        let name = raw["name"].as_str().unwrap();
        let text = without_reminder(raw["oracle_text"].as_str().unwrap()).unwrap();
        if let Some(outcome) = Outcome::from_source(name, &text) {
            cast_new_card(registry, &id, &raw, &vocabulary);
            receipts.push(format!("{}\t{}\t{}", id.as_str(), name, outcome.evidence()));
        }
    }
    assert!(
        receipts
            .iter()
            .any(|line| line.contains("\tGiant Growth\t"))
    );
    receipts.sort();
    if let Some(path) = std::env::var_os("SPELLWAKE_SPELL_AUDIT_OUTPUT") {
        let mut file = std::fs::File::create(path).unwrap();
        writeln!(file, "oracle_id\tname\tevidence").unwrap();
        for line in &receipts {
            writeln!(file, "{line}").unwrap();
        }
    }
    eprintln!(
        "Checked complete Oracle spell effects on {} identities",
        receipts.len()
    );
}
