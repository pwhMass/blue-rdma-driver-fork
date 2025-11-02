use std::{io, iter, sync::Arc};

use types::{WrInjector, WrWorker};
use worker::{SendQueueSync, SendWorker};

use crate::{
    csr::{build_send_rings, mode::Mode, DeviceAdaptor},
    mem::DmaBuf,
    ringbuf::DescRingBuffer,
    workers::spawner::{AbortSignal, SingleThreadPollingWorker},
};

mod types;
mod worker;

pub(crate) use types::*;
pub(crate) use worker::SendHandle;

pub(crate) fn spawn<Dev>(
    dev: &Dev,
    bufs: Vec<DmaBuf>,
    mode: Mode,
    abort: &AbortSignal,
) -> io::Result<SendHandle>
where
    Dev: DeviceAdaptor + Clone + Send + 'static,
{
    let injector = Arc::new(WrInjector::new());
    let handle = SendHandle::new(Arc::clone(&injector));
    let sq_rings = build_send_rings(dev.clone(), mode);
    for (ring, buf) in sq_rings.iter().zip(bufs.iter()) {
        ring.write_base_addr(buf.phys_addr)?;
    }
    let send_queues: Vec<_> = bufs
        .into_iter()
        .map(|p| SendQueue::new(DescRingBuffer::new(p.buf)))
        .collect();
    let workers: Vec<_> = iter::repeat_with(WrWorker::new_fifo)
        .take(send_queues.len())
        .collect();
    let stealers: Vec<_> = workers.iter().map(WrWorker::stealer).collect();
    let sqs = send_queues
        .into_iter()
        .zip(sq_rings)
        .map(|(sq, ring)| SendQueueSync::new(sq, ring));
    for (id, (local, sq)) in workers.into_iter().zip(sqs).enumerate() {
        let worker = SendWorker::new(
            id,
            local,
            Arc::clone(&injector),
            stealers
                .clone()
                .into_iter()
                .enumerate()
                .filter_map(|(i, x)| (i != id).then_some(x))
                .collect(),
            sq,
        );
        let name = format!("SendWorker{id}");
        worker.spawn(&name, abort.clone());
    }

    Ok(handle)
}
