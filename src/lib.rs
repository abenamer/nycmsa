//! NYC middle school admissions Monte Carlo.
//!
//! The crate follows the REAS ports-and-adapters layout. Each operation has
//! one visible code path:
//!
//! inbound adapter ([`adapters::http`] or [`adapters::cli`]) → application
//! use case ([`application`]) → port trait ([`application::ports`]) →
//! outbound adapter ([`adapters`]).
//!
//! [`domain`] holds the model and the pure rules. It has no I/O, no
//! framework types, and no clock. Random values enter the simulation only
//! through a seed, so the same seed gives the same report.
//!
//! [`composition`] is the composition root. The `nycmsa` binary calls it.

pub mod adapters;
pub mod application;
pub mod composition;
pub mod domain;
