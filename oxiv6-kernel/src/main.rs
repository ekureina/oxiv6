#![no_std]
#![no_main]
#![feature(naked_functions, asm_const)]

use crate::println::println;
use core::arch::{asm, global_asm};

const TRAPFRAME: usize = 4096;
const STACK_SIZE: usize = 4096;
const MAX_HART_COUNT: usize = 8;
static mut STACK_0: [[u8; STACK_SIZE]; MAX_HART_COUNT] = [[0; STACK_SIZE]; MAX_HART_COUNT];

mod println;

#[naked]
#[no_mangle]
#[link_section = ".text.entry"]
unsafe extern "C" fn _start(hartid: usize, device_tree_paddr: usize) -> ! {
    asm!(
        "la sp, {stack0}",
        "li t0, {stack_size}",
        "addi t1, a0, 1",
        "mul t0, t1, t0",
        "add sp, sp, t0",
        "j {rust_main}",
        stack0 = sym STACK_0,
        stack_size = const STACK_SIZE,
        rust_main = sym rust_main,
        options(noreturn),
    )
}

#[no_mangle]
extern "C" fn rust_main(_hartid: usize, _device_tree_paddr: usize) -> ! {
    if sbi_rt::probe_extension(sbi_rt::Console).is_available() {
        println::set_debug_console_print();
    } else {
        println::set_legacy_debug_print();
    }

    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);
    loop {}
}

global_asm!(include_str!("trampoline.S"), TRAPFRAME = const TRAPFRAME);

#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    println!("{}", info);
    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);
    loop {}
}
