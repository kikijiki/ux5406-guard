mod common;

use std::fs::OpenOptions;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use common::Fake;

const BIN: &str = env!("CARGO_BIN_EXE_ux5406-guard");

fn spawn(fake: &Fake) -> Child {
    Command::new(BIN)
        .args(["run", "--interval", "1", "--settle", "2", "--probe-interval", "60"])
        .arg("--sys").arg(&fake.sys)
        .arg("--kmsg").arg(fake.kmsg())
        .arg("--state-dir").arg(fake.run_dir())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn emit_event(fake: &Fake) {
    let mut f = OpenOptions::new().append(true).open(fake.kmsg()).unwrap();
    f.write_all(b"6,1,1,-;asus_wmi: Unknown key code 0x6d\n").unwrap();
}

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        sleep(Duration::from_millis(100));
    }
}

fn signal(child: &Child, name: &str) {
    assert!(Command::new("kill").arg(format!("-{name}")).arg(child.id().to_string()).status().unwrap().success());
}

#[test]
fn wrong_product_exits_78_and_touches_nothing() {
    let fake = Fake::new("ASUS Zenbook Duo UX8406CA", 8, 99);
    let before = fake.snapshot();
    let out = Command::new(BIN)
        .arg("--sys").arg(&fake.sys)
        .arg("--state-dir").arg(fake.run_dir())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(78));
    assert!(String::from_utf8_lossy(&out.stderr).contains("refusing to run"));
    assert_eq!(fake.snapshot(), before);
}

#[test]
fn sigterm_restores_the_original_offset() {
    let fake = Fake::new("UX5406SA", 8, 99);
    let mut child = spawn(&fake);
    sleep(Duration::from_millis(500)); // the log is opened at its end
    emit_event(&fake);
    wait_for("offset raised", || fake.offset() > 8);
    signal(&child, "TERM");
    let status = child.wait().unwrap();
    assert!(status.success());
    assert_eq!(fake.offset(), 8);
    assert!(!fake.run_dir().join("original-offset").exists());
}

#[test]
fn sigkill_leaves_a_note_and_restore_command_puts_the_offset_back() {
    let fake = Fake::new("UX5406SA", 8, 99);
    let mut child = spawn(&fake);
    sleep(Duration::from_millis(500));
    emit_event(&fake);
    wait_for("offset raised", || fake.offset() > 8);
    signal(&child, "KILL");
    child.wait().unwrap();
    assert!(fake.offset() > 8);
    let status = Command::new(BIN)
        .arg("restore")
        .arg("--sys").arg(&fake.sys)
        .arg("--state-dir").arg(fake.run_dir())
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fake.offset(), 8);
}

#[test]
fn events_older_than_the_start_are_ignored() {
    let fake = Fake::new("UX5406SA", 8, 99);
    emit_event(&fake);
    let mut child = spawn(&fake);
    sleep(Duration::from_millis(2500));
    assert_eq!(fake.offset(), 8);
    signal(&child, "TERM");
    child.wait().unwrap();
}
