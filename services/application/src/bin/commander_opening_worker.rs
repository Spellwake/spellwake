//! One scoped delivery of an admitted Commander reservation.
use spellwake_application::native_game::{CommanderWorker, open_commander_game_from_env};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [game] = args.as_slice() else {
        eprintln!("usage: commander_opening_worker GAME_ID | sweep");
        return ExitCode::FAILURE;
    };
    let game = if game == "sweep" {
        None
    } else {
        match game.parse() {
            Ok(game) => Some(game),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
    };
    // Match the stack size used by the real Commander catalogue qualification.
    let worker = std::thread::Builder::new()
        .name("commander-opening".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let result = match game {
                Some(game) => open_commander_game_from_env(game).map(usize::from),
                None => CommanderWorker::from_env().and_then(|worker| worker.sweep()),
            };
            result.map_err(|error| error.to_string())
        });
    let result = worker
        .map_err(|error| error.to_string())
        .and_then(|worker| {
            worker
                .join()
                .map_err(|_| "Commander opening worker stopped".to_owned())
        })
        .and_then(|result| result);
    match result {
        Ok(opened) => {
            println!("Commander openings accepted: {opened}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
