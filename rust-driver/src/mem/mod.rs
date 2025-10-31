use std::{
    io,
    ops::{Deref, DerefMut},
};

/// Tools for converting virtual address to physicall address
pub(crate) mod virt_to_phy;

/// Page implementation
pub(crate) mod page;

pub(crate) mod dmabuf;

pub(crate) mod u_dma_buf;

mod utils;

pub(crate) mod sim_alloc;

use page::MmapMut;
pub(crate) use utils::*;
use virt_to_phy::{AddressResolver, PhysAddrResolverEmulated, PhysAddrResolverLinuxX86};

/// Number of bits for a 4KB page size
#[cfg(target_arch = "x86_64")]
#[cfg(feature = "page_size_4k")]
pub(crate) const PAGE_SIZE_BITS: u8 = 12;

/// Number of bits for a 2MB huge page size
#[cfg(feature = "page_size_2m")]
pub(crate) const PAGE_SIZE_BITS: u8 = 21;

/// Size of a 2MB huge page in bytes
pub(crate) const PAGE_SIZE: usize = 1 << PAGE_SIZE_BITS;

/// Asserts system page size matches the expected page size.
///
/// # Panics
///
/// Panics if the system page size does not equal `HUGE_PAGE_2MB_SIZE`.
pub(crate) fn assert_equal_page_size() {
    assert_eq!(page_size(), PAGE_SIZE, "page size not match");
}

/// Returns the current page size
#[allow(
    unsafe_code, // Safe because sysconf(_SC_PAGESIZE) is guaranteed to return a valid value.
    clippy::as_conversions,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]
pub(crate) fn page_size() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

pub(crate) struct PageWithPhysAddr {
    pub(crate) page: page::ContiguousPages<1>,
    pub(crate) phys_addr: u64,
}

impl PageWithPhysAddr {
    pub(crate) fn new(page: page::ContiguousPages<1>, phys_addr: u64) -> Self {
        Self { page, phys_addr }
    }

    pub(crate) fn alloc<A, R>(allocator: &mut A, resolver: &R) -> io::Result<Self>
    where
        A: page::PageAllocator<1>,
        R: AddressResolver,
    {
        let page = allocator.alloc()?;
        let phys_addr = resolver
            .virt_to_phys(page.addr())?
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?;

        Ok(Self { page, phys_addr })
    }
}

pub(crate) struct DmaBuf {
    pub(crate) buf: MmapMut,
    pub(crate) phys_addr: u64,
}

impl DmaBuf {
    pub(crate) fn new(buf: MmapMut, phys_addr: u64) -> Self {
        Self { buf, phys_addr }
    }

    pub(crate) fn phys_addr(&self) -> u64 {
        self.phys_addr
    }
}

impl Deref for DmaBuf {
    type Target = MmapMut;

    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl DerefMut for DmaBuf {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buf
    }
}

pub(crate) trait DmaBufAllocator {
    fn alloc(&mut self, len: usize) -> io::Result<DmaBuf>;
}


pub(crate) trait MemoryPinner {
    /// Pins pages in memory to prevent swapping
    ///
    /// # Errors
    ///
    /// Returns an error if the pages could not be locked in memory
    fn pin_pages(&self, addr: u64, length: usize) -> io::Result<()>;

    /// Unpins previously pinned pages
    ///
    /// # Errors
    ///
    /// Returns an error if the pages could not be locked in memory
    fn unpin_pages(&self, addr: u64, length: usize) -> io::Result<()>;
}

pub(crate) trait UmemHandler: AddressResolver + MemoryPinner {}

pub(crate) struct HostUmemHandler {
    resolver: PhysAddrResolverLinuxX86,
}

impl HostUmemHandler {
    pub(crate) fn new() -> Self {
        Self {
            resolver: PhysAddrResolverLinuxX86,
        }
    }
}

impl MemoryPinner for HostUmemHandler {
    fn pin_pages(&self, addr: u64, length: usize) -> io::Result<()> {
        let result = unsafe { libc::mlock(addr as *const std::ffi::c_void, length) };
        if result != 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "failed to lock pages"));
        }
        Ok(())
    }

    fn unpin_pages(&self, addr: u64, length: usize) -> io::Result<()> {
        let result = unsafe { libc::munlock(addr as *const std::ffi::c_void, length) };
        if result != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "failed to unlock pages",
            ));
        }
        Ok(())
    }
}

impl AddressResolver for HostUmemHandler {
    fn virt_to_phys(&self, virt_addr: u64) -> io::Result<Option<u64>> {
        self.resolver.virt_to_phys(virt_addr)
    }

    fn virt_to_phys_range(
        &self,
        start_addr: u64,
        num_pages: usize,
    ) -> io::Result<Vec<Option<u64>>> {
        self.resolver.virt_to_phys_range(start_addr, num_pages)
    }
}

impl UmemHandler for HostUmemHandler {}

pub(crate) struct EmulatedUmemHandler {
    resolver: PhysAddrResolverEmulated,
}

impl EmulatedUmemHandler {
    pub(crate) fn new(heap_start_addr: u64) -> Self {
        Self {
            resolver: PhysAddrResolverEmulated::new(heap_start_addr),
        }
    }
}

impl MemoryPinner for EmulatedUmemHandler {
    fn pin_pages(&self, addr: u64, length: usize) -> io::Result<()> {
        Ok(())
    }

    fn unpin_pages(&self, addr: u64, length: usize) -> io::Result<()> {
        Ok(())
    }
}

impl AddressResolver for EmulatedUmemHandler {
    fn virt_to_phys(&self, virt_addr: u64) -> io::Result<Option<u64>> {
        self.resolver.virt_to_phys(virt_addr)
    }
}

impl UmemHandler for EmulatedUmemHandler {}
