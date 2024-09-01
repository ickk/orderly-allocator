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
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
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
  ///
  /// Uses a *best-fit* strategy, and returns [`Allocation`]s with the minimum
  /// alignment, `1`.
  pub fn alloc(&mut self, size: Size) -> Option<Allocation> {
    self.alloc_with_align(size, 1)
  }

  /// Try to allocate a region with the provided size & alignment
  ///
  /// Implements the following strategy (not quite *best-fit*):
  /// - Search for a region with at least `size + align - 1`, and then truncate
  ///   the start of the region such that alignment is reached.
  ///
  /// This is more prone to causing fragmentation compared to an unaligned
  /// [`alloc`](Self::alloc).
  ///
  /// # Panics
  ///
  /// - panics if `align == 0`.
  pub fn alloc_with_align(
    &mut self,
    size: Size,
    align: Size,
  ) -> Option<Allocation> {
    assert!(
      align >= 1,
      "`align` must be greater than or equal to 1. align = {align}"
    );

    let FreeRegion {
      location: mut free_region_location,
      size: mut free_region_size,
    } = self.find_free_region(size + align - 1)?;

    self.remove_free_region(free_region_location, free_region_size);

    let misalignment = free_region_location % align;
    if misalignment > 0 {
      self.insert_free_region(free_region_location, misalignment);
      free_region_location += misalignment;
      free_region_size -= misalignment;
    }

    if size < free_region_size {
      self.insert_free_region(
        free_region_location + size,
        free_region_size - size,
      );
    }

    self.available -= size;

    Some(Allocation {
      size,
      offset: free_region_location,
    })
  }

  /// Free the given allocation
  ///
  /// # Panics
  ///
  /// - May panics if the allocation's location gets freed twice, without first
  ///   being re-allocated. Note: This panic will not catch all double frees.
  pub fn free(&mut self, alloc: Allocation) {
    let mut free_region = FreeRegion {
      location: alloc.offset,
      size: alloc.size,
    };

    // coalesce
    {
      if let Some(FreeRegion { location, size }) =
        self.previous_free_region(alloc.offset)
      {
        if location + size == free_region.location {
          self.remove_free_region(location, size);
          free_region.location = location;
          free_region.size += size;
        }
      };

      if let Some(FreeRegion { location, size }) =
        self.following_free_region(alloc.offset)
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

  /// Try to find a region with at least `size`
  fn find_free_region(&mut self, size: Size) -> Option<FreeRegion> {
    self
      .free
      .range(FreeRegion { size, location: 0 }..)
      .copied()
      .next()
  }

  /// Get the first free-region before `location`
  fn previous_free_region(&self, location: Location) -> Option<FreeRegion> {
    self
      .location_map
      .range(..location)
      .next_back()
      .map(|(&location, &size)| FreeRegion { location, size })
  }

  /// Get the first free-region after `location`
  fn following_free_region(&self, location: Location) -> Option<FreeRegion> {
    use ::core::ops::Bound as B;
    self
      .location_map
      .range((B::Excluded(location), B::Unbounded))
      .next()
      .map(|(&location, &size)| FreeRegion { location, size })
  }

  /// remove a region from the internal free lists
  fn remove_free_region(&mut self, location: Location, size: Size) {
    self.location_map.remove(&location);
    let region_existed = self.free.remove(&FreeRegion { location, size });

    assert!(
      region_existed,
      "tried to remove a FreeRegion which did not exist: {:?}",
      FreeRegion { location, size }
    );
  }

  /// add a region to the internal free lists
  fn insert_free_region(&mut self, location: Location, size: Size) {
    self.free.insert(FreeRegion { location, size });
    let existing_size = self.location_map.insert(location, size);

    assert!(
      existing_size.is_none(),
      "Double free. Tried to add {new:?}, but {existing:?} was already there",
      new = FreeRegion { location, size },
      existing = FreeRegion {
        location,
        size: existing_size.unwrap()
      }
    )
  }
}
