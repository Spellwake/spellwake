//! Run one trusted daily assignment or one bounded activity batch.
use spellwake_application::terminal_consumers::{
    DailyQuestSelection, QuestActivityBatch, QuestWorker,
};
use std::{error::Error, io, process::ExitCode};

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, account, definition, day, request] if command == "assign" => {
            let selection = DailyQuestSelection {
                actor_id: account.parse()?,
                definition_id: definition.parse()?,
                day: day.clone(),
                request_id: request.parse()?,
            };
            serde_json::to_writer(
                io::stdout().lock(),
                &QuestWorker::from_env()?.assign(&selection)?,
            )?;
        }
        [command, game, limit] if command == "consume" => {
            let batch = QuestActivityBatch {
                game_id: game.parse()?,
                limit: limit.parse()?,
            };
            serde_json::to_writer(
                io::stdout().lock(),
                &QuestWorker::from_env()?.consume(&batch)?,
            )?;
        }
        _ => return Err(
            "usage: quest_worker assign ACCOUNT DEFINITION UTC_DAY REQUEST | consume GAME LIMIT"
                .into(),
        ),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
