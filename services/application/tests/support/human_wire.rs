//! Additional shared fixtures use public contracts and the original owner/target context.
use super::Case;
use serde::de::DeserializeOwned;
use spellwake_application::{ante::*, room_controls::*, supply_ballot::*};
use uuid::Uuid;
fn parses<T: DeserializeOwned>(case: &Case, validate: impl FnOnce(T) -> bool) -> bool {
    spellwake_application::decode_wire_object::<T>(case.wire.as_bytes()).is_ok_and(validate)
}
fn control(case: &Case) -> ControlKind {
    match case.operation.as_deref().unwrap() {
        "add" => ControlKind::Add,
        "bootstrap" => ControlKind::Bootstrap,
        "remove" => ControlKind::Remove,
        "unready" => ControlKind::Unready,
        _ => panic!("invalid fixture operation"),
    }
}
fn intent(case: &Case, owner: Uuid) -> AnteIntent {
    let basis = case.expected_revision.clone().unwrap();
    match case.operation.as_deref().unwrap() {
        "offer" => AnteIntent::Offer(OfferAnte {
            request_id: owner,
            expected_terms_revision: basis,
            assets: vec![].try_into().unwrap(),
        }),
        "accept" => AnteIntent::Accept(AcceptAnte {
            request_id: owner,
            expected_terms_revision: basis,
            terms_hash: "a".repeat(64),
        }),
        "cancel" => AnteIntent::Cancel(CancelAnte {
            request_id: owner,
            expected_terms_revision: basis,
        }),
        _ => panic!("invalid fixture operation"),
    }
}
pub(super) fn valid(case: &Case, owner: Uuid) -> Option<bool> {
    let limit = case.limit.unwrap_or(100);
    Some(match case.kind.as_str() {
        "deck_command" => {
            parses::<spellwake_application::decks::DeckCommand>(case, |v| v.validate().is_ok())
        }
        "social_command" => {
            parses::<spellwake_application::commands::SocialCommand>(case, |v| v.validate().is_ok())
        }
        "economy_command" => {
            parses::<spellwake_application::economy_commands::EconomyCommand>(case, |v| {
                v.validate().is_ok()
            })
        }
        "prediction_command" => {
            parses::<spellwake_application::predictions::PredictionCommand>(case, |v| {
                v.validate().is_ok()
            })
        }
        "source_preparation" => parses::<
            spellwake_application::prediction_publication::SourcePreparation,
        >(case, |v| v.account_id == owner),
        "match_detail" => parses::<spellwake_application::match_history::MatchDetail>(case, |v| {
            v.validate(owner, case.match_id.unwrap()).is_ok()
        }),
        "match_list" => parses::<spellwake_application::match_history::MatchList>(case, |v| {
            v.validate(owner, None, limit).is_ok()
        }),
        "game_page" => parses::<spellwake_application::match_history::GamePage>(case, |v| {
            v.validate(owner, case.match_id.unwrap(), None, limit)
                .is_ok()
        }),
        "seat_page" => parses::<spellwake_application::match_history::SeatPage>(case, |v| {
            v.validate(owner, case.match_id.unwrap(), None, limit)
                .is_ok()
        }),
        "decision_page" => {
            parses::<spellwake_application::match_history::DecisionPage>(case, |v| {
                v.validate(owner, case.match_id.unwrap(), None, limit)
                    .is_ok()
            })
        }
        "credit_page" => {
            parses::<spellwake_application::competitive_history::CreditPage>(case, |v| {
                v.validate(owner, None, limit, case.format_id, case.season_id)
                    .is_ok()
            })
        }
        "daily_win_page" => {
            parses::<spellwake_application::competitive_history::DailyWinPage>(case, |v| {
                v.validate(owner, None, limit).is_ok()
            })
        }
        "ai_status_page" => parses::<AiStatusPage>(case, |v| {
            v.validate(owner, case.room_id.unwrap(), None, limit)
                .is_ok()
        }),
        "ballot_page" => parses::<BallotPage>(case, |v| {
            v.validate(owner, case.season_id.unwrap(), None, limit)
                .is_ok()
        }),
        "ante_detail" => {
            parses::<AnteDetail>(case, |v| v.validate(owner, case.match_id.unwrap()).is_ok())
        }
        "revision_request" => parses::<RevisionRequest>(case, |v| !v.request_id.is_nil()),
        "bootstrap_request" => parses::<BootstrapRequest>(case, |v| !v.request_id.is_nil()),
        "replace_ballot" => parses::<ReplaceBallot>(case, |v| v.validate().is_ok()),
        "ante_offer" => parses::<OfferAnte>(case, |v| v.validate().is_ok()),
        "ante_accept" => parses::<AcceptAnte>(case, |v| v.validate().is_ok()),
        "ante_cancel" => parses::<CancelAnte>(case, |v| !v.request_id.is_nil()),
        "control_ack" => parses::<ControlAck>(case, |ack| {
            control(case).valid_ack(
                case.target_id.unwrap(),
                case.expected_revision.as_ref().unwrap(),
                &ack,
            )
        }),
        "control_disposition" => parses::<ControlDisposition>(case, |v| {
            control(case).valid_disposition(
                case.target_id.unwrap(),
                case.expected_revision.as_ref().unwrap(),
                &v,
            )
        }),
        "ante_ack" => parses::<AnteAck>(case, |ack| {
            intent(case, owner).valid_ack(case.target_id.unwrap(), &ack)
        }),
        "ante_disposition" => parses::<AnteDisposition>(case, |v| {
            intent(case, owner).valid_disposition(case.target_id.unwrap(), &v)
        }),
        "ballot_ack" => parses::<BallotAck>(case, |ack| {
            ack.valid(
                case.target_id.unwrap(),
                &ReplaceBallot {
                    request_id: owner,
                    set_ids: vec![owner, case.target_id.unwrap()],
                },
            )
        }),
        "ballot_disposition" => parses::<BallotDisposition>(case, |v| {
            ReplaceBallot {
                request_id: owner,
                set_ids: vec![owner, case.target_id.unwrap()],
            }
            .valid_disposition(case.target_id.unwrap(), &v)
        }),
        _ => return None,
    })
}
