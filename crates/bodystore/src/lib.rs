//! Large response bodies (D41): kept in memory up to a threshold and spilled
//! to temp files beyond it ([`BodySink`]), read by ranges or lines, searched,
//! pretty-printed as JSON with constant memory, and kept by id for the UI in
//! a [`ResponseStore`].

pub mod body;
pub mod lines;
pub mod pretty;
pub mod store;

pub use body::{Body, BodySink, SinkConfig, TEMP_PREFIX, utf8_floor};
pub use lines::{SearchMatch, SearchOptions, SearchResult, build_line_index, search};
pub use pretty::{PrettyError, pretty_json};
pub use store::{BodyChunk, BodyHandle, BodyLines, ResponseStore};
