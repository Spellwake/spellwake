//! Deliver native match setup, clocks and terminal handoffs from private durable work.
use spellwake_application::native_game::NativeMatchWorker;
use std::{
    error::Error,
    io::{Read, Write},
    process::ExitCode,
    time::Duration,
};
fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if let [command, path] = args.as_slice() {
        if command == "supervise" {
            return NativeMatchWorker::supervise(path.as_ref());
        }
        if command == "redeliver" {
            std::io::stdout().write_all(&NativeMatchWorker::redeliver_incident(path.as_ref())?)?;
            return Ok(());
        }
    }
    if let [command, signature] = args.as_slice()
        && command == "incident"
    {
        let mut body = Vec::new();
        std::io::stdin().take(16_385).read_to_end(&mut body)?;
        let ack = NativeMatchWorker::deliver_incident(&body, signature)?;
        std::io::stdout().write_all(&ack)?;
        return Ok(());
    }
    let continuous = match args.as_slice() {
        [command] if command == "once" => false,
        [command] if command == "serve" => true,
        [command,_] if command == "serve-supervised" => true,
        _ => return Err("usage: native_match_worker once|serve|supervise <spool>|redeliver <file>|incident <signature>".into()),
    };
    let worker = NativeMatchWorker::from_env()?;
    if let [command, instance] = args.as_slice()
        && command == "serve-supervised"
    {
        worker.register_supervised_host(instance.parse()?)?;
    }
    loop {
        let result = worker.sweep();
        if !continuous {
            return result;
        }
        if let Err(error) = result {
            eprintln!("{error}")
        }
        std::thread::sleep(Duration::from_secs(2));
    }
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
