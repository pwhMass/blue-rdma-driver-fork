use std::{
    io, iter,
    net::Ipv4Addr,
    sync::{atomic::AtomicBool, Arc},
    thread::current,
    time::Duration,
};

use crossbeam_deque::Worker;
use parking_lot::Mutex;
use log::{debug, info};

use crate::{
    cmd::{CommandConfigurator, MttUpdate, PgtUpdate, RecvBufferMeta, UpdateQp},
    config::DeviceConfig,
    constants::CARD_MAC_ADDRESS,
    csr::{mode::Mode, DeviceAdaptor},
    mem::{
        get_num_page, page::PageAllocator, pin_pages, virt_to_phy::AddressResolver, DmaBuf,
        DmaBufAllocator, MemoryPinner, PageWithPhysAddr, UmemHandler, PAGE_SIZE,
    },
    net::{config::NetworkConfig, reader::NetConfigReader, recv_chan::{
        post_recv_channel, PostRecvTx, PostRecvTxTable, RecvWorker, RecvWrQueueTable, TcpChannel,
    }, simple_nic::SimpleNicController},
    rdma_utils::{
        mtt::{Mtt, PgtEntry},
        pd::PdTable,
        qp::{QpManager, QpTableShared},
        types::{
            ibv_qp_attr::{IbvQpAttr, IbvQpInitAttr},
            QpAttr, RecvWr, SendWr, SendWrBase, SendWrRdma,
        },
    },
    ringbuf::DescRingBufAllocator,
    workers::{
        ack_responder::AckResponder,
        completion::{
            Completion, CompletionQueueTable, CompletionTask, CompletionWorker, CqManager, Event,
            PostRecvEvent,
        },
        meta_report,
        qp_timeout::QpAckTimeoutWorker,
        rdma::{RdmaWriteTask, RdmaWriteWorker},
        retransmit::PacketRetransmitWorker,
        send::{self, SendHandle},
        spawner::{task_channel, AbortSignal, SingleThreadTaskWorker, TaskTx},
    },
    RdmaError,
};

use crate::error::Result;

use super::dev::HwDevice;

pub(crate) trait VerbsOps {
    fn reg_mr(&mut self, addr: u64, length: usize, pd_handle: u32, access: u8) -> Result<u32>;
    fn dereg_mr(&mut self, mr_key: u32) -> Result<()>;
    fn create_qp(&mut self, attr: IbvQpInitAttr) -> Result<u32>;
    fn update_qp(&mut self, qpn: u32, attr: IbvQpAttr) -> Result<()>;
    fn destroy_qp(&mut self, qpn: u32) -> Result<()>;
    fn create_cq(&mut self) -> Result<u32>;
    fn destroy_cq(&mut self, handle: u32) -> Result<()>;
    fn poll_cq(&mut self, handle: u32, max_num_entries: usize) -> Vec<Completion>;
    fn post_send(&mut self, qpn: u32, wr: SendWr) -> Result<()>;
    fn post_recv(&mut self, qpn: u32, wr: RecvWr) -> Result<()>;
    fn alloc_pd(&mut self) -> Result<u32>;
    fn dealloc_pd(&mut self, handle: u32) -> Result<()>;
}

pub(crate) struct HwDeviceCtx<H: HwDevice> {
    device: H,
    mtt: Mtt,
    mtt_buffer: DmaBuf,
    qp_manager: QpManager,
    qp_attr_table: QpTableShared<QpAttr>,
    cq_manager: CqManager,
    cq_table: CompletionQueueTable,
    cmd_controller: CommandConfigurator<H::Adaptor>,
    post_recv_tx_table: PostRecvTxTable,
    recv_wr_queue_table: RecvWrQueueTable,
    rdma_write_tx: TaskTx<RdmaWriteTask>,
    completion_tx: TaskTx<CompletionTask>,
    config: DeviceConfig,
    allocator: H::DmaBufAllocator,
    pd_table: PdTable,
}

#[allow(private_bounds)]
impl<H> HwDeviceCtx<H>
where
    H: HwDevice,
    H::Adaptor: DeviceAdaptor + Send + 'static,
    H::DmaBufAllocator: DmaBufAllocator,
    H::UmemHandler: UmemHandler,
{
    pub(crate) fn initialize(device: H, config: DeviceConfig) -> Result<Self> {
        debug!("begin initializ...");
        let mode = Mode::default();
        let net_config = NetConfigReader::read();
        debug!("begin device adaptor initializ...");
        let adaptor = device.new_adaptor()?;
        debug!("device adaptor initialized...");
        let mut allocator = device.new_dma_buf_allocator()?;
        let mut rb_allocator = DescRingBufAllocator::new(&mut allocator);
        let cmd_controller =
            CommandConfigurator::init(&adaptor, rb_allocator.alloc()?, rb_allocator.alloc()?)?;
        debug!("command queue request controller initialized...");
        let send_bufs = iter::repeat_with(|| rb_allocator.alloc())
            .take(mode.num_channel())
            .collect::<std::result::Result<_, _>>()?;
        let meta_bufs = iter::repeat_with(|| rb_allocator.alloc())
            .take(mode.num_channel())
            .collect::<std::result::Result<_, _>>()?;

        let (rdma_write_tx, rdma_write_rx) = task_channel();
        let (completion_tx, completion_rx) = task_channel();
        let (ack_timeout_tx, ack_timeout_rx) = task_channel();
        let (packet_retransmit_tx, packet_retransmit_rx) = task_channel();
        let (ack_tx, ack_rx) = task_channel();

        let abort = AbortSignal::new();
        let rx_buffer = rb_allocator.alloc()?;
        let rx_buffer_pa = rx_buffer.phys_addr;
        let qp_attr_table =
            QpTableShared::new_with(|| QpAttr::new_with_ip(net_config.ip.ip().to_bits()));
        
        debug!("qp table initialized...");
        let qp_manager = QpManager::new();
        let cq_manager = CqManager::new();
        let cq_table = CompletionQueueTable::new();
        let simple_nic_controller = SimpleNicController::init(
            &adaptor,
            rb_allocator.alloc()?,
            rb_allocator.alloc()?,
            rb_allocator.alloc()?,
            rx_buffer,
        )?;
        debug!("simple_nic_controller initialized...");
        let (simple_nic_tx, simple_nic_rx) = simple_nic_controller.into_split();
        let handle = send::spawn(&adaptor, send_bufs, mode, &abort)?;
        AckResponder::new(qp_attr_table.clone(), Box::new(simple_nic_tx)).spawn(
            ack_rx,
            "AckResponder",
            abort.clone(),
        );
        PacketRetransmitWorker::new(handle.clone()).spawn(
            packet_retransmit_rx,
            "PacketRetransmitWorker",
            abort.clone(),
        );
        QpAckTimeoutWorker::new(packet_retransmit_tx.clone(), config.ack()).spawn_polling(
            ack_timeout_rx,
            "QpAckTimeoutWorker",
            abort.clone(),
            Duration::from_nanos(4096u64 << config.ack().check_duration_exp),
        );
        
        RdmaWriteWorker::new(
            qp_attr_table.clone(),
            handle,
            ack_timeout_tx.clone(),
            packet_retransmit_tx.clone(),
            completion_tx.clone(),
        )
        .spawn(rdma_write_rx, "RdmaWriteWorker", abort.clone());

        CompletionWorker::new(
            cq_table.clone_arc(),
            qp_attr_table.clone(),
            ack_tx.clone(),
            ack_timeout_tx.clone(),
            rdma_write_tx.clone(),
        )
        .spawn(completion_rx, "CompletionWorker", abort.clone());

        meta_report::spawn(
            &adaptor,
            meta_bufs,
            mode,
            ack_tx.clone(),
            ack_timeout_tx.clone(),
            packet_retransmit_tx.clone(),
            completion_tx.clone(),
            rdma_write_tx.clone(),
            abort.clone(),
        )?;
        debug!("meta_report worker spawn called...");

        cmd_controller.set_network(net_config);
        debug!("set network param finished...");
        cmd_controller.set_raw_packet_recv_buffer(RecvBufferMeta::new(rx_buffer_pa));
        debug!("set_raw_packet_recv_buffer finished...");

        #[allow(clippy::mem_forget)]
        std::mem::forget(simple_nic_rx); // prevent libc::munmap being called

        Ok(Self {
            device,
            cmd_controller,
            qp_manager,
            qp_attr_table,
            cq_manager,
            cq_table,
            mtt_buffer: rb_allocator.alloc()?,
            mtt: Mtt::new(),
            post_recv_tx_table: PostRecvTxTable::new(),
            recv_wr_queue_table: RecvWrQueueTable::new(),
            rdma_write_tx,
            completion_tx,
            config,
            allocator,
            pd_table: PdTable::new(),
        })
    }
}

impl<H: HwDevice> HwDeviceCtx<H> {
    fn send(&self, qpn: u32, mut wr: SendWrBase) -> Result<()> {
        match self.recv_wr_queue_table.pop(qpn) {
            Some(x) => {
                if wr.length != x.length {
                    return Err(RdmaError::InvalidInput(
                        "Send length does not match receive length".into(),
                    ));
                }
                let wr = SendWrRdma::new_from_base(wr, x.addr, x.lkey);
                self.rdma_write(qpn, wr);

                Ok(())
            }
            None => todo!("return rnr error"),
        }
    }

    fn rdma_read(&self, qpn: u32, wr: SendWrRdma) {
        let task = RdmaWriteTask::new_write(qpn, wr);
        self.rdma_write_tx.send(task);
    }

    fn rdma_write(&self, qpn: u32, wr: SendWrRdma) {
        let task = RdmaWriteTask::new_write(qpn, wr);
        self.rdma_write_tx.send(task);
    }
}

impl<H> VerbsOps for HwDeviceCtx<H>
where
    H: HwDevice,
    H::Adaptor: DeviceAdaptor + Send + 'static,
    H::UmemHandler: UmemHandler,
{
    fn reg_mr(&mut self, addr: u64, length: usize, pd_handle: u32, access: u8) -> Result<u32> {
        fn chunks(entry: PgtEntry) -> Vec<PgtEntry> {
            /// Maximum number of Page Table entries (PGT entries) that can be allocated in a single `PCIe` transaction.
            /// A `PCIe` transaction size is 128 bytes, and each PGT entry is a u64 (8 bytes).
            /// Therefore, 128 bytes / 8 bytes per entry = 16 entries per allocation.
            const MAX_NUM_PGT_ENTRY_PER_ALLOC: usize = 16;

            let base_index = entry.index;
            let end_index = base_index + entry.count;
            (base_index..end_index)
                .step_by(MAX_NUM_PGT_ENTRY_PER_ALLOC)
                .map(|index| PgtEntry {
                    index,
                    count: (MAX_NUM_PGT_ENTRY_PER_ALLOC as u32).min(end_index - index),
                })
                .collect()
        }

        let umem_handler = self.device.new_umem_handler();
        umem_handler.pin_pages(addr, length)?;
        let num_pages = get_num_page(addr, length);
        debug!("generate page table entries: addr=0x{addr:x}, length=0x{length:x} --> num_pages={num_pages}");
        let (mr_key, pgt_entry) = self.mtt.register(num_pages)?;
        let length_u32 = u32::try_from(length)
            .map_err(|_err| RdmaError::InvalidInput("Length too large".into()))?;
        let phys_addrs = umem_handler
            .virt_to_phys_range(addr, num_pages)?
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or(RdmaError::MemoryError("Physical address not found".into()))?;
        let phys_addrs_for_debug = phys_addrs.clone();
            // .into_iter();
        let buf = &mut self.mtt_buffer.buf;
        let base_index = pgt_entry.index;
        let mtt_update = MttUpdate::new(addr, length_u32, mr_key, pd_handle, access, base_index);
        // TODO: makes updates atomic
        self.cmd_controller.update_mtt(mtt_update);
        let mut phys_addrs = phys_addrs.into_iter();
        for PgtEntry { index, count } in chunks(pgt_entry) {
            let bytes: Vec<u8> = phys_addrs
                .by_ref()
                .take(count as usize)
                .flat_map(u64::to_ne_bytes)
                .collect();
            buf.copy_from(0, &bytes);
            let pgt_update = PgtUpdate::new(self.mtt_buffer.phys_addr, index, count - 1);
            debug!("new pgt update request: {pgt_update:?}");
            let mut va_start_for_debug = addr & (!(PAGE_SIZE as u64));
            for phy_addr in &phys_addrs_for_debug {
                debug!("pgt map va -> pa: 0x{va_start_for_debug:x} -> 0x{phy_addr:x}");
                va_start_for_debug += (PAGE_SIZE as u64);
            }
            self.cmd_controller.update_pgt(pgt_update);
        }

        Ok(mr_key)
    }

    fn dereg_mr(&mut self, mr_key: u32) -> Result<()> {
        self.mtt.deregister(mr_key)
    }

    fn create_qp(&mut self, attr: IbvQpInitAttr) -> Result<u32> {
        let qpn = self
            .qp_manager
            .create_qp()
            .ok_or(RdmaError::ResourceExhausted(
                "No QP numbers available".into(),
            ))?;
        let _ignore = self.qp_attr_table.map_qp_mut(qpn, |current| {
            current.qpn = qpn;
            current.qp_type = attr.qp_type();
            current.send_cq = attr.send_cq();
            current.recv_cq = attr.recv_cq();
            current.mac_addr = CARD_MAC_ADDRESS;
            current.pmtu = ibverbs_sys::IBV_MTU_4096 as u8;
        });
        let entry = UpdateQp {
            ip_addr: 0,
            peer_mac_addr: 0,
            local_udp_port: 0x100,
            qp_type: attr.qp_type(),
            qpn,
            ..Default::default()
        };
        self.cmd_controller.update_qp(entry);

        Ok(qpn)
    }

    fn update_qp(&mut self, qpn: u32, attr: IbvQpAttr) -> Result<()> {
        // TODO: This is a workaround for read-to-write conversion. Consider modifying the
        // hardware to allow remote writes for read responses.
        let rq_access_flags = (ibverbs_sys::ibv_access_flags::IBV_ACCESS_LOCAL_WRITE.0
            | ibverbs_sys::ibv_access_flags::IBV_ACCESS_REMOTE_READ.0
            | ibverbs_sys::ibv_access_flags::IBV_ACCESS_REMOTE_WRITE.0)
            as u8;

        debug!("before modify qp_attr_table");
        let entry = self
            .qp_attr_table
            .map_qp_mut(qpn, |current| {
                let current_ip = (current.dqp_ip != 0).then_some(current.dqp_ip);
                let attr_ip = attr.dest_qp_ip().map(Ipv4Addr::to_bits);
                let ip_addr = attr_ip.or(current_ip).unwrap_or(0);
                let entry = UpdateQp {
                    qpn,
                    ip_addr,
                    local_udp_port: 0x100,
                    peer_mac_addr: CARD_MAC_ADDRESS,
                    qp_type: current.qp_type,
                    peer_qpn: attr.dest_qp_num().unwrap_or(current.dqpn),
                    rq_access_flags,
                    pmtu: attr.path_mtu().map_or(current.pmtu, |x| x as u8),
                };
                current.dqpn = entry.peer_qpn;
                current.access_flags = rq_access_flags;
                current.pmtu = entry.pmtu;
                current.dqp_ip = ip_addr;
                entry
            })
            .ok_or(RdmaError::NotFound(format!("QP {qpn} not found",)))?;

        debug!("before send qp update request to hardware");
        self.cmd_controller.update_qp(entry);

        let qp = self
            .qp_attr_table
            .get_qp(qpn)
            .ok_or(RdmaError::NotFound(format!("QP {qpn} not found",)))?;

        if qp.dqpn != 0 && qp.dqp_ip != 0 && self.post_recv_tx_table.get_qp_mut(qpn).is_none() {
            let dqp_ip = Ipv4Addr::from_bits(qp.dqp_ip);
            debug!("update_qp get dqp_ip={dqp_ip:?}");
            //TODO 这里不会有并发问题吗？在 qp 准备好之后，马上 post_recv，会不会出现问题？
            let (tx, rx) =
                post_recv_channel::<TcpChannel>(qp.ip.into(), qp.dqp_ip.into(), qpn, qp.dqpn)?;
            debug!("after create post recv tx and rx table");
            self.post_recv_tx_table.insert(qpn, tx);
            let wr_queue =
                self.recv_wr_queue_table
                    .clone_recv_wr_queue(qpn)
                    .ok_or(RdmaError::NotFound(format!(
                        "Receive WR queue for QP {qpn} not found",
                    )))?;
            
            debug!("before spawn RecvWorker");
            RecvWorker::new(rx, wr_queue).spawn();
        }

        Ok(())
    }

    fn destroy_qp(&mut self, qpn: u32) -> Result<()> {
        if self.qp_manager.destroy_qp(qpn) {
            Ok(())
        } else {
            Err(RdmaError::InvalidInput(format!("QPN {qpn} not present")))
        }
    }

    fn create_cq(&mut self) -> Result<u32> {
        self.cq_manager
            .create_cq()
            .ok_or(RdmaError::ResourceExhausted("No CQ available".into()))
    }

    fn destroy_cq(&mut self, handle: u32) -> Result<()> {
        if self.cq_manager.destroy_cq(handle) {
            Ok(())
        } else {
            Err(RdmaError::InvalidInput(format!(
                "CQ handle {handle} not present"
            )))
        }
    }

    fn post_send(&mut self, qpn: u32, wr: SendWr) -> Result<()> {
        match wr {
            SendWr::Rdma(wr) => {
                self.rdma_write(qpn, wr);
                Ok(())
            }
            SendWr::Send(wr) => self.send(qpn, wr),
        }
    }

    fn poll_cq(&mut self, handle: u32, max_num_entries: usize) -> Vec<Completion> {
        let Some(cq) = self.cq_table.get_cq(handle) else {
            return vec![];
        };
        iter::repeat_with(|| cq.pop_front())
            .take_while(Option::is_some)
            .take(max_num_entries)
            .flatten()
            .collect()
    }

    fn post_recv(&mut self, qpn: u32, wr: RecvWr) -> Result<()> {
        let qp = self
            .qp_attr_table
            .get_qp(qpn)
            .ok_or(RdmaError::QpError(format!("QP {qpn} not found",)))?;
        let event = Event::PostRecv(PostRecvEvent::new(qpn, wr.wr_id));
        self.completion_tx
            .send(CompletionTask::Register { qpn, event });
        let tx = self
            .post_recv_tx_table
            .get_qp_mut(qpn)
            .ok_or(RdmaError::QpError(format!(
                "Post receive channel for QP {qpn} not found",
            )))?;
        tx.send(wr)?;

        Ok(())
    }

    fn alloc_pd(&mut self) -> Result<u32> {
        self.pd_table
            .alloc()
            .ok_or(RdmaError::ResourceExhausted("No PD available".into()))
    }

    fn dealloc_pd(&mut self, handle: u32) -> Result<()> {
        if self.pd_table.dealloc(handle) {
            Ok(())
        } else {
            Err(RdmaError::InvalidInput(format!(
                "PD handle {handle} not present"
            )))
        }
    }
}
