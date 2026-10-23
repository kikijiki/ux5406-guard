#![allow(dead_code)]
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

const MSR: &str = "class/powercap/intel-rapl:0/constraint_0_power_limit_uw";
const MMIO: &str = "class/powercap/intel-rapl-mmio:0/constraint_0_power_limit_uw";

/// A fake `/sys` and `/proc` below a fresh temporary directory.
pub struct Fake {
    pub root: PathBuf,
    pub sys: PathBuf,
    pub proc: PathBuf,
}

impl Fake {
    /// Limits are the MSR and firmware values in watts; the lid starts open.
    pub fn new(product: &str, msr_w: u64, mmio_w: u64) -> Fake {
        let root = std::env::temp_dir()
            .join(format!("ux5406-guard-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let fake = Fake { sys: root.join("sys"), proc: root.join("proc"), root };
        let w = |base: &Path, rel: &str, text: String| {
            let p = base.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        w(&fake.sys, "class/dmi/id/product_name", format!("{product}\n"));
        w(&fake.sys, MSR, format!("{}\n", msr_w * 1_000_000));
        w(&fake.sys, MMIO, format!("{}\n", mmio_w * 1_000_000));
        fs::create_dir_all(fake.root.join("run")).unwrap();
        fs::write(fake.root.join("kmsg"), "").unwrap();
        fake.set_lid(false);
        fake
    }

    pub fn set_lid(&self, closed: bool) {
        let p = self.proc.join("acpi/button/lid/LID/state");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, format!("state:      {}\n", if closed { "closed" } else { "open" })).unwrap();
    }

    /// The MSR and firmware limits in watts.
    pub fn limits(&self) -> (u64, u64) {
        let r = |rel: &str| read(&self.sys.join(rel)).trim().parse::<u64>().unwrap() / 1_000_000;
        (r(MSR), r(MMIO))
    }

    pub fn msr_path(&self) -> PathBuf {
        self.sys.join(MSR)
    }

    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }

    pub fn kmsg(&self) -> PathBuf {
        self.root.join("kmsg")
    }

    /// Every file below the fake trees with its content, to prove what was written.
    pub fn snapshot(&self) -> Vec<(PathBuf, String)> {
        fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
            let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
            entries.sort_by_key(|e| e.path());
            for e in entries {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out)
                } else {
                    out.push((p.clone(), fs::read_to_string(&p).unwrap()))
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.sys, &mut out);
        walk(&self.proc, &mut out);
        out
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}
