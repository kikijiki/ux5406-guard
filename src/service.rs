//! The guard at work: I/O around the control logic, and the note that lets a
//! killed guard's offset be put back.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::control::{Config, Input, State, What};
use crate::error::Error;
use crate::kmsg::Events;
use crate::sysfs::Machine;

/// The note of the offset found before the guard first changed it.
pub struct Note(pub PathBuf);

impl Note {
    pub fn save(&self, base: i32) -> Result<(), Error> {
        let tmp = self.0.with_extension("tmp");
        let write = || -> io::Result<()> {
            fs::write(&tmp, format!("{base}\n"))?;
            fs::rename(&tmp, &self.0)
        };
        write().map_err(|source| Error::Write { path: self.0.clone(), source })
    }

    pub fn load(&self) -> Result<Option<i32>, Error> {
        match fs::read_to_string(&self.0) {
            Ok(text) => text
                .trim()
                .parse()
                .map(Some)
                .map_err(|_| Error::Parse { path: self.0.clone(), text: text.trim().to_owned() }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(Error::Read { path: self.0.clone(), source }),
        }
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

/// Put back the offset noted by a guard that did not get to do it itself.
/// Returns the offset written, if there was a note.
pub fn restore(machine: &Machine, note: &Note) -> Result<Option<i32>, Error> {
    let Some(base) = note.load()? else { return Ok(None) };
    machine.set_offset(base)?;
    note.clear()?;
    Ok(Some(base))
}

pub struct Service<E: Events> {
    pub machine: Machine,
    pub note: Note,
    pub config: Config,
    pub events: E,
    pub state: State,
    /// An event that was read but not yet acted on because a step failed.
    pending: bool,
}

impl<E: Events> Service<E> {
    pub fn new(machine: Machine, note: Note, config: Config, events: E) -> Self {
        Service { machine, note, config, events, state: State::default(), pending: false }
    }

    /// One tick. Errors are returned for the caller to log; the next tick
    /// retries, and an event read by a failed tick is not lost.
    pub fn tick(&mut self, now: i32) -> Result<(), Error> {
        if self.events.poll()? {
            log(&format!("event: {}", crate::kmsg::EVENT_TEXT));
            self.pending = true;
        }
        let offset = self.machine.offset()?;
        let temp = self.machine.temp()?;
        let input = Input { now, event: self.pending, temp, offset };

        if self.state.would_start(&input) {
            // The note comes before the first change, so a kill cannot lose it.
            self.note.save(offset)?;
        }
        // The state only advances once its write has succeeded.
        let mut next = self.state;
        let output = next.step(&self.config, &input);
        if let Some(value) = output.write {
            self.machine.set_offset(value)?;
        }
        self.state = next;
        self.pending = false;
        if let Some(what) = output.what {
            let shown = output.write.unwrap_or(offset);
            log(&format!("{temp} C: {} {shown}", describe(what)));
        }
        if output.what == Some(What::Released) {
            self.note.clear()?;
        }
        Ok(())
    }

    /// Put the original offset back, if the guard changed anything.
    pub fn finish(&mut self) -> Result<(), Error> {
        if let Some(base) = self.state.base() {
            self.machine.set_offset(base)?;
            self.note.clear()?;
            self.state.reset();
            log(&format!("stopping: offset back to {base}"));
        }
        Ok(())
    }
}

fn describe(what: What) -> &'static str {
    match what {
        What::Raised => "above hold temperature: offset raised to",
        What::AtMax => "above hold temperature at the maximum offset",
        What::Cleared => "cooled, holding offset",
        What::Lowered => "no event: offset lowered to",
        What::Tripped => "event while holding: raising from",
        What::Released => "no event at the original offset: released, offset",
    }
}

pub fn log(message: &str) {
    eprintln!("{message}");
}

pub fn note_path(dir: &Path) -> PathBuf {
    dir.join("original-offset")
}
