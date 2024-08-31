#![doc = include_str!("../README.md")]
#![no_std]
extern crate alloc;
use {
  ::alloc::collections::{BTreeMap, BTreeSet},
  ::core::cmp::Ordering,
};

type Size = u32;
type Location = Size;

/// Metadata containing information about an allocation
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub struct Allocation {
  /// The location of this allocation within the buffer
  pub offset: Location,
  /// The size of this allocation
  pub size: Size,
}

/// A super simple fast soft-realtime allocator for managing an external pool
/// of memory
///
/// Since the pool of memory it manages is external, it could be useful as a
/// suballocator for e.g. a GPU buffer.
///
/// Has worst-case *O(log(n))* performance for `alloc` & `free`, but provides a
/// *best-fit* search strategy & immediately coalesces on `free` resulting in
/// low fragmentation.
///
/// The *O(log(n))* performance characteristics are due to using BTrees
/// internally. So, despite the *temporal-complexity*, expect excellent
/// real-world performance; Rust's BTree implementation uses a branching factor
/// of 11. This means even if the allocator were in a state where it had
/// ~100,000 separate free-regions, a worst-case lookup will traverse only 5
/// tree nodes.
pub struct OrderlyAllocator {
  /// An ordered collection of free-regions, sorted primarily by size, then by
  /// location
  free: BTreeSet<FreeRegion>,
  /// An ordered collection of free-regions, sorted by location
  location_map: BTreeMap<Location, Size>,
  /// The total capacity
  capacity: Size,
  /// The amount of free memory
  available: Size,
}

// This type has a special implementation of Ord
#[derive(PartialEq, Eq, Copy, Clone)]
struct FreeRegion {
  location: Location,
  size: Size,
}

impl PartialOrd for FreeRegion {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for FreeRegion {
  fn cmp(&self, other: &Self) -> Ordering {
    use Ordering as O;
    match (
      self.size.cmp(&other.size),
      self.location.cmp(&other.location),
    ) {
      (O::Equal, O::Equal) => O::Equal,
      (O::Equal, O::Less) | (O::Less, _) => O::Less,
      (O::Equal, O::Greater) | (O::Greater, _) => O::Greater,
    }
  }
}

impl OrderlyAllocator {
  /// Create a new allocator to manage a pool of memory
  pub fn new(capacity: Size) -> Self {
    let mut allocator = OrderlyAllocator {
      free: BTreeSet::new(),
      location_map: BTreeMap::new(),
      capacity,
      available: capacity,
    };

    allocator.insert_free_region(0, capacity);

    allocator
  }

  /// Try to allocate a region with the provided size
  pub fn alloc(&mut self, size: Size) -> Option<Allocation> {
    let free_region = self
      .free
      .range(FreeRegion { size, location: 0 }..)
      .copied()
      .next();

    if let Some(FreeRegion {
      size: free_region_size,
      location,
    }) = free_region
    {
      self.remove_free_region(location, free_region_size);
      if size < free_region_size {
        self.insert_free_region(location + size, free_region_size - size);
      }

      self.available -= size;
      return Some(Allocation {
        size,
        offset: location,
      });
    }

    None
  }

  /// Free the given allocation
  pub fn free(&mut self, alloc: Allocation) {
    let mut free_region = FreeRegion {
      location: alloc.offset,
      size: alloc.size,
    };

    // coalesce
    {
      // previous entry
      if let Some((&location, &size)) =
        self.location_map.range(..=alloc.offset).next_back()
      {
        if location + size == free_region.location {
          self.remove_free_region(location, size);
          free_region.location = location;
          free_region.size += size;
        }
      };
      // following entry
      if let Some((&location, &size)) =
        self.location_map.range(alloc.offset..).next()
      {
        if free_region.location + free_region.size == location {
          self.remove_free_region(location, size);
          free_region.size += size;
        }
      }
    }

    self.insert_free_region(free_region.location, free_region.size);
    self.available += alloc.size;
  }

  /// Get the total capacity of the pool
  pub fn capacity(&self) -> Size {
    self.capacity
  }

  /// Get the total available memory in this pool
  ///
  /// note: The memory may be fragmented, so it may not be possible to
  /// allocate an object of this size.
  pub fn total_available(&self) -> Size {
    self.available
  }

  /// Get the size of the largest available memory region in this pool
  pub fn largest_available(&self) -> Size {
    self.free.last().map_or(0, |region| region.size)
  }

  /// remove an entry from the internal free lists
  fn remove_free_region(&mut self, location: Location, size: Size) {
    self.free.remove(&FreeRegion { location, size });
    self.location_map.remove(&location);
  }

  /// add an entry to the internal free lists
  fn insert_free_region(&mut self, location: Location, size: Size) {
    self.free.insert(FreeRegion { location, size });
    self.location_map.insert(location, size);
  }
}
