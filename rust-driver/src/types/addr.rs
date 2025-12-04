//! Type-safe address abstractions
//!
//! This module provides newtype wrappers for different address spaces used in the driver:
//! - `VirtAddr`: User-space virtual addresses (local process)
//! - `PhysAddr`: Physical addresses for DMA operations
//! - `RemoteAddr`: Remote virtual addresses for RDMA operations
//! - `CsrOffset`: Control and Status Register offsets
//!
//! These types prevent accidental mixing of address spaces at compile time,
//! which could lead to serious hardware errors like DMA writing to wrong addresses,
//! using local addresses in RDMA operations, or invalid register accesses.

use std::fmt;

/// Virtual address in user-space memory
///
/// Represents a pointer to virtual memory that may be passed by applications
/// through ibverbs API. These addresses must be translated to physical addresses
/// before being used in DMA operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub(crate) struct VirtAddr(u64);

impl VirtAddr {
    /// Creates a new virtual address
    #[inline]
    pub(crate) const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// Returns the raw address value
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0
    }

    /// Creates a virtual address from a raw pointer
    #[inline]
    #[allow(clippy::as_conversions)]
    pub(crate) fn from_ptr<T>(ptr: *const T) -> Self {
        Self(ptr as u64)
    }

    /// Converts to a raw pointer
    #[inline]
    #[allow(clippy::as_conversions)]
    pub(crate) fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }

    /// Converts to a mutable raw pointer
    #[inline]
    #[allow(clippy::as_conversions)]
    pub(crate) fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }

    /// Adds an offset to the address
    #[inline]
    pub(crate) fn offset(self, offset: u64) -> Option<Self> {
        self.0.checked_add(offset).map(Self)
    }

    /// Checks if the address is aligned to the given alignment
    #[inline]
    pub(crate) const fn is_aligned_to(self, align: u64) -> bool {
        self.0 % align == 0
    }
}

impl fmt::Display for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VirtAddr(0x{:x})", self.0)
    }
}

impl fmt::LowerHex for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

impl fmt::UpperHex for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

/// Physical address for DMA operations
///
/// Represents a physical memory address that can be used by hardware for DMA.
/// These addresses are obtained by translating virtual addresses through
/// the `AddressResolver` trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub(crate) struct PhysAddr(u64);

impl PhysAddr {
    /// Creates a new physical address
    #[inline]
    pub(crate) const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// Returns the raw address value
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0
    }

    /// Adds an offset to the address
    #[inline]
    pub(crate) fn offset(self, offset: u64) -> Option<Self> {
        self.0.checked_add(offset).map(Self)
    }

    /// Checks if the address is aligned to the given alignment
    #[inline]
    pub(crate) const fn is_aligned_to(self, align: u64) -> bool {
        self.0 % align == 0
    }

    /// Splits a 64-bit physical address into low and high 32-bit parts
    ///
    /// This is useful for writing to hardware registers that accept
    /// 64-bit addresses as two 32-bit values.
    #[inline]
    #[allow(clippy::as_conversions)]
    pub(crate) fn split(self) -> (u32, u32) {
        let lo = (self.0 & 0xFFFF_FFFF) as u32;
        let hi = (self.0 >> 32) as u32;
        (lo, hi)
    }

    /// Combines low and high 32-bit parts into a 64-bit physical address
    #[inline]
    pub(crate) fn from_parts(lo: u32, hi: u32) -> Self {
        Self(u64::from(lo) | (u64::from(hi) << 32))
    }
}

impl fmt::Display for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PhysAddr(0x{:x})", self.0)
    }
}

impl fmt::LowerHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

impl fmt::UpperHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

/// Remote virtual address (RDMA target address)
///
/// Represents a virtual address in a remote machine's address space.
/// This is opaque to the local driver and is used as the target address
/// for RDMA Write/Read/Atomic operations. It cannot be dereferenced locally
/// as it exists in a different process's (often on a different machine)
/// virtual address space.
///
/// # Note
/// Unlike `VirtAddr`, `RemoteAddr` does not provide `as_ptr()` methods
/// since the address cannot be accessed locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub(crate) struct RemoteAddr(u64);

impl RemoteAddr {
    /// Creates a new remote address
    #[inline]
    pub(crate) const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// Returns the raw address value
    ///
    /// This is used for serialization into RDMA descriptors that will
    /// be sent over the network to the remote side.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0
    }

    /// Adds an offset to the remote address
    ///
    /// This can be used for calculating offsets within a remote memory region,
    /// though typically offset calculations should be done on the remote side.
    #[inline]
    pub(crate) fn offset(self, offset: u64) -> Option<Self> {
        self.0.checked_add(offset).map(Self)
    }

    /// Checks if the address is aligned to the given alignment
    #[inline]
    pub(crate) const fn is_aligned_to(self, align: u64) -> bool {
        self.0 % align == 0
    }
}

impl fmt::Display for RemoteAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RemoteAddr(0x{:x})", self.0)
    }
}

impl fmt::LowerHex for RemoteAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

impl fmt::UpperHex for RemoteAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

/// Control and Status Register offset
///
/// Represents an offset within the CSR address space. This type ensures
/// that CSR offsets cannot be accidentally used as memory addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub(crate) struct CsrOffset(usize);

impl CsrOffset {
    /// Creates a new CSR offset
    #[inline]
    pub(crate) const fn new(offset: usize) -> Self {
        Self(offset)
    }

    /// Returns the raw offset value
    #[inline]
    pub(crate) const fn as_usize(self) -> usize {
        self.0
    }

    /// Adds an offset to the CSR offset
    #[inline]
    pub(crate) fn offset(self, offset: usize) -> Option<Self> {
        self.0.checked_add(offset).map(Self)
    }
}

impl fmt::Display for CsrOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CsrOffset(0x{:x})", self.0)
    }
}

impl fmt::LowerHex for CsrOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

impl fmt::UpperHex for CsrOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virt_addr_basic() {
        let addr = VirtAddr::new(0x1000);
        assert_eq!(addr.as_u64(), 0x1000);
    }

    #[test]
    fn test_virt_addr_alignment() {
        let addr = VirtAddr::new(0x1000);
        assert!(addr.is_aligned_to(0x1000));
        assert!(addr.is_aligned_to(0x100));
        assert!(!addr.is_aligned_to(0x2000));
    }

    #[test]
    fn test_phys_addr_split() {
        let addr = PhysAddr::new(0x1234_5678_9ABC_DEF0);
        let (lo, hi) = addr.split();
        assert_eq!(lo, 0x9ABC_DEF0);
        assert_eq!(hi, 0x1234_5678);
        assert_eq!(PhysAddr::from_parts(lo, hi), addr);
    }

    #[test]
    fn test_offset() {
        let virt = VirtAddr::new(0x1000);
        assert_eq!(virt.offset(0x100), Some(VirtAddr::new(0x1100)));

        let phys = PhysAddr::new(0x2000);
        assert_eq!(phys.offset(0x200), Some(PhysAddr::new(0x2200)));

        let csr = CsrOffset::new(0x100);
        assert_eq!(csr.offset(0x10), Some(CsrOffset::new(0x110)));
    }

    #[test]
    fn test_offset_overflow() {
        let virt = VirtAddr::new(u64::MAX);
        assert_eq!(virt.offset(1), None);

        let csr = CsrOffset::new(usize::MAX);
        assert_eq!(csr.offset(1), None);
    }

    #[test]
    fn test_remote_addr_basic() {
        let addr = RemoteAddr::new(0x7000_0000);
        assert_eq!(addr.as_u64(), 0x7000_0000);
    }

    #[test]
    fn test_remote_addr_offset() {
        let addr = RemoteAddr::new(0x1000);
        assert_eq!(addr.offset(0x500), Some(RemoteAddr::new(0x1500)));

        let addr_max = RemoteAddr::new(u64::MAX);
        assert_eq!(addr_max.offset(1), None);
    }

    #[test]
    fn test_remote_addr_alignment() {
        let addr = RemoteAddr::new(0x4000);
        assert!(addr.is_aligned_to(0x1000));
        assert!(addr.is_aligned_to(0x4000));
        assert!(!addr.is_aligned_to(0x8000));
    }
}
