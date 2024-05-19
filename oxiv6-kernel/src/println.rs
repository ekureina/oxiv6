use core::fmt::Write;

static PRINT_IMPL: spin::once::Once<&'static dyn DebugPrint> = spin::once::Once::new();
const LEVEL_FILTER: log::LevelFilter = log::LevelFilter::Trace;

#[allow(unused_macros)]
macro_rules! print {
    ($($arg:tt)*) => { use core::fmt::Write; core::write!($crate::println::DebugWriter, $($arg)*).expect("Unable to write!"); }
}

macro_rules! println {
    ($($arg:tt)*) => { use core::fmt::Write; core::writeln!($crate::println::DebugWriter, $($arg)*).expect("Unable to write!"); }
}

#[inline]
pub(crate) fn set_debug_console_print() {
    PRINT_IMPL.call_once(|| &DebugConsoleDebugPrint);
    log::set_logger(&DebugWriter)
        .map(|()| log::set_max_level(LEVEL_FILTER))
        .expect("Unable to set logger");
}

#[inline]
pub(crate) fn set_legacy_debug_print() {
    PRINT_IMPL.call_once(|| &LegacyDebugPrint);
    log::set_logger(&DebugWriter)
        .map(|()| log::set_max_level(LEVEL_FILTER))
        .expect("Unable to set logger");
}
trait DebugPrint: Sync {
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
        if sbi_rt::legacy::console_putchar(byte as usize) != 0 {
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
        if sbi_rt::console_write(sbi_rt::Physical::new(
            string_bytes.len(),
            string_bytes.as_ptr() as _,
            0,
        ))
        .is_ok()
        {
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
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() >= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let file = record.file().unwrap_or("");
            let line = record.line().unwrap_or(0);

            println!(
                "[{}] ({}:{}:{}): {}",
                record.level(),
                record.target(),
                file,
                line,
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

#[allow(unused_imports)]
pub(crate) use print;
pub(crate) use println;
