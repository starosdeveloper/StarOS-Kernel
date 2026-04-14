// SPDX-License-Identifier: MIT
//! WWAN (Wireless Wide Area Network) Subsystem
//!
//! Ported from Linux: `drivers/net/wwan/`
//!
//! Implements:
//! - QMI (Qualcomm MSM Interface) protocol engine
//! - WWAN device registry
//! - AT command transport
//! - Data connection management
//!
//! Architecture:
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │  User / Modem Manager                               │
//! ├──────────────┬──────────────┬────────────────────────┤
//! │  QMI Service │  AT Commands │  Data Bearers (WWAN)   │
//! ├──────────────┴──────────────┴────────────────────────┤
//! │               WWAN Device Registry                   │
//! ├──────────────────────────────────────────────────────┤
//! │  qmi_wwan (MDM9x07)  │  hsi_wwan  │  mhi_wwan        │
//! └──────────────────────────────────────────────────────┘
//! ```

pub mod qmi;
pub mod at;
pub mod wwan_dev;
pub mod qmi_wwan;

pub use qmi::{
    QmiMsg, QmiClient, QmiService, QmiResult, QmiError,
    QMI_CTL, QMI_WDS, QMI_DMS, QMI_NAS, QMI_WMS,
    qmi_send, qmi_recv, qmi_encode_tlv, qmi_decode_tlv,
};
pub use at::{AtCmd, AtResponse, AtSession, AT_SESSIONS, at_send_cmd};
pub use wwan_dev::{WwanDev, WwanDevTable, WwanOps, WwanState, WWAN_DEVS, wwan_register};
pub use qmi_wwan::{QmiWwanHw, QmiWwanTable, QMI_WWAN_OPS, qmi_wwan_probe};
