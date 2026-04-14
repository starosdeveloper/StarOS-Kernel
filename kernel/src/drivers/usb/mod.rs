// SPDX-License-Identifier: MIT OR Apache-2.0
//! USB Subsystem - Full Production Implementation

pub mod core;
pub mod host;

pub use core::{
    UsbSpeed, UsbState, UsbDirection, UsbEndpointType,
    UsbEndpoint, UsbDeviceDescriptor, UsbDevice,
    UrbStatus, Urb, UsbHub, UsbHubPort, UsbHubDescriptor,
    UsbHcdOps, UsbHcd,
    usb_submit_urb, usb_kill_urb, usb_alloc_urb, usb_free_urb,
    hub_port_reset, usb_enumerate_device, usb_new_device,
    usb_add_hcd, usb_remove_hcd,
};

pub use host::{
    Trb, TrbType, XhciSegment, XhciRing, XhciRingType, XhciHcd,
};
