// SPDX-License-Identifier: MIT
//! Bluetooth protocol stack
//!
//! Ported from Linux: `net/bluetooth/` (~60,000 lines C → ~2500 lines Rust)
//!
//! Phase 14 — Week 7
//!
//! Layer diagram:
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │  SDP (PSM 1)  │  RFCOMM (PSM 3)  │  …       │
//! ├──────────────────────────────────────────────┤
//! │  L2CAP (channel multiplexer)                 │
//! ├──────────────────────────────────────────────┤
//! │  HCI (Host Controller Interface)             │
//! ├──────────────────────────────────────────────┤
//! │  Transport: btqca / btusb / hci_uart         │
//! └──────────────────────────────────────────────┘
//! ```

pub mod hci;
pub mod l2cap;
pub mod rfcomm;
pub mod sdp;

pub use hci::{
    HciDev, HciTransport, HciConn, HciCmdHdr, HciEventHdr, HciAclHdr,
    ConnType, ConnState, InquiryResult,
    hci_register_dev, hci_unregister_dev, with_hci_dev,
    opcode, event, le_event, hci_flags,
    HCI_COMMAND_PKT, HCI_ACLDATA_PKT, HCI_EVENT_PKT,
    MAX_HCI_DEVS,
};

pub use l2cap::{
    L2capChan, ChanState, ChanMode,
    l2cap_recv_frame, l2cap_send, l2cap_register_psm,
    l2cap_chan_create, l2cap_chan_close,
    L2CAP_CID_SIGNALING, L2CAP_CID_ATT, L2CAP_CID_SMP,
    sig_cmd, l2cap_result,
};

pub use rfcomm::{
    RfcommDlc, DlcState,
    rfcomm_init, rfcomm_recv_frame, rfcomm_send_data,
    rfcomm_session_open, rfcomm_dlc_open,
    RFCOMM_PSM, frame_type, mcc_type,
};

pub use sdp::{
    ServiceRecord, SdpAttr,
    sdp_init, sdp_register_service, sdp_unregister_service,
    sdp_build_spp_record, sdp_recv_pdu,
    SDP_PSM, uuid16, attr_id, pdu_id,
};
