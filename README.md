`orderly-allocator`
===================
[crates.io](https://crates.io/crates/orderly-allocator) |
[docs.rs](https://docs.rs/orderly-allocator) |
[github](https://github.com/ickk/orderly-allocator)

A super-simple soft-realtime allocator for managing an external pool of memory.

Since the allocator stores its metadata separately from the memory pool it
manages, it could be useful as a suballocator for e.g. a GPU buffer.

Has worst-case *O*(*log*(*n*)) performance\* for `alloc` & `free`. Provides
a best-fit search strategy and coalesces immediately on `free`, resulting in
low fragmentation.

`orderly-allocator` uses BTrees internally, so while it has *O*(*log*(*n*))
complexity expect excellent real-world performance; Rust's BTree implementation
uses a branching factor of 11. This means even if the allocator were in a state
where it had ~100,000 separate free-regions, a worst-case lookup will traverse
only 5 tree nodes.

### Usage

This crate provides two types [`Allocator`] and [`Allocation`] which can be
used to manage any kind of buffer as demonstrated.

```rust
use {
  ::core::mem::{align_of, size_of},
  ::orderly_allocator::{Allocation, Allocator},
};

#[repr(transparent)]
struct Object([u8; 16]);

// get a pool of memory and create an allocator to manage it
const POOL_SIZE: u32 = 2u32.pow(16);
let mut memory: Vec<u8> = vec![0; POOL_SIZE as usize];
let mut allocator = Allocator::new(POOL_SIZE);

assert_eq!(allocator.total_available(), POOL_SIZE);

// allocate some memory
let allocation = allocator.alloc_with_align(
  size_of::<Object>() as u32,
  align_of::<Object>() as u32,
);

// fill the corresponding memory region with some data
if let Some(allocation) = allocation {
  let object = Object([
    0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x2C, 0x20, 0x6F,
    0x72, 0x64, 0x65, 0x72, 0x6C, 0x79, 0x21, 0x0,
  ]);
  &memory[allocation.range()].copy_from_slice(&object.0[..]);
}

assert_eq!(
  allocator.total_available(),
  POOL_SIZE - size_of::<Object>() as u32,
);

// free the memory region when it is no longer needed
allocator.free(allocation.unwrap());

assert_eq!(allocator.total_available(), POOL_SIZE);
```


### `#![no_std]`

This crate works in a `no_std` context, however it currently requires the
`alloc` crate for the BTree implementation.


### Future Work

*Currently the BTree implementation at the heart of `orderly-allocator` will
ask the global-allocator for memory, for newly-created nodes, every now and
then.

It would be possible to turn this into a firm- or hard-realtime allocator by
using a different BTree implementation, one which preallocated memory for its
nodes ahead of time.


### Other Libraries

Other libraries in the ecosystem that might serve a similar purpose:

- [offset-allocator], A Rust port of Sebastian Aaltonen's
  [C++ package][sebbbi/OffsetAllocator] of the same name.

[offset-allocator]: https://github.com/pcwalton/offset-allocator
[sebbbi/OffsetAllocator]: https://github.com/sebbbi/OffsetAllocator


License
-------

This crate is licensed under any of the [Apache license 2.0], or the
[MIT license], or the [Zlib license] at your option.

[Apache license 2.0]: ./LICENSE-APACHE
[MIT license]: ./LICENSE-MIT
[Zlib license]: ./LICENSE-ZLIB


### Contribution

Unless explicitly stated otherwise, any contributions you intentionally submit
for inclusion in this work shall be licensed as above, without any additional
terms or conditions.
