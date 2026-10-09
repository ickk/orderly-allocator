#![doc = include_str!("../README.md")]
#![no_std]
extern crate alloc;
use {
  ::alloc::collections::{BTreeMap, BTreeSet},
  ::core::{cmp::Ordering, error::Error, fmt, ops::Range},
};

use private::{NonZeroSize, Size};

/// The non-zero counterpart of `S`, e.g. `core::num::NonZero<u32>` for `u32`
type NonZero<S> = <S as Size>::NonZero;

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
pub struct Allocation<S: Size = u32> {
  /// The location of this allocation within the buffer
  pub offset: S,
  /// The size of this allocation
  pub size: NonZero<S>,
}

impl<S: Size> Allocation<S> {
  /// Get the offset of the allocation
  ///
  /// This is just a wrapper for `allocation.offset` for symmetry with `size`.
  pub fn offset(&self) -> S {
    self.offset
  }

  /// Get the size of the allocation
  ///
  /// This is just sugar for `allocation.size.get()`.
  pub fn size(&self) -> S {
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
  ///   size: NonZero::<S>::new(4).unwrap()
  /// };
  ///
  /// let region = &buffer[allocation.range()];
  ///
  /// assert_eq!(region, &[25, 26, 27, 28]);
  /// ```
  pub fn range(&self) -> Range<usize> {
    self.offset.as_usize()..(self.offset + self.size.get()).as_usize()
  }
}

/// A super-simple soft-realtime allocator for managing an external pool of
/// memory
///
/// The type used to measure offsets and sizes defaults to `u32`, but any
/// unsigned integer type can be used.
#[derive(Clone)]
pub struct Allocator<S: Size = u32> {
  /// An ordered collection of free-regions, sorted primarily by size, then by
  /// location
  free: BTreeSet<FreeRegion<S>>,
  /// An ordered collection of free-regions, sorted by location
  location_map: BTreeMap<S, NonZero<S>>,
  /// The total capacity
  capacity: NonZero<S>,
  /// The amount of free memory
  available: S,
}

// This type has an explicit implementation of Ord, since we rely on properties
// of its behaviour to find and select free regions.
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
struct FreeRegion<S: Size> {
  location: S,
  size: NonZero<S>,
}

impl<S: Size> PartialOrd for FreeRegion<S> {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl<S: Size> Ord for FreeRegion<S> {
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

impl<S: Size> Allocator<S> {
  /// Create a new allocator to manage a pool of memory
  ///
  /// Panics:
  /// - Panics if `capacity == 0`
  pub fn new(capacity: S) -> Self {
    let capacity = NonZero::<S>::new(capacity).expect("`capacity == 0`");

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
  pub fn alloc(&mut self, size: S) -> Option<Allocation<S>> {
    self.alloc_with_align(size, S::ONE)
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
    size: S,
    align: S,
  ) -> Option<Allocation<S>> {
    let size = NonZero::<S>::new(size)?;
    let align = NonZero::<S>::new(align)?;

    let FreeRegion {
      location: mut free_region_location,
      size: free_region_size,
    } = self.find_free_region(size.checked_add(align.get() - S::ONE)?)?;

    self.remove_free_region(free_region_location, free_region_size);

    let mut free_region_size = free_region_size.get();

    if let Some(misalignment) = NonZero::<S>::new(
      (align.get() - (free_region_location % align.get())) % align.get(),
    ) {
      self.insert_free_region(free_region_location, misalignment);
      free_region_location += misalignment.get();
      free_region_size -= misalignment.get();
    }

    if let Some(size_leftover) =
      NonZero::<S>::new(free_region_size - size.get())
    {
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
  pub fn free(&mut self, alloc: Allocation<S>) {
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
    self.insert_free_region(S::ZERO, self.capacity);
  }

  /// Add new free space at the end of the allocator
  ///
  /// Returns `Err(Overflow)` if `self.capacity + additional` would overflow.
  pub fn grow_capacity(&mut self, additional: S) -> Result<(), Overflow<S>> {
    let Some(additional) = NonZero::<S>::new(additional) else {
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
    alloc: Allocation<S>,
    new_size: S,
  ) -> Result<Allocation<S>, ReallocateError<S>> {
    let Some(new_size) = NonZero::<S>::new(new_size) else {
      return Err(ReallocateError::Invalid);
    };

    match new_size.cmp(&alloc.size) {
      Ordering::Greater => {
        let required_additional =
          NonZero::<S>::new(new_size.get() - alloc.size())
            .unwrap_or_else(|| unreachable!());
        // find the next free-region;
        let Some(next_free) = self.following_free_region(alloc.offset) else {
          return Err(ReallocateError::InsufficientSpace {
            required_additional,
            available: S::ZERO,
          });
        };
        // Check that the free-region we found is actually contiguous with our
        // allocation, and that it has enough space
        if next_free.location != alloc.offset + alloc.size() {
          return Err(ReallocateError::InsufficientSpace {
            required_additional,
            available: S::ZERO,
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
        if let Some(size_leftover) =
          NonZero::<S>::new(next_free.size.get() - required_additional.get())
        {
          self.insert_free_region(
            new_alloc.offset + new_alloc.size(),
            size_leftover,
          );
        }
        self.available -= required_additional.get();

        Ok(new_alloc)
      },
      Ordering::Less => {
        // free the additional space
        let additional = NonZero::<S>::new(alloc.size() - new_size.get())
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
  pub fn capacity(&self) -> S {
    self.capacity.get()
  }

  /// Get the total available memory in this pool
  ///
  /// Note: The memory may be fragmented, so it may not be possible to allocate
  /// an object of this size.
  pub fn total_available(&self) -> S {
    self.available
  }

  /// Get the size of the largest available memory region in this pool
  pub fn largest_available(&self) -> S {
    self.free.last().map_or(S::ZERO, |region| region.size.get())
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
  ) -> impl Iterator<Item = Allocation<S>> + use<'_, S> {
    self.free.iter().map(|free_region| Allocation {
      offset: free_region.location,
      size: free_region.size,
    })
  }

  /// Try to find a region with at least `size`
  fn find_free_region(&mut self, size: NonZero<S>) -> Option<FreeRegion<S>> {
    self
      .free
      .range(
        FreeRegion {
          size,
          location: S::ZERO,
        }..,
      )
      .copied()
      .next()
  }

  /// Get the first free-region before `location`
  fn previous_free_region(&self, location: S) -> Option<FreeRegion<S>> {
    self
      .location_map
      .range(..location)
      .next_back()
      .map(|(&location, &size)| FreeRegion { location, size })
  }

  /// Get the first free-region after `location`
  fn following_free_region(&self, location: S) -> Option<FreeRegion<S>> {
    use ::core::ops::Bound as B;
    self
      .location_map
      .range((B::Excluded(location), B::Unbounded))
      .next()
      .map(|(&location, &size)| FreeRegion { location, size })
  }

  /// remove a region from the internal free lists
  fn remove_free_region(&mut self, location: S, size: NonZero<S>) {
    self.location_map.remove(&location);
    let region_existed = self.free.remove(&FreeRegion { location, size });

    assert!(
      region_existed,
      "tried to remove a FreeRegion which did not exist: {:?}",
      FreeRegion { location, size }
    );
  }

  /// add a region to the internal free lists
  fn insert_free_region(&mut self, location: S, size: NonZero<S>) {
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

impl<S: Size> fmt::Debug for Allocator<S> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Allocator")
      .field("capacity", &self.capacity)
      .field("total_available", &self.available)
      .field("largest_available", &self.largest_available())
      .finish()
  }
}

#[derive(Debug, Copy, Clone)]
pub struct Overflow<S: Size = u32> {
  pub current_capacity: NonZero<S>,
  pub additional: NonZero<S>,
}
impl<S: Size> Error for Overflow<S> {}
impl<S: Size> fmt::Display for Overflow<S> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_fmt(format_args!(
      "Overflow Error: Allocator with capacity {} could not grow by additional {}.",
      self.current_capacity, self.additional
    ))
  }
}

#[derive(Debug, Copy, Clone)]
pub enum ReallocateError<S: Size = u32> {
  InsufficientSpace {
    required_additional: NonZero<S>,
    available: S,
  },
  Invalid,
}

impl<S: Size> Error for ReallocateError<S> {}
impl<S: Size> fmt::Display for ReallocateError<S> {
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

/// These traits are public so they can appear in the bounds of the public
/// types, but they live in a private module so they cannot be named (or
/// implemented) outside of this crate.
mod private {
  use ::core::{
    fmt,
    hash::Hash,
    ops::{Add, AddAssign, Rem, Sub, SubAssign},
  };

  /// An unsigned integer type that can be used to measure sizes and offsets
  ///
  /// This trait is implemented for all of the primitive unsigned integer types
  /// (`u8`, `u16`, `u32`, `u64`, `u128` and `usize`). It can also be implemented
  /// for other integer-like types, provided they have a non-zero counterpart
  /// (see `Size::NonZero`).
  pub trait Size:
    Copy
    + Ord
    + Hash
    + fmt::Debug
    + fmt::Display
    + Add<Output = Self>
    + Sub<Output = Self>
    + Rem<Output = Self>
    + AddAssign
    + SubAssign
  {
    /// The value `0`
    const ZERO: Self;
    /// The value `1`
    const ONE: Self;

    /// The non-zero counterpart of this type
    ///
    /// [`Allocation`] stores its size in this form so that it has a niche.
    type NonZero: NonZeroSize<Size = Self>;

    /// Convert to a `usize`, as if by an `as` cast
    fn as_usize(self) -> usize;
  }

  /// The non-zero counterpart of a `Size`
  ///
  /// This mirrors the interface of `core::num::NonZero`, which cannot be used
  /// generically on stable Rust. It is implemented for each `NonZero<T>` whose
  /// `T` implements `Size`.
  pub trait NonZeroSize:
    Copy + Ord + Hash + fmt::Debug + fmt::Display
  {
    /// The underlying `Size`
    type Size: Size<NonZero = Self>;

    /// Create a non-zero value if `n` is non-zero
    fn new(n: Self::Size) -> Option<Self>;

    /// Get the underlying value
    fn get(self) -> Self::Size;

    /// Add to the value, returning `None` if the result overflows
    fn checked_add(self, other: Self::Size) -> Option<Self>;
  }

  macro_rules! impl_size {
    ($($ty:ty),* $(,)?) => {$(
      impl Size for $ty {
        const ZERO: Self = 0;
        const ONE: Self = 1;

        type NonZero = ::core::num::NonZero<$ty>;

        fn as_usize(self) -> usize {
          self as usize
        }
      }

      impl NonZeroSize for ::core::num::NonZero<$ty> {
        type Size = $ty;

        fn new(n: $ty) -> Option<Self> {
          <::core::num::NonZero<$ty>>::new(n)
        }

        fn get(self) -> $ty {
          <::core::num::NonZero<$ty>>::get(self)
        }

        fn checked_add(self, other: $ty) -> Option<Self> {
          <::core::num::NonZero<$ty>>::checked_add(self, other)
        }
      }
    )*};
  }

  impl_size!(u8, u16, u32, u64, u128, usize);
}
