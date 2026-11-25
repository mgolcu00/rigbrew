//! Rigbrew core.
//!
//! Everything the app does that is not UI lives here: scanning the machine, choosing the
//! model variant that fits it, downloading and verifying packages, running the engines.

pub mod bundle;
pub mod chats;
pub mod crash;
pub mod download;
pub mod engine;
pub mod engines;
pub mod events;
pub mod hardware;
pub mod listen;
pub mod manifest;
pub mod select;
pub mod voice;

pub use hardware::{scan, HardwareProfile, Tier};
pub use manifest::Manifest;
pub use select::{recommend, Recommendation};
