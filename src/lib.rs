//! ux5406-guard: sheds CPU load on the ASUS Zenbook S 14 UX5406SA when its
//! embedded controller warns of heat, so that the EC never cuts the power.

pub mod control;
pub mod error;
pub mod kmsg;
pub mod service;
pub mod sysfs;
