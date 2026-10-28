//! Maximus core: notebook model, backend abstraction and the evaluation session.
//! No platform-specific dependencies, so it compiles to WASM.

pub mod backend;
pub mod notebook;
pub mod plot;
pub mod plugin;
pub mod session;

pub use backend::*;
pub use notebook::*;
pub use plot::*;
pub use session::*;
