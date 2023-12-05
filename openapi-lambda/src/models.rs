use serde::{Deserialize, Serialize};

pub use chrono;
pub use indexmap::IndexSet;
pub use serde_json;

/// An empty object (e.g., `{}` in a JSON request/response body).
///
/// Note that this type ignores any unexpected fields during deserialization.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct EmptyModel {}
