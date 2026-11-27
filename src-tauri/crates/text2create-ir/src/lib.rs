//! Project IR v1: the durable, structured state of a Text2Create project.
//!
//! This crate implements the data model, strict loading, invariants, domain codec
//! interface, and canonical serialization specified in `docs/PROJECT_IR.md`
//! (ADR-0008, ADR-0014). It has no Tauri dependency and performs no I/O.
//!
//! A validated project is obtained only through [`Project::load`],
//! [`Project::create`], or [`Project::migrate`] (for documents written with an
//! older format or domain version), and changed only through
//! [`Project::apply`], the write path. `Project` has no other public mutators.
//! Every error that rejects data reports where the data is, as a
//! [`FieldPath`].
//!
//! A project exposes its document only by shared reference:
//!
//! ```compile_fail
//! fn change(project: &mut text2create_ir::Project) {
//!     project.document().requirements.clear();
//! }
//! ```
//!
//! and its document field is private:
//!
//! ```compile_fail
//! use text2create_ir::{model::ProjectDocument, Project};
//! fn replace(project: &mut Project, document: ProjectDocument) {
//!     project.document = document;
//! }
//! ```

#![forbid(unsafe_code)]

pub mod change;
pub mod domain;
pub mod ids;
mod invariants;
mod migrate;
pub mod model;
mod path;
mod project;
pub mod scalar;
pub mod source;
pub mod strict;
mod write;

pub use invariants::{InvariantError, Rule};
pub use migrate::{
    DOMAIN_MIGRATION_PREFIX, FORMAT_MIGRATION_COMPONENT, MigrateError, Migrated, MigrationStep,
};
pub use path::FieldPath;
pub use project::{LoadError, Project};
pub use write::{Applied, WriteEnv, WriteError, WriteErrorKind};
