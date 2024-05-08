use spin::once::Once;

static PHYSICAL_ADDRESS_STOP: Once<usize> = Once::new();
static CPU_COUNT: Once<usize> = Once::new();
const MAX_VA: usize = 1 << (9 + 9 + 9 + 12 - 1);

/// Loads data from the FDT pointed to at `fdt_address`
/// # Safety
/// Assumes that the `fdt_address` points to a valid fdt and that the memory is mapped correctly.
/// # Panics
/// Panics  if the address or the data at the address is invalid
pub(crate) unsafe fn load_fdt(fdt_address: usize) {
    let fdt = unsafe { fdt::Fdt::from_ptr(fdt_address as *const u8) }.expect("Unable to load fdt");
    // The true size of physical memory, calculated from the FDT
    let true_physical_stop = fdt
        .memory()
        .regions()
        .fold(crate::_start as usize, |mem_size, region| {
            mem_size + region.size.expect("Unable to load memory size")
        });
    // Get the CPU Count from the FDT. The max for this value for qemu's `virt` architecture is 8, but we allow for more memory to
    // be used if less CPUs are allocated.
    let cpu_count = *CPU_COUNT.call_once(|| fdt.cpus().count());
    // Reserved pages for the Trampoline and Kernel stacks (2 for trampoline, and 2 per CPU (stack + guard page))
    let reserved_pages = 4096 * (2 * cpu_count + 1);
    // Set the `PHYSICAL_ADDRESS_STOP` to the minimum of the true amount of system RAM, and the maxiumum amount of
    // physical RAM before Xv6 breaks this is about 256GiB, so this is probably unecessary, but just covering all
    // the bases here
    PHYSICAL_ADDRESS_STOP.call_once(|| core::cmp::min(true_physical_stop, MAX_VA - reserved_pages));
}

#[inline]
pub(crate) fn get_cpu_count() -> usize {
    *CPU_COUNT.wait()
}

#[inline]
pub(crate) fn get_physical_memory_size() -> usize {
    *PHYSICAL_ADDRESS_STOP.wait()
}
