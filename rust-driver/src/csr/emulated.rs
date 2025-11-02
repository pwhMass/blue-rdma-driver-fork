//! Emulated CSR Access Implementation
//!
//! This module provides CSR access for hardware simulation and RTL testing through
//! UDP-based RPC communication with a simulator backend.
//!
//! # Architecture
//!
//! ```text
//! Rust Driver (EmulatedDevice)
//!         ↓ UDP RPC
//!    127.0.0.1:7701 (uverbs0)
//!    127.0.0.1:7702 (uverbs1)
//!         ↓
//! RTL Simulator (Cocotb/Verilator)
//!         ↓
//! Hardware Model (Verilog/VHDL)
//! ```
//!
//! # Use Cases
//!
//! - RTL verification before hardware tapeout
//! - Driver development without physical hardware
//! - Regression testing in CI/CD environments
//! - Protocol debugging with full hardware state visibility
//!
//! # Communication Protocol
//!
//! The RPC protocol uses JSON-serialized messages over UDP:
//!
//! **Read Request**:
//! ```json
//! {"is_write": false, "addr": 0x1000, "value": 0}
//! ```
//!
//! **Write Request**:
//! ```json
//! {"is_write": true, "addr": 0x2000, "value": 0x42}
//! ```
//!
//! **Response** (for reads):
//! ```json
//! {"is_write": false, "addr": 0x1000, "value": 0x12345678}
//! ```
//!
//! # Port Mapping
//!
//! Device names map to UDP ports:
//! - `uverbs0` → 127.0.0.1:7701
//! - `uverbs1` → 127.0.0.1:7702
//! - `uverbs2` → 127.0.0.1:7703
//! - etc.
//!
//! # Example
//!
//! ```rust,ignore
//! use crate::csr::emulated::EmulatedDevice;
//!
//! // Connect to simulator
//! let dev = EmulatedDevice::new("uverbs0")?;  // Connects to 127.0.0.1:7701
//!
//! // CSR operations are forwarded to simulator
//! dev.write_csr(0x1000, 0x42)?;  // → UDP packet to simulator
//! let value = dev.read_csr(0x1000)?;  // ← Response from simulator
//! ```

use std::{
    io,
    net::{SocketAddr, UdpSocket},
    sync::Arc,
};

use log::debug;
use serde::{Deserialize, Serialize};

use super::DeviceAdaptor;

#[derive(Debug, Clone)]
pub(super) struct RpcClient(Arc<UdpSocket>);

#[derive(Debug, Serialize, Deserialize)]
struct CsrAccessRpcMessage {
    is_write: bool,
    addr: usize,
    value: u32,
}

impl RpcClient {
    pub(super) fn new(server_addr: SocketAddr) -> io::Result<Self> {
        debug!("connect to: {server_addr}");
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.connect(server_addr)?;
        Ok(Self(socket.into()))
    }

    pub(super) fn read_csr(&self, addr: usize) -> io::Result<u32> {
        let msg = CsrAccessRpcMessage {
            is_write: false,
            addr,
            value: 0,
        };

        debug!("send msg: {msg:?}");
        let send_buf = serde_json::to_vec(&msg)?;
        let _: usize = self.0.send(&send_buf)?;

        let mut recv_buf = [0; 128];
        let (recv_cnt, _addr) = self.0.recv_from(&mut recv_buf)?;
        // the length of CsrAccessRpcMessage is fixed,
        #[allow(clippy::indexing_slicing)]
        let response = serde_json::from_slice::<CsrAccessRpcMessage>(&recv_buf[..recv_cnt])?;

        Ok(response.value)
    }

    pub(super) fn write_csr(&self, addr: usize, data: u32) -> io::Result<()> {
        let msg = CsrAccessRpcMessage {
            is_write: true,
            addr,
            value: data,
        };
        debug!("send msg write: {msg:?}");

        let send_buf = serde_json::to_vec(&msg)?;
        let _: usize = self.0.send(&send_buf)?;
        Ok(())
    }
}

#[non_exhaustive]
#[derive(Clone, Debug)]
pub(crate) struct EmulatedDevice(RpcClient);

impl EmulatedDevice {
    /// Create a new emulated device with a default address based on device name
    pub(crate) fn new(device_name: &str) -> io::Result<Self> {
        // Parse device name like "uverbs0" or "test" to determine port
        // For testing, use a default port
        let port = if device_name.starts_with("uverbs") {
            let idx: usize = device_name
                .trim_start_matches("uverbs")
                .parse()
                .unwrap_or(0);
            7701 + idx
        } else {
            7701 // default port for testing
        };

        let addr = format!("127.0.0.1:{port}").parse().map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("invalid address: {e}"))
        })?;

        Ok(EmulatedDevice(RpcClient::new(addr)?))
    }

    #[allow(clippy::expect_used)]
    pub(crate) fn new_with_addr(addr: &str) -> Self {
        EmulatedDevice(
            RpcClient::new(addr.parse().expect("invalid socket addr"))
                .expect("failed to connect to emulator"),
        )
    }
}

impl DeviceAdaptor for EmulatedDevice {
    fn read_csr(&self, addr: usize) -> io::Result<u32> {
        self.0.read_csr(addr)
    }

    fn write_csr(&self, addr: usize, data: u32) -> io::Result<()> {
        self.0.write_csr(addr, data)
    }
}
