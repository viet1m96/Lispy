use crate::asm::Expr;
use crate::control::{self, ControlSignals, DecodedInstruction, MemAddrSel, OpASel, OpBSel, WbSel};
use crate::isa::{AluRKind, Instruction, Reg};
use crate::machine::Machine;
use crate::vector::{LaneComparator, LaneOffsetShifter, VectorAlu, VectorLaneAddressAdder};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BranchCompareFlags {
    pub eq: bool,
    pub lt: bool,
    pub gt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchCompareInput {
    pub rs1_value: u32,
    pub rs2_value: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BranchComparator;

impl BranchComparator {
    pub fn eval(self, input: BranchCompareInput) -> BranchCompareFlags {
        let lhs_signed = input.rs1_value as i32;
        let rhs_signed = input.rs2_value as i32;
        BranchCompareFlags {
            eq: input.rs1_value == input.rs2_value,
            lt: lhs_signed < rhs_signed,
            gt: lhs_signed > rhs_signed,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProgramCounter;

impl ProgramCounter {
    pub fn read(self, machine: &Machine) -> u32 {
        machine.pc
    }

    pub fn write(self, machine: &mut Machine, value: u32, enable: bool) -> Option<u32> {
        if enable {
            machine.pc = value;
            Some(value)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InstructionRegister;

impl InstructionRegister {
    pub fn read(self, machine: &Machine) -> u32 {
        machine.ir
    }

    pub fn write(self, machine: &mut Machine, value: u32, enable: bool) -> Option<u32> {
        if enable {
            machine.ir = value;
            Some(value)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterFile {
    regs: [u32; 32],
}

impl RegisterFile {
    pub fn new(stack_top: u32) -> Self {
        let mut regs = [0_u32; 32];
        regs[Reg::Sp.bits() as usize] = stack_top;
        Self { regs }
    }

    pub fn read(&self, reg: Reg) -> u32 {
        if reg == Reg::Zero {
            0
        } else {
            self.regs[reg.bits() as usize]
        }
    }

    pub fn write(&mut self, reg: Reg, value: u32, enable: bool) -> Option<(Reg, u32)> {
        if !enable {
            return None;
        }

        if reg == Reg::Zero {
            self.regs[0] = 0;
            None
        } else {
            self.regs[reg.bits() as usize] = value;
            Some((reg, value))
        }
    }

    pub fn force_zero(&mut self) {
        self.regs[0] = 0;
    }
}

impl Default for RegisterFile {
    fn default() -> Self {
        Self::new(0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AddPcPlus4;

impl AddPcPlus4 {
    pub fn eval(self, pc: u32) -> u32 {
        pc.wrapping_add(4)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MemAddrMux;

impl MemAddrMux {
    pub fn select(
        self,
        sel: MemAddrSel,
        pc: u32,
        alu_out: u32,
        trap_vector_addr: u32,
        vector_lane_addr: u32,
    ) -> u32 {
        match sel {
            MemAddrSel::Pc => pc,
            MemAddrSel::AluOut => alu_out,
            MemAddrSel::TrapVectorAddr => trap_vector_addr,
            MemAddrSel::VectorLaneAddr => vector_lane_addr,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpAMux;

impl OpAMux {
    pub fn select(self, sel: OpASel, pc: u32, rs1_value: u32) -> u32 {
        match sel {
            OpASel::Pc => pc,
            OpASel::Rs1 => rs1_value,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpBMux;

impl OpBMux {
    pub fn select(self, sel: OpBSel, rs2_value: u32, imm_value: u32) -> u32 {
        match sel {
            OpBSel::Rs2 => rs2_value,
            OpBSel::Imm => imm_value,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ImmediateGenerator;

impl ImmediateGenerator {
    pub fn generate(self, inst: &Instruction, sig: &ControlSignals) -> Result<u32, String> {
        match sig.imm_sel {
            control::ImmSel::None => Ok(0),
            control::ImmSel::I | control::ImmSel::S | control::ImmSel::B | control::ImmSel::J => {
                Ok(read_imm(inst)? as u32)
            }
            control::ImmSel::U => match inst {
                Instruction::Lui { imm20, .. } => upper_u_imm(imm20),
                _ => Err(format!(
                    "ImmGen U selected for non-lui instruction: {}",
                    inst.mnemonic()
                )),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Alu;

impl Alu {
    pub fn execute(self, op: AluRKind, lhs: u32, rhs: u32) -> Result<u32, String> {
        execute_alu(op, lhs, rhs)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WriteBackMux;

impl WriteBackMux {
    pub fn select(
        self,
        inst: &Instruction,
        sig: &ControlSignals,
        pc_plus4: u32,
        alu_out: Option<u32>,
        mem_data: Option<u32>,
        imm_value: u32,
    ) -> Result<u32, String> {
        match sig.wb_sel {
            WbSel::None => Err(format!(
                "WriteBack MUX selected None while reg_wr=1 for instruction: {}",
                inst.mnemonic()
            )),
            WbSel::Alu => alu_out.ok_or_else(|| {
                format!(
                    "WriteBack MUX selected ALU but ALU_out is missing: {}",
                    inst.mnemonic()
                )
            }),
            WbSel::Mem => mem_data.ok_or_else(|| {
                format!(
                    "WriteBack MUX selected Memory but mem_out is missing: {}",
                    inst.mnemonic()
                )
            }),
            WbSel::PcPlus4 => Ok(pc_plus4),
            WbSel::ImmUpper => Ok(imm_value),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PcMux;

impl PcMux {
    pub fn select(
        self,
        sig: &ControlSignals,
        pc_old: u32,
        alu_out: u32,
        trap_handler_addr: Option<u32>,
        mepc: Option<u32>,
    ) -> Result<u32, String> {
        control::select_next_pc(sig, pc_old, alu_out, trap_handler_addr, mepc)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TrapVectorAddressGenerator;

impl TrapVectorAddressGenerator {
    pub fn eval(self, vtor: u32, irq_id: u32) -> u32 {
        vtor.wrapping_add(irq_id.wrapping_mul(4))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryBlock;

impl MemoryBlock {
    pub fn read_word(
        self,
        machine: &Machine,
        addr: u32,
        enable: bool,
    ) -> Result<Option<u32>, String> {
        if enable {
            Ok(Some(machine.memory.load_u32(addr)?))
        } else {
            Ok(None)
        }
    }

    pub fn write_word(
        self,
        machine: &mut Machine,
        addr: u32,
        value: u32,
        enable: bool,
    ) -> Result<Option<(u32, u32)>, String> {
        if enable {
            machine.memory.store_u32(addr, value)?;
            Ok(Some((addr, value)))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MemWriteDataMux;

impl MemWriteDataMux {
    pub fn select(
        self,
        sel: control::MemWriteDataSel,
        rs2_value: u32,
        vector_lane_value: Option<u32>,
    ) -> Result<u32, String> {
        match sel {
            control::MemWriteDataSel::Rs2 => Ok(rs2_value),
            control::MemWriteDataSel::VecLane => vector_lane_value.ok_or_else(|| {
                "MemWriteDataMUX selected VecLane but VectorRF lane output is missing".to_string()
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Datapath {
    pub pc: ProgramCounter,
    pub ir: InstructionRegister,
    pub pc_plus4_adder: AddPcPlus4,
    pub imm_gen: ImmediateGenerator,
    pub opa_mux: OpAMux,
    pub opb_mux: OpBMux,
    pub alu: Alu,
    pub branch_comparator: BranchComparator,

    // Explicit vector datapath blocks from datapath_v2.
    pub vector_alu: VectorAlu,
    pub vector_lane_offset_shifter: LaneOffsetShifter,
    pub vector_lane_addr_adder: VectorLaneAddressAdder,
    pub lane_comparator: LaneComparator,

    pub trap_vector_addr_gen: TrapVectorAddressGenerator,
    pub mem_addr_mux: MemAddrMux,
    pub memory: MemoryBlock,
    pub mem_write_data_mux: MemWriteDataMux,
    pub wb_mux: WriteBackMux,
    pub pc_mux: PcMux,
}

impl Datapath {
    pub fn tick_fetch(self, machine: &mut Machine, sig: &ControlSignals) -> Result<String, String> {
        let pc_old = self.pc.read(machine);
        let pc_plus4 = self.pc_plus4_adder.eval(pc_old);
        let mem_addr = self.mem_addr_mux.select(sig.addr_sel, pc_old, 0, 0, 0);
        let mem_out = self
            .memory
            .read_word(machine, mem_addr, sig.mem_read)?
            .ok_or_else(|| "fetch needs mem_read=1".to_string())?;
        self.ir
            .write(machine, mem_out, sig.ir_write)
            .ok_or_else(|| "fetch needs ir_wr=1".to_string())?;

        Ok(format!(
            "signals: {}; MemAddrMUX(PC)=0x{mem_addr:08x}; Memory -> IR=0x{mem_out:08x}; ADD pc+4=0x{pc_plus4:08x}; PC hold=0x{pc_old:08x}",
            sig.signal_summary()
        ))
    }

    pub fn branch_flags(
        self,
        machine: &Machine,
        inst: &Instruction,
        decoded: &DecodedInstruction,
    ) -> Result<Option<BranchCompareFlags>, String> {
        if !decoded.is_branch {
            return Ok(None);
        }
        let rs1_value = read_rs1(machine, inst)?;
        let rs2_value = read_rs2(machine, inst)?;
        Ok(Some(self.branch_comparator.eval(BranchCompareInput {
            rs1_value,
            rs2_value,
        })))
    }

    pub fn tick_execute(
        self,
        machine: &mut Machine,
        inst: &Instruction,
        sig: &ControlSignals,
        pc_old: u32,
    ) -> Result<String, String> {
        let mut note_parts = vec![format!("signals: {}", sig.signal_summary())];

        if sig.halt_req {
            machine.set_halt("halt instruction");
            note_parts.push("ControlSignalGenerator halt_req=1; HALT latch <- 1".to_string());
            return Ok(note_parts.join("; "));
        }

        if sig.trap_exit {
            let mepc = machine.trap.mepc;
            let next_pc = self.pc_mux.select(sig, pc_old, 0, None, Some(mepc))?;
            machine.trap.exit();
            self.pc
                .write(machine, next_pc, sig.pc_write)
                .ok_or_else(|| "mret needs pc_wr=1".to_string())?;
            machine.refresh_interrupt_lines();
            note_parts.push(format!(
                "TrapBlock.mret: in_trap <- 0 mie <- 1; PC_MUX(MEPC=0x{mepc:08x}) -> PC <- 0x{next_pc:08x}"
            ));
            return Ok(note_parts.join("; "));
        }

        let pc_plus4 = self.pc_plus4_adder.eval(pc_old);
        let imm_value = self.imm_gen.generate(inst, sig)?;
        if sig.imm_sel != control::ImmSel::None {
            note_parts.push(format!("ImmGen({:?})=0x{imm_value:08x}", sig.imm_sel));
        }

        let rs1_value = read_rs1_optional(machine, inst).unwrap_or(0);
        let rs2_value = read_rs2_optional(machine, inst).unwrap_or(0);
        if instruction_reads_rs1(inst) {
            note_parts.push(format!("RegisterFile.rs1=0x{rs1_value:08x}"));
        }
        if instruction_reads_rs2(inst) {
            note_parts.push(format!("RegisterFile.rs2=0x{rs2_value:08x}"));
        }

        let alu_out = if let Some(op) = sig.alu_op {
            let lhs = self.opa_mux.select(sig.opa_sel, pc_old, rs1_value);
            let rhs = self.opb_mux.select(sig.opb_sel, rs2_value, imm_value);
            let value = self.alu.execute(op, lhs, rhs)?;
            note_parts.push(format!(
                "OpA_MUX={:?}->0x{lhs:08x}; OpB_MUX={:?}->0x{rhs:08x}; ALU({op:?})=0x{value:08x}",
                sig.opa_sel, sig.opb_sel
            ));
            Some(value)
        } else {
            None
        };

        if let Some(flags) = sig.branch_flags {
            note_parts.push(format!(
                "BranchComparator eq={} lt={} gt={}; BranchDecision take_branch={}",
                bit(flags.eq),
                bit(flags.lt),
                bit(flags.gt),
                bit(sig.take_branch.unwrap_or(false))
            ));
        }

        match inst {
            Instruction::Vld { vd, .. } if sig.start_vec_op => {
                let base = alu_out
                    .ok_or_else(|| "vld setup requires scalar ALU_out base address".to_string())?;
                machine
                    .vector
                    .base_register
                    .write(base, sig.vector_base_write)
                    .ok_or_else(|| "vld setup needs vector_base_write=1".to_string())?;
                machine
                    .vector
                    .lane_counter
                    .reset(sig.lane_counter_reset)
                    .ok_or_else(|| "vld setup needs lane_counter_reset=1".to_string())?;
                note_parts.push(format!(
                    "VectorBaseRegister <- ALU_out=0x{base:08x}; LaneCounterRegister <- 0; IR keeps active instruction vld {vd}"
                ));
            }
            Instruction::Vst { vs, .. } if sig.start_vec_op => {
                let base = alu_out
                    .ok_or_else(|| "vst setup requires scalar ALU_out base address".to_string())?;
                machine
                    .vector
                    .base_register
                    .write(base, sig.vector_base_write)
                    .ok_or_else(|| "vst setup needs vector_base_write=1".to_string())?;
                machine
                    .vector
                    .lane_counter
                    .reset(sig.lane_counter_reset)
                    .ok_or_else(|| "vst setup needs lane_counter_reset=1".to_string())?;
                note_parts.push(format!(
                    "VectorBaseRegister <- ALU_out=0x{base:08x}; LaneCounterRegister <- 0; IR keeps active instruction vst {vs}"
                ));
            }
            Instruction::VectorR { vd, vs1, vs2, .. } if sig.vector_full_write => {
                let op = sig
                    .vector_alu_op
                    .ok_or_else(|| "VectorR needs vec_alu_op from ControlUnit".to_string())?;
                let lhs = machine.vector.register_file.read(*vs1);
                let rhs = machine.vector.register_file.read(*vs2);
                let result = self.vector_alu.execute(op, lhs, rhs)?;
                machine
                    .vector
                    .register_file
                    .write_full_from_alu(*vd, result, sig.vector_full_write)
                    .ok_or_else(|| "VectorR needs vector_full_write=1".to_string())?;
                note_parts.push(format!(
                    "VectorRegisterFile.{vs1}={lhs:?}; VectorRegisterFile.{vs2}={rhs:?}; vec_alu_op={op:?}; VectorALU=vec_res={result:?}; vec_full_wr -> VectorRegisterFile.{vd}"
                ));
            }
            _ => {}
        }

        let mem_addr = if sig.mem_read || sig.mem_write {
            let alu_value = alu_out.ok_or_else(|| {
                format!(
                    "memory access requested but ALU_out address is missing: {}",
                    inst.mnemonic()
                )
            })?;
            let addr = self
                .mem_addr_mux
                .select(sig.addr_sel, pc_old, alu_value, 0, 0);
            note_parts.push(format!("MemAddrMUX({:?})=0x{addr:08x}", sig.addr_sel));
            Some(addr)
        } else {
            None
        };

        if sig.mem_write {
            let addr = mem_addr.expect("mem_addr exists when mem_write is set");
            let value = self
                .mem_write_data_mux
                .select(sig.mem_write_data_sel, rs2_value, None)?;
            if let Some((addr, value)) = self.memory.write_word(machine, addr, value, true)? {
                note_parts.push(format!(
                    "MemWriteDataMUX({:?})=0x{value:08x}; Memory[0x{addr:08x}] <- 0x{value:08x}",
                    sig.mem_write_data_sel
                ));
                machine.refresh_interrupt_lines();
            }
        }

        let mem_data = if sig.mem_read {
            let addr = mem_addr.expect("mem_addr exists when mem_read is set");
            let value = self
                .memory
                .read_word(machine, addr, true)?
                .expect("enabled memory read returns data");
            note_parts.push(format!("Memory[0x{addr:08x}] -> mem_out=0x{value:08x}"));
            Some(value)
        } else {
            None
        };

        if sig.reg_write {
            let rd = writeback_dest(inst)?;
            let wb_value = self
                .wb_mux
                .select(inst, sig, pc_plus4, alu_out, mem_data, imm_value)?;
            let wb_note = match machine.register_file.write(rd, wb_value, sig.reg_write) {
                Some((reg, value)) => format!(
                    "WriteBackMUX({:?})=0x{value:08x}; RegisterFile.{reg} <- 0x{value:08x}",
                    sig.wb_sel
                ),
                None => format!(
                    "WriteBackMUX({:?})=0x{wb_value:08x}; RegisterFile.x0 write discarded",
                    sig.wb_sel
                ),
            };
            note_parts.push(wb_note);
        }

        let next_pc = self
            .pc_mux
            .select(sig, pc_old, alu_out.unwrap_or(0), None, None)?;
        if let Some(value) = self.pc.write(machine, next_pc, sig.pc_write) {
            note_parts.push(format!("PC_MUX({:?}) -> PC <- 0x{value:08x}", sig.pc_sel));
        }
        machine.register_file.force_zero();

        Ok(note_parts.join("; "))
    }
    pub fn tick_vec_op(
        self,
        machine: &mut Machine,
        inst: &Instruction,
        sig: &ControlSignals,
        pc_old: u32,
    ) -> Result<String, String> {
        let mut note_parts = vec![format!("signals: {}", sig.signal_summary())];

        let lane = machine.vector.lane_counter.read();
        let base = machine.vector.base_register.read();
        let lane_done_before = self.lane_comparator.eval(lane);
        let lane_offset = self.vector_lane_offset_shifter.eval(lane);
        let lane_addr = self.vector_lane_addr_adder.eval(base, lane_offset);
        let mem_addr = self
            .mem_addr_mux
            .select(sig.addr_sel, pc_old, 0, 0, lane_addr);

        note_parts.push(format!(
            "VectorBaseRegister=0x{base:08x}; LaneCounterRegister={lane}; LaneOffsetShifter(lane<<2)=0x{lane_offset:08x}; VectorLaneAddressAdder=0x{lane_addr:08x}; MemAddrMUX(VectorLaneAddr)=0x{mem_addr:08x}"
        ));

        match inst {
            Instruction::Vld { vd, .. } => {
                if !sig.mem_read || !sig.vector_lane_write {
                    return Err("vld VEC_OP needs mem_read=1 and vector_lane_write=1".to_string());
                }

                let mem_lane = self
                    .memory
                    .read_word(machine, mem_addr, true)?
                    .expect("enabled memory read returns data");

                machine
                    .vector
                    .register_file
                    .write_lane_from_memory(*vd, lane, mem_lane, sig.vector_lane_write)?
                    .ok_or_else(|| "vld needs vector_lane_write=1".to_string())?;

                note_parts.push(format!(
                    "Memory[0x{mem_addr:08x}] -> mem_out=0x{mem_lane:08x}; vec_lane_wr -> VectorRegisterFile.{vd}[{lane}]"
                ));
            }
            Instruction::Vst { vs, .. } => {
                if !sig.mem_write || !sig.vector_lane_read {
                    return Err("vst VEC_OP needs mem_write=1 and vector_lane_read=1".to_string());
                }

                let vec_lane = machine
                    .vector
                    .register_file
                    .read_lane_to_memory(*vs, lane, sig.vector_lane_read)?
                    .ok_or_else(|| "vst needs vector_lane_read=1".to_string())?;
                let value =
                    self.mem_write_data_mux
                        .select(sig.mem_write_data_sel, 0, Some(vec_lane))?;

                self.memory.write_word(machine, mem_addr, value, true)?;
                machine.refresh_interrupt_lines();
                note_parts.push(format!(
                    "VectorRegisterFile.{vs}[{lane}]=0x{vec_lane:08x}; vec_lane_out -> MemWriteDataMUX(VecLane)=0x{value:08x}; Memory[0x{mem_addr:08x}] <- 0x{value:08x}"
                ));
            }
            other => {
                return Err(format!(
                    "VEC_OP phase expected vld/vst in IR, got {}",
                    other.mnemonic()
                ));
            }
        }

        let next_lane = machine
            .vector
            .lane_counter
            .increment_or_clear(lane_done_before, sig.lane_counter_inc)
            .ok_or_else(|| "VEC_OP needs lane_counter_inc=1".to_string())?;

        if lane_done_before {
            let next_pc = self.pc_mux.select(sig, pc_old, 0, None, None)?;
            self.pc
                .write(machine, next_pc, sig.pc_write)
                .ok_or_else(|| "last vector lane needs pc_wr=1".to_string())?;
            note_parts.push(format!("PC_MUX({:?}) -> PC <- 0x{next_pc:08x}", sig.pc_sel));
        }

        note_parts.push(format!(
            "LaneComparator(lane_done={}); LaneCounterRegister {}",
            bit(lane_done_before),
            if lane_done_before {
                "finished -> 0".to_string()
            } else {
                format!("<- {next_lane}")
            }
        ));

        Ok(note_parts.join("; "))
    }

    pub fn tick_trap_enter(
        self,
        machine: &mut Machine,
        sig: &ControlSignals,
    ) -> Result<String, String> {
        if !sig.trap_enter {
            return Err("trap_enter phase needs trap_enter=1".to_string());
        }
        let mepc = machine.pc;
        let irq_id = machine.interrupt_lines.irq_id;
        let vector_addr = self.trap_vector_addr_gen.eval(machine.trap.vtor, irq_id);
        let mem_addr = self
            .mem_addr_mux
            .select(sig.addr_sel, mepc, 0, vector_addr, 0);
        let handler_addr = self
            .memory
            .read_word(machine, mem_addr, sig.mem_read)?
            .ok_or_else(|| "trap enter needs mem_read=1".to_string())?;
        let next_pc = self.pc_mux.select(sig, mepc, 0, Some(handler_addr), None)?;
        machine.trap.enter(mepc);
        self.pc
            .write(machine, next_pc, sig.pc_write)
            .ok_or_else(|| "trap enter needs pc_wr=1".to_string())?;
        Ok(format!(
            "signals: {}; TrapBlock: mepc <- 0x{mepc:08x}, irq_id={irq_id}, in_trap <- 1, mie <- 0; TrapVectorAddressGenerator(VTOR+irq_id*4)=0x{vector_addr:08x}; MemAddrMUX(TrapVectorAddr)=0x{mem_addr:08x}; Memory[0x{mem_addr:08x}] -> handler=0x{handler_addr:08x}; PC_MUX(TrapVector=0x{handler_addr:08x}) -> PC <- 0x{next_pc:08x}",
            sig.signal_summary()
        ))
    }
}

fn bit(value: bool) -> u8 {
    u8::from(value)
}

pub fn tick_fetch(machine: &mut Machine, sig: &ControlSignals) -> Result<String, String> {
    Datapath::default().tick_fetch(machine, sig)
}

pub fn branch_feedback(
    machine: &Machine,
    inst: &Instruction,
    decoded: &DecodedInstruction,
) -> Result<Option<BranchCompareFlags>, String> {
    Datapath::default().branch_flags(machine, inst, decoded)
}

pub fn apply_execute(
    machine: &mut Machine,
    inst: &Instruction,
    sig: &ControlSignals,
    pc_old: u32,
) -> Result<String, String> {
    Datapath::default().tick_execute(machine, inst, sig, pc_old)
}

pub fn apply_vec_op(
    machine: &mut Machine,
    inst: &Instruction,
    sig: &ControlSignals,
    pc_old: u32,
) -> Result<String, String> {
    Datapath::default().tick_vec_op(machine, inst, sig, pc_old)
}

pub fn apply_trap_enter(machine: &mut Machine, sig: &ControlSignals) -> Result<String, String> {
    Datapath::default().tick_trap_enter(machine, sig)
}

fn writeback_dest(inst: &Instruction) -> Result<Reg, String> {
    match inst {
        Instruction::Lui { rd, .. }
        | Instruction::Addi { rd, .. }
        | Instruction::Lw { rd, .. }
        | Instruction::AluR { rd, .. }
        | Instruction::Jal { rd, .. }
        | Instruction::Jalr { rd, .. } => Ok(*rd),
        Instruction::Sw { .. }
        | Instruction::Branch { .. }
        | Instruction::Mret
        | Instruction::Halt
        | Instruction::Vld { .. }
        | Instruction::Vst { .. }
        | Instruction::VectorR { .. } => Err(format!(
            "instruction has no scalar writeback destination: {}",
            inst.mnemonic()
        )),
    }
}

fn instruction_reads_rs1(inst: &Instruction) -> bool {
    matches!(
        inst,
        Instruction::Addi { .. }
            | Instruction::Lw { .. }
            | Instruction::Sw { .. }
            | Instruction::AluR { .. }
            | Instruction::Branch { .. }
            | Instruction::Jalr { .. }
            | Instruction::Vld { .. }
            | Instruction::Vst { .. }
    )
}

fn instruction_reads_rs2(inst: &Instruction) -> bool {
    matches!(
        inst,
        Instruction::Sw { .. } | Instruction::AluR { .. } | Instruction::Branch { .. }
    )
}

fn read_rs1_optional(machine: &Machine, inst: &Instruction) -> Option<u32> {
    read_rs1(machine, inst).ok()
}

fn read_rs2_optional(machine: &Machine, inst: &Instruction) -> Option<u32> {
    read_rs2(machine, inst).ok()
}

fn read_rs1(machine: &Machine, inst: &Instruction) -> Result<u32, String> {
    let reg = match inst {
        Instruction::Addi { rs1, .. }
        | Instruction::Lw { rs1, .. }
        | Instruction::Sw { rs1, .. }
        | Instruction::AluR { rs1, .. }
        | Instruction::Branch { rs1, .. }
        | Instruction::Jalr { rs1, .. }
        | Instruction::Vld { rs1, .. }
        | Instruction::Vst { rs1, .. } => *rs1,
        Instruction::Lui { .. }
        | Instruction::Jal { .. }
        | Instruction::Mret
        | Instruction::Halt
        | Instruction::VectorR { .. } => {
            return Err(format!("instruction has no rs1 field: {}", inst.mnemonic()));
        }
    };
    Ok(machine.register_file.read(reg))
}

fn read_rs2(machine: &Machine, inst: &Instruction) -> Result<u32, String> {
    let reg = match inst {
        Instruction::Sw { rs2, .. }
        | Instruction::AluR { rs2, .. }
        | Instruction::Branch { rs2, .. } => *rs2,
        Instruction::Addi { .. }
        | Instruction::Lw { .. }
        | Instruction::Lui { .. }
        | Instruction::Jal { .. }
        | Instruction::Jalr { .. }
        | Instruction::Mret
        | Instruction::Halt
        | Instruction::Vld { .. }
        | Instruction::Vst { .. }
        | Instruction::VectorR { .. } => {
            return Err(format!("instruction has no rs2 field: {}", inst.mnemonic()));
        }
    };
    Ok(machine.register_file.read(reg))
}

fn read_imm(inst: &Instruction) -> Result<i32, String> {
    match inst {
        Instruction::Addi { imm, .. }
        | Instruction::Lw { off: imm, .. }
        | Instruction::Sw { off: imm, .. }
        | Instruction::Vld { off: imm, .. }
        | Instruction::Vst { off: imm, .. }
        | Instruction::Branch { off: imm, .. }
        | Instruction::Jal { off: imm, .. }
        | Instruction::Jalr { off: imm, .. } => resolved_i32(imm),
        Instruction::Lui { .. }
        | Instruction::AluR { .. }
        | Instruction::Mret
        | Instruction::Halt
        | Instruction::VectorR { .. } => Err(format!(
            "instruction has no scalar immediate field: {}",
            inst.mnemonic()
        )),
    }
}

fn upper_u_imm(expr: &Expr) -> Result<u32, String> {
    Ok((resolved_i32(expr)? as u32) << 12)
}

fn resolved_i32(expr: &Expr) -> Result<i32, String> {
    expr.resolved_i32()
        .ok_or_else(|| format!("execute saw unresolved expression: {expr}"))
}

fn execute_alu(op: AluRKind, lhs: u32, rhs_u: u32) -> Result<u32, String> {
    let rhs = rhs_u as i32;
    let value = match op {
        AluRKind::Add => lhs.wrapping_add(rhs_u),
        AluRKind::Sub => lhs.wrapping_sub(rhs_u),
        AluRKind::And => lhs & rhs_u,
        AluRKind::Or => lhs | rhs_u,
        AluRKind::Xor => lhs ^ rhs_u,
        AluRKind::Sll => lhs.wrapping_shl(rhs_u & 0x1f),
        AluRKind::Srl => lhs.wrapping_shr(rhs_u & 0x1f),
        AluRKind::Sra => ((lhs as i32) >> (rhs_u & 0x1f)) as u32,
        AluRKind::Slt => u32::from((lhs as i32) < rhs),
        AluRKind::Sltu => u32::from(lhs < rhs_u),
        AluRKind::Mul => lhs.wrapping_mul(rhs_u),
        AluRKind::Mulh => mulh(lhs, rhs_u),
        AluRKind::Mulhsu => mulhsu(lhs, rhs_u),
        AluRKind::Mulhu => mulhu(lhs, rhs_u),
        AluRKind::Div => signed_div(lhs, rhs_u)?,
        AluRKind::Divu => unsigned_div(lhs, rhs_u)?,
        AluRKind::Rem => signed_rem(lhs, rhs_u)?,
        AluRKind::Remu => unsigned_rem(lhs, rhs_u)?,
    };
    Ok(value)
}

fn signed_div(lhs: u32, rhs: u32) -> Result<u32, String> {
    if rhs == 0 {
        return Err("division by zero".to_string());
    }

    let lhs = lhs as i32;
    let rhs = rhs as i32;
    let value = if lhs == i32::MIN && rhs == -1 {
        i32::MIN
    } else {
        lhs / rhs
    };
    Ok(value as u32)
}

fn unsigned_div(lhs: u32, rhs: u32) -> Result<u32, String> {
    if rhs == 0 {
        return Err("division by zero".to_string());
    }
    Ok(lhs / rhs)
}

fn signed_rem(lhs: u32, rhs: u32) -> Result<u32, String> {
    if rhs == 0 {
        return Err("remainder by zero".to_string());
    }

    let lhs = lhs as i32;
    let rhs = rhs as i32;
    let value = if lhs == i32::MIN && rhs == -1 {
        0
    } else {
        lhs % rhs
    };
    Ok(value as u32)
}

fn unsigned_rem(lhs: u32, rhs: u32) -> Result<u32, String> {
    if rhs == 0 {
        return Err("remainder by zero".to_string());
    }
    Ok(lhs % rhs)
}

fn mulh(lhs: u32, rhs: u32) -> u32 {
    let wide = (lhs as i32 as i64) * (rhs as i32 as i64);
    ((wide >> 32) & 0xffff_ffff) as u32
}

fn mulhsu(lhs: u32, rhs: u32) -> u32 {
    let wide = (lhs as i32 as i64) * (rhs as u64 as i64);
    ((wide >> 32) & 0xffff_ffff) as u32
}

fn mulhu(lhs: u32, rhs: u32) -> u32 {
    let wide = (lhs as u64) * (rhs as u64);
    ((wide >> 32) & 0xffff_ffff) as u32
}
