`orderly-allocator`
===================
[crates.io](https://crates.io/crates/orderly-allocator) |
[docs.rs](https://docs.rs/orderly-allocator) |
[github](https://github.com/ickk/orderly-allocator)

A super-simple soft-realtime allocator for managing an external pool of memory.

This allocator stores its metadata separately from the memory it manages, so it
can be useful as a suballocator for a GPU buffer.

A pair of BTrees is used to manage state internally, giving `orderly-allocator`
worst-case O(log(n)) performance for `alloc` & `free`\*. Uses a "best-fit"
search strategy and coalesces immediately on `free`, resulting in low
fragmentation.

Provided functionality:
- `alloc(size)`
- `alloc_with_align(size, align)`
- `free(allocation)`
- `try_reallocate(allocation, new_size)` - grow/shrink an allocation in-place
- `grow_capacity(additional)` - expand the allocator itself
- `reset()` - free all allocations

Metadata facilities:
- `capacity()`
- `is_empty()`
- `largest_available()` - size of the biggest free region
- `total_available()` - size of the sum of all free regions
- `report_free_regions()` - iterator of free regions


### Usage

This crate provides [`Allocator`] and [`Allocation`] which can be used to
manage any kind of buffer.

```rust
use {
  ::core::mem::{align_of, size_of},
  ::orderly_allocator::{Allocation, Allocator},
};

// Create memory and allocator
const CAPACITY: u32 = 65_536;
let mut memory: Vec<u8> = vec![0; CAPACITY as usize];
let mut allocator = Allocator::new(CAPACITY);

// An object to store
type Object = [u8; 16];
let object: Object = [
  0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x2C, 0x20, 0x6F,
  0x72, 0x64, 0x65, 0x72, 0x6C, 0x79, 0x21, 0x0,
];

// Allocate some memory
let allocation = allocator.alloc_with_align(
  size_of::<Object>() as u32,
  align_of::<Object>() as u32,
).unwrap();

// Fill the allocation
memory[allocation.range()].copy_from_slice(&object[..]);

// Later, free the memory region
allocator.free(allocation);
```


### `#![no_std]`

This crate works in a `no_std` context, however it requires the `alloc` crate
for the BTree implementation.


### Future Work

*The BTree implementation at the heart of `orderly-allocator` is simply the
standard library's `BTreeMap`/`BTreeSet`. This means the global-allocator is
used to create new tree-nodes every now and then. For real-time graphics this
is fine as the cost is amortised, and more importantly the I/O to actually
*fill* the allocated memory is likely to be the far greater cost.

It would be possible to improve performance and turn this into a firm- or
hard-realtime allocator by using a BTree implementation that pre-allocated
nodes ahead of time.


### Alternatives

Other libraries in the ecosystem that serve a similar purpose:

- [range-alloc] Generic range allocator, from the gfx-rs/wgpu project.
- [offset-allocator] A Rust port of Sebastian Aaltonen's
  [C++ package][sebbbi/OffsetAllocator] of the same name.

[range-alloc]: https://crates.io/crates/range-alloc
[offset-allocator]: https://crates.io/crates/offset-allocator
[sebbbi/OffsetAllocator]: https://github.com/sebbbi/OffsetAllocator


License
-------

This crate is licensed under any of the [Apache license 2.0], or the
[MIT license], or the [Zlib license] at your option.

[Apache license 2.0]: ./LICENSE-APACHE
[MIT license]: ./LICENSE-MIT
[Zlib license]: ./LICENSE-ZLIB


### Contribution

Unless you explicitly state otherwise, any contributions you intentionally
submit for inclusion in this work shall be licensed as above, without any
additional terms or conditions.
