//! Prelude for conditional std/no_std support

#[cfg(any(test, feature = "std"))]
pub use std::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
    format,
    collections::BTreeMap,
    sync::Arc,
};

#[cfg(not(any(test, feature = "std")))]
pub use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
    format,
    collections::BTreeMap,
    sync::Arc,
};
