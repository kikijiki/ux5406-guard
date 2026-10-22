use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;

use ux5406_guard::control::Config;
use ux5406_guard::error::Error;
use ux5406_guard::kmsg::Kmsg;
use ux5406_guard::service::{self, log, Note, Service};
use ux5406_guard::sysfs::Machine;

const USAGE: &str = "usage: ux5406-guard [run|restore] [--hold-temp C] [--step-up C] \
[--max-offset C] [--interval S] [--settle S] [--probe-interval S] \
[--state-dir DIR] [--sys DIR] [--kmsg FILE]";

struct Options {
    restore: bool,
    config: Config,
    state_dir: PathBuf,
    sys: PathBuf,
    kmsg: PathBuf,
}

fn parse() -> Result<Options, Error> {
    let mut args = std::env::args().skip(1).peekable();
    let mut o = Options {
        restore: false,
        config: Config::default(),
        state_dir: "/run/ux5406-guard".into(),
        sys: "/sys".into(),
        kmsg: "/dev/kmsg".into(),
    };
    match args.peek().map(String::as_str) {
        Some("restore") => {
            o.restore = true;
            args.next();
        }
        Some("run") => {
            args.next();
        }
        _ => {}
    }
    while let Some(flag) = args.next() {
        let usage = || Error::Usage(format!("bad option {flag:?}\n{USAGE}"));
        let value = args.next().ok_or_else(usage)?;
        let number = || value.parse::<i32>().map_err(|_| usage());
        match flag.as_str() {
            "--hold-temp" => o.config.hold_temp = number()?,
            "--step-up" => o.config.step_up = number()?,
            "--max-offset" => o.config.max_offset = number()?,
            "--interval" => o.config.interval = number()?,
            "--settle" => o.config.settle = number()?,
            "--probe-interval" => o.config.probe_interval = number()?,
            "--state-dir" => o.state_dir = value.into(),
            "--sys" => o.sys = value.into(),
            "--kmsg" => o.kmsg = value.into(),
            _ => return Err(usage()),
        }
    }
    o.config.validate()?;
    Ok(o)
}

fn run(o: Options) -> Result<(), Error> {
    let machine = Machine::open(&o.sys)?;
    let note = Note(service::note_path(&o.state_dir));

    // A guard that was killed left its note: put that offset back first.
    if let Some(base) = service::restore(&machine, &note)? {
        log(&format!("a previous guard did not finish: offset back to {base}"));
    }
    if o.restore {
        return Ok(());
    }

    let stop = Arc::new(AtomicBool::new(false));
    for signal in [SIGTERM, SIGINT] {
        flag::register(signal, Arc::clone(&stop))
            .map_err(|source| Error::Write { path: "signal handler".into(), source })?;
    }
    let events = Kmsg::open(&o.kmsg)?;
    let mut service = Service::new(machine, note, o.config, events);
    let start = Instant::now();
    log(&format!(
        "watching for the EC event; offset {}, hold temperature {} C, maximum offset {}",
        service.machine.offset()?,
        o.config.hold_temp,
        o.config.max_offset
    ));

    let mut last_error = String::new();
    while !stop.load(Ordering::Relaxed) {
        let now = start.elapsed().as_secs() as i32;
        match service.tick(now) {
            Ok(()) => last_error.clear(),
            // Logged once per distinct failure; the service stays up.
            Err(e) if e.to_string() != last_error => {
                last_error = e.to_string();
                log(&format!("error: {e}"));
            }
            Err(_) => {}
        }
        sleep(Duration::from_secs(1));
    }
    service.finish()
}

fn main() -> ExitCode {
    let result = parse().and_then(run);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log(&format!("{e}"));
            ExitCode::from(e.exit_code() as u8)
        }
    }
}
