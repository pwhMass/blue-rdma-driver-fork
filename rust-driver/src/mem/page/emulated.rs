use std::{ffi::c_void, io, ops::Range};

use crate::mem::{
    sim_alloc,
    virt_to_phy::{AddressResolver, PhysAddrResolverEmulated},
    DmaBuf, DmaBufAllocator, PageWithPhysAddr, PAGE_SIZE,
};

use super::{ContiguousPages, MmapMut, PageAllocator};

/// A page allocator for allocating pages of emulated physical memory
#[derive(Debug)]
pub(crate) struct EmulatedPageAllocator<const N: usize> {
    /// Inner
    inner: Vec<MmapMut>,
}

impl<const N: usize> EmulatedPageAllocator<N> {
    /// TODO: implements allocating multiple consecutive pages
    const _OK: () = assert!(
        N == 1,
        "allocating multiple contiguous pages is currently unsupported"
    );

    /// Creates a new `EmulatedPageAllocator`
    #[allow(clippy::as_conversions)] // usize to *mut c_void is safe
    pub(crate) fn new(addr_range: Range<usize>) -> Self {
        let inner: Vec<_> = addr_range
            .step_by(PAGE_SIZE)
            .map(|addr| MmapMut::new(addr as *mut c_void, PAGE_SIZE))
            .collect();

        Self { inner }
    }
}

impl<const N: usize> PageAllocator<N> for EmulatedPageAllocator<N> {
    #[allow(unsafe_code)]
    fn alloc(&mut self) -> io::Result<ContiguousPages<N>> {
        self.inner
            .pop()
            .map(ContiguousPages::new)
            .ok_or(io::ErrorKind::OutOfMemory.into())
    }
}

impl DmaBufAllocator for EmulatedPageAllocator<1> {
    #[allow(clippy::unwrap_in_result, clippy::unwrap_used)]
    fn alloc(&mut self, _len: usize) -> io::Result<DmaBuf> {
        let buf = self
            .inner
            .pop()
            .ok_or(io::Error::from(io::ErrorKind::OutOfMemory))?;
        let resolver = PhysAddrResolverEmulated::new(sim_alloc::shm_start_addr() as u64);
        //TODO 需要修改，需要注册到pa_va_map
        let phys_addr = resolver.virt_to_phys(buf.as_ptr() as u64)?.unwrap();
        Ok(DmaBuf::new(buf, phys_addr))
    }
}
