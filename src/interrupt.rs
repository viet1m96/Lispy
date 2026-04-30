pub const INPUT_IRQ_ID: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InterruptLines {
    pub pending: bool,
    pub irq_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterruptRequestInput {
    pub irq_pending: bool,
    pub mie: bool,
    pub in_trap: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InterruptRequestLogic;

impl InterruptRequestLogic {
    pub fn eval(self, input: InterruptRequestInput) -> bool {
        input.irq_pending && input.mie && !input.in_trap
    }
}
