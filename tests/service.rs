mod common;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::rc::Rc;

use common::Fake;
use ux5406_guard::control::{Config, Input, Level, State};
use ux5406_guard::error::Error;
use ux5406_guard::kmsg::{has_event, Events};
use ux5406_guard::service::{note_path, restore, Note, Service};
use ux5406_guard::sysfs::Machine;

/// A fake kernel-log stream: the test pushes events, the service polls them.
#[derive(Clone, Default)]
struct Stream(Rc<RefCell<VecDeque<bool>>>);

impl Stream {
    fn event(&self) {
        self.0.borrow_mut().push_back(true);
    }
}

impl Events for Stream {
    fn poll(&mut self) -> Result<bool, Error> {
        Ok(self.0.borrow_mut().pop_front().unwrap_or(false))
    }
}

fn config() -> Config {
    Config { cap_w: 13, safe_w: 9, hold: 60 }
}

/// Original limits: 37 W (MSR) and 22 W (firmware).
fn setup() -> (Fake, Service<Stream>, Stream) {
    let fake = Fake::new("ASUS Zenbook S 14 UX5406SA_UX5406SA", 37, 22);
    let machine = Machine::open(&fake.sys, &fake.proc).unwrap();
    let stream = Stream::default();
    let service = Service::new(machine, Note(note_path(&fake.run_dir())), config(), stream.clone());
    (fake, service, stream)
}

/// Tick once a second from `from` to `to`, inclusive.
fn run(s: &mut Service<Stream>, from: i32, to: i32) {
    for t in from..=to {
        s.tick(t).unwrap();
    }
}

#[test]
fn nothing_is_written_with_the_lid_open() {
    let (fake, mut s, _) = setup();
    let before = fake.snapshot();
    run(&mut s, 0, 300);
    assert_eq!(fake.snapshot(), before);
    assert_eq!(fake.limits(), (37, 22));
}

#[test]
fn closing_the_lid_caps_both_limits_and_opening_it_restores_them() {
    let (fake, mut s, _) = setup();
    fake.set_lid(true);
    run(&mut s, 0, 2);
    assert_eq!(fake.limits(), (13, 13));
    assert!(note_path(&fake.run_dir()).exists());
    fake.set_lid(false);
    run(&mut s, 3, 5);
    assert_eq!(fake.limits(), (37, 22));
    assert!(!note_path(&fake.run_dir()).exists());
}

#[test]
fn an_event_drops_to_the_safe_level_for_the_hold_then_returns() {
    let (fake, mut s, stream) = setup();
    fake.set_lid(true);
    run(&mut s, 0, 9);
    stream.event();
    run(&mut s, 10, 10);
    assert_eq!(fake.limits(), (9, 9));
    run(&mut s, 11, 69);
    assert_eq!(fake.limits(), (9, 9), "held for 60 s");
    run(&mut s, 70, 70);
    assert_eq!(fake.limits(), (13, 13));
}

#[test]
fn an_event_with_the_lid_open_is_also_met_and_released_to_the_original() {
    let (fake, mut s, stream) = setup();
    stream.event();
    run(&mut s, 0, 0);
    assert_eq!(fake.limits(), (9, 9));
    run(&mut s, 1, 60);
    assert_eq!(fake.limits(), (37, 22));
    assert!(!note_path(&fake.run_dir()).exists());
}

#[test]
fn a_second_event_extends_the_hold() {
    let (fake, mut s, stream) = setup();
    fake.set_lid(true);
    stream.event();
    run(&mut s, 0, 40);
    stream.event();
    run(&mut s, 41, 100);
    assert_eq!(fake.limits(), (9, 9));
    run(&mut s, 101, 101);
    assert_eq!(fake.limits(), (13, 13));
}

#[test]
fn limits_changed_by_someone_else_are_put_back() {
    let (fake, mut s, _) = setup();
    fake.set_lid(true);
    run(&mut s, 0, 2);
    fs::write(fake.msr_path(), "37000000\n").unwrap();
    run(&mut s, 3, 3);
    assert_eq!(fake.limits(), (13, 13));
}

#[test]
fn the_original_is_the_first_one_found_not_a_capped_value() {
    let (fake, mut s, stream) = setup();
    fake.set_lid(true);
    run(&mut s, 0, 2);
    stream.event();
    run(&mut s, 3, 3);
    s.finish().unwrap();
    assert_eq!(fake.limits(), (37, 22));
}

#[test]
fn restore_on_finish_and_note_removed() {
    let (fake, mut s, _) = setup();
    fake.set_lid(true);
    run(&mut s, 0, 2);
    s.finish().unwrap();
    assert_eq!(fake.limits(), (37, 22));
    assert!(!note_path(&fake.run_dir()).exists());
}

#[test]
fn restore_after_a_kill_uses_the_note() {
    let (fake, mut s, _) = setup();
    fake.set_lid(true);
    run(&mut s, 0, 2);
    drop(s); // killed: no finish()
    assert_eq!(fake.limits(), (13, 13));
    let machine = Machine::open(&fake.sys, &fake.proc).unwrap();
    let note = Note(note_path(&fake.run_dir()));
    assert_eq!(restore(&machine, &note).unwrap(), Some((37_000_000, 22_000_000)));
    assert_eq!(fake.limits(), (37, 22));
    assert_eq!(restore(&machine, &note).unwrap(), None);
}

#[test]
fn wrong_product_is_refused() {
    let fake = Fake::new("ASUS Zenbook Duo UX8406CA", 37, 22);
    match Machine::open(&fake.sys, &fake.proc) {
        Err(Error::WrongProduct { found, .. }) => assert!(found.contains("UX8406CA")),
        other => panic!("expected WrongProduct, got {other:?}"),
    }
}

#[test]
fn a_missing_lid_is_a_typed_error() {
    let fake = Fake::new("UX5406SA", 37, 22);
    fs::remove_dir_all(fake.proc.join("acpi")).unwrap();
    assert!(matches!(Machine::open(&fake.sys, &fake.proc), Err(Error::NoLid)));
}

#[test]
fn unwritable_limit_is_a_typed_error_and_the_service_continues() {
    let (fake, mut s, _) = setup();
    fs::set_permissions(fake.msr_path(), fs::Permissions::from_mode(0o444)).unwrap();
    if fs::OpenOptions::new().write(true).open(fake.msr_path()).is_ok() {
        eprintln!("running as root: read-only mode is not enforced, skipping");
        return;
    }
    fake.set_lid(true);
    assert!(matches!(s.tick(0), Err(Error::Write { .. })));
    // The attribute becomes writable again: the next tick works.
    fs::set_permissions(fake.msr_path(), fs::Permissions::from_mode(0o644)).unwrap();
    s.tick(1).unwrap();
    assert_eq!(fake.limits(), (13, 13));
    s.finish().unwrap();
    assert_eq!(fake.limits(), (37, 22), "the original survives the failed write");
}

#[test]
fn an_event_is_kept_when_the_step_fails() {
    let (fake, mut s, stream) = setup();
    fs::write(fake.proc.join("acpi/button/lid/LID/state"), "banana\n").unwrap();
    stream.event();
    assert!(matches!(s.tick(0), Err(Error::Parse { .. })));
    fake.set_lid(false);
    s.tick(1).unwrap(); // no new event was pushed
    assert_eq!(fake.limits(), (9, 9));
}

#[test]
fn garbage_in_a_limit_is_not_believed() {
    let (fake, mut s, _) = setup();
    fs::write(fake.msr_path(), "banana\n").unwrap();
    assert!(matches!(s.tick(0), Err(Error::Parse { .. })));
    fs::write(fake.msr_path(), "1\n").unwrap();
    assert!(matches!(s.tick(1), Err(Error::Parse { .. })), "out of range");
}

#[test]
fn only_the_two_limits_are_ever_written() {
    let (fake, mut s, stream) = setup();
    let before = fake.snapshot();
    fake.set_lid(true);
    stream.event();
    run(&mut s, 0, 100);
    s.finish().unwrap();
    fake.set_lid(false);
    assert_eq!(fake.snapshot(), before);
}

#[test]
fn event_line_matching() {
    let kern = "6,1234,5678,-;asus_wmi: Unknown key code 0x6d\n SUBSYSTEM=platform\n";
    assert!(has_event(kern));
    assert!(has_event(&format!("6,1,1,-;other\n{kern}")));
    // user space (facility 1) writing the same text is not the kernel
    assert!(!has_event("14,1234,5678,-;asus_wmi: Unknown key code 0x6d\n"));
    assert!(!has_event("6,1,1,-;asus_wmi: Unknown key code 0x6e\n"));
    assert!(!has_event("6,1,1,-;note: asus_wmi: Unknown key code 0x6d\n"));
    assert!(!has_event(" asus_wmi: Unknown key code 0x6d\n"));
}

#[test]
fn control_levels() {
    let cfg = config();
    let mut st = State::default();
    let at = |now, event, lid_closed| Input { now, event, lid_closed };
    assert_eq!(st.step(&cfg, &at(0, false, false)), Level::Original);
    assert_eq!(st.step(&cfg, &at(1, false, true)), Level::Cap);
    assert_eq!(st.step(&cfg, &at(2, true, true)), Level::Safe);
    assert_eq!(st.step(&cfg, &at(61, false, true)), Level::Safe);
    assert_eq!(st.step(&cfg, &at(62, false, true)), Level::Cap);
}

#[test]
fn config_limits() {
    let ok = Config::default();
    assert!(ok.validate().is_ok());
    assert!(Config { cap_w: 16, ..ok }.validate().is_err(), "16 W produced the event");
    assert!(Config { safe_w: 14, ..ok }.validate().is_err(), "safe above cap");
    assert!(Config { hold: 5, ..ok }.validate().is_err());
}
