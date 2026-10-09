use ::orderly_allocator::{Allocation, Allocator, ReallocateError};

#[test]
fn allocaton_type_size() {
  assert_eq!(
    size_of::<Allocation>(),
    size_of::<u64>(),
    "`Allocation` has the size of a `u64`"
  );
  assert_eq!(
    size_of::<Option<Allocation>>(),
    size_of::<Allocation>(),
    "`Allocation` includes a niche"
  );
}

#[test]
fn allocation_size_and_align() {
  let mut allocator = Allocator::<u32>::new(1_000_000);
  {
    let a = allocator.alloc(59).unwrap();
    assert_eq!(a.size(), 59, "Allocation size is as requested");
  }
  {
    let b = allocator.alloc_with_align(10_000, 8).unwrap();
    assert_eq!(b.size(), 10_000, "Allocation size is as requested");
    assert_eq!(b.offset() % 8, 0, "Allocation align is as requested");
  }
}

#[test]
fn available() {
  const CAPACITY: u32 = 10_000_000;
  let mut allocator = Allocator::new(CAPACITY);

  assert_eq!(allocator.total_available(), CAPACITY);
  assert_eq!(
    allocator.largest_available(),
    allocator.total_available(),
    "A new allocator has a free region available the size of entire capacity"
  );

  {
    let a = allocator.alloc(1_000).unwrap();
    assert_eq!(
      allocator.total_available(),
      CAPACITY - 1_000,
      "Allocating consumes a range of the given size from the allocator"
    );
    assert_eq!(
      allocator.largest_available(),
      CAPACITY - 1_000,
      "The first allocation is at the edge of the pool"
    );

    allocator.free(a);
    assert_eq!(
      allocator.total_available(),
      CAPACITY,
      "Freeing an allocation returns it's range to the allocator"
    );
    assert_eq!(
      allocator.largest_available(),
      CAPACITY,
      "Freeing the only allocation returns it's range to the only free region"
    );
  }
}

#[test]
fn coalesce() {
  // start with an empty allocator
  // [------------------------------free-------------------------------------]
  const CAPACITY: u32 = 10_000_000;
  let mut allocator = Allocator::new(CAPACITY);

  // allocate some things of various sizes
  // [-------large------][small-][--medium--][--------------free--------------]
  let large = allocator.alloc(CAPACITY / 2).unwrap();
  let small = allocator.alloc(3_000).unwrap();
  let medium = allocator.alloc(50_000).unwrap();
  assert_eq!(
    allocator.total_available(),
    CAPACITY - large.size() - small.size() - medium.size(),
    "Consumes space from the allocator"
  );
  assert_eq!(
    allocator.largest_available(),
    CAPACITY - large.size() - small.size() - medium.size(),
    "Groups successive allocations when possible to maximise size \
      of free regions"
  );

  // after freeing `small`
  // [-------large------][-free-][--medium--][--------------free--------------]
  allocator.free(small);
  assert_eq!(
    allocator.total_available(),
    CAPACITY - large.size() - medium.size(),
    "Recovers space when freeing allocation"
  );
  assert_eq!(
    allocator.largest_available(),
    CAPACITY - large.size() - small.size() - medium.size(),
    "Floating free region when free'd allocation was girt by two living \
    allocations"
  );

  // after freeing `large`
  // [-----------free-----------][--medium--][--------------free--------------]
  allocator.free(large);
  assert_eq!(
    allocator.total_available(),
    CAPACITY - medium.size(),
    "Recovers space when freeing allocation"
  );
  assert_eq!(
    allocator.largest_available(),
    large.size() + small.size(),
    "Coalesces neighbouring free regions when freeing"
  );

  // after freeing `medium`
  // [-------------------------------free-------------------------------------]
  allocator.free(medium);
  assert_eq!(
    allocator.total_available(),
    CAPACITY,
    "Recovers space when freeing allocation"
  );
  assert_eq!(
    allocator.largest_available(),
    CAPACITY,
    "Coalesces neighbouring free regions when freeing"
  );
}

#[test]
fn reset() {
  const CAPACITY: u32 = 10_000_000;
  let mut allocator = Allocator::new(CAPACITY);

  let large = allocator.alloc(CAPACITY / 2).unwrap();
  let small = allocator.alloc(3_000).unwrap();
  let medium = allocator.alloc(50_000).unwrap();
  assert_eq!(
    allocator.total_available(),
    CAPACITY - large.size() - small.size() - medium.size(),
    "Consumes space from the allocator"
  );

  allocator.reset();
  assert_eq!(
    allocator.total_available(),
    CAPACITY,
    "Reset recovers space"
  );
  assert_eq!(
    allocator.largest_available(),
    CAPACITY,
    "Reset recovers space"
  );
}

#[test]
fn grow_capacity() {
  const CAPACITY: u32 = 10_000_000;
  let mut allocator = Allocator::new(CAPACITY);

  const ADDITIONAL_CAPACITY: u32 = 5_000_000;
  allocator.grow_capacity(ADDITIONAL_CAPACITY).unwrap();
  assert_eq!(
    allocator.capacity(),
    CAPACITY + ADDITIONAL_CAPACITY,
    "grow_capacity adds capacity"
  );
  assert_eq!(
    allocator.total_available(),
    CAPACITY + ADDITIONAL_CAPACITY,
    "grow_capacity adds capacity"
  );
  assert_eq!(
    allocator.largest_available(),
    CAPACITY + ADDITIONAL_CAPACITY,
    "grow_capacity coalesces additional capacity"
  );
}

#[test]
fn try_reallocate() {
  // create an allocator with some free-space after an allocation
  // [-------alloc------][-free-][----c----][--------------free--------------]
  const CAPACITY: u32 = 10_000_000;
  const ALLOC_SIZE: u32 = 50_000;
  let mut allocator = Allocator::new(CAPACITY);
  let a = allocator.alloc(ALLOC_SIZE).unwrap();
  let _b = allocator.alloc(3_000).unwrap();
  let _c = allocator.alloc(50_000).unwrap();
  allocator.free(_b);

  let initial_available = allocator.total_available();

  // try to grow alloc too much (error)
  {
    let err = allocator.try_reallocate(a, a.size() + 10_000);
    assert!(matches!(
      err,
      Err(ReallocateError::InsufficientSpace { .. })
    ));
    assert_eq!(
      allocator.total_available(),
      initial_available,
      "Allocator doesn't alloc or free when failing to reallocate"
    );
  }

  // try to shrink alloc too much (error)
  {
    let err = allocator.try_reallocate(a, 0);
    assert!(matches!(err, Err(ReallocateError::Invalid)));
    assert_eq!(
      allocator.total_available(),
      initial_available,
      "Allocator doesn't alloc or free when failing to reallocate"
    );
  }

  // try to grow alloc (success)
  let a = {
    let new_size = ALLOC_SIZE + 1_000;
    let new_a = allocator.try_reallocate(a, new_size).unwrap();
    assert_eq!(new_a.offset(), a.offset());
    assert_eq!(new_a.size(), new_size);
    assert_eq!(
      allocator.total_available(),
      initial_available - 1_000,
      "Allocates additional space when reallocating"
    );

    new_a // shadow `a` so we don't use the wrong thing below
  };

  // try to grow alloc just enough (success)
  let a = {
    let new_size = ALLOC_SIZE + 3_000;
    let new_a = allocator.try_reallocate(a, new_size).unwrap();
    assert_eq!(new_a.offset(), a.offset());
    assert_eq!(new_a.size(), new_size);
    assert_eq!(
      allocator.total_available(),
      initial_available - 3_000,
      "Allocates additional space when reallocating"
    );
    new_a
  };

  // try to shrink alloc
  #[allow(unused)]
  let a = {
    let new_size = ALLOC_SIZE - 333;
    let new_a = allocator.try_reallocate(a, new_size).unwrap();
    assert_eq!(new_a.offset(), a.offset());
    assert_eq!(new_a.size(), new_size);
    assert_eq!(
      allocator.total_available(),
      initial_available + 333,
      "Frees additional space when reallocating"
    );
    new_a
  };
}

/// Exercise alloc, alignment, coalescing, grow, and reallocate for a size type
macro_rules! exercise {
  ($ty:ty, $capacity:expr) => {{
    type S = $ty;
    let capacity: S = $capacity;

    let mut allocator = Allocator::new(capacity);
    assert_eq!(allocator.capacity(), capacity);
    assert_eq!(allocator.largest_available(), capacity);

    let a = allocator.alloc(10).unwrap();
    let b = allocator.alloc_with_align(10, 8).unwrap();
    assert_eq!(a.size(), 10);
    assert_eq!(b.offset() % 8, 0);
    assert!(allocator.alloc(0).is_none());
    assert!(allocator.alloc_with_align(1, 0).is_none());

    let a = allocator.try_reallocate(a, 5).unwrap();
    assert_eq!(a.size(), 5);
    assert!(matches!(
      allocator.try_reallocate(a, 0),
      Err(ReallocateError::Invalid)
    ));

    allocator.free(a);
    allocator.free(b);
    assert!(allocator.is_empty());
    assert_eq!(allocator.largest_available(), capacity);
    assert_eq!(allocator.report_free_regions().count(), 1);

    allocator.grow_capacity(20).unwrap();
    assert_eq!(allocator.capacity(), capacity + 20);
    assert_eq!(allocator.total_available(), capacity + 20);
    assert_eq!(allocator.report_free_regions().count(), 1);

    allocator.reset();
    assert!(allocator.is_empty());
  }};
}

#[test]
fn generic_sizes() {
  exercise!(u8, 100);
  exercise!(u16, 1_000);
  exercise!(u32, 1_000);
  exercise!(u64, 1_000);
  exercise!(u128, 1_000);
  exercise!(usize, 1_000);
}

#[test]
fn generic_niche_and_size() {
  use core::mem::size_of;
  assert_eq!(size_of::<Allocation<u8>>(), 2);
  assert_eq!(size_of::<Option<Allocation<u8>>>(), 2);
  assert_eq!(size_of::<Allocation<u64>>(), 16);
  assert_eq!(size_of::<Option<Allocation<u64>>>(), 16);
}

#[test]
fn generic_overflow() {
  let mut allocator = Allocator::<u8>::new(200);
  assert!(allocator.grow_capacity(100).is_err());
  assert_eq!(allocator.capacity(), 200, "Capacity unchanged on overflow");
  assert!(allocator.alloc_with_align(100, 200).is_none());
}

#[test]
fn range_generic() {
  let mut allocator = Allocator::<u16>::new(100);
  allocator.alloc(3).unwrap();
  let b = allocator.alloc(4).unwrap();
  assert_eq!(b.range(), 3..7);
}
