`orderly-allocator`
===================

A super simple fast soft-realtime allocator for managing an external pool of
memory

Since the pool of memory it manages is external, it could be useful as a
suballocator for e.g. a GPU buffer.

Has worst-case *O(log(n))* performance for `alloc` & `free`, but provides a
*best-fit* search strategy & immediately coalesces on `free` resulting in low
fragmentation.

The *O(log(n))* performance characteristics are due to using BTrees internally.
So, despite the *temporal-complexity*, expect excellent real-world performance;
Rust's BTree implementation uses a branching factor of 11. This means even if
the allocator were in a state where it had ~100,000 separate free-regions, a
worst-case lookup will traverse only 5 tree nodes.


### `#![no_std]`

This crate is `no_std`, but requires the `alloc` crate for the BTree
implementation.


### Future Work

Currently, the BTree implementation at the heart of `orderly-allocator` asks
the global-allocator for memory for newly-created nodes every now and then. It
would be possible to turn this into a firm- or hard-realtime allocator by using
a different BTree implementation which pulled new nodes from a preallocated
set.


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
