use ::orderly_allocator::{Allocation, Allocator};
use orderly_allocator::ReallocateError;

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
  let mut allocator = Allocator::new(1_000_000);
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
  let a = &allocator.alloc(ALLOC_SIZE).unwrap();
  let _b = allocator.alloc(3_000).unwrap();
  let _c = allocator.alloc(50_000).unwrap();
  allocator.free(_b);

  let initial_available = allocator.total_available();

  // try to grow alloc too much (error)
  {
    let err = allocator.try_reallocate(*a, a.size() + 10_000);
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
    let err = allocator.try_reallocate(*a, 0);
    assert!(matches!(err, Err(ReallocateError::Invalid)));
    assert_eq!(
      allocator.total_available(),
      initial_available,
      "Allocator doesn't alloc or free when failing to reallocate"
    );
  }

  // try to grow alloc (success)
  let new_size = ALLOC_SIZE + 1_000;
  let grown_a = allocator.try_reallocate(*a, new_size).unwrap();
  assert_eq!(grown_a.offset(), a.offset());
  assert_eq!(grown_a.size(), new_size);
  assert_eq!(
    allocator.total_available(),
    initial_available - 1_000,
    "Allocates additional space when reallocating"
  );
  #[allow(unused)]
  let a = (); // shadow a so we don't use the wrong thing below

  // try to shrink alloc
  let new_size = ALLOC_SIZE - 333;
  let shrunk_a = allocator.try_reallocate(grown_a, new_size).unwrap();
  assert_eq!(shrunk_a.offset(), grown_a.offset());
  assert_eq!(shrunk_a.size(), new_size);
  assert_eq!(
    allocator.total_available(),
    initial_available + 333,
    "Frees additional space when reallocating"
  );
  #[allow(unused)]
  let grown_a = ();
}
