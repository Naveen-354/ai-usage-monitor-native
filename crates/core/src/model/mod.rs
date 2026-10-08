//! Normalised internal model. The UI and database only ever see these types, never an
//! agent-specific format.

mod accuracy;
mod agents;
mod event;
mod health;

pub use accuracy::Accuracy;
pub use agents::{catalog, AgentMeta};
pub use event::{AgentId, ProjectRef, UsageEvent};
pub use health::{Availability, CollectorHealth};
