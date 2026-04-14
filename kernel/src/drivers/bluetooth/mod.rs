// SPDX-License-Identifier: MIT
//! Bluetooth hardware drivers
//!
//! Phase 14 — Week 8
//!
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │  net::bluetooth::hci  (protocol stack)               │
//! ├────────────────┬──────────────┬───────────────────────┤
//! │  btqca         │  btusb       │  hci_uart             │
//! │  (Qualcomm)    │  (USB dongle)│  (UART H4/H5)        │
//! └────────────────┴──────────────┴───────────────────────┘
//! ```

pub mod btqca;
pub mod btusb;
pub mod hci_uart;

pub use btqca::{btqca_probe, btqca_irq_handler, BTQCA_TRANSPORT};
pub use btusb::{btusb_probe, btusb_irq_handler, BTUSB_TRANSPORT};
pub use hci_uart::{hci_uart_probe, hci_uart_irq_handler, HCI_UART_TRANSPORT};
