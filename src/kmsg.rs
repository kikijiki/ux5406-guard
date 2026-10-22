//! The kernel log as the source of the EC's event.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::error::Error;

pub const EVENT_TEXT: &str = "asus_wmi: Unknown key code 0x6d";
const RECORD_MAX: usize = 8192;
const READS_PER_POLL: usize = 4096;
const O_NONBLOCK: i32 = 0o4000;

/// Where events come from; the service is tested with a scripted one.
pub trait Events {
    /// Whether the event was logged since the last call.
    fn poll(&mut self) -> Result<bool, Error>;
}

/// True if a `/dev/kmsg` chunk holds the event: a record of the kernel itself
/// (syslog facility 0), whose message starts with [`EVENT_TEXT`]. Lines that
/// start with a space continue the previous record and are skipped.
pub fn has_event(chunk: &str) -> bool {
    chunk.lines().any(|line| {
        let Some((header, message)) = line.split_once(';') else { return false };
        let priority: Option<u32> = header.split(',').next().and_then(|p| p.parse().ok());
        matches!(priority, Some(p) if p >> 3 == 0) && message.starts_with(EVENT_TEXT)
    })
}

/// `/dev/kmsg` opened at its end: what is in it already is not news.
pub struct Kmsg {
    path: PathBuf,
    file: File,
}

impl Kmsg {
    pub fn open(path: &Path) -> Result<Kmsg, Error> {
        let fail = |source| Error::Read { path: path.to_owned(), source };
        let mut file = File::options()
            .read(true)
            .custom_flags(O_NONBLOCK)
            .open(path)
            .map_err(fail)?;
        // A pipe has no end to go to.
        let _ = file.seek(SeekFrom::End(0));
        Ok(Kmsg { path: path.to_owned(), file })
    }
}

impl Events for Kmsg {
    fn poll(&mut self) -> Result<bool, Error> {
        let mut buf = [0u8; RECORD_MAX];
        let mut seen = false;
        for _ in 0..READS_PER_POLL {
            match self.file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => seen |= has_event(&String::from_utf8_lossy(&buf[..n])),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                // Records were overwritten before they were read.
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(source) => return Err(Error::Read { path: self.path.clone(), source }),
            }
        }
        Ok(seen)
    }
}
