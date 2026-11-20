//! Execute one private queue claim or due countdown with the caller's durable request ID.
use spellwake_application::admission_ports::{AdmissionWorker, MatchmakingClaim, QueueSelection};
use std::{
    error::Error,
    io::{self, Read},
    process::ExitCode,
};

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, request] if command == "claim" => {
            let mut bytes = Vec::new();
            io::stdin().take(262_145).read_to_end(&mut bytes)?;
            if bytes.len() > 262_144 {
                return Err("queue selection exceeds the input limit".into());
            }
            let claim = MatchmakingClaim { request_id: request.parse()?, selection: QueueSelection::from_bytes(&bytes)? };
            serde_json::to_writer(io::stdout().lock(), &AdmissionWorker::from_env()?.claim(&claim)?)?;
        }
        [command, room, basis, request] if command == "start" => {
            let ack = AdmissionWorker::from_env()?.start(room.parse()?, &basis.parse()?, request.parse()?)?;
            serde_json::to_writer(io::stdout().lock(), &ack)?;
        }
        _ => return Err("usage: room_admission_worker claim REQUEST_ID < selection.json | start ROOM_ID REVISION REQUEST_ID".into()),
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
