#[derive(Debug, Clone, Default)]
pub struct TrapState {
    pub mstatus: u32,
    pub mtvec: u32,
    pub mepc: u32,
    pub mcause: u32,
    pub interrupt_pending: bool,
}
