use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::Arc,
    thread,
};

use bincode::{Decode, Encode};
use log::debug;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::rdma_utils::{
    qp::{qpn_to_index, QpTable},
    types::RecvWr,
};

pub(crate) trait PostRecvChannel {
    type Tx: PostRecvTx;
    type Rx: PostRecvRx;
}

/// A channel for the responder to pass `ibv_recv_wr` to the initiator
pub(crate) trait PostRecvTx: Sized {
    fn connect(addr: Ipv4Addr, dqpn: u32) -> io::Result<Self>;
    fn send(&mut self, wr: RecvWr) -> io::Result<()>;
}

pub(crate) trait PostRecvRx: Sized {
    fn listen(addr: Ipv4Addr, qpn: u32) -> io::Result<Self>;
    fn recv(&mut self) -> io::Result<RecvWr>;
}

const BASE_PORT: u16 = 60000;
const PORT_RANGE: u32 = 5535;  // 使用端口范围 60000-65534

pub(crate) struct TcpChannel;

impl PostRecvChannel for TcpChannel {
    type Tx = TcpChannelTx;
    type Rx = TcpChannelRx;
}

pub(crate) struct TcpChannelTx {
    addr: Ipv4Addr,
    dqpn: u32,
    inner: Option<TcpStream>,
}

impl PostRecvTx for TcpChannelTx {
    fn connect(addr: Ipv4Addr, dqpn: u32) -> io::Result<Self> {
        Ok(Self {
            inner: None,
            addr,
            dqpn,
        })
    }

    fn send(&mut self, wr: RecvWr) -> io::Result<()> {
        if self.inner.is_none() {
            debug!("TcpChannelTx try connect {}:{}", self.addr, qpn_to_port(self.dqpn));
            self.inner = Some(TcpStream::connect((self.addr, qpn_to_port(self.dqpn)))?);
        }
        let stream = self.inner.as_mut().unwrap_or_else(|| unreachable!());
        stream.write_all(&wr.to_bytes())?;

        Ok(())
    }
}

pub(crate) struct TcpChannelRx {
    inner: TcpListener,
    stream: Option<TcpStream>,
    buf: [u8; size_of::<RecvWr>()],
}

impl PostRecvRx for TcpChannelRx {
    fn listen(addr: Ipv4Addr, qpn: u32) -> io::Result<Self> {
        debug!("TcpChannelRx bind port {}", qpn_to_port(qpn));
        let inner = TcpListener::bind((addr, qpn_to_port(qpn)))?;
        Ok(Self {
            inner,
            stream: None,
            buf: [0; size_of::<RecvWr>()],
        })
    }

    fn recv(&mut self) -> io::Result<RecvWr> {
        if self.stream.is_none() {
            let (stream, _socket_addr) = self.inner.accept()?;
            self.stream = Some(stream);
        }
        let stream = self.stream.as_mut().unwrap_or_else(|| unreachable!());
        stream.read_exact(self.buf.as_mut())?;
        Ok(RecvWr::from_bytes(&self.buf))
    }
}

pub(crate) fn post_recv_channel<C: PostRecvChannel>(
    local_addr: Ipv4Addr,
    dest_addr: Ipv4Addr,
    local_qpn: u32,
    dest_qpn: u32,
) -> io::Result<(C::Tx, C::Rx)> {
    let tx = C::Tx::connect(dest_addr, dest_qpn)?;
    let rx = C::Rx::listen(local_addr, local_qpn)?;
    Ok((tx, rx))
}

fn qpn_to_port(qpn: u32) -> u16 {
    // 使用 Fibonacci 哈希将 QPN 的所有 32 位混合
    // 0x9E3779B9 = 2^32 / φ (黄金比例)，提供良好的位分布
    let hash = qpn.wrapping_mul(0x9E3779B9);
    BASE_PORT + (hash % PORT_RANGE) as u16
}

pub(crate) struct PostRecvTxTable<Tx = TcpChannelTx> {
    inner: QpTable<Option<Tx>>,
}

impl<Tx> PostRecvTxTable<Tx> {
    pub(crate) fn new() -> Self {
        Self {
            inner: QpTable::new(),
        }
    }

    pub(crate) fn insert(&mut self, qpn: u32, tx: Tx) {
        let _ignore = self.inner.replace(qpn, Some(tx));
    }

    pub(crate) fn get_qp_mut(&mut self, qpn: u32) -> Option<&mut Tx> {
        self.inner.get_qp_mut(qpn).and_then(Option::as_mut)
    }
}

pub(crate) type SharedRecvWrQueue = Arc<Mutex<VecDeque<RecvWr>>>;

pub(crate) struct RecvWrQueueTable {
    inner: QpTable<SharedRecvWrQueue>,
}

impl RecvWrQueueTable {
    pub(crate) fn new() -> Self {
        Self {
            inner: QpTable::new(),
        }
    }

    pub(crate) fn clone_recv_wr_queue(&self, qpn: u32) -> Option<SharedRecvWrQueue> {
        self.inner.get_qp(qpn).cloned()
    }

    pub(crate) fn pop(&self, qpn: u32) -> Option<RecvWr> {
        let queue = self.inner.get_qp(qpn)?;
        queue.lock().pop_front()
    }
}

pub(crate) struct RecvWorker<Rx = TcpChannelRx> {
    rx: Rx,
    wr_queue: SharedRecvWrQueue,
}

impl<Rx: PostRecvRx + Send + 'static> RecvWorker<Rx> {
    pub(crate) fn new(rx: Rx, wr_queue: SharedRecvWrQueue) -> Self {
        Self { rx, wr_queue }
    }

    // TODO: use tokio
    pub(crate) fn spawn(self) {
        let _handle = thread::Builder::new()
            .name("recv-worker".into())
            .spawn(move || self.run())
            .unwrap_or_else(|err| unreachable!("Failed to spawn rx thread: {err}"));
    }

    #[allow(clippy::needless_pass_by_value)] // consume the flag
    /// Run the handler loop
    fn run(mut self) {
        while let Ok(wr) = self.rx.recv() {
            self.wr_queue.lock().push_back(wr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
        time::Duration,
    };

    #[test]
    fn test_qpn_to_port() {
        // 测试端口范围在有效区间内
        for qpn in [0, 1 << 8, 2 << 8, 0x1f4, 0x194, 0xFFFFFFFF] {
            let port = qpn_to_port(qpn);
            assert!(port >= BASE_PORT && port < BASE_PORT + PORT_RANGE as u16,
                    "Port {} for QPN 0x{:x} is out of range [{}, {})",
                    port, qpn, BASE_PORT, BASE_PORT + PORT_RANGE as u16);
        }

        // 测试确定性：相同 QPN 总是映射到相同端口
        let qpn = 0x1f4;
        assert_eq!(qpn_to_port(qpn), qpn_to_port(qpn));

        // 测试不同 QPN（即使 index 相同但 key 不同）产生不同端口
        let qpn1 = 0x1f4;  // index=1, key=0xf4
        let qpn2 = 0x194;  // index=1, key=0x94
        assert_ne!(qpn_to_port(qpn1), qpn_to_port(qpn2),
                   "Different QPNs with same index should map to different ports");
    }

    #[test]
    fn test_tcp_channel_basic() {
        let local_addr = Ipv4Addr::LOCALHOST;
        let dest_addr = Ipv4Addr::LOCALHOST;
        let local_qpn = 1 << 8;
        let dest_qpn = 2 << 8;

        let (mut tx0, mut rx0) =
            post_recv_channel::<TcpChannel>(local_addr, dest_addr, local_qpn, dest_qpn).unwrap();
        let (mut tx1, mut rx1) =
            post_recv_channel::<TcpChannel>(dest_addr, local_addr, dest_qpn, local_qpn).unwrap();

        let test_wr = RecvWr {
            wr_id: 12345,
            addr: 0x1000,
            length: 1024,
            lkey: 0x5678,
        };

        let rx0_handle = thread::spawn(move || rx0.recv().unwrap());
        let rx1_handle = thread::spawn(move || rx1.recv().unwrap());
        thread::sleep(Duration::from_millis(100));

        tx0.send(test_wr).unwrap();
        tx1.send(test_wr).unwrap();

        let rx0_received = rx0_handle.join().unwrap();
        let rx1_received = rx1_handle.join().unwrap();

        assert_eq!(rx0_received, test_wr);
        assert_eq!(rx1_received, test_wr);
    }

    #[test]
    fn test_tcp_channel_multiple_sends() {
        let local_addr = Ipv4Addr::LOCALHOST;
        let dest_addr = Ipv4Addr::LOCALHOST;
        let local_qpn = 3 << 8;
        let dest_qpn = 4 << 8;

        let (mut tx0, mut rx0) =
            post_recv_channel::<TcpChannel>(local_addr, dest_addr, local_qpn, dest_qpn).unwrap();
        let (mut tx1, mut rx1) =
            post_recv_channel::<TcpChannel>(dest_addr, local_addr, dest_qpn, local_qpn).unwrap();

        let test_wrs = vec![
            RecvWr {
                wr_id: 1,
                addr: 0x1000,
                length: 100,
                lkey: 0x1111,
            },
            RecvWr {
                wr_id: 2,
                addr: 0x2000,
                length: 200,
                lkey: 0x2222,
            },
            RecvWr {
                wr_id: 3,
                addr: 0x3000,
                length: 300,
                lkey: 0x3333,
            },
        ];

        let num_wrs = test_wrs.len();

        let rx0_handle = thread::spawn(move || {
            std::iter::repeat_with(|| rx0.recv().unwrap())
                .take(num_wrs)
                .collect::<Vec<_>>()
        });
        let rx1_handle = thread::spawn(move || {
            std::iter::repeat_with(|| rx1.recv().unwrap())
                .take(num_wrs)
                .collect::<Vec<_>>()
        });
        thread::sleep(Duration::from_millis(100));

        for test_wr in test_wrs.clone() {
            tx0.send(test_wr).unwrap();
            tx1.send(test_wr).unwrap();
        }

        let rx0_received = rx0_handle.join().unwrap();
        let rx1_received = rx1_handle.join().unwrap();

        assert_eq!(rx0_received, test_wrs);
        assert_eq!(rx1_received, test_wrs);
    }
}
