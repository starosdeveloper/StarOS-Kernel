// SPDX-License-Identifier: MIT
//! DRM (Direct Rendering Manager) Subsystem
//!
//! Ported from Linux: `drivers/gpu/drm/`
//!
//! Implements:
//! - DRM core + connector/CRTC/encoder pipeline
//! - KMS atomic modesetting
//! - Framebuffer object management
//! - MIPI DSI panel driver interface
//! - MSM MDP5 (Snapdragon display controller) driver

pub mod core;
pub mod mipi_dsi;
pub mod msm_mdp;

pub use core::{
    DrmDev, DrmDevTable, DrmOps, DrmState, DRM_DEVS,
    DrmMode, DrmConnector, DrmCrtc, DrmEncoder,
    DrmFb, drm_register, drm_set_mode, drm_flip,
};
pub use mipi_dsi::{MipiDsiDev, MipiDsiOps, MIPI_DEVS, MipiDsiMsg, mipi_dsi_probe};
pub use msm_mdp::{MsmMdpHw, MsmMdpTable, MSM_MDP_OPS, msm_mdp_probe};
