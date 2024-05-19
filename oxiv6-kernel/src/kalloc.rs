use crate::dev::spec::get_physical_memory_size;
use alloc::alloc::{GlobalAlloc, Layout};
use core::cell::Cell;
use core::ptr::{self, null_mut, NonNull};
use log::debug;
use spin::mutex::{Mutex, MutexGuard};

pub(crate) const PAGE_SIZE: usize = 4096;

macro_rules! PGROUNDUP {
    ($e:expr) => {
        ($e as usize + $crate::kalloc::PAGE_SIZE - 1)
            & !($crate::kalloc::PAGE_SIZE - 1)
    };
}

macro_rules! PGROUNDDOWN {
    ($e:expr) => {
        $e as usize & !($crate::kalloc::PAGE_SIZE - 1)
    };
}

pub(crate) use PGROUNDDOWN;
pub(crate) use PGROUNDUP;

#[repr(C)]
struct Run {
    pub next: Cell<Option<NonNull<Run>>>,
}

#[repr(C, align(16))]
struct TinyHeader {
    next: Cell<Option<NonNull<TinyHeader>>>,
    size: usize,
}

pub(crate) struct KernelPageAllocator<'a> {
    freelist: Mutex<Cell<Option<NonNull<Run>>>>,
    page_refcounts: Mutex<Cell<Option<&'a mut [u8]>>>,
}

pub(crate) struct KernelAllocator<'a> {
    page_allocator: KernelPageAllocator<'a>,
    tiny_page_list: Mutex<Cell<Option<NonNull<TinyHeader>>>>,
}

#[global_allocator]
pub(crate) static ALLOCATOR: KernelAllocator = KernelAllocator {
    page_allocator: KernelPageAllocator {
        freelist: Mutex::new(Cell::new(None)),
        page_refcounts: Mutex::new(Cell::new(None)),
    },
    tiny_page_list: Mutex::new(Cell::new(None)),
};

unsafe impl<'a> Sync for KernelPageAllocator<'a> {}
unsafe impl<'a> Send for KernelPageAllocator<'a> {}
unsafe impl<'a> Sync for KernelAllocator<'a> {}
unsafe impl<'a> Send for KernelAllocator<'a> {}

unsafe impl<'a> GlobalAlloc for KernelPageAllocator<'a> {
    /// Allocates a page of physical memory
    /// Ignores `layout`, except to check that the request is for no more than a page of memory
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        // Cannot allocate or align more than a page
        if size > PAGE_SIZE || align > PAGE_SIZE {
            return ptr::null_mut();
        }

        let freelist = self.freelist.lock();
        let return_cell = Cell::new(None);
        return_cell.swap(&freelist);
        if let Some(run_cell) = return_cell.get() {
            freelist.swap(&unsafe { run_cell.as_ref() }.next);
        }

        match return_cell.get().map(NonNull::cast::<u8>) {
            None => null_mut(),
            Some(ptr) => {
                let final_ptr = ptr.as_ptr();
                {
                    let page_refcounts = self.page_refcounts.lock();
                    let refcount_data = page_refcounts.take().unwrap();
                    // The index in the refcount data to update.
                    let page_index = Self::convert_physical_to_index(final_ptr as usize);
                    refcount_data[page_index] += 1;
                    page_refcounts.set(Some(refcount_data));
                }
                core::mem::drop(freelist);
                unsafe { ptr::write_bytes(final_ptr, 5, PAGE_SIZE) };
                final_ptr
            }
        }
    }

    /// Deallocate a page allocated by this allocator
    #[allow(clippy::cast_ptr_alignment)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            let size = layout.size();
            let align = layout.align();
            let ptr_int = ptr as usize;
            if ptr_int % PAGE_SIZE != 0
                || ptr_int < crate::end as usize
                || ptr_int >= get_physical_memory_size()
                || size > PAGE_SIZE
                || align > PAGE_SIZE
            {
                panic!("KPA_dealloc: Out of bounds");
            }

            // Lock any modifications to the freelist for the remainder of the execution
            // We want to make sure that we don't deadlock, and that we don't change the refcount before deallocating
            // We also want to have the same lock order as alloc
            let freelist = self.freelist.lock();

            let page_refcounts = self.page_refcounts.lock();
            let refcount = {
                let refcount_data = page_refcounts.take().unwrap();
                // The index in the refcount data to update. Previous checks ensure this is in bounds
                let page_index = Self::convert_physical_to_index(ptr_int);
                // Panic if no references were loaned out to the Kernel
                let mut refcount = refcount_data[page_index];
                assert!(refcount != 0, "KPA_dealloc: No page references");
                // Remove a reference to this page
                refcount -= 1;
                refcount_data[page_index] = refcount;
                page_refcounts.set(Some(refcount_data));
                refcount
            };
            core::mem::drop(page_refcounts);

            // Only actually deallocate if we have 0 references
            if refcount == 0 {
                ptr::write_bytes(ptr, 1, PAGE_SIZE);
                let run_ref_option: Option<&'static mut Run> = ptr.cast::<Run>().as_mut();
                if let Some(run_ref) = run_ref_option {
                    run_ref.next = Cell::new(None);
                    run_ref.next.swap(&freelist);
                    let run_cell =
                        Cell::new(Some(NonNull::new_unchecked(core::ptr::from_mut::<Run>(
                            run_ref,
                        ))));
                    freelist.swap(&run_cell);
                }
            }
        }
    }
}

impl KernelPageAllocator<'_> {
    pub fn init(&self, page_count: usize) {
        debug!(
            "Initializing allocator, writing bytes to {:x}",
            crate::end as usize
        );
        unsafe {
            core::ptr::write_bytes(crate::end as *mut u8, 1, page_count);
        }
        let refcount_cell = self.page_refcounts.lock();
        refcount_cell.set(Some(unsafe {
            core::slice::from_raw_parts_mut(crate::end as *mut u8, page_count)
        }));
        debug!("Set Refcounts!");
        core::mem::drop(refcount_cell);
        debug!("Dropped Refcounts!");

        let mut ptr = PGROUNDUP!(crate::end as usize + page_count) as *mut u8;
        let layout = unsafe { Layout::from_size_align_unchecked(PAGE_SIZE, PAGE_SIZE) };

        debug!("Deallocating pages");
        let phystop = get_physical_memory_size();

        while unsafe { ptr.byte_add(PAGE_SIZE) <= phystop as *mut u8 } {
            unsafe {
                self.dealloc(ptr, layout);
            }
            debug!("Deallocated {:x}/{:x}", ptr as usize, phystop);
            ptr = unsafe { ptr.byte_add(PAGE_SIZE) };
        }
        debug!("Deallocated memory");
    }

    #[allow(dead_code)]
    pub(crate) fn pfree_count(&self) -> usize {
        let mut free_memory = 0usize;
        let freelist = self.freelist.lock();
        let mut optional_run_ref = freelist.get();
        while optional_run_ref.is_some() {
            free_memory += PAGE_SIZE;
            optional_run_ref = optional_run_ref.and_then(|ptr| unsafe { ptr.as_ref() }.next.get());
        }
        free_memory
    }

    #[inline]
    fn convert_physical_to_index(physical_address: usize) -> usize {
        PGROUNDDOWN!(physical_address - PGROUNDUP!(crate::end as usize)) / PAGE_SIZE
    }

    #[allow(dead_code)]
    pub fn in_place_copy(&self, physical_address: usize) {
        let index = Self::convert_physical_to_index(physical_address);
        let refcounts = self.page_refcounts.lock();
        let refcount_data = refcounts.take().unwrap();
        refcount_data[index] += 1;
        refcounts.set(Some(refcount_data));
    }

    #[allow(dead_code)]
    pub(crate) fn exactly_one_reference(&self, physical_address: usize) -> bool {
        let index = Self::convert_physical_to_index(physical_address);
        let reference_counts = self.page_refcounts.lock();
        let reference_data = reference_counts.take().unwrap();
        let is_exactly_one_reference = reference_data[index] == 1;
        reference_counts.set(Some(reference_data));
        is_exactly_one_reference
    }
}

unsafe impl GlobalAlloc for KernelAllocator<'_> {
    #[allow(clippy::cast_ptr_alignment)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        // Pass off allocations greater or equal to a page to the page allocator
        // Size will delegate to the page allocator if it is bigger than
        if size >= (PAGE_SIZE - (2 * core::mem::size_of::<TinyHeader>())) || align >= PAGE_SIZE {
            unsafe { self.page_allocator.alloc(layout) }
        } else {
            if align > Self::MAX_ALIGNMENT {
                return ptr::null_mut();
            }
            let size = (size + Self::MAX_ALIGNMENT - 1) & !(Self::MAX_ALIGNMENT - 1);
            let tiny_list = self.tiny_page_list.lock();
            if let Some(list) = tiny_list.get() {
                let mut header = list;
                let mut prev: Option<NonNull<TinyHeader>> = None;
                let data = loop {
                    let header_mut = unsafe { header.as_mut() };
                    let header_size = header_mut.size;
                    if header_size >= size {
                        break Self::write_blocks(&mut prev, &tiny_list, header_mut, size);
                    }

                    if header_mut.next.get().is_none() {
                        break ptr::null_mut();
                    }

                    prev = Some(header);
                    header = header_mut.next.get().unwrap();
                };
                if data.is_null() {
                    let page_layout =
                        unsafe { Layout::from_size_align_unchecked(PAGE_SIZE, PAGE_SIZE) };
                    let new_page = unsafe { self.page_allocator.alloc(page_layout) };
                    if new_page.is_null() {
                        new_page
                    } else {
                        let free_header =
                            unsafe { new_page.add(size + core::mem::size_of::<TinyHeader>()) }
                                .cast::<TinyHeader>();
                        unsafe {
                            *free_header = TinyHeader {
                                next: Cell::new(tiny_list.get()),
                                size: PAGE_SIZE - (size + 2 * core::mem::size_of::<TinyHeader>()),
                            }
                        };
                        unsafe {
                            *new_page.cast::<TinyHeader>() = TinyHeader {
                                next: Cell::new(None),
                                size,
                            }
                        }
                        tiny_list.set(NonNull::new(free_header));
                        unsafe { new_page.cast::<TinyHeader>().add(1) }.cast()
                    }
                } else {
                    data
                }
            } else {
                let page_layout =
                    unsafe { Layout::from_size_align_unchecked(PAGE_SIZE, PAGE_SIZE) };
                let new_page = unsafe { self.page_allocator.alloc(page_layout) };
                if new_page.is_null() {
                    new_page
                } else {
                    let free_header =
                        unsafe { new_page.add(size + core::mem::size_of::<TinyHeader>()) }
                            .cast::<TinyHeader>();
                    unsafe {
                        *free_header = TinyHeader {
                            next: Cell::new(None),
                            size: PAGE_SIZE - (size + 2 * core::mem::size_of::<TinyHeader>()),
                        }
                    };
                    unsafe {
                        *new_page.cast::<TinyHeader>() = TinyHeader {
                            next: Cell::new(None),
                            size,
                        }
                    }
                    tiny_list.set(NonNull::new(free_header));
                    unsafe { new_page.cast::<TinyHeader>().offset(1) }.cast()
                }
            }
        }
    }

    #[allow(clippy::cast_ptr_alignment)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();
        let align = layout.align();

        // Pass off deallocations greater or equal to a page to the page allocator
        // Size will delegate to the page allocator if it is bigger than
        if size >= (PAGE_SIZE - (2 * core::mem::size_of::<TinyHeader>())) || align >= PAGE_SIZE {
            unsafe { self.page_allocator.dealloc(ptr, layout) };
        } else {
            let ptr_int = ptr as usize;
            if ptr_int % Self::MAX_ALIGNMENT != 0
                || ptr_int < crate::end as usize
                || ptr_int >= get_physical_memory_size()
                || size > PAGE_SIZE
                || align > PAGE_SIZE
            {
                panic!("KTA_dealloc: Out of bounds\0");
            }

            let header_list = self.tiny_page_list.lock();
            let header = unsafe { ptr.cast::<TinyHeader>().offset(-1).as_mut().unwrap() };
            header.next = Cell::new(header_list.get());
            header_list.set(Some(header.into()));
        }
    }

    #[allow(clippy::cast_ptr_alignment)]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old_size = layout.size();
        let align = layout.align();
        if align >= PAGE_SIZE {
            unsafe { self.page_allocator.realloc(ptr, layout, new_size) }
        } else if old_size >= PAGE_SIZE - (2 * core::mem::size_of::<TinyHeader>()) {
            if new_size <= old_size {
                ptr
            } else {
                unsafe { self.page_allocator.realloc(ptr, layout, new_size) }
            }
        } else {
            match unsafe { ptr.cast::<TinyHeader>().sub(1).as_ref() } {
                Some(header) => {
                    if header.size >= new_size {
                        ptr
                    } else {
                        self.default_realloc(ptr, layout, new_size)
                    }
                }
                None => self.default_realloc(ptr, layout, new_size),
            }
        }
    }
}

impl KernelAllocator<'_> {
    const MAX_ALIGNMENT: usize = 16;

    pub fn init(&self) {
        let page_count =
            (PGROUNDDOWN!(get_physical_memory_size()) - PGROUNDUP!(crate::end)) / PAGE_SIZE;
        self.page_allocator.init(page_count);
    }

    #[allow(dead_code)]
    pub(crate) fn memfree_count(&self) -> usize {
        let tiny_space = {
            let tiny_allocations = self.tiny_page_list.lock();
            let mut list_ptr = tiny_allocations.get();
            let mut tiny_space = 0usize;
            while list_ptr.is_some() {
                if let Some(ptr) = list_ptr {
                    let data_ref = unsafe { ptr.as_ptr().as_ref() }.unwrap();
                    tiny_space += data_ref.size;
                    list_ptr = data_ref.next.get();
                }
            }
            tiny_space
        };
        tiny_space + self.page_allocator.pfree_count()
    }

    #[allow(dead_code)]
    pub fn in_place_copy(&self, physical_address: usize) {
        if PGROUNDDOWN!(physical_address) == physical_address {
            self.page_allocator.in_place_copy(physical_address);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn exactly_one_reference(&self, physical_address: usize) -> bool {
        PGROUNDDOWN!(physical_address) == physical_address
            && self.page_allocator.exactly_one_reference(physical_address)
    }

    fn write_blocks(
        prev: &mut Option<NonNull<TinyHeader>>,
        tiny_list: &MutexGuard<'_, Cell<Option<NonNull<TinyHeader>>>>,
        header: &mut TinyHeader,
        size: usize,
    ) -> *mut u8 {
        if header.size > size {
            header.size -= size + core::mem::size_of::<TinyHeader>();
            let new_header = unsafe { ptr::addr_of_mut!(*header).add(1).byte_add(header.size) };
            unsafe {
                *new_header = TinyHeader {
                    next: Cell::new(None),
                    size,
                };
            }
            unsafe { new_header.add(1).cast() }
        } else {
            if let Some(mut prev) = prev {
                unsafe { prev.as_mut() }.next = Cell::new(header.next.get());
            } else {
                tiny_list.set(header.next.get());
            }
            unsafe { ptr::addr_of_mut!(*header).add(1).cast::<u8>() }
        }
    }

    fn default_realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        // SAFETY: the caller must ensure that `new_layout` is greater than zero.
        let new_ptr = unsafe { self.alloc(new_layout) };
        if !new_ptr.is_null() {
            // SAFETY: the previously allocated block cannot overlap the newly allocated block.
            // The safety contract for `dealloc` must be upheld by the caller.
            unsafe {
                ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(layout.size(), new_size));
                self.dealloc(ptr, layout);
            }
        }
        new_ptr
    }
}
