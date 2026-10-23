//! The control logic, free of I/O.
//!
//! The package power limit is one of three levels:
//!
//! - **Original**: the limits found at start. Used while the lid is open.
//! - **Cap**: `cap_w`. Used while the lid is closed.
//! - **Safe**: `safe_w`. Used for `hold` seconds after the EC's event, whatever
//!   the lid says; a further event extends it.

use crate::error::Error;

/// Highest cap that ran 20 minutes at full load with the lid closed and no event.
pub const MAX_CAP_W: u32 = 14;
pub const MIN_W: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Package power (W) while the lid is closed.
    pub cap_w: u32,
    /// Package power (W) after an event.
    pub safe_w: u32,
    /// Seconds the safe level is kept after the last event.
    pub hold: i32,
}

impl Default for Config {
    fn default() -> Self {
        Config { cap_w: 13, safe_w: 9, hold: 300 }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), Error> {
        let bad = |what: String| Err(Error::Usage(format!("invalid setting: {what}")));
        if !(MIN_W..=MAX_CAP_W).contains(&self.cap_w) {
            return bad(format!("cap must be {MIN_W}..={MAX_CAP_W}"));
        }
        if !(MIN_W..=self.cap_w).contains(&self.safe_w) {
            return bad(format!("safe must be {MIN_W}..=cap"));
        }
        if !(30..=3600).contains(&self.hold) {
            return bad("hold must be 30..=3600".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Original,
    Cap,
    Safe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    /// Seconds, monotonic.
    pub now: i32,
    /// The EC's event arrived since the last step.
    pub event: bool,
    pub lid_closed: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    safe_until: Option<i32>,
}

impl State {
    pub fn step(&mut self, config: &Config, input: &Input) -> Level {
        if input.event {
            self.safe_until = Some(input.now + config.hold);
        }
        if self.safe_until.is_some_and(|until| input.now < until) {
            return Level::Safe;
        }
        self.safe_until = None;
        if input.lid_closed {
            Level::Cap
        } else {
            Level::Original
        }
    }
}
