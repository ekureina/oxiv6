use alloc::alloc::{alloc_zeroed, Layout};
use bitfield::{bitfield, BitMut, BitRange, BitRangeMut};
use core::{mem::size_of, slice::from_raw_parts_mut};
use num_enum::{FromPrimitive, IntoPrimitive};
use riscv::register::satp;

/// A full Page Table
#[repr(transparent)]
pub(crate) struct PageTable<'a> {
    first_level: &'a mut [PageTableEntry],
}

impl<'a> PageTable<'a> {
    /// Creates a new page table, located on the heap
    pub(crate) fn new() -> Self {
        let layout = Layout::from_size_align(PAGE_SIZE, PAGE_SIZE)
            .expect("Unable to allocate for page table");
        #[allow(clippy::cast_ptr_alignment)]
        let page_table_page = unsafe { alloc_zeroed(layout).cast::<PageTableEntry>() };
        PageTable {
            first_level: unsafe {
                from_raw_parts_mut(page_table_page, PAGE_SIZE / size_of::<PageTableEntry>())
            },
        }
    }

    /// Sets this page table as the active table
    pub(crate) fn set_as_active_table(&self) {
        unsafe {
            satp::set(
                satp::Mode::Sv39,
                0,
                (self.first_level.as_ptr() as usize) >> 12,
            );
        }
    }

    /// Map a contiguous region of virtual addresses to a contigous region of physical addresses
    /// `virtual_base` and `region_size` need not be page aligned
    pub(crate) fn map_pages(
        &mut self,
        virtual_base: usize,
        region_size: usize,
        physical_base: usize,
        permissions: u8,
    ) -> Result<(), PageTableMapError> {
        assert!(region_size != 0, "map_pages: size");

        let virtual_page_start = PGROUNDDOWN!(virtual_base);
        let virtual_page_end = PGROUNDDOWN!(virtual_base + region_size - 1);
        for virtual_addr in (virtual_page_start..virtual_page_end).step_by(PAGE_SIZE) {
            self.walk_mut(virtual_addr, true, |pte| {
                assert!(!pte.valid(), "map_pages: remap");
                pte.set_mapping(virtual_addr - virtual_page_start + physical_base);
                pte.set_flags(permissions);
                pte.set_valid(true);
            })?;
        }
        Ok(())
    }

    pub(crate) fn walk_mut(
        &mut self,
        virtual_address: usize,
        should_allocate: bool,
        pte_edit: impl FnOnce(&mut PageTableEntry),
    ) -> Result<(), PageTableWalkError> {
        assert!(virtual_address < MAX_VIRTUAL_ADDRESS, "walk_mut");

        let mut page_table = core::ptr::addr_of_mut!(self.first_level[0]);

        for level in (1..=2).rev() {
            let page_index = (virtual_address >> (12 + (9 * level))) & 0x1FF;
            let page_table_entry = unsafe { page_table.wrapping_add(page_index).as_mut() }.unwrap();
            if page_table_entry.valid() {
                page_table =
                    core::ptr::addr_of_mut!(page_table_entry.pa_mut::<PageTableEntry>()[0]);
            } else if !should_allocate {
                return Err(PageTableWalkError::PageTableUnallocated);
            } else {
                let layout = Layout::from_size_align(PAGE_SIZE, PAGE_SIZE)
                    .expect("Unable to allocate for page table");
                page_table = unsafe {
                    #[allow(clippy::cast_ptr_alignment)]
                    alloc_zeroed(layout).cast::<PageTableEntry>()
                };
                if page_table.is_null() {
                    return Err(PageTableWalkError::UnableToAllocate);
                }
                page_table_entry.set_mapping(page_table as usize);
                page_table_entry.set_valid(true);
            }
        }

        pte_edit(
            unsafe {
                page_table
                    .wrapping_add((virtual_address >> 12) & 0x1FF)
                    .as_mut()
            }
            .unwrap(),
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn walk_const<T>(
        &self,
        virtual_address: usize,
        pte_lookup: impl FnOnce(&PageTableEntry) -> T,
    ) -> Result<T, PageTableWalkError> {
        assert!(virtual_address < MAX_VIRTUAL_ADDRESS, "walk");

        let mut page_table = core::ptr::addr_of!(self.first_level[0]);

        for level in (1..=2).rev() {
            let page_index = (virtual_address >> (12 + (9 * level))) & 0x1FF;
            let page_table_entry = unsafe { page_table.wrapping_add(page_index).as_ref() }.unwrap();
            if page_table_entry.valid() {
                page_table = core::ptr::addr_of!(page_table_entry.pa_const::<PageTableEntry>()[0]);
            } else {
                return Err(PageTableWalkError::PageTableUnallocated);
            }
        }
        return Ok(pte_lookup(
            unsafe {
                page_table
                    .wrapping_add((virtual_address >> 12) & 0x1FF)
                    .as_ref()
            }
            .unwrap(),
        ));
    }
}

#[derive(Debug)]
pub(crate) enum PageTableWalkError {
    PageTableUnallocated,
    UnableToAllocate,
}

#[derive(Debug)]
pub(crate) enum PageTableMapError {
    #[allow(dead_code)]
    PageTableWalkError(PageTableWalkError),
}

impl From<PageTableWalkError> for PageTableMapError {
    fn from(value: PageTableWalkError) -> Self {
        Self::PageTableWalkError(value)
    }
}

pub(crate) static KERNEL_PAGE_TABLE: spin::once::Once<PageTable<'static>> = spin::once::Once::new();

pub(crate) fn kvmmake() {
    KERNEL_PAGE_TABLE.call_once(|| {
        let mut page_table = PageTable::new();

        page_table
            .map_pages(
                crate::_start as usize,
                crate::etext as usize - crate::_start as usize,
                crate::_start as usize,
                10,
            )
            .expect("Unable to map kernel text");
        page_table
            .map_pages(
                crate::etext as usize,
                crate::dev::spec::get_physical_memory_size() - crate::etext as usize,
                crate::etext as usize,
                6,
            )
            .expect("Unable to map data");
        page_table
            .map_pages(TRAMPOLINE, PAGE_SIZE, crate::trampoline as usize, 10)
            .expect("Unable to map trampoline page");

        page_table
    });
}

bitfield! {
    /// A wrapper around a Sv39 Page Table Entry
    #[derive(PartialEq, Eq, Copy, Clone)]
    #[repr(transparent)]
    pub struct PageTableEntry(u64);
    impl Debug;
    /// Find if the referenced page is valid
    pub valid, set_valid: 0;
    /// Can this page be read?
    pub readable, set_readable: 1;
    /// Can this page be written to?
    pub writeable, set_writeable: 2;
    /// Can memory in this page be executed?
    pub executable, set_executable: 3;
    /// Can user code access this page?
    pub user_accessible, set_user_accessible: 4;
    /// Has this page been accessed since the last reset?
    /// Must be cleared by [`clear_accessed`]
    pub accessed, _: 6;
    /// Has this page been written since the last reset?
    /// Must be cleared by [`clear_dirty`]
    pub dirty, _: 7;
    /// The RSW field, used by rv6 to track COWs
    pub u8, from into RSW, rsw, set_rsw: 9, 8;
    /// Physical Page to map to
    pa, set_pa: 53, 10;
}

impl PageTableEntry {
    /// Clear the accessed bit on the Page Table Entry
    /// Cannot set this bit, only read and clear
    pub fn clear_accessed(&mut self) {
        self.0.set_bit(6, false);
    }

    /// Clear the accessed bit on the Page Table Entry
    /// Cannot set this bit, only read and clear
    pub fn clear_dirty(&mut self) {
        self.0.set_bit(7, false);
    }

    /// Map this PTE to a physical address as a u64
    #[must_use]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn pa_int(&self) -> u64 {
        self.pa() << 12
    }

    /// Map this PTE to a physical address as a mutable slice
    #[must_use]
    #[allow(clippy::mut_from_ref)]
    pub fn pa_mut<T>(&self) -> &mut [T] {
        unsafe {
            core::slice::from_raw_parts_mut(
                (self.pa() << 12) as *mut T,
                PAGE_SIZE / core::mem::size_of::<T>(),
            )
        }
    }

    /// Map this PTE to a physical address as a constant slice
    #[must_use]
    pub fn pa_const<T>(&self) -> &[T] {
        unsafe {
            core::slice::from_raw_parts(
                (self.pa() << 12) as *const T,
                PAGE_SIZE / core::mem::size_of::<T>(),
            )
        }
    }

    /// Set the physical address this PTE points to
    #[allow(clippy::missing_panics_doc)]
    pub fn set_mapping(&mut self, physical_address: usize) {
        self.set_pa(u64::try_from(physical_address).unwrap() >> 12);
    }

    /// Get the flag bits in this PTE
    #[must_use]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn get_flags(&self) -> u64 {
        self.bit_range(7, 0)
    }

    pub fn set_flags(&mut self, flags: u8) {
        self.set_bit_range(7, 0, flags);
    }
}

impl From<PageTableEntry> for u64 {
    fn from(value: PageTableEntry) -> Self {
        value.0
    }
}

impl From<u64> for PageTableEntry {
    fn from(value: u64) -> Self {
        PageTableEntry(value)
    }
}

/// Values set in the RSW field of the [`PageTableEntry`]
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Default, Copy, Clone, IntoPrimitive, FromPrimitive)]
#[allow(clippy::upper_case_acronyms)]
pub enum RSW {
    #[default]
    /// Default value of the RSW
    Default,
    /// Set if the page in question is a `COWable` page (Writeable, but COW'd)
    COWPage,
}

/// The size of pages used in oxiv6
pub(crate) const PAGE_SIZE: usize = 4096;
/// One beyond the highest possible virtual address.
/// `MAX_VIRTUAL_ADDRESS` is actually one bit less than the max allowed by
/// Sv39, to avoid having to sign-extend virtual addresses
/// that have the high bit set.
pub(crate) const MAX_VIRTUAL_ADDRESS: usize = 1 << (9 + 9 + 9 + 12 - 1);
pub(crate) const TRAMPOLINE: usize = MAX_VIRTUAL_ADDRESS - PAGE_SIZE;

macro_rules! PGROUNDUP {
    ($e:expr) => {
        ($e as usize + $crate::vm::PAGE_SIZE - 1)
            & !($crate::vm::PAGE_SIZE - 1)
    };
}

macro_rules! PGROUNDDOWN {
    ($e:expr) => {
        $e as usize & !($crate::vm::PAGE_SIZE - 1)
    };
}

pub(crate) use PGROUNDDOWN;
pub(crate) use PGROUNDUP;
