#![no_std]
#![no_main]
#![feature(naked_functions, asm_const)]

const TRAPFRAME: usize = 4096;
const STACK_SIZE: usize = 4096;
const MAX_HART_COUNT: usize = 8;
static mut STACK_0: [[u8; STACK_SIZE]; MAX_HART_COUNT] = [[0; STACK_SIZE]; MAX_HART_COUNT];

#[naked]
#[no_mangle]
#[link_section = ".text.entry"]
unsafe extern "C" fn _start(hartid: usize, device_tree_paddr: usize) -> ! {
    core::arch::asm!(
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
extern "C" fn rust_main(hartid: usize, device_tree_paddr: usize) -> ! {
    loop {}
}

core::arch::global_asm!(include_str!("trampoline.S"), TRAPFRAME = const TRAPFRAME);

#[cfg(not(test))]
#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
