pub mod document_formats;
pub mod error;
pub mod index_uid;
pub mod keys;
pub mod settings;
pub mod star_or;
pub mod tasks;

pub use milli;
pub use milli::heed;
pub use milli::Index;
use uuid::Uuid;

pub type Document = serde_json::Map<String, serde_json::Value>;
pub type InstanceUid = Uuid;
