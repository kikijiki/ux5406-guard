#![allow(dead_code)]
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

/// A fake `/sys` below a fresh temporary directory.
pub struct Fake {
    pub root: PathBuf,
    pub sys: PathBuf,
}

impl Fake {
    pub fn new(product: &str, offset: i32, temp_c: i32) -> Fake {
        let root = std::env::temp_dir()
            .join(format!("ux5406-guard-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let sys = root.join("sys");
        let w = |rel: &str, text: String| {
            let p = sys.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        w("class/dmi/id/product_name", format!("{product}\n"));
        w("class/hwmon/hwmon3/name", "nvme\n".into());
        w("class/hwmon/hwmon8/name", "coretemp\n".into());
        w("class/hwmon/hwmon8/temp1_label", "Package id 0\n".into());
        w("class/hwmon/hwmon8/temp1_input", format!("{}\n", temp_c * 1000));
        w("bus/pci/devices/0000:00:04.0/tcc_offset_degree_celsius", format!("{offset}\n"));
        fs::create_dir_all(root.join("run")).unwrap();
        fs::write(root.join("kmsg"), "").unwrap();
        Fake { root, sys }
    }

    pub fn set_temp(&self, c: i32) {
        fs::write(self.sys.join("class/hwmon/hwmon8/temp1_input"), format!("{}\n", c * 1000)).unwrap();
    }

    pub fn offset(&self) -> i32 {
        read(&self.offset_path()).trim().parse().unwrap()
    }

    pub fn offset_path(&self) -> PathBuf {
        self.sys.join("bus/pci/devices/0000:00:04.0/tcc_offset_degree_celsius")
    }

    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }

    pub fn kmsg(&self) -> PathBuf {
        self.root.join("kmsg")
    }

    /// Every file below the fake tree with its content, to prove what was written.
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
