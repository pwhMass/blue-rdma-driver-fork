use std::{
    io,
    net::{IpAddr, Ipv4Addr},
    sync::atomic::{fence, Ordering},
    time::Duration,
};
use log::debug;

use ipnetwork::IpNetwork;
use parking_lot::Mutex;

use crate::{
    constants::CARD_MAC_ADDRESS,
    csr::{cmd_req_ring, cmd_resp_ring, CmdReqRing, CmdRespRing, DeviceAdaptor, ReaderOps, WriterOps},
    descriptors::{
        cmd::{CmdQueueReqDescUpdateMrTable, CmdQueueReqDescUpdatePGT},
        CmdQueueReqDescQpManagement, CmdQueueReqDescSetNetworkParam,
        CmdQueueReqDescSetRawPacketReceiveMeta,
    },
    mem::{page::ContiguousPages, DmaBuf, PageWithPhysAddr},
    net::config::NetworkConfig,
    ringbuf::DescRingBuffer,
};

use super::{
    types::{CmdQueue, CmdQueueDesc, CmdRespQueue},
    MttUpdate, PgtUpdate, RecvBufferMeta, UpdateQp,
};

/// Controller of the command queue
pub(crate) struct CommandConfigurator<Dev: DeviceAdaptor> {
    /// Command queue pair
    cmd_qp: Mutex<CmdQp>,
    /// Ring for accessing command queue CSRs
    req_csr_ring: CmdReqRing<Dev>,
    /// Ring for accessing command response queue CSRs
    resp_csr_ring: CmdRespRing<Dev>,
}

impl<Dev: DeviceAdaptor> CommandConfigurator<Dev> {

    /// Creates a new command controller instance
    ///
    /// # Returns
    /// A new `CommandConfigurator` with an initialized command queue
    pub(crate) fn init(dev: &Dev, req_buf: DmaBuf, resp_buf: DmaBuf) -> io::Result<Self> {
        let req_queue = CmdQueue::new(DescRingBuffer::new(req_buf.buf));
        let resp_queue = CmdRespQueue::new(DescRingBuffer::new(resp_buf.buf));
        let req_csr_ring = cmd_req_ring(dev.clone());
        let resp_csr_ring = cmd_resp_ring(dev.clone());
        debug!("cmd req queue pa = 0x{:x}", req_buf.phys_addr);
        req_csr_ring.write_base_addr(req_buf.phys_addr)?;
        debug!("cmd resp queue pa = 0x{:x}", resp_buf.phys_addr);
        resp_csr_ring.write_base_addr(resp_buf.phys_addr)?;

        Ok(Self {
            cmd_qp: Mutex::new(CmdQp::new(req_queue, resp_queue)),
            req_csr_ring,
            resp_csr_ring,
        })
    }

    /// Flush cmd request queue pointer to device
    pub(crate) fn flush_req_queue(&self, req_queue: &CmdQueue) -> io::Result<()> {
        self.req_csr_ring.write_head(req_queue.head())
    }

    /// Flush cmd response queue pointer to device
    pub(crate) fn flush_resp_queue(&self, resp_queue: &CmdRespQueue) -> io::Result<()> {
        self.resp_csr_ring.write_tail(resp_queue.tail())
    }
}

impl<Dev: DeviceAdaptor> CommandConfigurator<Dev> {
    pub(crate) fn update_mtt(&self, update: MttUpdate) {
        let update_mr_table = CmdQueueReqDescUpdateMrTable::new(
            0,
            update.mr_base_va,
            update.mr_length,
            update.mr_key,
            update.pd_handler,
            update.acc_flags,
            update.base_pgt_offset,
        );
        let mut qp = self.cmd_qp.lock();
        let mut qp_update = qp.update();
        qp_update.push(CmdQueueDesc::UpdateMrTable(update_mr_table));
        qp_update.flush(&self.req_csr_ring);
        qp_update.wait(&self.resp_csr_ring);
    }

    pub(crate) fn update_pgt(&self, update: PgtUpdate) {
        let desc = CmdQueueReqDescUpdatePGT::new(
            0,
            update.dma_addr,
            update.pgt_offset,
            update.zero_based_entry_count,
        );
        let mut qp = self.cmd_qp.lock();
        let mut qp_update = qp.update();
        qp_update.push(CmdQueueDesc::UpdatePGT(desc));
        qp_update.flush(&self.req_csr_ring);
        qp_update.wait(&self.resp_csr_ring);
    }

    pub(crate) fn update_qp(&self, entry: UpdateQp) {
        let desc = CmdQueueReqDescQpManagement::new(
            0,
            entry.ip_addr,
            entry.qpn,
            false,
            true,
            entry.peer_qpn,
            entry.rq_access_flags,
            entry.qp_type,
            entry.pmtu,
            entry.local_udp_port,
            entry.peer_mac_addr,
        );

        let mut qp = self.cmd_qp.lock();
        let mut update = qp.update();
        update.push(CmdQueueDesc::ManageQP(desc));
        update.flush(&self.req_csr_ring);
        update.wait(&self.resp_csr_ring);
    }

    pub(crate) fn set_network(&self, param: NetworkConfig) {
        let desc = CmdQueueReqDescSetNetworkParam::new(
            0,
            param.gateway.map_or(0, Ipv4Addr::to_bits),
            param.ip.mask().to_bits(),
            param.ip.ip().to_bits(),
            CARD_MAC_ADDRESS,
        );
        let mut qp = self.cmd_qp.lock();
        let mut update = qp.update();
        update.push(CmdQueueDesc::SetNetworkParam(desc));
        update.flush(&self.req_csr_ring);
        update.wait(&self.resp_csr_ring);
    }

    pub(crate) fn set_raw_packet_recv_buffer(&self, meta: RecvBufferMeta) {
        let desc = CmdQueueReqDescSetRawPacketReceiveMeta::new(0, meta.phys_addr);
        let mut qp = self.cmd_qp.lock();
        let mut update = qp.update();
        update.push(CmdQueueDesc::SetRawPacketReceiveMeta(desc));
        update.flush(&self.req_csr_ring);
        update.wait(&self.resp_csr_ring);
    }
}

/// Command queue pair
struct CmdQp {
    /// The command request queue
    req_queue: CmdQueue,
    /// The command response queue
    resp_queue: CmdRespQueue,
}

impl CmdQp {
    /// Creates a new command queue pair
    fn new(req_queue: CmdQueue, resp_queue: CmdRespQueue) -> Self {
        Self {
            req_queue,
            resp_queue,
        }
    }

    /// Creates a queue pair update handle to process commands
    fn update(&mut self) -> QpUpdate<'_> {
        QpUpdate {
            num: 0,
            req_queue: &mut self.req_queue,
            resp_queue: &mut self.resp_queue,
        }
    }
}

/// An updates handle
struct QpUpdate<'a> {
    /// Number of updates
    num: usize,
    /// The command request queue
    req_queue: &'a mut CmdQueue,
    /// The command response queue
    resp_queue: &'a mut CmdRespQueue,
}

impl QpUpdate<'_> {
    /// Pushes a new command queue descriptor to the queue.
    fn push(&mut self, desc: CmdQueueDesc) {
        self.num = self.num.wrapping_add(1);
        //FIXME: handle failed condition
        let _ignore = self.req_queue.push(desc);
    }

    /// Flushes the command queue by writing the head pointer to the CSR ring.
    fn flush<Dev: DeviceAdaptor>(&mut self, req_csr_ring: &CmdReqRing<Dev>) {
        let _ = req_csr_ring.write_head(self.req_queue.head());
        if let Ok(tail_ptr) = req_csr_ring.read_tail() {
            self.req_queue.set_tail(tail_ptr);
        }
    }

    /// Waits for responses to all pushed commands.
    fn wait<Dev: DeviceAdaptor>(mut self, resp_csr_ring: &CmdRespRing<Dev>) {
        while self.num != 0 {
            if let Some(_resp) = self.resp_queue.try_pop() {
                self.num = self.num.wrapping_sub(1);
                let _ = resp_csr_ring.write_tail(self.resp_queue.tail());
                if let Ok(head_ptr) = resp_csr_ring.read_head() {
                    self.resp_queue.set_head(head_ptr);
                }
            }
        }
    }
}
