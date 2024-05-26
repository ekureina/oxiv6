use crate::vm::PageTable;
use spin::mutex::Mutex;

#[derive(Debug, Default)]
pub(crate) struct Proc<'a> {
    public_data: Mutex<PublicProcData>,
    private_data: PrivateProcData<'a>,
}

#[derive(Debug, Default)]
pub(crate) struct PublicProcData {
    pub(crate) state: ProcState,
    pub(crate) chan: usize,
    pub(crate) killed: bool,
    pub(crate) pid: usize,
}

#[derive(Debug, Default)]
pub(crate) enum ProcState {
    #[default]
    Unused,
    Used,
    Sleeping,
    Runnable,
    Running,
    Zombie,
}

#[derive(Debug, Default)]
struct PrivateProcData<'a> {
    kstack: usize,
    size: usize,
    tracing_mask: u32,
    page_table: Option<PageTable<'a>>,
    name: &'a str,
}
