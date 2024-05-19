#![no_std]
#![no_main]
#![feature(naked_functions, asm_const)]

use crate::dev::spec::{get_cpu_count, get_physical_memory_size, load_fdt};
use crate::println::println;
use core::arch::{asm, global_asm};
use log::info;

const TRAPFRAME: usize = 4096;
const STACK_SIZE: usize = 8192;
const MAX_HART_COUNT: usize = 8;
static mut STACK_0: [[u8; STACK_SIZE]; MAX_HART_COUNT] = [[0; STACK_SIZE]; MAX_HART_COUNT];

extern crate alloc;

mod dev;
mod kalloc;
mod println;

extern "C" {
    // TODO: Understand why linker can't provide this as a usize
    pub(crate) fn etext();
    pub(crate) fn end();
}

#[naked]
#[no_mangle]
#[link_section = ".text.entry"]
pub(crate) unsafe extern "C" fn _start(hartid: usize, device_tree_paddr: usize) -> ! {
    unsafe {
        asm!(
            "la sp, {stack0}",
            "li t0, {stack_size}",
            "addi t1, a0, 1",
            "mul t0, t0, t1",
            "add sp, sp, t0",
            "j {rust_main}",
            stack0 = sym STACK_0,
            stack_size = const STACK_SIZE,
            rust_main = sym rust_main,
            options(noreturn),
        )
    }
}

#[no_mangle]
extern "C" fn rust_main(_hartid: usize, device_tree_paddr: usize) -> ! {
    if sbi_rt::probe_extension(sbi_rt::Console).is_available() {
        println::set_debug_console_print();
    } else {
        println::set_legacy_debug_print();
    }
    unsafe {
        load_fdt(device_tree_paddr);
    }
    info!(
        "end: 0x{:x}, etext: 0x{:x}, PHYSICAL_ADDRESS_STOP: 0x{:x}, CPU_COUNT: {}",
        end as usize,
        etext as usize,
        get_physical_memory_size(),
        get_cpu_count()
    );

    crate::kalloc::ALLOCATOR.init();

    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);
    #[allow(clippy::empty_loop)]
    loop {}
}

global_asm!(include_str!("trampoline.S"), TRAPFRAME = const TRAPFRAME);

#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    println!("{}", info);
    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::SystemFailure);
    loop {}
}
