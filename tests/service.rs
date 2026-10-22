mod common;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::rc::Rc;

use common::Fake;
use ux5406_guard::control::Config;
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
    // 1 s steps keep the scripted timelines short.
    Config { hold_temp: 85, step_up: 2, max_offset: 30, interval: 1, settle: 3, probe_interval: 60 }
}

fn setup(offset: i32, temp: i32) -> (Fake, Service<Stream>, Stream) {
    let fake = Fake::new("ASUS Zenbook S 14 UX5406SA_UX5406SA", offset, temp);
    let machine = Machine::open(&fake.sys).unwrap();
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
fn nothing_is_written_without_an_event() {
    let (fake, mut s, _) = setup(8, 99);
    let before = fake.snapshot();
    run(&mut s, 0, 300);
    assert_eq!(fake.snapshot(), before);
    assert_eq!(fake.offset(), 8);
}

#[test]
fn event_raises_the_offset_in_steps_while_hot() {
    let (fake, mut s, stream) = setup(8, 92);
    stream.event();
    s.tick(0).unwrap();
    assert_eq!(fake.offset(), 10);
    s.tick(1).unwrap();
    assert_eq!(fake.offset(), 12);
    run(&mut s, 2, 5);
    assert_eq!(fake.offset(), 20);
}

#[test]
fn offset_never_exceeds_the_maximum() {
    let (fake, mut s, stream) = setup(8, 99);
    stream.event();
    run(&mut s, 0, 100);
    assert_eq!(fake.offset(), 30);
}

#[test]
fn step_down_and_release_with_hysteresis() {
    let (fake, mut s, stream) = setup(8, 99);
    stream.event();
    run(&mut s, 0, 3); // 8 -> 16 while hot
    assert_eq!(fake.offset(), 16);
    fake.set_temp(80);
    run(&mut s, 4, 6); // settle 3 s at or below 85: holding
    assert_eq!(fake.offset(), 16);
    run(&mut s, 7, 6 + 59); // under probe_interval: no change
    assert_eq!(fake.offset(), 16);
    run(&mut s, 66, 66);
    assert_eq!(fake.offset(), 15, "one degree down after probe_interval");
    run(&mut s, 67, 66 + 60);
    assert_eq!(fake.offset(), 14);
    // Walk down to the original offset: released, note gone.
    run(&mut s, 127, 127 + 60 * 8);
    assert_eq!(fake.offset(), 8);
    assert!(!note_path(&fake.run_dir()).exists());
    assert!(!s.state.active());
}

#[test]
fn event_while_holding_raises_again() {
    let (fake, mut s, stream) = setup(8, 99);
    stream.event();
    run(&mut s, 0, 2);
    fake.set_temp(80);
    run(&mut s, 3, 8); // cooled, holding at 14
    assert_eq!(fake.offset(), 14);
    fake.set_temp(90);
    stream.event();
    run(&mut s, 9, 9);
    assert_eq!(fake.offset(), 16, "repeated event while throttled raises again");
    // The original offset is still the one found at the first event.
    assert_eq!(s.state.base(), Some(8));
}

#[test]
fn restore_on_finish_and_note_removed() {
    let (fake, mut s, stream) = setup(8, 99);
    stream.event();
    run(&mut s, 0, 4);
    assert!(fake.offset() > 8);
    assert!(note_path(&fake.run_dir()).exists());
    s.finish().unwrap();
    assert_eq!(fake.offset(), 8);
    assert!(!note_path(&fake.run_dir()).exists());
}

#[test]
fn restore_after_a_kill_uses_the_note() {
    let (fake, mut s, stream) = setup(8, 99);
    stream.event();
    run(&mut s, 0, 4);
    drop(s); // killed: no finish()
    assert!(fake.offset() > 8);
    let machine = Machine::open(&fake.sys).unwrap();
    let note = Note(note_path(&fake.run_dir()));
    assert_eq!(restore(&machine, &note).unwrap(), Some(8));
    assert_eq!(fake.offset(), 8);
    assert_eq!(restore(&machine, &note).unwrap(), None);
}

#[test]
fn wrong_product_is_refused() {
    let fake = Fake::new("ASUS Zenbook Duo UX8406CA", 8, 50);
    match Machine::open(&fake.sys) {
        Err(Error::WrongProduct { found, .. }) => assert!(found.contains("UX8406CA")),
        other => panic!("expected WrongProduct, got {other:?}"),
    }
}

#[test]
fn unwritable_attribute_is_a_typed_error_and_the_service_continues() {
    let (fake, mut s, stream) = setup(8, 99);
    fs::set_permissions(fake.offset_path(), fs::Permissions::from_mode(0o444)).unwrap();
    if fs::OpenOptions::new().write(true).open(fake.offset_path()).is_ok() {
        eprintln!("running as root: read-only mode is not enforced, skipping");
        return;
    }
    stream.event();
    assert!(matches!(s.tick(0), Err(Error::Write { .. })));
    assert_eq!(fake.offset(), 8);
    assert!(!s.state.active(), "state does not advance on a failed write");
    // The attribute becomes writable again: the next tick works.
    fs::set_permissions(fake.offset_path(), fs::Permissions::from_mode(0o644)).unwrap();
    s.tick(1).unwrap();
    assert_eq!(fake.offset(), 10);
}

#[test]
fn an_event_is_kept_when_the_step_fails() {
    let (fake, mut s, stream) = setup(8, 99);
    fs::write(fake.sys.join("class/hwmon/hwmon8/temp1_input"), "banana\n").unwrap();
    stream.event();
    assert!(matches!(s.tick(0), Err(Error::Parse { .. })));
    fake.set_temp(99);
    s.tick(1).unwrap(); // no new event was pushed
    assert_eq!(fake.offset(), 10);
}

#[test]
fn garbage_in_the_attribute_is_not_believed() {
    let (fake, mut s, _) = setup(8, 50);
    fs::write(fake.offset_path(), "banana\n").unwrap();
    assert!(matches!(s.tick(0), Err(Error::Parse { .. })));
}

#[test]
fn only_the_one_attribute_is_ever_written() {
    let (fake, mut s, stream) = setup(8, 99);
    let before = fake.snapshot();
    stream.event();
    run(&mut s, 0, 10);
    s.finish().unwrap();
    let after = fake.snapshot();
    let changed: Vec<_> = before.iter().zip(&after).filter(|(a, b)| a != b).map(|(a, _)| a.0.clone()).collect();
    assert!(changed.is_empty(), "restored: nothing differs, got {changed:?}");
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
fn control_reports_a_trip_on_event_while_holding() {
    use ux5406_guard::control::{Input, State, What};
    let cfg = config();
    let mut st = State::default();
    let at = |now, event, temp, offset| Input { now, event, temp, offset };
    st.step(&cfg, &at(0, true, 90, 8));
    for t in 1..=3 {
        st.step(&cfg, &at(t, false, 80, 10));
    }
    let out = st.step(&cfg, &at(4, true, 90, 10));
    assert_eq!(out.what, Some(What::Tripped));
    assert_eq!(out.write, Some(12));
}
