# PA→VA Map Usage Example with Remaining Length

## API Change Summary

The `lookup()` function has been enhanced to return both the virtual address AND the remaining length from that address to the end of the mapped range.

### Old API (Before)
```rust
pub(crate) fn lookup(&self, pa: u64) -> Option<u64>
```

### New API (After)
```rust
pub(crate) fn lookup(&self, pa: u64) -> Option<(u64, usize)>
```

## Usage Example

```rust
use crate::mem::pa_va_map::PaVaMap;

// Create a new PA↔VA mapping table
let map = PaVaMap::new();

// Register a memory region: PA [0x1000, 0x3000) -> VA [0x7000, 0x9000)
// This is an 8KB region (0x2000 bytes)
map.insert(0x1000, 0x7000, 0x2000);

// Lookup PA at the beginning of the range
match map.lookup(0x1000) {
    Some((va, remaining_len)) => {
        println!("PA 0x1000 -> VA 0x{:x}", va);           // VA 0x7000
        println!("Remaining bytes: {}", remaining_len);   // 8192 bytes (0x2000)

        // Safe to access up to `remaining_len` bytes from `va`
        assert_eq!(va, 0x7000);
        assert_eq!(remaining_len, 0x2000);
    }
    None => println!("PA not found"),
}

// Lookup PA in the middle of the range
match map.lookup(0x2000) {
    Some((va, remaining_len)) => {
        println!("PA 0x2000 -> VA 0x{:x}", va);           // VA 0x8000
        println!("Remaining bytes: {}", remaining_len);   // 4096 bytes (0x1000)

        // Only 4KB remaining from this point to the end
        assert_eq!(va, 0x8000);
        assert_eq!(remaining_len, 0x1000);
    }
    None => println!("PA not found"),
}

// Lookup near the end of the range
match map.lookup(0x2fff) {
    Some((va, remaining_len)) => {
        println!("PA 0x2fff -> VA 0x{:x}", va);           // VA 0x8fff
        println!("Remaining bytes: {}", remaining_len);   // 1 byte

        // Only 1 byte remaining - be careful!
        assert_eq!(va, 0x8fff);
        assert_eq!(remaining_len, 1);
    }
    None => println!("PA not found"),
}
```

## Use Case: Memory Proxy Read/Write Operations

This is particularly useful for the simulation mode memory proxy, where the simulator sends memory access requests using physical addresses:

```rust
#[allow(unsafe_code)]
pub(crate) fn handle_read_request(&self, req: SimpleMemRequest) -> SimpleMemResponse {
    // Lookup the VA and get remaining length
    match self.pa_va_map.read().lookup(req.address) {
        Some((va, remaining_len)) => {
            // Check if the requested length is within bounds
            if req.length > remaining_len {
                log::error!(
                    "Read request exceeds mapped range: requested {} bytes, only {} available",
                    req.length, remaining_len
                );
                // Handle error appropriately
            }

            // Safe to read up to `remaining_len` bytes from `va`
            let ptr = va as *const u8;
            let mut data = Vec::with_capacity(req.length.min(remaining_len));

            for i in 0..req.length.min(remaining_len) {
                unsafe {
                    data.push(ptr.add(i).read_volatile());
                }
            }

            // Return response with data
            SimpleMemResponse {
                response_type: "mem_read_response".to_string(),
                channel_id: req.channel_id,
                data: Some(data),
                // ... other fields
            }
        }
        None => {
            log::error!("PA {:#x} not found in PA→VA map", req.address);
            // Handle error
        }
    }
}
```

## Benefits

1. **Safety**: Know exactly how many bytes are safe to access from the returned VA
2. **Bounds Checking**: Prevent buffer overruns by checking against `remaining_len`
3. **Single Operation**: Get both VA and length in one lookup, more efficient
4. **Debug Info**: Easier to log and debug memory access patterns

## Testing

The implementation includes comprehensive tests covering:
- Basic lookup with length calculation
- Multiple non-overlapping ranges
- Adjacent ranges
- Edge cases (first byte, last byte)
- Various range sizes (4KB, 8KB, 64KB)

Run tests with:
```bash
cargo test --lib pa_va_map
```
