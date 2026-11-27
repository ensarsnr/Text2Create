//! The source of a change, and the clock and identifier generator the core
//! uses to allocate timestamps and identifiers.
//!
//! [`Source`] has no serde implementation and its fields are private, so it
//! can never be read from frontend command arguments, a change set, or model
//! output. The trusted Rust code that knows where a change comes from picks
//! the matching constructor.

use crate::ids::{TaskId, TaskRunId};
use crate::model::{Activity, Actor, ModelInvocation};
use crate::scalar::{Text, Timestamp};
use uuid::Uuid;

/// Who or what makes a change, and in which activity. Becomes the actor,
/// activity, and model invocation of the change's provenance record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub(crate) actor: Actor,
    pub(crate) activity: Activity,
    pub(crate) model: Option<ModelInvocation>,
}

impl Source {
    /// The user edited the project directly.
    pub fn user_edit() -> Self {
        Self::new(Actor::User {}, Activity::UserEdit {}, None)
    }

    /// The user imported existing material, such as a document.
    pub fn user_import() -> Self {
        Self::new(Actor::User {}, Activity::Import {}, None)
    }

    /// A model turned user input into structured records.
    pub fn model_interpretation(
        provider: Text<64>,
        model: Text<128>,
        invocation: ModelInvocation,
    ) -> Self {
        Self::new(
            Actor::Model { provider, model },
            Activity::Interpretation {},
            Some(invocation),
        )
    }

    /// A model executed a task run.
    pub fn model_task_run(
        provider: Text<64>,
        model: Text<128>,
        invocation: ModelInvocation,
        task: TaskId,
        run: TaskRunId,
    ) -> Self {
        Self::new(
            Actor::Model { provider, model },
            Activity::TaskRun { task, run },
            Some(invocation),
        )
    }

    /// A tool executed a task run.
    pub fn tool_task_run(name: Text<64>, version: Text<64>, task: TaskId, run: TaskRunId) -> Self {
        Self::new(
            Actor::Tool { name, version },
            Activity::TaskRun { task, run },
            None,
        )
    }

    /// A component of Text2Create imported material on its own.
    pub fn system_import(component: Text<64>) -> Self {
        Self::new(Actor::System { component }, Activity::Import {}, None)
    }

    fn new(actor: Actor, activity: Activity, model: Option<ModelInvocation>) -> Self {
        Self {
            actor,
            activity,
            model,
        }
    }

    /// The actor recorded for changes from this source.
    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    /// The activity recorded for changes from this source.
    pub fn activity(&self) -> &Activity {
        &self.activity
    }

    pub(crate) fn is_user(&self) -> bool {
        matches!(self.actor, Actor::User {})
    }

    pub(crate) fn is_tool_or_system(&self) -> bool {
        matches!(self.actor, Actor::Tool { .. } | Actor::System { .. })
    }
}

/// Provides the timestamps of provenance records.
pub trait Clock {
    fn now(&mut self) -> Timestamp;
}

/// Provides the UUIDs of new record identifiers.
pub trait IdGenerator {
    fn next_uuid(&mut self) -> Uuid;
}

/// The system clock in UTC, truncated to milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&mut self) -> Timestamp {
        let now = time::OffsetDateTime::now_utc();
        let text = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second(),
            now.millisecond()
        );
        Timestamp::try_from(text).expect("the system clock produces a valid timestamp")
    }
}

/// Time-ordered UUIDv7 identifiers (RFC 9562).
#[derive(Clone, Copy, Debug, Default)]
pub struct UuidV7Generator;

impl IdGenerator for UuidV7Generator {
    fn next_uuid(&mut self) -> Uuid {
        Uuid::now_v7()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_clock_and_generator() {
        let a = SystemClock.now();
        assert_eq!(a.as_str().len(), 24);
        let first = UuidV7Generator.next_uuid();
        let second = UuidV7Generator.next_uuid();
        assert_eq!(first.get_version_num(), 7);
        assert_ne!(first, second);
    }

    #[test]
    fn sources_map_to_actor_and_activity() {
        assert_eq!(Source::user_edit().actor(), &Actor::User {});
        assert_eq!(Source::user_import().activity(), &Activity::Import {});
        let component = Text::try_from("scanner".to_owned()).unwrap();
        assert!(Source::system_import(component).is_tool_or_system());
        assert!(!Source::user_edit().is_tool_or_system());
    }
}
