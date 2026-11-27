//! Project IR v1: the durable, structured state of a Text2Create project.
//!
//! This crate implements the data model, strict loading, invariants, domain codec
//! interface, and canonical serialization specified in `docs/PROJECT_IR.md`
//! (ADR-0008, ADR-0014). It has no Tauri dependency and performs no I/O.
//!
//! The only way to obtain a validated project is [`Project::load`]. Changing a
//! project (the write path) is not part of this crate yet (#43).

#![forbid(unsafe_code)]

pub mod domain;
pub mod ids;
mod invariants;
pub mod model;
mod project;
pub mod scalar;
pub mod strict;

pub use invariants::{InvariantError, Rule};
pub use project::{LoadError, Project};
