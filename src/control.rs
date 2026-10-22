//! The control logic, free of I/O.
//!
//! - **Idle**: nothing is written until the EC's event arrives.
//! - **Clearing**: the offset is raised `step_up` every `interval` seconds
//!   while the package is above `hold_temp`; once it has stayed at or below
//!   `hold_temp` for `settle` seconds the EC's alert counts as cleared.
//! - **Holding**: the offset stays. After `probe_interval` seconds without an
//!   event it is lowered by one degree, and so on until the offset that was in
//!   force before the first event is reached, which ends the episode. An event
//!   while holding means the value was too low: back to Clearing.

use crate::error::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Clearing ends once the package is at or below this (degrees C).
    pub hold_temp: i32,
    /// Degrees added per clearing step.
    pub step_up: i32,
    /// The offset is never raised beyond this.
    pub max_offset: i32,
    /// Seconds between control steps.
    pub interval: i32,
    /// Seconds at or below `hold_temp` before clearing ends.
    pub settle: i32,
    /// Seconds without an event before one step down.
    pub probe_interval: i32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hold_temp: 75,
            step_up: 2,
            max_offset: 30,
            interval: 4,
            settle: 20,
            probe_interval: 600,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), Error> {
        let bad = |what: &str| Err(Error::Usage(format!("invalid setting: {what}")));
        if !(60..=95).contains(&self.hold_temp) {
            return bad("hold-temp must be 60..=95");
        }
        if !(1..=10).contains(&self.step_up) {
            return bad("step-up must be 1..=10");
        }
        if !(1..=40).contains(&self.max_offset) {
            return bad("max-offset must be 1..=40");
        }
        if !(1..=30).contains(&self.interval) {
            return bad("interval must be 1..=30");
        }
        if self.settle < self.interval {
            return bad("settle must be at least interval");
        }
        if self.probe_interval < 60 {
            return bad("probe-interval must be at least 60");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Clearing,
    Holding,
}

/// What a step did, for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What {
    Raised,
    /// Still too hot at the maximum offset.
    AtMax,
    Cleared,
    Lowered,
    /// Event while holding: back to clearing.
    Tripped,
    /// The original offset is back; the episode is over.
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    /// Seconds, monotonic.
    pub now: i32,
    /// The EC's event arrived since the last step.
    pub event: bool,
    /// Package temperature, degrees C.
    pub temp: i32,
    /// Offset currently in force.
    pub offset: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    /// The offset to write, if it is to be written.
    pub write: Option<i32>,
    pub what: Option<What>,
}

#[derive(Clone, Copy, Debug)]
pub struct State {
    phase: Phase,
    /// Offset found at the first event: the release point.
    base: i32,
    below: i32,
    quiet: i32,
    next_step: i32,
}

impl Default for State {
    fn default() -> Self {
        State { phase: Phase::Idle, base: 0, below: 0, quiet: 0, next_step: 0 }
    }
}

impl State {
    pub fn active(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// The offset to put back when the guard stops, if it changed anything.
    pub fn base(&self) -> Option<i32> {
        self.active().then_some(self.base)
    }

    /// Whether this input starts an episode (so the base must be noted first).
    pub fn would_start(&self, input: &Input) -> bool {
        self.phase == Phase::Idle && input.event
    }

    pub fn step(&mut self, config: &Config, input: &Input) -> Output {
        if input.event {
            let tripped = self.phase == Phase::Holding;
            if self.phase == Phase::Idle {
                self.base = input.offset;
            }
            self.phase = Phase::Clearing;
            self.below = 0;
            self.quiet = 0;
            self.next_step = input.now;
            if tripped {
                let mut out = self.clearing(config, input);
                out.what = Some(What::Tripped);
                self.next_step = input.now + config.interval;
                return out;
            }
        }
        if self.phase == Phase::Idle || input.now < self.next_step {
            return Output::default();
        }
        self.next_step = input.now + config.interval;
        match self.phase {
            Phase::Clearing => self.clearing(config, input),
            _ => self.holding(config, input),
        }
    }

    fn clearing(&mut self, config: &Config, input: &Input) -> Output {
        if input.temp > config.hold_temp {
            self.below = 0;
            let raised = (input.offset + config.step_up).min(config.max_offset);
            return if raised > input.offset {
                Output { write: Some(raised), what: Some(What::Raised) }
            } else {
                Output { write: None, what: Some(What::AtMax) }
            };
        }
        self.below += config.interval;
        if self.below < config.settle {
            return Output::default();
        }
        self.phase = Phase::Holding;
        self.quiet = 0;
        Output { write: None, what: Some(What::Cleared) }
    }

    fn holding(&mut self, config: &Config, input: &Input) -> Output {
        self.quiet += config.interval;
        if self.quiet < config.probe_interval {
            return Output::default();
        }
        self.quiet = 0;
        let lower = input.offset - 1;
        if lower <= self.base {
            let base = self.base;
            *self = State::default();
            return Output { write: Some(base), what: Some(What::Released) };
        }
        Output { write: Some(lower), what: Some(What::Lowered) }
    }

    /// Forget the episode after the original offset has been restored.
    pub fn reset(&mut self) {
        *self = State::default();
    }
}
