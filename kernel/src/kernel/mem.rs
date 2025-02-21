//! Memory allocation.

use core::{
    alloc::{Allocator, GlobalAlloc},
    fmt::Debug,
    mem::MaybeUninit,
    num::NonZero,
    ops::Range,
    ptr::{NonNull, null_mut},
};

use crate::boot::{MemoryMap, MemoryType};

use aphrodite_proc_macros::*;

#[derive(Clone, Copy)]
struct Allocation {
    /// Whether this allocation is used. This is used so that the
    /// entire allocation table doesn't need to be shifted back
    /// on every allocation.
    pub used: bool,
    /// The starting address of the allocation.
    pub addr: u64,
    /// The length of the allocation.
    pub len: u64,
}

#[derive(Clone, Copy)]
struct AllocationHeader {
    /// Whether this allocation table is used. Kept for parity with [Allocation]s.
    #[allow(dead_code)]
    pub used: bool,
    /// The starting address of the allocation table.
    #[allow(dead_code)]
    pub addr: u64,
    /// The length in bytes of the allocation table.
    pub len: u64,
    /// The number of allocations in the allocation table.
    pub num_allocations: u64,
}

struct AllocationIter {
    ptr: *const Allocation,
    num_allocations: u64,
    idx: u64,
}

impl Iterator for AllocationIter {
    type Item = *mut Allocation;
    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        self.idx += 1;
        if self.idx > self.num_allocations {
            return None;
        }
        crate::arch::output::sdebugsln("Providing allocation from iterator");

        Some(&unsafe {
            *((self.ptr as usize + (size_of::<Allocation>() * (self.idx as usize - 1)))
                as *const Allocation)
        } as *const Allocation as *mut Allocation)
    }
}

#[global_allocator]
static mut ALLOCATOR: MaybeMemoryMapAlloc<'static> = MaybeMemoryMapAlloc::new(None);
static mut ALLOCATOR_MEMMAP: MaybeUninit<MemoryMap> = MaybeUninit::uninit();
static mut ALLOCATOR_INITALIZED: bool = false;

#[kernel_item(MemMapAlloc)]
fn get_allocator() -> Option<&'static MemoryMapAlloc<'static>> {
    if unsafe { ALLOCATOR_INITALIZED } {
        #[allow(static_mut_refs)]
        return Some(unsafe { ALLOCATOR.assume_init_ref() });
    } else {
        return None;
    }
}

/// The unsafe counterpart of [MemMapAlloc()]. Doesn't check if the allocator is initalized.
/// Internally, uses [MaybeUninit::assume_init_ref].
///
/// # Safety
///
/// Calling this instead of [MemMapAlloc] or when the allocator is uninitalized causes
/// undefined behavior; check [MaybeUninit::assume_init_ref] for safety guarantees.
pub unsafe fn get_allocator_unchecked() -> &'static MemoryMapAlloc<'static> {
    #[allow(static_mut_refs)]
    unsafe {
        ALLOCATOR.assume_init_ref()
    }
}

#[kernel_item(MemMapAllocInit)]
fn memory_map_alloc_init(memmap: crate::boot::MemoryMap) -> Result<(), crate::Error<'static>> {
    #[allow(static_mut_refs)]
    unsafe {
        ALLOCATOR_MEMMAP.write(memmap);
    }
    #[allow(static_mut_refs)]
    let alloc = MemoryMapAlloc::new(unsafe { ALLOCATOR_MEMMAP.assume_init_mut() })?;

    unsafe {
        #[allow(static_mut_refs)]
        ALLOCATOR.add_alloc(alloc);
    }
    unsafe {
        ALLOCATOR_INITALIZED = true;
    }

    Ok(())
}

/// A implementation of a physical memory allocator that uses a [crate::boot::MemoryMap].
pub struct MemoryMapAlloc<'a> {
    /// The memory map to use to allocate memory.
    pub memory_map: &'a mut crate::boot::MemoryMap,

    allocationheader: *mut AllocationHeader,
    allocations: *mut Allocation,
    max_allocations_size: u64,
}

/// Too many allocations have been created, pushing the size of [MemoryMapAlloc::allocations] over [MemoryMapAlloc::max_allocations_size].
pub const TOO_MANY_ALLOCATIONS: i16 = -2;

/// There isn't enough space for 32 allocations(the minimum available).
pub const ALLOCATIONS_NOT_ENOUGH_SPACE: i16 = -3;

/// The index provided to [MemoryMapAlloc::extend_allocation] is too big.
pub const EXTEND_ALLOCATION_INVALID_INDEX: i16 = -4;

/// The allocation provided to [MemoryMapAlloc::extend_allocation] is unused.
pub const EXTEND_ALLOCATION_ALLOCATION_UNUSED: i16 = -5;

/// The allocation provided to [MemoryMapAlloc::extend_allocation], if extended, would extend into another allocation.
pub const EXTEND_ALLOCATION_OTHER_ALLOCATION: i16 = -6;

impl<'a> Debug for MemoryMapAlloc<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("MemoryMapAlloc with ")?;
        f.write_str(
            core::str::from_utf8(&crate::u64_as_u8_slice(
                unsafe { *self.allocationheader }.num_allocations,
            ))
            .unwrap(),
        )?;
        f.write_str(" allocations")?;
        Ok(())
    }
}

impl<'a> MemoryMapAlloc<'a> {
    /// Creates a new [MemoryMapAlloc]. Please call this method instead of creating it manually!
    ///
    /// This method internally stores the memory map in the outputted MemoryMapAlloc.
    ///
    /// Note that this function will return an error only if there isn't enough allocatable space
    /// for at least 32 allocations.
    pub fn new(
        memory_map: &'a mut crate::boot::MemoryMap,
    ) -> Result<MemoryMapAlloc<'a>, crate::Error<'a>> {
        let mut out = MemoryMapAlloc {
            memory_map,
            allocations: core::ptr::null_mut(),
            allocationheader: core::ptr::null_mut(),
            max_allocations_size: 0,
        };
        out.memory_map.reset_iter();
        for mapping in &mut *out.memory_map {
            mapping.output();
            crate::arch::output::sdebugsnpln("");
            if mapping.len < (size_of::<Allocation>() * 32) as u64 {
                continue;
            }
            if mapping.mem_type == MemoryType::Free {
                out.allocationheader = core::ptr::from_raw_parts_mut(
                    core::ptr::without_provenance_mut::<()>(mapping.start as usize),
                    (),
                );
                out.allocations = core::ptr::from_raw_parts_mut(
                    core::ptr::without_provenance_mut::<()>(
                        mapping.start as usize + size_of::<AllocationHeader>(),
                    ),
                    (),
                );
                out.max_allocations_size = mapping.len;
            } else if let MemoryType::HardwareSpecific(_, allocatable) = mapping.mem_type {
                if allocatable {
                    out.allocationheader = core::ptr::from_raw_parts_mut(
                        core::ptr::without_provenance_mut::<()>(mapping.start as usize),
                        (),
                    );
                    out.allocations = core::ptr::from_raw_parts_mut(
                        core::ptr::without_provenance_mut::<()>(
                            mapping.start as usize + size_of::<AllocationHeader>(),
                        ),
                        (),
                    );
                    out.max_allocations_size = mapping.len;
                }
            }
        }
        if out.allocations == core::ptr::null_mut() {
            return Err(crate::Error::new(
                "no free memory with space for 32 allocations",
                ALLOCATIONS_NOT_ENOUGH_SPACE,
            ));
        }
        unsafe {
            (*out.allocations) = Allocation {
                used: false,
                addr: 0,
                len: 0,
            };
            (*out.allocationheader) = AllocationHeader {
                used: true,
                addr: out.allocations as usize as u64,
                len: (size_of::<Allocation>() * 32) as u64,
                num_allocations: 1,
            }
        }
        Ok(out)
    }

    /// Returns the number of allocations.
    pub fn number_of_allocations(&self) -> u64 {
        unsafe { *self.allocationheader }.num_allocations
    }

    /// Creates a [AllocationIter] to iterate over the current allocations.
    fn allocations_iter(&self) -> AllocationIter {
        AllocationIter {
            ptr: self.allocations,
            num_allocations: unsafe { *self.allocationheader }.num_allocations,
            idx: 0,
        }
    }

    /// Add an allocation to [MemoryMapAlloc::allocations]. It will overwrite allocations with `used` set to false.
    fn add_allocation(&self, allocation: Allocation) -> Result<(), crate::Error<'static>> {
        if !allocation.used {
            crate::arch::output::swarningsln("Adding unused allocation");
        }
        for alloc in self.allocations_iter() {
            if !unsafe { *alloc }.used {
                unsafe { (*alloc) = allocation }
                return Ok(());
            }
        }

        unsafe { *self.allocationheader }.num_allocations += 1;

        let num_allocations = unsafe { *self.allocationheader }.num_allocations;

        if unsafe { *self.allocations }.len < (size_of::<Allocation>() as u64 * (num_allocations)) {
            if unsafe { *self.allocationheader }.len + size_of::<Allocation>() as u64
                >= self.max_allocations_size
            {
                return Err(crate::Error::new(
                    "not enough space for another allocation",
                    TOO_MANY_ALLOCATIONS,
                ));
            }

            let res = self.extend_allocation_header(size_of::<Allocation>() as u64);
            if let Err(err) = res {
                unsafe { *self.allocationheader }.num_allocations -= 1;
                return Err(err);
            }
        }

        let new_alloc = (self.allocations as usize
            + (size_of::<Allocation>() * (num_allocations) as usize))
            as *const Allocation as *mut Allocation;

        unsafe { (*new_alloc) = allocation }

        Ok(())
    }

    /// Extend an allocation. This has numerous checks, so please use this
    /// instead of manually changing [Allocation::len]!
    fn extend_allocation(&self, idx: u64, by: u64) -> Result<(), crate::Error<'static>> {
        if idx > unsafe { *self.allocationheader }.num_allocations {
            return Err(crate::Error::new(
                "the index provided to extend_allocation is too large",
                EXTEND_ALLOCATION_INVALID_INDEX,
            ));
        }
        let alloc = (self.allocations as usize + (size_of::<Allocation>() * idx as usize))
            as *const Allocation as *mut Allocation;

        if !unsafe { *alloc }.used {
            return Err(crate::Error::new(
                "the allocation provided to extend_allocation is unused",
                EXTEND_ALLOCATION_ALLOCATION_UNUSED,
            ));
        }

        if self.check_range(
            (unsafe { *alloc }.addr + unsafe { *alloc }.len)
                ..(unsafe { *alloc }.addr + unsafe { *alloc }.len + by),
        ) {
            return Err(crate::Error::new(
                "the allocation, if extended, would extend into another allocation",
                EXTEND_ALLOCATION_OTHER_ALLOCATION,
            ));
        }

        unsafe {
            (*alloc).len += by;
        }
        Ok(())
    }

    /// Extend the allocation header. This has numerous checks, so please use this
    /// instead of manually changing [AllocationHeader::len]!
    fn extend_allocation_header(&self, by: u64) -> Result<(), crate::Error<'static>> {
        let alloc = self.allocationheader;

        if self.check_range(
            (unsafe { *alloc }.addr + unsafe { *alloc }.len)
                ..(unsafe { *alloc }.addr + unsafe { *alloc }.len + by),
        ) {
            return Err(crate::Error::new(
                "the allocation header, if extended, would extend into another allocation",
                EXTEND_ALLOCATION_OTHER_ALLOCATION,
            ));
        }

        unsafe {
            (*alloc).len += by;
        }
        Ok(())
    }

    /// Check to see if any allocations contain the given address. Returns true if so.
    fn check_addr(&self, addr: u64) -> bool {
        if cfg!(CONFIG_MEMORY_UNION_ALL = "true") {
            return false;
        }
        if addr >= (self.allocationheader as u64)
            && addr < (self.allocationheader as u64 + unsafe { *self.allocationheader }.len)
        {
            return true;
        }
        for ele in self.allocations_iter() {
            let alloc = unsafe { *ele };
            if addr >= alloc.addr && addr < alloc.addr + alloc.len {
                return true;
            }
        }
        false
    }

    /// Check to see if a range of addresses have any allocations within. Returns true if so.
    fn check_range(&self, addr: Range<u64>) -> bool {
        if cfg!(CONFIG_MEMORY_UNION_ALL = "true") {
            return false;
        }
        for addr in addr {
            // REALLY inefficient, but I don't think there's a better way.
            if self.check_addr(addr) {
                return true;
            }
        }
        false
    }
}

/// Error returned when free memory is not available.
pub const FREE_MEMORY_UNAVAILABLE: i16 = -1;

/// Error returned when memory wasn't allocated.
pub const MEMORY_NOT_ALLOCATED: i16 = -7;

/// Error returned when the [MaybeMemoryMapAlloc] doesn't have
/// an initalized allocator.
pub const MAYBE_MEMORY_MAP_ALLOC_UNINITALIZED: i16 = -8;

struct MaybeMemoryMapAlloc<'a> {
    alloc: MaybeUninit<MemoryMapAlloc<'a>>,
    initalized: bool,
}
impl<'a> MaybeMemoryMapAlloc<'a> {
    const fn new(alloc: Option<MemoryMapAlloc<'a>>) -> Self {
        if alloc.is_none() {
            return MaybeMemoryMapAlloc {
                alloc: MaybeUninit::uninit(),
                initalized: false,
            };
        }
        MaybeMemoryMapAlloc {
            alloc: MaybeUninit::new(alloc.unwrap()),
            initalized: true,
        }
    }
    const unsafe fn assume_init_ref(&self) -> &MemoryMapAlloc<'a> {
        unsafe { self.alloc.assume_init_ref() }
    }
    /// Note that if the allocator isn't initalized then this will do nothing.
    const fn add_alloc(&mut self, alloc: MemoryMapAlloc<'a>) {
        if self.initalized {
            return;
        }
        self.alloc.write(alloc);
        self.initalized = true;
    }
    fn remove_alloc(&mut self) {
        if !self.initalized {
            return;
        }
        unsafe {
            self.alloc.assume_init_drop();
        }
        self.initalized = false;
    }
}

unsafe impl<'a> GlobalAlloc for MaybeMemoryMapAlloc<'a> {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        if self.initalized {
            unsafe {
                LAST_MEMMAP_ERR = Err(crate::Error::new(
                    "MaybeMemoryMapAlloc not initalized",
                    MAYBE_MEMORY_MAP_ALLOC_UNINITALIZED,
                ))
            }
            return null_mut();
        }
        unsafe { self.alloc.assume_init_ref().alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        if self.initalized {
            unsafe {
                LAST_MEMMAP_ERR = Err(crate::Error::new(
                    "MaybeMemoryMapAlloc not initalized",
                    MAYBE_MEMORY_MAP_ALLOC_UNINITALIZED,
                ))
            }
            return;
        }
        unsafe { self.alloc.assume_init_ref().dealloc(ptr, layout) }
    }
}

unsafe impl<'a> Allocator for MaybeMemoryMapAlloc<'a> {
    fn allocate(
        &self,
        layout: core::alloc::Layout,
    ) -> Result<NonNull<[u8]>, core::alloc::AllocError> {
        if !self.initalized {
            unsafe {
                LAST_MEMMAP_ERR = Err(crate::Error::new(
                    "MaybeMemoryMapAlloc not initalized",
                    MAYBE_MEMORY_MAP_ALLOC_UNINITALIZED,
                ))
            }
            return Err(core::alloc::AllocError {});
        }
        unsafe { self.alloc.assume_init_ref() }.allocate(layout)
    }
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: core::alloc::Layout) {
        if !self.initalized {
            unsafe {
                LAST_MEMMAP_ERR = Err(crate::Error::new(
                    "MaybeMemoryMapAlloc not initalized",
                    MAYBE_MEMORY_MAP_ALLOC_UNINITALIZED,
                ))
            }
            return;
        }
        unsafe { self.alloc.assume_init_ref().deallocate(ptr, layout) }
    }
}

unsafe impl<'a> GlobalAlloc for MemoryMapAlloc<'a> {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let result = self.allocate(layout);
        if result.is_err() {
            return null_mut();
        }
        result.unwrap().as_mut_ptr()
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        unsafe {
            self.deallocate(
                NonNull::without_provenance(NonZero::new(ptr as usize).unwrap()),
                layout,
            );
        }
    }
}

/// The last status of memory allocation or deallocation for a [MemoryMapAlloc].
/// This can be used for more insight to why an allocation or deallocation failed.
pub static mut LAST_MEMMAP_ERR: Result<(), crate::Error<'static>> = Ok(());

unsafe impl<'a> Allocator for MemoryMapAlloc<'a> {
    fn allocate(
        &self,
        layout: core::alloc::Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        unsafe { LAST_MEMMAP_ERR = Ok(()) }
        if self.allocations == core::ptr::null_mut() {
            unsafe {
                LAST_MEMMAP_ERR = Err(crate::Error::new(
                    "Allocations storage not set up",
                    FREE_MEMORY_UNAVAILABLE,
                ))
            }
            return Err(core::alloc::AllocError {});
        }
        let mut addr = 0u64;
        for mapping in self.memory_map.clone() {
            if mapping.len < layout.size() as u64 {
                continue;
            }
            let mut allocatable = false;
            if mapping.mem_type == MemoryType::Free {
                allocatable = true;
            } else if let MemoryType::HardwareSpecific(_, alloc) = mapping.mem_type {
                allocatable = alloc;
            }
            if allocatable {
                addr = mapping.start + mapping.len - layout.size() as u64;
                while self.check_range(addr..addr + layout.size() as u64)
                    && (addr as usize % layout.align() != 0)
                    && addr >= mapping.start
                {
                    addr -= layout.size() as u64 / crate::cfg_int!("CONFIG_ALLOC_PRECISION", u64);
                }
                if (!self.check_range(addr..addr + layout.size() as u64))
                    && (addr as usize % layout.align() == 0)
                    && addr >= mapping.start
                {
                    break;
                }
                continue;
            }
        }

        if addr == 0 {
            unsafe {
                LAST_MEMMAP_ERR = Err(crate::Error::new(
                    "Free memory of the correct size and alignment couldn't be found",
                    FREE_MEMORY_UNAVAILABLE,
                ))
            }
            return Err(core::alloc::AllocError {});
        }

        if cfg!(CONFIG_MEMORY_UNION_ALL = "true") {
            return Ok(NonNull::from_raw_parts(
                NonNull::<u8>::without_provenance(NonZero::new(addr as usize).unwrap()),
                layout.size(),
            ));
        }

        if let Err(err) = self.add_allocation(Allocation {
            used: true,
            addr,
            len: layout.size() as u64,
        }) {
            unsafe { LAST_MEMMAP_ERR = Err(err) }
            return Err(core::alloc::AllocError {});
        }

        Ok(NonNull::from_raw_parts(
            NonNull::<u8>::without_provenance(NonZero::new(addr as usize).unwrap()),
            layout.size(),
        ))
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, _layout: core::alloc::Layout) {
        unsafe { LAST_MEMMAP_ERR = Ok(()) }
        if cfg!(CONFIG_MEMORY_UNION_ALL = "true") {
            return;
        }
        let addr = ptr.addr().get() as u64;
        if self.allocations == core::ptr::null_mut() {
            unsafe {
                LAST_MEMMAP_ERR = Err(crate::Error::new(
                    "Allocations storage not set up",
                    FREE_MEMORY_UNAVAILABLE,
                ))
            }
            return;
        }
        crate::arch::output::sdebugsln("Searching for allocation");
        crate::arch::output::sdebugbln(&crate::u64_as_u8_slice(
            unsafe { *self.allocationheader }.num_allocations,
        ));
        for allocation in self.allocations_iter() {
            crate::arch::output::sdebugsln("Allocation");
            let alloc = unsafe { *allocation }.clone();
            if !alloc.used {
                crate::arch::output::sdebugs("Unused, addr is ");
                if alloc.addr == addr {
                    crate::arch::output::sdebugsnp("correct and ");
                } else {
                    crate::arch::output::sdebugsnp("incorrect and ");
                }
                if alloc.addr == 0 {
                    crate::arch::output::sdebugsnpln("null");
                } else {
                    crate::arch::output::sdebugsnpln("non-null");
                }
                continue;
            }
            crate::arch::output::sdebugsln("Used");
            if alloc.addr == addr {
                unsafe { *allocation }.used = false;
                return;
            }
        }
        crate::arch::output::sdebugsln("Memory unallocated");
        // Memory not allocated, something is up, this is put after the loop to prevent a costly call to check_addr
        unsafe {
            LAST_MEMMAP_ERR = Err(crate::Error::new(
                "memory not allocated",
                MEMORY_NOT_ALLOCATED,
            ))
        }
        return;
    }
}
