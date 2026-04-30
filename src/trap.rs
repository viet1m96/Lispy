pub const MSTATUS_MIE: u32 = 1 << 0;
pub const MSTATUS_IN_TRAP: u32 = 1 << 1;

#[derive(Debug, Clone)]
pub struct TrapState {
    pub mstatus: u32,
    pub vtor: u32,
    pub mepc: u32,
}

impl Default for TrapState {
    fn default() -> Self {
        Self { mstatus: MSTATUS_MIE, vtor: 0, mepc: 0 }
    }
}

impl TrapState {
    pub fn mie(&self) -> bool { self.mstatus & MSTATUS_MIE != 0 }
    pub fn in_trap(&self) -> bool { self.mstatus & MSTATUS_IN_TRAP != 0 }
    pub fn enter(&mut self, mepc: u32) {
        self.mepc = mepc;
        self.mstatus |= MSTATUS_IN_TRAP;
        self.mstatus &= !MSTATUS_MIE;
    }
    pub fn exit(&mut self) {
        self.mstatus &= !MSTATUS_IN_TRAP;
        self.mstatus |= MSTATUS_MIE;
    }
    pub fn set_vtor(&mut self, vtor: u32) { self.vtor = vtor; }
}
