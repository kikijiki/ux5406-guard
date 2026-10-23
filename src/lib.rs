//! ux5406-guard: caps package power on the ASUS Zenbook S 14 UX5406SA while the
//! lid is closed, and harder after the EC's thermal event, so that the EC never
//! cuts the power.

pub mod control;
pub mod error;
pub mod kmsg;
pub mod service;
pub mod sysfs;
