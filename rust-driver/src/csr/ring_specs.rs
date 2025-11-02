//! Ring Buffer Specifications and Constructors
//!
//! This module defines concrete ring buffer specifications for all ring types in the system
//! and provides type-safe constructors for creating ring instances.
//!
//! # Ring Types and Directions
//!
//! Each ring specification implements either `RingSpecToCard` or `RingSpecToHost` to indicate
//! the data flow direction:
//!
//! ## ToCard Rings (Host → Card)
//! Host is the producer, card is the consumer:
//! - `CmdReqSpec` - Management command requests
//! - `SimpleNicTxSpec` - Raw packet transmission
//! - `SendRingSpec` - RDMA send work requests
//!
//! ## ToHost Rings (Card → Host)
//! Card is the producer, host is the consumer:
//! - `CmdRespSpec` - Management command responses
//! - `SimpleNicRxSpec` - Raw packet reception
//! - `MetaReportRingSpec` - Packet metadata and completion reports
//!
//! # Type Aliases
//!
//! Convenient type aliases combine device adaptors with ring specifications:
//! ```rust,ignore
//! type SendRing<Dev> = Ring<Dev, SendRingSpec>;
//! type MetaReportRing<Dev> = Ring<Dev, MetaReportRingSpec>;
//! ```
//!
//! # Constructor Functions
//!
//! ## Singleton Rings
//! Functions for creating single-instance rings:
//! - `cmd_req_ring()` - Command request ring
//! - `cmd_resp_ring()` - Command response ring
//! - `simple_nic_tx_ring()` - Simple NIC TX ring
//! - `simple_nic_rx_ring()` - Simple NIC RX ring
//!
//! ## Multi-Channel Rings
//! Builder functions that create rings for all channels based on device mode:
//! - `build_send_rings()` - Creates send rings (1 for 100G, 2 for 200G, 4 for 400G)
//! - `build_meta_report_rings()` - Creates metadata report rings (same count as send rings)
//!
//! # Example
//!
//! ```rust,ignore
//! use crate::csr::{EmulatedDevice, Mode, build_send_rings, cmd_req_ring};
//!
//! let dev = EmulatedDevice::new("uverbs0")?;
//!
//! // Create singleton ring
//! let cmd_ring = cmd_req_ring(dev.clone());
//!
//! // Create multi-channel rings based on mode
//! let send_rings = build_send_rings(dev.clone(), Mode::Mode400G);
//! assert_eq!(send_rings.len(), 4);  // 4 channels in 400G mode
//! ```

use crate::csr::{
    device_adaptor::{DeviceAdaptor, Ring, RingSpec, RingSpecToCard, RingSpecToHost},
    constants::{
        CMD_REQ_RING_BASE, CMD_RESP_RING_BASE, NUM_QPS, QP_RECV_RING_BASES, QP_SEND_RING_BASES,
        SIMPLE_NIC_RX_RING_BASE, SIMPLE_NIC_TX_RING_BASE,
    },
    mode::Mode,
};

pub(crate) struct CmdReqSpec;
impl RingSpec for CmdReqSpec {
    fn csr_base(&self) -> usize {
        CMD_REQ_RING_BASE
    }
}
impl RingSpecToCard for CmdReqSpec {}

pub(crate) struct CmdRespSpec;
impl RingSpec for CmdRespSpec {
    fn csr_base(&self) -> usize {
        CMD_RESP_RING_BASE
    }
}
impl RingSpecToHost for CmdRespSpec {}

pub(crate) struct SimpleNicTxSpec;
impl RingSpec for SimpleNicTxSpec {
    fn csr_base(&self) -> usize {
        SIMPLE_NIC_TX_RING_BASE
    }
}
impl RingSpecToCard for SimpleNicTxSpec {}

pub(crate) struct SimpleNicRxSpec;
impl RingSpec for SimpleNicRxSpec {
    fn csr_base(&self) -> usize {
        SIMPLE_NIC_RX_RING_BASE
    }
}
impl RingSpecToHost for SimpleNicRxSpec {}

pub(crate) struct SendRingSpec(pub(crate) usize);
impl RingSpec for SendRingSpec {
    fn csr_base(&self) -> usize {
        QP_SEND_RING_BASES[self.0]
    }
}
impl RingSpecToCard for SendRingSpec {}

pub(crate) struct MetaReportRingSpec(pub(crate) usize);
impl RingSpec for MetaReportRingSpec {
    fn csr_base(&self) -> usize {
        QP_RECV_RING_BASES[self.0]
    }
}
impl RingSpecToHost for MetaReportRingSpec {}

// ============================================================================
// Public type aliases
// ============================================================================

pub(crate) type CmdReqRing<Dev> = Ring<Dev, CmdReqSpec>;
pub(crate) type CmdRespRing<Dev> = Ring<Dev, CmdRespSpec>;
pub(crate) type SimpleNicTxRing<Dev> = Ring<Dev, SimpleNicTxSpec>;
pub(crate) type SimpleNicRxRing<Dev> = Ring<Dev, SimpleNicRxSpec>;
pub(crate) type SendRing<Dev> = Ring<Dev, SendRingSpec>;
pub(crate) type MetaReportRing<Dev> = Ring<Dev, MetaReportRingSpec>;

// ============================================================================
// Singleton ring constructors
// ============================================================================

/// Create a command request ring
#[inline]
pub(crate) fn cmd_req_ring<Dev: DeviceAdaptor>(dev: Dev) -> CmdReqRing<Dev> {
    Ring::new(dev, CmdReqSpec)
}

/// Create a command response ring
#[inline]
pub(crate) fn cmd_resp_ring<Dev: DeviceAdaptor>(dev: Dev) -> CmdRespRing<Dev> {
    Ring::new(dev, CmdRespSpec)
}

/// Create a Simple NIC TX ring
#[inline]
pub(crate) fn simple_nic_tx_ring<Dev: DeviceAdaptor>(dev: Dev) -> SimpleNicTxRing<Dev> {
    Ring::new(dev, SimpleNicTxSpec)
}

/// Create a Simple NIC RX ring
#[inline]
pub(crate) fn simple_nic_rx_ring<Dev: DeviceAdaptor>(dev: Dev) -> SimpleNicRxRing<Dev> {
    Ring::new(dev, SimpleNicRxSpec)
}

// ============================================================================
// Multi-channel ring builders
// ============================================================================

/// Build send rings for all channels based on device mode
pub(crate) fn build_send_rings<Dev: DeviceAdaptor + Clone>(
    dev: Dev,
    mode: Mode,
) -> Vec<SendRing<Dev>> {
    mode.channel_ids()
        .iter()
        .map(|&id| Ring::new(dev.clone(), SendRingSpec(id)))
        .collect()
}

/// Build meta report rings for all channels based on device mode
pub(crate) fn build_meta_report_rings<Dev: DeviceAdaptor + Clone>(
    dev: Dev,
    mode: Mode,
) -> Vec<MetaReportRing<Dev>> {
    mode.channel_ids()
        .iter()
        .map(|&id| Ring::new(dev.clone(), MetaReportRingSpec(id)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_rings_100g() {
        use crate::csr::emulated::EmulatedDevice;
        let dev = EmulatedDevice::new("test").unwrap();

        let send_rings = build_send_rings(dev.clone(), Mode::Mode100G);
        assert_eq!(send_rings.len(), 1);

        let meta_rings = build_meta_report_rings(dev, Mode::Mode100G);
        assert_eq!(meta_rings.len(), 1);
    }

    #[test]
    fn test_build_rings_400g() {
        use crate::csr::emulated::EmulatedDevice;
        let dev = EmulatedDevice::new("test").unwrap();

        let send_rings = build_send_rings(dev.clone(), Mode::Mode400G);
        assert_eq!(send_rings.len(), 4);

        let meta_rings = build_meta_report_rings(dev, Mode::Mode400G);
        assert_eq!(meta_rings.len(), 4);
    }
}
