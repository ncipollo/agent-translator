//! The translation engine: turns a byte stream of native agent output into common events.
//! Synchronous and I/O-free so it can be tested with byte slices.

mod event_mapper;
mod expr;
mod line_framer;
mod path;

pub use event_mapper::EventMapper;
pub use line_framer::LineFramer;
