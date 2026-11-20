//! Deliver one server-owned multiplayer deadline job.
use spellwake_application::{native_game::run_owned_response_job, player_wire::PlayerBasis};
use std::{error::Error, io, process::ExitCode};

fn run() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let [game, context, epoch, revision, request] = args.as_slice() else {
        return Err(
            "usage: native_response_worker GAME CONTEXT NATIVE_EPOCH REVISION REQUEST".into(),
        );
    };
    let basis = PlayerBasis {
        game_id: game.parse()?,
        context_id: context.parse()?,
        epoch: epoch.clone().try_into()?,
        revision: revision.clone().try_into()?,
    };
    serde_json::to_writer(
        io::stdout().lock(),
        &run_owned_response_job(&basis, request.parse()?)?,
    )?;
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
