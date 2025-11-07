/*
 * Physical Address to Virtual Address Bidirectional Mapping
 *
 * This module provides a global mapping table for simulation mode that tracks
 * the relationship between physical addresses (PA) and virtual addresses (VA).
 * This is necessary because the simulator sends memory access requests using
 * physical addresses, but the driver must perform operations on virtual addresses.
 *
 * The mapping table is only compiled and used in simulation mode (feature = "sim").
 */

use parking_lot::RwLock;
use std::collections::BTreeMap;

/// A range of memory addresses with its corresponding mapping
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AddressRange {
    /// Starting physical address
    pa_start: u64,
    /// Ending physical address (exclusive)
    pa_end: u64,
    /// Starting virtual address
    va_start: u64,
}

impl AddressRange {
    /// Check if a physical address falls within this range
    fn contains(&self, pa: u64) -> bool {
        pa >= self.pa_start && pa < self.pa_end
    }

    /// Convert a physical address to virtual address within this range
    fn pa_to_va(&self, pa: u64) -> Option<u64> {
        if self.contains(pa) {
            let offset = pa - self.pa_start;
            Some(self.va_start + offset)
        } else {
            None
        }
    }
}

/// Global bidirectional mapping table for PA ↔ VA translation
pub(crate) struct PaVaMap {
    /// Mapping from PA ranges to VA ranges
    /// Key: PA start address, Value: AddressRange
    ranges: RwLock<BTreeMap<u64, AddressRange>>,
    /// Reverse mapping from VA to PA for fast VA→PA lookups
    /// Key: VA start address, Value: PA start address
    va_to_pa: RwLock<BTreeMap<u64, u64>>,
}

impl PaVaMap {
    /// Create a new empty mapping table
    pub(crate) const fn new() -> Self {
        Self {
            ranges: RwLock::new(BTreeMap::new()),
            va_to_pa: RwLock::new(BTreeMap::new()),
        }
    }

    /// Insert a new PA ↔ VA mapping
    ///
    /// # Arguments
    ///
    /// * `pa` - Physical address start
    /// * `va` - Virtual address start
    /// * `size` - Size of the region in bytes
    ///
    /// # Panics
    ///
    /// Panics if the region overlaps with an existing mapping
    pub(crate) fn insert(&self, pa: u64, va: u64, size: usize) {
        let range = AddressRange {
            pa_start: pa,
            pa_end: pa + size as u64,
            va_start: va,
        };

        let mut ranges = self.ranges.write();

        // Check for overlaps with existing ranges
        for existing_range in ranges.values() {
            if (range.pa_start < existing_range.pa_end && range.pa_end > existing_range.pa_start) {
                panic!(
                    "PA range overlap detected: new [{:#x}, {:#x}) conflicts with existing [{:#x}, {:#x})",
                    range.pa_start, range.pa_end, existing_range.pa_start, existing_range.pa_end
                );
            }
        }

        let _ = ranges.insert(pa, range);
        drop(ranges); // Release the write lock on ranges

        // Insert reverse mapping VA → PA
        let mut va_to_pa = self.va_to_pa.write();
        let _ = va_to_pa.insert(va, pa);

        log::debug!(
            "PA_VA_MAP: Inserted mapping PA [{:#x}, {:#x}) -> VA [{:#x}, {:#x})",
            pa,
            pa + size as u64,
            va,
            va + size as u64
        );
    }

    /// Lookup the virtual address corresponding to a physical address
    ///
    /// # Arguments
    ///
    /// * `pa` - Physical address to look up
    ///
    /// # Returns
    ///
    /// The corresponding virtual address, or `None` if not found
    pub(crate) fn lookup(&self, pa: u64) -> Option<u64> {
        let ranges = self.ranges.read();

        // Use BTreeMap's range query to efficiently find the range
        // We look for the largest key that is <= pa
        for (_, range) in ranges.range(..=pa).rev() {
            if let Some(va) = range.pa_to_va(pa) {
                return Some(va);
            }
        }

        log::warn!("PA_VA_MAP: Failed to lookup PA {:#x}", pa);
        None
    }

    /// Lookup the physical address corresponding to a virtual address
    ///
    /// # Arguments
    ///
    /// * `va` - Virtual address start to look up (must be exact match)
    ///
    /// # Returns
    ///
    /// The corresponding physical address start, or `None` if not found
    pub(crate) fn lookup_by_va(&self, va: u64) -> Option<u64> {
        let va_to_pa = self.va_to_pa.read();

        match va_to_pa.get(&va) {
            Some(&pa) => Some(pa),
            None => {
                log::warn!("PA_VA_MAP: Failed to lookup VA {:#x}", va);
                None
            }
        }
    }

    /// Remove a PA ↔ VA mapping by physical address
    ///
    /// # Arguments
    ///
    /// * `pa` - Physical address start of the region to remove
    pub(crate) fn remove(&self, pa: u64) {
        let mut ranges = self.ranges.write();
        if let Some(range) = ranges.remove(&pa) {
            let va = range.va_start;
            drop(ranges); // Release write lock on ranges

            // Remove reverse mapping VA → PA
            let mut va_to_pa = self.va_to_pa.write();
            let _ = va_to_pa.remove(&va);

            log::debug!(
                "PA_VA_MAP: Removed mapping PA [{:#x}, {:#x}) -> VA [{:#x}, {:#x})",
                range.pa_start,
                range.pa_end,
                range.va_start,
                range.va_start + (range.pa_end - range.pa_start)
            );
        } else {
            log::warn!("PA_VA_MAP: Attempted to remove non-existent mapping at PA {:#x}", pa);
        }
    }

    /// Remove a PA ↔ VA mapping by virtual address
    ///
    /// # Arguments
    ///
    /// * `va` - Virtual address start of the region to remove
    ///
    /// This is useful for unpinning operations where the VA is known but PA needs to be looked up.
    pub(crate) fn remove_by_va(&self, va: u64) {
        // First lookup PA from VA
        let mut va_to_pa = self.va_to_pa.write();
        if let Some(pa) = va_to_pa.remove(&va) {
            drop(va_to_pa); // Release write lock on va_to_pa

            // Remove from main ranges map
            let mut ranges = self.ranges.write();
            if let Some(range) = ranges.remove(&pa) {
                log::debug!(
                    "PA_VA_MAP: Removed mapping VA [{:#x}, {:#x}) -> PA [{:#x}, {:#x})",
                    range.va_start,
                    range.va_start + (range.pa_end - range.pa_start),
                    range.pa_start,
                    range.pa_end
                );
            } else {
                log::warn!(
                    "PA_VA_MAP: Inconsistent state - VA {:#x} mapped to PA {:#x} but PA mapping not found",
                    va, pa
                );
            }
        } else {
            log::warn!("PA_VA_MAP: Attempted to remove non-existent mapping at VA {:#x}", va);
        }
    }

    /// Get the number of registered regions
    pub(crate) fn len(&self) -> usize {
        self.ranges.read().len()
    }

    /// Check if the mapping table is empty
    pub(crate) fn is_empty(&self) -> bool {
        self.ranges.read().is_empty()
    }

    /// Clear all mappings (primarily for testing)
    #[cfg(test)]
    pub(crate) fn clear(&self) {
        self.ranges.write().clear();
        self.va_to_pa.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_lookup() {
        let map = PaVaMap::new();

        // Insert a mapping
        map.insert(0x1000, 0x7000, 0x1000);

        // Lookup addresses within the range
        assert_eq!(map.lookup(0x1000), Some(0x7000));
        assert_eq!(map.lookup(0x1500), Some(0x7500));
        assert_eq!(map.lookup(0x1fff), Some(0x7fff));

        // Lookup addresses outside the range
        assert_eq!(map.lookup(0x0fff), None);
        assert_eq!(map.lookup(0x2000), None);
    }

    #[test]
    fn test_multiple_ranges() {
        let map = PaVaMap::new();

        // Insert multiple non-overlapping ranges
        map.insert(0x1000, 0x7000, 0x1000);
        map.insert(0x3000, 0x8000, 0x2000);
        map.insert(0x6000, 0xa000, 0x1000);

        // Lookup in first range
        assert_eq!(map.lookup(0x1500), Some(0x7500));

        // Lookup in second range
        assert_eq!(map.lookup(0x3500), Some(0x8500));
        assert_eq!(map.lookup(0x4fff), Some(0x9fff));

        // Lookup in third range
        assert_eq!(map.lookup(0x6500), Some(0xa500));

        // Lookup in gaps
        assert_eq!(map.lookup(0x2000), None);
        assert_eq!(map.lookup(0x5000), None);
        assert_eq!(map.lookup(0x7000), None);
    }

    #[test]
    fn test_remove() {
        let map = PaVaMap::new();

        map.insert(0x1000, 0x7000, 0x1000);
        assert_eq!(map.lookup(0x1500), Some(0x7500));

        map.remove(0x1000);
        assert_eq!(map.lookup(0x1500), None);
    }

    #[test]
    #[should_panic(expected = "PA range overlap detected")]
    fn test_overlap_detection() {
        let map = PaVaMap::new();

        // Insert first range
        map.insert(0x1000, 0x7000, 0x2000);

        // Try to insert overlapping range (should panic)
        map.insert(0x1500, 0x8000, 0x1000);
    }

    #[test]
    fn test_adjacent_ranges() {
        let map = PaVaMap::new();

        // Insert adjacent (non-overlapping) ranges
        map.insert(0x1000, 0x7000, 0x1000);
        map.insert(0x2000, 0x8000, 0x1000);

        // Both ranges should be accessible
        assert_eq!(map.lookup(0x1fff), Some(0x7fff));
        assert_eq!(map.lookup(0x2000), Some(0x8000));
    }

    #[test]
    fn test_len_and_is_empty() {
        let map = PaVaMap::new();

        assert!(map.is_empty());
        assert_eq!(map.len(), 0);

        map.insert(0x1000, 0x7000, 0x1000);
        assert!(!map.is_empty());
        assert_eq!(map.len(), 1);

        map.insert(0x2000, 0x8000, 0x1000);
        assert_eq!(map.len(), 2);

        map.remove(0x1000);
        assert_eq!(map.len(), 1);

        map.remove(0x2000);
        assert!(map.is_empty());
    }

    #[test]
    fn test_lookup_by_va() {
        let map = PaVaMap::new();

        // Insert mappings
        map.insert(0x1000, 0x7000, 0x1000);
        map.insert(0x3000, 0x8000, 0x2000);

        // Test VA → PA lookups (exact matches only)
        assert_eq!(map.lookup_by_va(0x7000), Some(0x1000));
        assert_eq!(map.lookup_by_va(0x8000), Some(0x3000));

        // Non-existent VA
        assert_eq!(map.lookup_by_va(0x9000), None);

        // Offset addresses should not match (exact match only)
        assert_eq!(map.lookup_by_va(0x7500), None);
        assert_eq!(map.lookup_by_va(0x8500), None);
    }

    #[test]
    fn test_bidirectional_lookup() {
        let map = PaVaMap::new();

        map.insert(0x1000, 0x7000, 0x1000);

        // Test PA → VA
        assert_eq!(map.lookup(0x1000), Some(0x7000));
        assert_eq!(map.lookup(0x1500), Some(0x7500));

        // Test VA → PA
        assert_eq!(map.lookup_by_va(0x7000), Some(0x1000));
    }

    #[test]
    fn test_remove_by_va() {
        let map = PaVaMap::new();

        // Insert mapping
        map.insert(0x1000, 0x7000, 0x1000);

        // Verify it exists
        assert_eq!(map.lookup_by_va(0x7000), Some(0x1000));
        assert_eq!(map.lookup(0x1000), Some(0x7000));

        // Remove by VA
        map.remove_by_va(0x7000);

        // Verify both directions are removed
        assert_eq!(map.lookup_by_va(0x7000), None);
        assert_eq!(map.lookup(0x1000), None);
        assert!(map.is_empty());
    }

    #[test]
    fn test_remove_maintains_both_indices() {
        let map = PaVaMap::new();

        // Insert mapping
        map.insert(0x1000, 0x7000, 0x1000);

        // Verify it exists
        assert_eq!(map.lookup_by_va(0x7000), Some(0x1000));
        assert_eq!(map.lookup(0x1000), Some(0x7000));

        // Remove by PA (original method)
        map.remove(0x1000);

        // Verify both directions are removed
        assert_eq!(map.lookup_by_va(0x7000), None);
        assert_eq!(map.lookup(0x1000), None);
        assert!(map.is_empty());
    }

    #[test]
    fn test_bidirectional_with_multiple_ranges() {
        let map = PaVaMap::new();

        // Insert multiple ranges
        map.insert(0x1000, 0x7000, 0x1000);
        map.insert(0x3000, 0x8000, 0x2000);
        map.insert(0x6000, 0xa000, 0x1000);

        // Test all VA → PA lookups
        assert_eq!(map.lookup_by_va(0x7000), Some(0x1000));
        assert_eq!(map.lookup_by_va(0x8000), Some(0x3000));
        assert_eq!(map.lookup_by_va(0xa000), Some(0x6000));

        // Test all PA → VA lookups
        assert_eq!(map.lookup(0x1000), Some(0x7000));
        assert_eq!(map.lookup(0x3000), Some(0x8000));
        assert_eq!(map.lookup(0x6000), Some(0xa000));

        // Remove middle range by VA
        map.remove_by_va(0x8000);

        // Verify only that range is removed
        assert_eq!(map.lookup_by_va(0x7000), Some(0x1000));
        assert_eq!(map.lookup_by_va(0x8000), None);
        assert_eq!(map.lookup_by_va(0xa000), Some(0x6000));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_clear_removes_both_indices() {
        let map = PaVaMap::new();

        // Insert multiple mappings
        map.insert(0x1000, 0x7000, 0x1000);
        map.insert(0x2000, 0x8000, 0x1000);

        assert_eq!(map.len(), 2);

        // Clear all
        map.clear();

        // Verify both indices are empty
        assert!(map.is_empty());
        assert_eq!(map.lookup_by_va(0x7000), None);
        assert_eq!(map.lookup_by_va(0x8000), None);
        assert_eq!(map.lookup(0x1000), None);
        assert_eq!(map.lookup(0x2000), None);
    }
}
