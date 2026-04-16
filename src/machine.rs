use crate::image::ProgramImage;
use crate::isa::Reg;
use crate::memory::Memory;
use crate::trap::TrapState;
use crate::vector::VectorState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Fetch,
    Execute,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Execute => "execute",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Machine {
    pub regs: [u32; 32],
    pub pc: u32,
    pub ir: u32,
    pub phase: Phase,
    pub tick: u64,
    pub halted: bool,
    pub halt_reason: Option<String>,
    pub memory: Memory,
    pub trap: TrapState,
    pub vector: VectorState,
}

impl Machine {
    pub fn from_image(image: &ProgramImage) -> Result<Self, String> {
        let mut regs = [0_u32; 32];
        regs[Reg::Sp.bits() as usize] = image.layout.stack_top;

        Ok(Self {
            regs,
            pc: image.entry,
            ir: 0,
            phase: Phase::Fetch,
            tick: 0,
            halted: false,
            halt_reason: None,
            memory: Memory::from_image(image)?,
            trap: TrapState::default(),
            vector: VectorState::default(),
        })
    }

    pub fn read_reg(&self, reg: Reg) -> u32 {
        if reg == Reg::Zero {
            0
        } else {
            self.regs[reg.bits() as usize]
        }
    }

    pub fn write_reg(&mut self, reg: Reg, value: u32) -> Option<(Reg, u32)> {
        if reg == Reg::Zero {
            self.regs[0] = 0;
            None
        } else {
            self.regs[reg.bits() as usize] = value;
            Some((reg, value))
        }
    }

    pub fn force_zero_reg(&mut self) {
        self.regs[0] = 0;
    }

    pub fn set_halt(&mut self, reason: impl Into<String>) {
        self.halted = true;
        self.halt_reason = Some(reason.into());
    }

    pub fn output_as_string(&self) -> String {
        self.memory.output_as_string()
    }
}
