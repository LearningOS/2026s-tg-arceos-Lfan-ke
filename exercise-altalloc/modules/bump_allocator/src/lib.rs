#![no_std]

use axallocator::{AllocError, AllocResult, BaseAllocator, ByteAllocator, PageAllocator};
use core::alloc::Layout;
use core::ptr::NonNull;

#[inline]
const fn align_up(pos: usize, align: usize) -> usize {
    (pos + align - 1) & !(align - 1)
}

#[inline]
const fn align_down(pos: usize, align: usize) -> usize {
    pos & !(align - 1)
}

/// Early memory allocator
/// Use it before formal bytes-allocator and pages-allocator can work!
/// This is a double-end memory range:
/// - Alloc bytes forward
/// - Alloc pages backward
///
/// [ bytes-used | avail-area | pages-used ]
/// |            | -->    <-- |            |
/// start       b_pos        p_pos       end
///
/// For bytes area, 'count' records number of allocations.
/// When it goes down to ZERO, free bytes-used area.
/// For pages area, it will never be freed!
///
pub struct EarlyAllocator<const PAGE_SIZE: usize> {
    /// Start of the managed region (inclusive).
    start: usize,
    /// End of the managed region (exclusive).
    end: usize,
    /// Forward bump pointer for the byte area.
    b_pos: usize,
    /// Backward bump pointer for the page area.
    p_pos: usize,
    /// Number of live byte allocations.
    count: usize,
}

impl<const PAGE_SIZE: usize> EarlyAllocator<PAGE_SIZE> {
    pub const fn new() -> Self {
        Self {
            start: 0,
            end: 0,
            b_pos: 0,
            p_pos: 0,
            count: 0,
        }
    }
}

impl<const PAGE_SIZE: usize> Default for EarlyAllocator<PAGE_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const PAGE_SIZE: usize> BaseAllocator for EarlyAllocator<PAGE_SIZE> {
    fn init(&mut self, start: usize, size: usize) {
        self.start = start;
        self.end = start + size;
        self.b_pos = start;
        self.p_pos = self.end;
        self.count = 0;
    }

    fn add_memory(&mut self, start: usize, size: usize) -> AllocResult {
        // A double-ended bump allocator manages a single contiguous range. We
        // can only usefully absorb a region directly adjacent to the current
        // one, growing it in place. A non-adjacent region is accepted (so
        // callers that add several disjoint free regions don't fail) but is
        // left unused — sound, because we never hand out an address outside
        // the tracked range.
        let end = start + size;
        if start == self.end {
            // Adjacent above: grow the top of the range.
            self.end = end;
            self.p_pos = end;
        } else if end == self.start && self.count == 0 {
            // Adjacent below, nothing allocated yet: grow the bottom.
            self.start = start;
            self.b_pos = start;
        }
        Ok(())
    }
}

impl<const PAGE_SIZE: usize> ByteAllocator for EarlyAllocator<PAGE_SIZE> {
    fn alloc(&mut self, layout: Layout) -> AllocResult<NonNull<u8>> {
        let start = align_up(self.b_pos, layout.align());
        let next = start + layout.size();
        // Out of memory if the forward pointer would cross the backward one.
        if next > self.p_pos {
            return Err(AllocError::NoMemory);
        }
        self.b_pos = next;
        self.count += 1;
        NonNull::new(start as *mut u8).ok_or(AllocError::NoMemory)
    }

    fn dealloc(&mut self, _pos: NonNull<u8>, _layout: Layout) {
        // Classic bump: only reclaim the byte area once everything is freed.
        self.count -= 1;
        if self.count == 0 {
            self.b_pos = self.start;
        }
    }

    fn total_bytes(&self) -> usize {
        self.end - self.start
    }

    fn used_bytes(&self) -> usize {
        self.b_pos - self.start
    }

    fn available_bytes(&self) -> usize {
        self.p_pos - self.b_pos
    }
}

impl<const PAGE_SIZE: usize> PageAllocator for EarlyAllocator<PAGE_SIZE> {
    const PAGE_SIZE: usize = PAGE_SIZE;

    fn alloc_pages(&mut self, num_pages: usize, align_pow2: usize) -> AllocResult<usize> {
        if align_pow2 == 0 || !align_pow2.is_power_of_two() {
            return Err(AllocError::InvalidParam);
        }
        let size = num_pages * PAGE_SIZE;
        // Bump the backward pointer downward, keeping the result aligned.
        let new_pos = align_down(self.p_pos - size, align_pow2);
        // Out of memory if the backward pointer would cross the forward one.
        if new_pos < self.b_pos {
            return Err(AllocError::NoMemory);
        }
        self.p_pos = new_pos;
        Ok(new_pos)
    }

    fn dealloc_pages(&mut self, _pos: usize, _num_pages: usize) {
        // Pages are never freed in this early allocator.
    }

    fn alloc_pages_at(
        &mut self,
        _base: usize,
        _num_pages: usize,
        _align_pow2: usize,
    ) -> AllocResult<usize> {
        Err(AllocError::InvalidParam)
    }

    fn total_pages(&self) -> usize {
        (self.end - self.start) / PAGE_SIZE
    }

    fn used_pages(&self) -> usize {
        (self.end - self.p_pos) / PAGE_SIZE
    }

    fn available_pages(&self) -> usize {
        (self.p_pos - self.b_pos) / PAGE_SIZE
    }
}
