#![no_std]
#![no_main]
#![feature(naked_functions, asm_const)]

use core::arch::{asm, global_asm};
use core::fmt::Write;
use spin;

const TRAPFRAME: usize = 4096;
const STACK_SIZE: usize = 4096;
const MAX_HART_COUNT: usize = 8;
static mut STACK_0: [[u8; STACK_SIZE]; MAX_HART_COUNT] = [[0; STACK_SIZE]; MAX_HART_COUNT];
static PRINT_IMPL: spin::once::Once<&'static dyn DebugPrint> = spin::once::Once::new();

#[allow(unused_macros)]
macro_rules! print {
    ($($arg:tt)*) => { core::write!($crate::DebugWriter, $($arg)*).expect("Unable to write!"); }
}

macro_rules! println {
    ($($arg:tt)*) => { core::writeln!($crate::DebugWriter, $($arg)*).expect("Unable to write!"); }
}

#[allow(unused_imports)]
pub(crate) use print;
pub(crate) use println;

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
        PRINT_IMPL.call_once(|| &DebugConsoleDebugPrint);
    } else {
        PRINT_IMPL.call_once(|| &LegacyDebugPrint);
    }

    log::set_logger(&DebugWriter).map(|()| log::set_max_level(log::LevelFilter::Debug)).unwrap();

    log::info!("Logging correctly!");
    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);
    loop {}
}

trait DebugPrint : Sync {
    fn print_byte(&self, byte: u8) -> core::fmt::Result;

    fn print_str(&self, string: &str) -> core::fmt::Result {
        for byte in string.bytes() {
            self.print_byte(byte)?;
        }
        Ok(())
    }
}

struct LegacyDebugPrint;

impl DebugPrint for LegacyDebugPrint {
    #[allow(deprecated)]
    fn print_byte(&self, byte: u8) -> core::fmt::Result {
        return if sbi_rt::legacy::console_putchar(byte as usize) != 0 {
            Err(core::fmt::Error)
        } else {
            Ok(())
        }
    }
}

struct DebugConsoleDebugPrint;

impl DebugPrint for DebugConsoleDebugPrint {
    fn print_byte(&self, byte: u8) -> core::fmt::Result {
        if sbi_rt::console_write_byte(byte).is_ok() {
            Ok(())
        } else {
            Err(core::fmt::Error)
        }
    }

    fn print_str(&self, string: &str) -> core::fmt::Result {
        let string_bytes = string.as_bytes();
        if sbi_rt::console_write(sbi_rt::Physical::new(string_bytes.len(), string_bytes.as_ptr() as _, 0)).is_ok() {
            Ok(())
        } else {
            Err(core::fmt::Error)
        }
    }
}

pub(crate) struct DebugWriter;

impl Write for DebugWriter {
    fn write_str(&mut self, string: &str) -> core::fmt::Result {
        PRINT_IMPL.get().unwrap().print_str(string)
    }
}

impl log::Log for DebugWriter {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let file = record.file().unwrap_or("");
        let line = record.line().unwrap_or(0);

        println!("[{}] ({}:{}:{}): {}", record.level(), record.target(), file, line, record.args());
    }

    fn flush(&self) {}
}

global_asm!(include_str!("trampoline.S"), TRAPFRAME = const TRAPFRAME);

#[cfg(not(test))]
#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
