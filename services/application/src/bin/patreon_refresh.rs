//! Explicit single background observation; no workflow/provider registration or grant processing.
use spellwake_application::{
    ApiResponse,
    patreon::{PatreonRefreshOutcome, PatreonRefreshService},
};
use std::process::ExitCode;
use uuid::Uuid;
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [association] = args.as_slice() else {
        eprintln!("usage: patreon_refresh <association-uuid>");
        return ExitCode::FAILURE;
    };
    let Ok(association) = association.parse::<Uuid>() else {
        eprintln!("patreon_refresh code=invalid_argument");
        return ExitCode::FAILURE;
    };
    if association.is_nil() {
        eprintln!("patreon_refresh code=invalid_argument");
        return ExitCode::FAILURE;
    }
    let result = (|| {
        let service = PatreonRefreshService::from_env()?;
        match service.observe(association)? {
            Some(attempt) => service.commit(&attempt),
            None => Ok(PatreonRefreshOutcome::Unavailable {
                association_id: association,
            }),
        }
    })();
    match result {
        Ok(outcome) => {
            if serde_json::to_writer(
                std::io::stdout().lock(),
                &ApiResponse::Success { data: outcome },
            )
            .is_err()
            {
                eprintln!("patreon_refresh code=output_failed");
                return ExitCode::FAILURE;
            }
            println!();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("patreon_refresh code=unresolved message={error}");
            ExitCode::FAILURE
        }
    }
}
