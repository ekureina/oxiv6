#![no_std]
#![no_main]
#![feature(naked_functions, asm_const)]

/*!
   Copyright 2024 Claire Moore

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
*/

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
mod vm;

extern "C" {
    // TODO: Understand why linker can't provide this as a usize
    pub(crate) fn etext();
    pub(crate) fn end();
    pub(crate) fn trampoline();
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
            "j {rust_boot}",
            stack0 = sym STACK_0,
            stack_size = const STACK_SIZE,
            rust_boot = sym rust_boot,
            options(noreturn),
        )
    }
}

#[naked]
#[no_mangle]
unsafe extern "C" fn subhart_start(hartid: usize, root_sp_location: usize) -> ! {
    unsafe {
        asm!(
            "add sp, a1, zero",
            "j {rust_main}",
            rust_main = sym rust_main,
            options(noreturn),
        )
    }
}

#[no_mangle]
extern "C" fn rust_boot(hartid: usize, device_tree_paddr: usize) -> ! {
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
    info!("Allocator Initialized");
    crate::vm::kvmmake();
    info!("Set up Kernel page table");

    rust_main(hartid)
}

#[no_mangle]
extern "C" fn rust_main(_hartid: usize) -> ! {
    crate::vm::KERNEL_PAGE_TABLE
        .get()
        .expect("Expected kernel page table to be initialized")
        .set_as_active_table();
    info!("Installed Kernel page table");

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
