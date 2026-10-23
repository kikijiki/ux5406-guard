//! The machine as sysfs and procfs show it. The guard writes exactly two files:
//! the package power limits.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::Error;

pub const PRODUCT: &str = "UX5406SA";
const PRODUCT_FILE: &str = "class/dmi/id/product_name";
/// The MSR limit and the firmware (MMIO) limit; the lower one wins.
const LIMIT_FILES: [&str; 2] = [
    "class/powercap/intel-rapl:0/constraint_0_power_limit_uw",
    "class/powercap/intel-rapl-mmio:0/constraint_0_power_limit_uw",
];
const LID_DIR: &str = "acpi/button/lid";
/// A plausible limit (microwatts); anything else in the file is not believed.
const LIMIT_RANGE: std::ops::RangeInclusive<u64> = 1_000_000..=250_000_000;

/// The two limits, in microwatts: MSR, then firmware.
pub type Limits = (u64, u64);

/// The files the guard uses, resolved below `/sys` and `/proc`-like roots.
#[derive(Clone, Debug)]
pub struct Machine {
    limits: [PathBuf; 2],
    lid: PathBuf,
}

fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|source| Error::Read { path: path.to_owned(), source })
}

fn read_limit(path: &Path) -> Result<u64, Error> {
    let text = read(path)?;
    let parse = || Error::Parse { path: path.to_owned(), text: text.trim().to_owned() };
    let value: u64 = text.trim().parse().map_err(|_| parse())?;
    if LIMIT_RANGE.contains(&value) {
        Ok(value)
    } else {
        Err(parse())
    }
}

pub fn check_product(sys: &Path) -> Result<(), Error> {
    let name = read(&sys.join(PRODUCT_FILE))?;
    let name = name.trim();
    if name.contains(PRODUCT) {
        Ok(())
    } else {
        Err(Error::WrongProduct { wanted: PRODUCT, found: name.to_owned() })
    }
}

impl Machine {
    /// Check the product and find the limits and the lid.
    pub fn open(sys: &Path, proc: &Path) -> Result<Machine, Error> {
        check_product(sys)?;
        let limits = LIMIT_FILES.map(|f| sys.join(f));
        for path in &limits {
            read_limit(path)?;
        }
        let dir = proc.join(LID_DIR);
        let entries = fs::read_dir(&dir).map_err(|_| Error::NoLid)?;
        let mut states: Vec<PathBuf> =
            entries.flatten().map(|e| e.path().join("state")).filter(|p| p.is_file()).collect();
        states.sort();
        let lid = states.into_iter().next().ok_or(Error::NoLid)?;
        Ok(Machine { limits, lid })
    }

    pub fn limits(&self) -> Result<Limits, Error> {
        Ok((read_limit(&self.limits[0])?, read_limit(&self.limits[1])?))
    }

    /// Both writes are tried; the first failure is returned.
    pub fn set_limits(&self, (msr, mmio): Limits) -> Result<(), Error> {
        let a = write_limit(&self.limits[0], msr);
        let b = write_limit(&self.limits[1], mmio);
        a.and(b)
    }

    pub fn lid_closed(&self) -> Result<bool, Error> {
        let text = read(&self.lid)?;
        match text.split_once(':').map(|(_, state)| state.trim()) {
            Some("closed") => Ok(true),
            Some("open") => Ok(false),
            _ => Err(Error::Parse { path: self.lid.clone(), text: text.trim().to_owned() }),
        }
    }
}

/// The only writes the guard makes. The file is opened, never created.
fn write_limit(path: &Path, value: u64) -> Result<(), Error> {
    let fail = |source| Error::Write { path: path.to_owned(), source };
    let mut file = OpenOptions::new().write(true).truncate(true).open(path).map_err(fail)?;
    file.write_all(format!("{value}\n").as_bytes()).map_err(fail)
}
