//! Everything that can go wrong, as values.

use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    /// The machine is not the one this guard is for.
    WrongProduct { wanted: &'static str, found: String },
    /// No `coretemp` package sensor below hwmon.
    NoSensor,
    /// A file could not be read.
    Read { path: PathBuf, source: io::Error },
    /// A file could not be written.
    Write { path: PathBuf, source: io::Error },
    /// A file held something other than a plausible integer.
    Parse { path: PathBuf, text: String },
    /// A command-line option is unknown or out of range.
    Usage(String),
}

impl Error {
    /// Exit status of the process: 78 (EX_CONFIG) means "do not restart me".
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::WrongProduct { .. } | Error::Usage(_) | Error::NoSensor => 78,
            _ => 1,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::WrongProduct { wanted, found } => {
                write!(f, "refusing to run: product is {found:?}, not {wanted}")
            }
            Error::NoSensor => write!(f, "no coretemp \"Package id 0\" sensor found"),
            Error::Read { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Error::Write { path, source } => {
                write!(f, "cannot write {}: {source}", path.display())
            }
            Error::Parse { path, text } => {
                write!(f, "{} holds {text:?}, not a valid number", path.display())
            }
            Error::Usage(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}
