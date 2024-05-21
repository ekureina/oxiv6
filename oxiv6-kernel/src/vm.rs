use bitfield::{bitfield, BitMut, BitRange};
use num_enum::{FromPrimitive, IntoPrimitive};

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
    pub fn set_mapping(&mut self, physical_address: *mut u8) {
        let data = physical_address as usize;
        self.set_pa(u64::try_from(data).unwrap() >> 12);
    }

    /// Get the flag bits in this PTE
    #[must_use]
    pub fn get_flags(&self) -> u64 {
        self.bit_range(7, 0)
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
pub enum RSW {
    #[default]
    /// Default value of the RSW
    Default,
    /// Set if the page in question is a COWable page (Writeable, but COW'd)
    COWPage,
}

pub(crate) const PAGE_SIZE: usize = 4096;

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
