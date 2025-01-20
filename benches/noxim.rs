// https://gist.github.com/Noxime/4189986317953dc8353032f35c9a5e8a
use std::{hint::black_box, time::Instant};

use rand::{Rng, SeedableRng};

type RangeAlloc = range_alloc::RangeAllocator<u32>;
type OrderlyAlloc = orderly_allocator::Allocator;
type OffsetAlloc = offset_allocator::Allocator;

trait Allocator {
  type Allocation;
  fn with_capacity(capacity: u32) -> Self;

  fn allocate(&mut self, size: u32) -> Self::Allocation;
  fn deallocate(&mut self, allocation: Self::Allocation);
}

impl Allocator for RangeAlloc {
  type Allocation = std::ops::Range<u32>;

  fn with_capacity(capacity: u32) -> Self {
    RangeAlloc::new(0..capacity)
  }

  fn allocate(&mut self, size: u32) -> Self::Allocation {
    self.allocate_range(size).unwrap()
  }

  fn deallocate(&mut self, allocation: Self::Allocation) {
    self.free_range(allocation);
  }
}

impl Allocator for OrderlyAlloc {
  type Allocation = orderly_allocator::Allocation;

  fn with_capacity(capacity: u32) -> Self {
    OrderlyAlloc::new(capacity)
  }

  fn allocate(&mut self, size: u32) -> Self::Allocation {
    self.alloc(size).unwrap()
  }

  fn deallocate(&mut self, allocation: Self::Allocation) {
    self.free(allocation);
  }
}

impl Allocator for OffsetAlloc {
  type Allocation = offset_allocator::Allocation;

  fn with_capacity(capacity: u32) -> Self {
    OffsetAlloc::new(capacity)
  }

  fn allocate(&mut self, size: u32) -> Self::Allocation {
    self.allocate(size).unwrap()
  }

  fn deallocate(&mut self, allocation: Self::Allocation) {
    self.free(allocation);
  }
}

/// This type disables constant folding optimizations for the wrapped allocator.
struct Blacked<A>(A);

impl<A: Allocator> Allocator for Blacked<A> {
  type Allocation = A::Allocation;

  fn with_capacity(capacity: u32) -> Self {
    let capacity = black_box(capacity);
    Blacked(A::with_capacity(capacity))
  }

  fn allocate(&mut self, size: u32) -> Self::Allocation {
    let size = black_box(size);
    self.0.allocate(size)
  }

  fn deallocate(&mut self, allocation: Self::Allocation) {
    let allocation = black_box(allocation);
    self.0.deallocate(allocation)
  }
}

fn bench_fill_free<A: Allocator>() {
  let capacity = 100_000;
  let mut allocations = Vec::with_capacity(capacity as usize);

  let start = Instant::now();

  let mut alloc = Blacked::<A>::with_capacity(capacity * 2);

  for _ in 0..capacity {
    let a = alloc.allocate(1);
    allocations.push(a);
  }

  for a in allocations {
    alloc.deallocate(a);
  }

  let elapsed = start.elapsed();
  println!("{} took {elapsed:?}", std::any::type_name::<A>());
}

fn bench_random<A: Allocator>() {
  let mut allocations = vec![];
  let mut rng = rand::rngs::SmallRng::from_seed([0xAF; 32]);

  let start = Instant::now();

  let mut alloc = Blacked::<A>::with_capacity(1000000);

  for _ in 0..10000 {
    // Allocate some random sizes
    for _ in 0..rng.gen_range(1..10) {
      let size = rng.gen_range(1..1000);
      let a = alloc.allocate(size);
      allocations.push(a);
    }

    // Deallocate some random allocations
    for _ in 0..rng.gen_range(1..10).min(allocations.len()) {
      let idx = rng.gen_range(0..allocations.len());
      let a = allocations.swap_remove(idx);
      alloc.deallocate(a);
    }
  }

  for a in allocations {
    alloc.deallocate(a);
  }

  let elapsed = start.elapsed();
  println!("{} took {elapsed:?}", std::any::type_name::<A>());
}

fn main() {
  println!("= fill free =");
  bench_fill_free::<RangeAlloc>();
  bench_fill_free::<OrderlyAlloc>();
  bench_fill_free::<OffsetAlloc>();

  println!("= random =");
  bench_random::<RangeAlloc>();
  bench_random::<OrderlyAlloc>();
  bench_random::<OffsetAlloc>();
}
