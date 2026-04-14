pub mod parser;
pub mod properties;
pub mod discovery;

pub use parser::{FdtParser, FdtHeader};
pub use properties::*;
pub use discovery::DeviceDiscovery;
