// SPDX-License-Identifier: MIT
//! GPU / Display subsystem
pub mod drm;
pub use drm::{DrmDev, DrmDevTable, DrmOps, DrmState, DRM_DEVS};
