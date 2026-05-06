use crate::control::ControlUnitState;
use crate::datapath::RegisterFile;
use crate::image::ProgramImage;
use crate::input_device::InputDevice;
use crate::interrupt::InterruptLines;
use crate::isa::Reg;
use crate::memory_state::MemoryState;
use crate::trap::TrapState;
use crate::vector::VectorState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Fetch,
    Execute,
    VecOp,
    TrapEnter,
    Halt,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Execute => "execute",
            Self::VecOp => "vec_op",
            Self::TrapEnter => "trap_enter",
            Self::Halt => "halt",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Machine {
    pub register_file: RegisterFile,
    pub pc: u32,
    pub ir: u32,
    pub control_state: ControlUnitState,
    pub tick: u64,
    pub halted: bool,
    pub halt_reason: Option<String>,
    pub memory: MemoryState,
    pub trap: TrapState,
    pub vector: VectorState,
    pub input_device: InputDevice,
    pub interrupt_lines: InterruptLines,
}

impl Machine {
    pub fn from_image(image: &ProgramImage) -> Result<Self, String> {
        let mut trap = TrapState::default();
        trap.set_vtor(image.layout.text_base);
        Ok(Self {
            register_file: RegisterFile::new(image.layout.stack_top),
            pc: image.entry,
            ir: 0,
            control_state: ControlUnitState::default(),
            tick: 0,
            halted: false,
            halt_reason: None,
            memory: MemoryState::from_image(image)?,
            trap,
            vector: VectorState::default(),
            input_device: InputDevice::default(),
            interrupt_lines: InterruptLines::default(),
        })
    }

    pub fn phase(&self) -> Phase {
        self.control_state.phase()
    }

    pub fn clock_control(&mut self, reset: bool, next_state: Phase) {
        self.control_state.clock(reset, next_state);
    }

    pub fn read_reg(&self, reg: Reg) -> u32 {
        self.register_file.read(reg)
    }

    pub fn write_reg(&mut self, reg: Reg, value: u32) -> Option<(Reg, u32)> {
        self.register_file.write(reg, value, true)
    }

    pub fn force_zero_reg(&mut self) {
        self.register_file.force_zero();
    }

    pub fn set_halt(&mut self, reason: impl Into<String>) {
        self.halted = true;
        self.halt_reason = Some(reason.into());
    }

    pub fn output_as_string(&self) -> String {
        self.memory.output_as_string()
    }

    pub fn load_input_schedule_text(&mut self, text: &str) -> Result<(), String> {
        self.input_device.load_schedule_text(text)
    }

    pub fn tick_devices(&mut self) -> Vec<String> {
        let result = self.input_device.tick(self.tick, &mut self.memory);
        self.interrupt_lines = result.interrupt_lines;
        result.events
    }

    pub fn refresh_interrupt_lines(&mut self) {
        self.interrupt_lines.pending = self.memory.input_interrupt_pending();
        if !self.interrupt_lines.pending {
            self.interrupt_lines.irq_id = 0;
        }
    }
}
