//! The guard at work: I/O around the control logic, and the note that lets a
//! killed guard's limits be put back.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::control::{Config, Input, Level, State};
use crate::error::Error;
use crate::kmsg::Events;
use crate::sysfs::{Limits, Machine};

/// The note of the limits found before the guard first changed them.
pub struct Note(pub PathBuf);

impl Note {
    pub fn save(&self, (msr, mmio): Limits) -> Result<(), Error> {
        let tmp = self.0.with_extension("tmp");
        let write = || -> io::Result<()> {
            fs::write(&tmp, format!("{msr} {mmio}\n"))?;
            fs::rename(&tmp, &self.0)
        };
        write().map_err(|source| Error::Write { path: self.0.clone(), source })
    }

    pub fn load(&self) -> Result<Option<Limits>, Error> {
        let text = match fs::read_to_string(&self.0) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(Error::Read { path: self.0.clone(), source }),
        };
        let parse = || Error::Parse { path: self.0.clone(), text: text.trim().to_owned() };
        let (msr, mmio) = text.trim().split_once(' ').ok_or_else(parse)?;
        Ok(Some((msr.parse().map_err(|_| parse())?, mmio.parse().map_err(|_| parse())?)))
    }

    pub fn clear(&self) -> Result<(), Error> {
        match fs::remove_file(&self.0) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                Err(Error::Write { path: self.0.clone(), source: e })
            }
            _ => Ok(()),
        }
    }
}

/// Put back the limits noted by a guard that did not get to do it itself.
/// Returns the limits written, if there was a note.
pub fn restore(machine: &Machine, note: &Note) -> Result<Option<Limits>, Error> {
    let Some(limits) = note.load()? else { return Ok(None) };
    machine.set_limits(limits)?;
    note.clear()?;
    Ok(Some(limits))
}

pub struct Service<E: Events> {
    pub machine: Machine,
    pub note: Note,
    pub config: Config,
    pub events: E,
    pub state: State,
    /// The limits found before the first change; `Some` while changed.
    original: Option<Limits>,
    /// The level last logged.
    shown: Level,
    /// An event that was read but not yet acted on because a step failed.
    pending: bool,
}

fn watts(w: u32) -> Limits {
    let uw = u64::from(w) * 1_000_000;
    (uw, uw)
}

impl<E: Events> Service<E> {
    pub fn new(machine: Machine, note: Note, config: Config, events: E) -> Self {
        Service {
            machine,
            note,
            config,
            events,
            state: State::default(),
            original: None,
            shown: Level::Original,
            pending: false,
        }
    }

    /// One tick. Errors are returned for the caller to log; the next tick
    /// retries, and an event read by a failed tick is not lost.
    pub fn tick(&mut self, now: i32) -> Result<(), Error> {
        if self.events.poll()? {
            log(&format!("event: {}", crate::kmsg::EVENT_TEXT));
            self.pending = true;
        }
        let lid_closed = self.machine.lid_closed()?;
        let current = self.machine.limits()?;
        let input = Input { now, event: self.pending, lid_closed };

        // The state only advances once its write has succeeded.
        let mut next = self.state;
        let level = next.step(&self.config, &input);
        let wanted = match level {
            Level::Original => self.original,
            Level::Cap => Some(watts(self.config.cap_w)),
            Level::Safe => Some(watts(self.config.safe_w)),
        };
        if let Some(wanted) = wanted.filter(|w| *w != current) {
            if self.original.is_none() {
                // The note comes before the first change, so a kill cannot lose it.
                self.note.save(current)?;
                self.original = Some(current);
            }
            self.machine.set_limits(wanted)?;
        }
        if level == Level::Original && self.original.is_some() {
            self.original = None;
            self.note.clear()?;
        }
        self.state = next;
        self.pending = false;
        if level != self.shown {
            self.shown = level;
            log(&describe(level, &self.config, lid_closed));
        }
        Ok(())
    }

    /// Put the original limits back, if the guard changed anything.
    pub fn finish(&mut self) -> Result<(), Error> {
        if let Some(original) = self.original {
            self.machine.set_limits(original)?;
            self.note.clear()?;
            self.original = None;
            self.shown = Level::Original;
            log(&format!("stopping: limits back to {} / {} uW", original.0, original.1));
        }
        Ok(())
    }
}

fn describe(level: Level, config: &Config, lid_closed: bool) -> String {
    match level {
        Level::Original => "lid open: original limits".into(),
        Level::Cap => format!("lid closed: package power capped at {} W", config.cap_w),
        Level::Safe => format!(
            "safe level: package power capped at {} W for {} s (lid {})",
            config.safe_w,
            config.hold,
            if lid_closed { "closed" } else { "open" }
        ),
    }
}

pub fn log(message: &str) {
    eprintln!("{message}");
}

pub fn note_path(dir: &Path) -> PathBuf {
    dir.join("original-limits")
}
