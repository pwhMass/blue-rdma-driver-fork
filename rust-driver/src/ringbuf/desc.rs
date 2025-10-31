use std::{
    io,
    ops::{Deref, DerefMut},
};

use crate::{
    descriptors::DESC_SIZE,
    mem::{
        page::{ContiguousPages, HostPageAllocator, MmapMut, PageAllocator},
        DmaBuf, DmaBufAllocator,
    },
    ringbuf::dma_rb::{DmaRingBuf, RING_BUF_LEN},
};

pub(crate) trait DescSerialize {
    fn serialize(&self) -> [u8; 32];
}

pub(crate) trait DescDeserialize {
    fn deserialize(d: [u8; 32]) -> Self;
}

pub(crate) struct DescRingBuffer(DmaRingBuf<[u8; 32]>);

impl DescRingBuffer {
    pub(crate) fn new(buf: MmapMut) -> Self {
        let rb = DmaRingBuf::new(buf);
        Self(rb)
    }

    pub(crate) fn push<T: DescSerialize>(&mut self, value: &T) -> bool {
        self.0.push(value.serialize())
    }

    pub(crate) fn pop<T: DescDeserialize>(&mut self) -> Option<T> {
        self.0.pop(Self::is_valid).map(DescDeserialize::deserialize)
    }

    pub(crate) fn pop_two<A: DescDeserialize, B: DescDeserialize>(
        &mut self,
    ) -> (Option<A>, Option<B>) {
        let (a, b) = self.0.pop_two(Self::is_valid, Self::has_next);
        (
            a.map(DescDeserialize::deserialize),
            b.map(DescDeserialize::deserialize),
        )
    }

    pub(crate) fn remaining(&self) -> usize {
        self.0.remaining()
    }

    pub(crate) fn set_tail(&mut self, tail: u32) {
        self.0.set_tail(tail);
    }

    pub(crate) fn set_head(&mut self, head: u32) {
        self.0.set_head(head);
    }

    /// Returns the current head index in the ring buffer
    pub(crate) fn head(&self) -> usize {
        self.0.head()
    }

    /// Returns the current tail index in the ring buffer
    pub(crate) fn tail(&self) -> usize {
        self.0.tail()
    }

    fn is_valid(desc: &[u8; 32]) -> bool {
        // highest bit is the valid bit
        desc[31] >> 7 == 1
    }

    fn has_next(desc: &[u8; 32]) -> bool {
        (desc[31] >> 6) & 1 == 1
    }
}

pub(crate) struct DescRingBufAllocator<'a,  A> {
    dma_buf_allocator: &'a mut A,
}

impl<'a, A: DmaBufAllocator> DescRingBufAllocator<'a, A> {
    pub(crate) fn new(dma_buf_allocator: &'a mut A) -> Self {
        Self { dma_buf_allocator }
    }

    pub(crate) fn alloc(&mut self) -> io::Result<DmaBuf> {
        self.dma_buf_allocator.alloc(RING_BUF_LEN * DESC_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::NonNull;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct TestDesc {
        id: u32,
        data: u64,
        valid: bool,
        has_next: bool,
    }

    impl DescSerialize for TestDesc {
        fn serialize(&self) -> [u8; 32] {
            let mut bytes = [0u8; 32];
            bytes[0..4].copy_from_slice(&self.id.to_le_bytes());
            bytes[4..12].copy_from_slice(&self.data.to_le_bytes());

            // Set valid bit (bit 7) and has_next bit (bit 6) in the last byte
            let mut flags = 0u8;
            if self.valid {
                flags |= 1 << 7;
            }
            if self.has_next {
                flags |= 1 << 6;
            }
            bytes[31] = flags;

            bytes
        }
    }

    impl DescDeserialize for TestDesc {
        fn deserialize(d: [u8; 32]) -> Self {
            let id = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
            let data = u64::from_le_bytes([d[4], d[5], d[6], d[7], d[8], d[9], d[10], d[11]]);
            let flags = d[31];
            let valid = (flags >> 7) & 1 == 1;
            let has_next = (flags >> 6) & 1 == 1;

            Self {
                id,
                data,
                valid,
                has_next,
            }
        }
    }

    #[allow(unsafe_code)]
    fn create_test_mmap() -> MmapMut {
        let len = 0x10_0000;
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_ANON,
                -1,
                0,
            )
        };

        MmapMut { ptr, len }
    }

    #[test]
    fn test_desc_ring_buffer_push_pop() {
        let mmap = create_test_mmap();
        let mut rb = DescRingBuffer::new(mmap);

        let desc = TestDesc {
            id: 42,
            data: 0x123,
            valid: true,
            has_next: false,
        };

        assert!(rb.push(&desc));
        assert_eq!(rb.remaining(), RING_BUF_LEN - 1);

        let popped: Option<TestDesc> = rb.pop();
        assert_eq!(popped, Some(desc));
        assert_eq!(rb.remaining(), RING_BUF_LEN);
    }

    #[test]
    fn test_desc_ring_buffer_invalid_desc() {
        let mmap = create_test_mmap();
        let mut rb = DescRingBuffer::new(mmap);

        let invalid_desc = TestDesc {
            id: 42,
            data: 0x123,
            valid: false,
            has_next: false,
        };

        assert!(rb.push(&invalid_desc));

        let popped: Option<TestDesc> = rb.pop();
        assert_eq!(popped, None);
    }

    #[test]
    fn test_desc_ring_buffer_pop_two() {
        let mmap = create_test_mmap();
        let mut rb = DescRingBuffer::new(mmap);

        let desc1 = TestDesc {
            id: 1,
            data: 0x1111,
            valid: true,
            has_next: true,
        };

        let desc2 = TestDesc {
            id: 2,
            data: 0x2222,
            valid: true,
            has_next: false,
        };

        assert!(rb.push(&desc1));
        assert!(rb.push(&desc2));

        let (popped1, popped2): (Option<TestDesc>, Option<TestDesc>) = rb.pop_two();
        assert_eq!(popped1, Some(desc1));
        assert_eq!(popped2, Some(desc2));
    }

    #[test]
    fn test_desc_ring_buffer_pop_two_no_next() {
        let mmap = create_test_mmap();
        let mut rb = DescRingBuffer::new(mmap);

        let desc1 = TestDesc {
            id: 1,
            data: 0x1111,
            valid: true,
            has_next: false,
        };

        let desc2 = TestDesc {
            id: 2,
            data: 0x2222,
            valid: true,
            has_next: false,
        };

        assert!(rb.push(&desc1));
        assert!(rb.push(&desc2));

        let (popped1, popped2): (Option<TestDesc>, Option<TestDesc>) = rb.pop_two();
        assert_eq!(popped1, Some(desc1));
        assert_eq!(popped2, None);
    }

    #[test]
    fn test_desc_ring_buffer_set_head_tail() {
        let mmap = create_test_mmap();
        let mut rb = DescRingBuffer::new(mmap);

        rb.set_head(10);
        rb.set_tail(5);

        assert_eq!(rb.head(), 10);
        assert_eq!(rb.tail(), 5);
    }

    #[test]
    fn test_desc_serialize_deserialize() {
        let desc = TestDesc {
            id: 0x12,
            data: 0x123,
            valid: true,
            has_next: true,
        };

        let serialized = desc.serialize();
        let deserialized = TestDesc::deserialize(serialized);

        assert_eq!(desc, deserialized);
    }
}
