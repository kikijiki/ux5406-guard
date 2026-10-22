//! The machine as sysfs shows it. The guard writes exactly one file: the CPU
//! thermal offset.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::Error;

pub const PRODUCT: &str = "UX5406SA";
const PRODUCT_FILE: &str = "class/dmi/id/product_name";
const OFFSET_FILE: &str = "bus/pci/devices/0000:00:04.0/tcc_offset_degree_celsius";
/// A plausible offset; anything else in the file is not believed.
const OFFSET_RANGE: std::ops::RangeInclusive<i32> = 0..=63;

/// The files the guard uses, resolved below a `/sys`-like root.
#[derive(Clone, Debug)]
pub struct Machine {
    pub offset: PathBuf,
    pub temp: PathBuf,
}

fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|source| Error::Read { path: path.to_owned(), source })
}

fn read_int(path: &Path) -> Result<i32, Error> {
    let text = read(path)?;
    text.trim()
        .parse()
        .map_err(|_| Error::Parse { path: path.to_owned(), text: text.trim().to_owned() })
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

pub fn offset_path(sys: &Path) -> PathBuf {
    sys.join(OFFSET_FILE)
}

impl Machine {
    /// Check the product and find the package sensor.
    pub fn open(sys: &Path) -> Result<Machine, Error> {
        check_product(sys)?;
        let hwmon = sys.join("class/hwmon");
        let entries = fs::read_dir(&hwmon)
            .map_err(|source| Error::Read { path: hwmon.clone(), source })?;
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for dir in dirs {
            let named = fs::read_to_string(dir.join("name")).is_ok_and(|n| n.trim() == "coretemp");
            let package =
                fs::read_to_string(dir.join("temp1_label")).is_ok_and(|l| l.trim() == "Package id 0");
            if named && package {
                return Ok(Machine { offset: offset_path(sys), temp: dir.join("temp1_input") });
            }
        }
        Err(Error::NoSensor)
    }

    /// Package temperature, degrees C.
    pub fn temp(&self) -> Result<i32, Error> {
        let milli = read_int(&self.temp)?;
        if !(0..=150_000).contains(&milli) {
            return Err(Error::Parse { path: self.temp.clone(), text: milli.to_string() });
        }
        Ok(milli / 1000)
    }

    pub fn offset(&self) -> Result<i32, Error> {
        let value = read_int(&self.offset)?;
        if OFFSET_RANGE.contains(&value) {
            Ok(value)
        } else {
            Err(Error::Parse { path: self.offset.clone(), text: value.to_string() })
        }
    }

    /// The only write the guard makes. The file is opened, never created.
    pub fn set_offset(&self, value: i32) -> Result<(), Error> {
        let fail = |source| Error::Write { path: self.offset.clone(), source };
        let mut file = OpenOptions::new().write(true).truncate(true).open(&self.offset).map_err(fail)?;
        file.write_all(format!("{value}\n").as_bytes()).map_err(fail)
    }
}
