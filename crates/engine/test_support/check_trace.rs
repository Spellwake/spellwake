//! Structured native check evidence. Identity strings are opaque local handles;
//! aliases annotate exact objects without replacing their physical identities.
use super::damage_trace::{GameObjectView, TurnPositionView, game_object_view, turn_position_view};
use serde::Serialize;
use spellwake_engine::{
    checks::{
        CheckBoundary, CheckChoice, CheckDecision, CheckEventId, CheckJournal, CheckMoveCause,
        LegendConflict, LegendConflictId,
    },
    object_identity::{CardId, GameObject, GameObjects, ObjectId},
    occurrence::TriggerOrder,
};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CheckEventView {
    pub started_revision: u64,
    pub pass: u64,
}

pub fn check_event_view(event: CheckEventId) -> CheckEventView {
    CheckEventView {
        started_revision: event.started_revision(),
        pass: event.pass(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckBoundaryView {
    Priority,
    Cleanup { position: TurnPositionView },
}

pub fn check_boundary_view(boundary: CheckBoundary) -> CheckBoundaryView {
    match boundary {
        CheckBoundary::Priority => CheckBoundaryView::Priority,
        CheckBoundary::Cleanup(position) => CheckBoundaryView::Cleanup {
            position: turn_position_view(position),
        },
    }
}

/// Privileged exact native frame, not a player-safe or cross-game identifier view.
#[derive(Debug, Clone, Serialize)]
pub struct TriggerOrderView {
    pub revision: String,
    pub boundary: CheckBoundaryView,
    pub event: CheckEventView,
    pub wave: String,
    pub chooser: u32,
    pub domain: Vec<String>,
}

pub fn trigger_order_view(frame: &TriggerOrder) -> TriggerOrderView {
    TriggerOrderView {
        revision: frame.revision().to_string(),
        boundary: check_boundary_view(frame.boundary()),
        event: check_event_view(frame.event()),
        wave: frame.wave().sequence().to_string(),
        chooser: frame.chooser().0,
        domain: frame
            .instances()
            .iter()
            .map(|id| id.sequence().to_string())
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckGameObjectView {
    pub alias: Option<String>,
    #[serde(flatten)]
    pub snapshot: GameObjectView,
}

/// Full actual minimal physical snapshot with a caller-defined stable-card label.
pub fn check_game_object_view(
    object: &GameObject,
    label: impl Fn(CardId) -> String,
) -> CheckGameObjectView {
    CheckGameObjectView {
        alias: object.card_id().map(&label),
        snapshot: game_object_view(object),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LegendConflictIdView {
    pub event: CheckEventView,
    pub controller: u32,
    pub ordinal: u64,
}
pub fn legend_conflict_id_view(id: LegendConflictId) -> LegendConflictIdView {
    LegendConflictIdView {
        event: check_event_view(id.event()),
        controller: id.controller().0,
        ordinal: id.ordinal(),
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct LegendConflictView {
    pub id: LegendConflictIdView,
    pub comparison_names: Vec<String>,
    pub domain: Vec<CheckGameObjectView>,
}
#[derive(Debug, Clone, Serialize)]
pub struct CheckDecisionHeaderView {
    pub chooser: u32,
    pub revision: u64,
    pub boundary: CheckBoundaryView,
    pub event: CheckEventView,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckDecisionView {
    CleanupDiscard {
        #[serde(flatten)]
        header: CheckDecisionHeaderView,
        required: usize,
        domain: Vec<CheckGameObjectView>,
    },
    GraveyardOrder {
        #[serde(flatten)]
        header: CheckDecisionHeaderView,
        required: usize,
        domain: Vec<CheckGameObjectView>,
    },
    LegendChoice {
        #[serde(flatten)]
        header: CheckDecisionHeaderView,
        conflicts: Vec<LegendConflictView>,
    },
}

/// Observe exact variant-specific domains. Missing incarnations fail rather
/// than being followed through stable identity or omitted from the evidence.
pub fn check_decision_view(
    frame: &CheckDecision,
    cards: &GameObjects,
    label: impl Fn(CardId) -> String,
) -> Result<CheckDecisionView, String> {
    let header = CheckDecisionHeaderView {
        chooser: frame.chooser().0,
        revision: frame.revision(),
        boundary: check_boundary_view(frame.boundary()),
        event: check_event_view(frame.event()),
    };
    let domain = |ids: &[ObjectId]| {
        ids.iter()
            .map(|id| {
                let object = cards
                    .get(*id)
                    .ok_or_else(|| format!("check domain contains missing exact object {id:?}"))?;
                Ok(check_game_object_view(object, &label))
            })
            .collect::<Result<Vec<_>, String>>()
    };
    Ok(match frame {
        CheckDecision::BattleProtector(_) => {
            return Err("historical check observer excludes battle protectors".into());
        }
        CheckDecision::ReturnCommanders(_) => {
            return Err("historical check observer excludes commander choices".into());
        }
        CheckDecision::CleanupSelect(_) => {
            return Err(
                "historical check frame excludes replacement-aware cleanup selection".into(),
            );
        }
        CheckDecision::CleanupDiscard(f) => CheckDecisionView::CleanupDiscard {
            header,
            required: f.required(),
            domain: domain(f.domain())?,
        },
        CheckDecision::GraveyardOrder(f) => CheckDecisionView::GraveyardOrder {
            header,
            required: f.domain().len(),
            domain: domain(f.domain())?,
        },
        CheckDecision::Legend(f) => CheckDecisionView::LegendChoice {
            header,
            conflicts: f
                .conflicts()
                .iter()
                .map(|c: &LegendConflict| {
                    Ok(LegendConflictView {
                        id: legend_conflict_id_view(c.id()),
                        comparison_names: c
                            .comparison_names()
                            .iter()
                            .map(|n| n.as_str().to_owned())
                            .collect(),
                        domain: domain(c.domain())?,
                    })
                })
                .collect::<Result<_, String>>()?,
        },
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckObjectReferenceView {
    pub object: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckChoiceView {
    CleanupDiscard {
        ordered: Vec<CheckObjectReferenceView>,
    },
    GraveyardOrder {
        ordered: Vec<CheckObjectReferenceView>,
    },
    LegendKeep {
        conflict: LegendConflictIdView,
        domain: Vec<CheckObjectReferenceView>,
        keep: CheckObjectReferenceView,
    },
}
#[derive(Debug, Clone, Serialize)]
pub struct CheckChoiceRecordView {
    pub event: CheckEventView,
    pub boundary: CheckBoundaryView,
    pub player: u32,
    #[serde(flatten)]
    pub choice: CheckChoiceView,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckMoveCauseView {
    CleanupDiscard,
    StateBasedAction,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckMoveRecordView {
    pub event: CheckEventView,
    pub boundary: CheckBoundaryView,
    pub cause: CheckMoveCauseView,
    pub before: CheckGameObjectView,
    pub after: CheckGameObjectView,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckJournalView {
    pub choices: Vec<CheckChoiceRecordView>,
    pub moves: Vec<CheckMoveRecordView>,
}

/// Project one actual current-operation journal. Accepted choice identities are
/// resolved against the pre-command registry or actual movement-before rows.
/// Neither route follows a stable card into a different incarnation.
pub fn check_journal_view(
    journal: &CheckJournal,
    source_cards: &GameObjects,
    label: impl Fn(CardId) -> String,
) -> Result<CheckJournalView, String> {
    let source_object = |id: ObjectId| {
        source_cards.get(id).or_else(|| {
            journal
                .moves
                .iter()
                .find(|movement| movement.before.id() == id)
                .map(|movement| &movement.before)
        })
    };
    let choices = journal
        .choices
        .iter()
        .map(|choice| {
            let reference = |id: ObjectId| {
                let object = source_object(id).ok_or_else(|| {
                    format!("check choice has no exact source snapshot for {id:?}")
                })?;
                Ok(CheckObjectReferenceView {
                    object: format!("{id:?}"),
                    alias: object.card_id().map(&label),
                })
            };
            let ordered = |ids: &[ObjectId]| {
                ids.iter()
                    .copied()
                    .map(reference)
                    .collect::<Result<Vec<_>, String>>()
            };
            let payload = match &choice.choice {
                CheckChoice::BattleProtector { .. } => {
                    return Err("historical check journal excludes battle protectors".into());
                }
                CheckChoice::ReturnCommanders { .. } => {
                    return Err("historical check journal excludes commander choices".into());
                }
                CheckChoice::CleanupSelected { .. } => {
                    return Err("historical check journal excludes CleanupSelected".into());
                }
                CheckChoice::CleanupDiscard { ordered: ids } => CheckChoiceView::CleanupDiscard {
                    ordered: ordered(ids)?,
                },
                CheckChoice::GraveyardOrder { ordered: ids } => CheckChoiceView::GraveyardOrder {
                    ordered: ordered(ids)?,
                },
                CheckChoice::LegendKeep {
                    conflict,
                    domain,
                    keep,
                } => CheckChoiceView::LegendKeep {
                    conflict: legend_conflict_id_view(*conflict),
                    domain: ordered(domain)?,
                    keep: reference(*keep)?,
                },
            };
            Ok(CheckChoiceRecordView {
                event: check_event_view(choice.event),
                boundary: check_boundary_view(choice.boundary),
                player: choice.player.0,
                choice: payload,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let moves = journal
        .moves
        .iter()
        .map(|movement| CheckMoveRecordView {
            event: check_event_view(movement.event),
            boundary: check_boundary_view(movement.boundary),
            cause: match movement.cause {
                CheckMoveCause::CleanupDiscard => CheckMoveCauseView::CleanupDiscard,
                CheckMoveCause::StateBasedAction => CheckMoveCauseView::StateBasedAction,
            },
            before: check_game_object_view(&movement.before, &label),
            after: check_game_object_view(&movement.after, &label),
        })
        .collect();
    Ok(CheckJournalView { choices, moves })
}
