pub const MAX_STACK_HEIGHT: usize = 1000;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum Event {
    Empty,
    Entry(Call),
    Exit(Exit)
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Call {
    pub time: u64,
    pub from: *const usize,
    pub to: *const usize,
    pub tid: Option<core::num::NonZeroU64>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Exit {
    pub time: u64,
    pub from: *const usize,
    pub tid: Option<core::num::NonZeroU64>,
}
