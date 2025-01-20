#![doc = include_str!("../README.md")]
#![no_std]
extern crate alloc;
use {
  ::btree_slab::{BTreeMap, BTreeSet},
  ::core::{cmp::Ordering, error::Error, fmt, num::NonZero, ops::Range},
};

type Size = u32;
type Location = Size;

/// Metadata containing information about an allocation
///
/// This is a small `Copy` type. It also provides a niche, so that
/// `Option<Allocation>` has the same size as `Allocation`.
/// ```
/// # use {::core::mem::size_of, ::orderly_allocator::Allocation};
/// assert_eq!(size_of::<Allocation>(), size_of::<u64>());
/// assert_eq!(size_of::<Option<Allocation>>(), size_of::<Allocation>());
/// ```
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Allocation {
  /// The location of this allocation within the buffer
  pub offset: Location,
  /// The size of this allocation
  pub size: NonZero<Size>,
}

impl Allocation {
  /// Get the offset of the allocation
  ///
  /// This is just a wrapper for `allocation.offset` for symmetry with `size`.
  pub fn offset(&self) -> Location {
    self.offset
  }

  /// Get the size of the allocation
  ///
  /// This is just sugar for `allocation.size.get()`.
  pub fn size(&self) -> Size {
    self.size.get()
  }

  /// Get a [`Range<usize>`] from `offset` to `offset + size`
  ///
  /// This can be used to directly index a buffer.
  ///
  /// For example:
  /// ```ignore
  /// # use {::core::num::NonZero, ::orderly_allocator::Allocation};
  /// let buffer: Vec<usize> = (0..100).collect();
  /// let allocation = Allocation {
  ///   offset: 25,
  ///   size: NonZero::new(4).unwrap()
  /// };
  ///
  /// let region = &buffer[allocation.range()];
  ///
  /// assert_eq!(region, &[25, 26, 27, 28]);
  /// ```
  pub fn range(&self) -> Range<usize> {
    (self.offset as usize)..((self.offset + self.size.get()) as usize)
  }
}

/// A super-simple soft-realtime allocator for managing an external pool of
/// memory
#[derive(Clone)]
pub struct Allocator {
  /// An ordered collection of free-regions, sorted primarily by size, then by
  /// location
  free: BTreeSet<FreeRegion>,
  /// An ordered collection of free-regions, sorted by location
  location_map: BTreeMap<Location, NonZero<Size>>,
  /// The total capacity
  capacity: NonZero<Size>,
  /// The amount of free memory
  available: Size,
}

// This type has an explicit implementation of Ord, since we rely on properties
// of its behaviour to find and select free regions.
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
struct FreeRegion {
  location: Location,
  size: NonZero<Size>,
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

impl Allocator {
  /// Create a new allocator to manage a pool of memory
  ///
  /// Panics:
  /// - Panics if `capacity == 0`
  pub fn new(capacity: Size) -> Self {
    let capacity = NonZero::new(capacity).expect("`capacity == 0`");

    let mut allocator = Allocator {
      free: BTreeSet::new(),
      location_map: BTreeMap::new(),
      capacity,
      available: capacity.get(),
    };

    allocator.reset();

    allocator
  }

  /// Try to allocate a region with the provided size
  ///
  /// Uses a *best-fit* strategy, and returns [`Allocation`]s with arbitrary
  /// alignment.
  ///
  /// Returns `None` if:
  /// - `size == 0`, or
  /// - `size + 1` overflows.
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
  /// Returns `None` if:
  /// - there are no free-regions with `size + align - 1` available space, or
  /// - `size == 0`, or
  /// - `align == 0`, or
  /// - `size + align` overflows.
  pub fn alloc_with_align(
    &mut self,
    size: Size,
    align: Size,
  ) -> Option<Allocation> {
    let size = NonZero::new(size)?;
    let align = NonZero::new(align)?;

    let FreeRegion {
      location: mut free_region_location,
      size: free_region_size,
    } = self.find_free_region(size.checked_add(align.get() - 1)?)?;

    self.remove_free_region(free_region_location, free_region_size);

    let mut free_region_size = free_region_size.get();

    if let Some(misalignment) =
      NonZero::new((align.get() - (free_region_location % align)) % align)
    {
      self.insert_free_region(free_region_location, misalignment);
      free_region_location += misalignment.get();
      free_region_size -= misalignment.get();
    }

    if let Some(size_leftover) = NonZero::new(free_region_size - size.get()) {
      self
        .insert_free_region(free_region_location + size.get(), size_leftover);
    }

    self.available -= size.get();

    Some(Allocation {
      size,
      offset: free_region_location,
    })
  }

  /// Free the given allocation
  ///
  /// # Panics
  ///
  /// - May panic if the allocation's location gets freed twice, without first
  ///   being re-allocated.
  ///
  ///   Note: This panic will not catch all double frees.
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
        if location + size.get() == free_region.location {
          self.remove_free_region(location, size);
          free_region.location = location;
          // note: this unwrap is ok because the sum of all free-regions cannot
          // be larger than the total size of the allocator; which we know is
          // some `Size`.
          free_region.size = free_region.size.checked_add(size.get()).unwrap();
        }
      };

      if let Some(FreeRegion { location, size }) =
        self.following_free_region(alloc.offset)
      {
        if free_region.location + free_region.size.get() == location {
          self.remove_free_region(location, size);
          // note: this unwrap is ok because the sum of all free-regions cannot
          // be larger than the total size of the allocator; which we know is
          // some `Size`.
          free_region.size = free_region.size.checked_add(size.get()).unwrap();
        }
      }
    }

    self.insert_free_region(free_region.location, free_region.size);
    self.available += alloc.size.get();
  }

  /// Free ***all*** allocations
  pub fn reset(&mut self) {
    self.free.clear();
    self.location_map.clear();
    self.available = self.capacity.get();
    self.insert_free_region(0, self.capacity);
  }

  /// Add new free space at the end of the allocator
  ///
  /// Returns `Err(Overflow)` if `self.capacity + additional` would overflow.
  pub fn grow_capacity(&mut self, additional: Size) -> Result<(), Overflow> {
    let Some(additional) = NonZero::new(additional) else {
      return Ok(()); // `additional` is zero, so do nothing
    };

    let current_capacity = self.capacity;
    let Some(new_capacity) = current_capacity.checked_add(additional.get())
    else {
      return Err(Overflow {
        current_capacity,
        additional,
      });
    };

    self.capacity = new_capacity;
    self.free(Allocation {
      offset: current_capacity.get(),
      size: additional,
    });
    Ok(())
  }

  /// Try to re-size an existing allocation in-place
  ///
  /// Will not change the offset of the allocation and tries to expand the
  /// allocation to the right if there is sufficient free space.
  ///
  /// Returns:
  /// - `Ok(Allocation)` on success.
  /// - `Err(InsufficientSpace)` if there is not enough available space
  ///   to expand the allocation to `new_size`. In this case, the existing
  ///   allocation is left untouched.
  pub fn try_reallocate(
    &mut self,
    alloc: Allocation,
    new_size: Size,
  ) -> Result<Allocation, ReallocateError> {
    let Some(new_size) = NonZero::new(new_size) else {
      return Err(ReallocateError::Invalid);
    };

    match new_size.cmp(&alloc.size) {
      Ordering::Greater => {
        let required_additional = NonZero::new(new_size.get() - alloc.size())
          .unwrap_or_else(|| unreachable!());
        // find the next free-region;
        let Some(next_free) = self.following_free_region(alloc.offset) else {
          return Err(ReallocateError::InsufficientSpace {
            required_additional,
            available: 0,
          });
        };
        // Check that the free-region we found is actually contiguous with our
        // allocation, and that it has enough space
        if next_free.location != alloc.offset + alloc.size() {
          return Err(ReallocateError::InsufficientSpace {
            required_additional,
            available: 0,
          });
        }
        if next_free.size < required_additional {
          return Err(ReallocateError::InsufficientSpace {
            required_additional,
            available: next_free.size.get(),
          });
        }
        // all good, take what we need and return the rest
        let new_alloc = Allocation {
          offset: alloc.offset,
          size: new_size,
        };
        self.remove_free_region(next_free.location, next_free.size);
        if let Some(new_free_region_size) =
          NonZero::new(next_free.size.get() - required_additional.get())
        {
          self.insert_free_region(
            new_alloc.offset + new_alloc.size(),
            new_free_region_size,
          );
        }
        self.available -= required_additional.get();

        Ok(new_alloc)
      },
      Ordering::Less => {
        // free the additional space
        let additional = NonZero::new(alloc.size() - new_size.get())
          .unwrap_or_else(|| unreachable!());
        self.free(Allocation {
          offset: alloc.offset + alloc.size() - additional.get(),
          size: additional,
        });

        Ok(Allocation {
          offset: alloc.offset,
          size: new_size,
        })
      },
      Ordering::Equal => {
        // do nothing
        Ok(alloc)
      },
    }
  }

  /// Get the total capacity of the pool
  pub fn capacity(&self) -> Size {
    self.capacity.get()
  }

  /// Get the total available memory in this pool
  ///
  /// Note: The memory may be fragmented, so it may not be possible to allocate
  /// an object of this size.
  pub fn total_available(&self) -> Size {
    self.available
  }

  /// Get the size of the largest available memory region in this pool
  pub fn largest_available(&self) -> Size {
    self.free.last().map_or(0, |region| region.size.get())
  }

  /// Returns true if there are no allocations
  pub fn is_empty(&self) -> bool {
    self.capacity.get() == self.available
  }

  /// Returns an iterator over the unallocated regions
  ///
  /// This should be used **only** for gathering metadata about the internal
  /// state of the allocator for debugging purposes.
  ///
  /// You must not use this instead of allocating; subsequent calls to `alloc`
  /// will freely allocate from the reported regions.
  pub fn report_free_regions(
    &self,
  ) -> impl Iterator<Item = Allocation> + use<'_> {
    self.free.iter().map(|free_region| Allocation {
      offset: free_region.location,
      size: free_region.size,
    })
  }

  /// Try to find a region with at least `size`
  fn find_free_region(&mut self, size: NonZero<Size>) -> Option<FreeRegion> {
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
  fn remove_free_region(&mut self, location: Location, size: NonZero<Size>) {
    self.location_map.remove(&location);
    let region_existed = self.free.remove(&FreeRegion { location, size });

    assert!(
      region_existed,
      "tried to remove a FreeRegion which did not exist: {:?}",
      FreeRegion { location, size }
    );
  }

  /// add a region to the internal free lists
  fn insert_free_region(&mut self, location: Location, size: NonZero<Size>) {
    self.free.insert(FreeRegion { location, size });
    let existing_size = self.location_map.insert(location, size);

    assert!(
      existing_size.is_none(),
      "Double free. Tried to add {new:?}, but {existing:?} was already there",
      new = FreeRegion { location, size },
      existing = FreeRegion {
        location,
        size: existing_size.unwrap_or_else(|| unreachable!())
      }
    )
  }
}

impl fmt::Debug for Allocator {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Allocator")
      .field("capacity", &self.capacity)
      .field("total_available", &self.available)
      .field("largest_available", &self.largest_available())
      .finish()
  }
}

#[derive(Debug, Copy, Clone)]
pub struct Overflow {
  pub current_capacity: NonZero<Size>,
  pub additional: NonZero<Size>,
}
impl Error for Overflow {}
impl fmt::Display for Overflow {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_fmt(format_args!(
      "Overflow Error: Allocator with capacity {} could not grow by additional {}.",
      self.current_capacity, self.additional
    ))
  }
}

#[derive(Debug, Copy, Clone)]
pub enum ReallocateError {
  InsufficientSpace {
    required_additional: NonZero<Size>,
    available: Size,
  },
  Invalid,
}

impl Error for ReallocateError {}
impl fmt::Display for ReallocateError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      ReallocateError::InsufficientSpace {
        required_additional,
        available,
      } => f.write_fmt(format_args!(
        "InsufficientSpace Error: Unable to expand allocation: \
          required_additional:{required_additional}, available:{available}."
      )),
      ReallocateError::Invalid => {
        f.write_str("Invalid allocation or `new_size` was 0")
      },
    }
  }
}
