use crate::datapath::BranchCompareFlags;
use crate::interrupt::{InterruptRequestInput, InterruptRequestLogic};
use crate::isa::{AluRKind, BranchKind, Instruction};
use crate::machine::Phase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrClass {
    Lui,
    AluI,
    Load,
    Store,
    AluR,
    Branch,
    Jal,
    Jalr,
    Mret,
    Halt,
    VectorLoad,
    VectorStore,
    VectorR,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImmSel {
    None,
    I,
    S,
    B,
    U,
    J,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AluDecodeInfo {
    NoAlu,
    UseFixed(AluRKind),
    UseRType(AluRKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpASel {
    Pc,
    Rs1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpBSel {
    Rs2,
    Imm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WbSel {
    None,
    Alu,
    Mem,
    PcPlus4,
    ImmUpper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcSel {
    PcPlus4,
    AluTarget,
    Branch,
    TrapVector,
    Mepc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemAddrSel {
    Pc,
    AluOut,
    TrapVectorAddr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemWriteDataSel {
    Rs2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedInstruction {
    pub instr_class: InstrClass,
    pub imm_sel: ImmSel,
    pub is_branch: bool,
    pub branch_kind: Option<BranchKind>,
    pub is_halt: bool,
    pub alu_dec_info: AluDecodeInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlSignals {
    pub phase: Phase,
    pub instr_class: Option<InstrClass>,

    pub pc_write: bool,
    pub ir_write: bool,
    pub reg_write: bool,
    pub mem_read: bool,
    pub mem_write: bool,
    pub halt_req: bool,
    pub trap_enter: bool,
    pub trap_exit: bool,

    pub addr_sel: MemAddrSel,
    pub opa_sel: OpASel,
    pub opb_sel: OpBSel,
    pub wb_sel: WbSel,
    pub pc_sel: PcSel,
    pub imm_sel: ImmSel,
    pub mem_write_data_sel: MemWriteDataSel,

    pub alu_op: Option<AluRKind>,
    pub branch_kind: Option<BranchKind>,
    pub branch_flags: Option<BranchCompareFlags>,
    pub take_branch: Option<bool>,
}

impl ControlSignals {
    pub fn inactive(phase: Phase) -> Self {
        Self {
            phase,
            instr_class: None,
            pc_write: false,
            ir_write: false,
            reg_write: false,
            mem_read: false,
            mem_write: false,
            halt_req: false,
            trap_enter: false,
            trap_exit: false,
            addr_sel: MemAddrSel::Pc,
            opa_sel: OpASel::Rs1,
            opb_sel: OpBSel::Rs2,
            wb_sel: WbSel::None,
            pc_sel: PcSel::PcPlus4,
            imm_sel: ImmSel::None,
            mem_write_data_sel: MemWriteDataSel::Rs2,
            alu_op: None,
            branch_kind: None,
            branch_flags: None,
            take_branch: None,
        }
    }

    pub fn signal_summary(&self) -> String {
        format!(
            "pc_wr={} ir_wr={} reg_wr={} mem_rd={} mem_wr={} addr_sel={:?} opa_sel={:?} opb_sel={:?} imm_sel={:?} alu_op={:?} wb_sel={:?} pc_sel={:?} halt_req={} trap_enter={} trap_exit={} take_branch={:?}",
            bit(self.pc_write),
            bit(self.ir_write),
            bit(self.reg_write),
            bit(self.mem_read),
            bit(self.mem_write),
            self.addr_sel,
            self.opa_sel,
            self.opb_sel,
            self.imm_sel,
            self.alu_op,
            self.wb_sel,
            self.pc_sel,
            bit(self.halt_req),
            bit(self.trap_enter),
            bit(self.trap_exit),
            self.take_branch,
        )
    }
}

fn bit(value: bool) -> u8 {
    u8::from(value)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InstructionDecoder;

impl InstructionDecoder {
    pub fn decode(self, inst: &Instruction) -> DecodedInstruction {
        match inst {
            Instruction::Lui { .. } => DecodedInstruction {
                instr_class: InstrClass::Lui,
                imm_sel: ImmSel::U,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::NoAlu,
            },
            Instruction::Addi { .. } => DecodedInstruction {
                instr_class: InstrClass::AluI,
                imm_sel: ImmSel::I,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Lw { .. } => DecodedInstruction {
                instr_class: InstrClass::Load,
                imm_sel: ImmSel::I,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Sw { .. } => DecodedInstruction {
                instr_class: InstrClass::Store,
                imm_sel: ImmSel::S,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::AluR { op, .. } => DecodedInstruction {
                instr_class: InstrClass::AluR,
                imm_sel: ImmSel::None,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::UseRType(*op),
            },
            Instruction::Branch { op, .. } => DecodedInstruction {
                instr_class: InstrClass::Branch,
                imm_sel: ImmSel::B,
                is_branch: true,
                branch_kind: Some(*op),
                is_halt: false,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Jal { .. } => DecodedInstruction {
                instr_class: InstrClass::Jal,
                imm_sel: ImmSel::J,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Jalr { .. } => DecodedInstruction {
                instr_class: InstrClass::Jalr,
                imm_sel: ImmSel::I,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Mret => DecodedInstruction {
                instr_class: InstrClass::Mret,
                imm_sel: ImmSel::None,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::NoAlu,
            },
            Instruction::Halt => DecodedInstruction {
                instr_class: InstrClass::Halt,
                imm_sel: ImmSel::None,
                is_branch: false,
                branch_kind: None,
                is_halt: true,
                alu_dec_info: AluDecodeInfo::NoAlu,
            },
            Instruction::Vld { .. } => DecodedInstruction {
                instr_class: InstrClass::VectorLoad,
                imm_sel: ImmSel::I,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Vst { .. } => DecodedInstruction {
                instr_class: InstrClass::VectorStore,
                imm_sel: ImmSel::S,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::VectorR { .. } => DecodedInstruction {
                instr_class: InstrClass::VectorR,
                imm_sel: ImmSel::None,
                is_branch: false,
                branch_kind: None,
                is_halt: false,
                alu_dec_info: AluDecodeInfo::NoAlu,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AluDecoder;

impl AluDecoder {
    pub fn decode(self, info: AluDecodeInfo) -> Option<AluRKind> {
        match info {
            AluDecodeInfo::NoAlu => None,
            AluDecodeInfo::UseFixed(op) | AluDecodeInfo::UseRType(op) => Some(op),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchDecisionOutput {
    pub kind: BranchKind,
    pub flags: BranchCompareFlags,
    pub taken: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BranchDecision;

impl BranchDecision {
    pub fn decide(self, kind: BranchKind, flags: BranchCompareFlags) -> BranchDecisionOutput {
        let taken = match kind {
            BranchKind::Beq => flags.eq,
            BranchKind::Bne => !flags.eq,
            BranchKind::Blt => flags.lt,
            BranchKind::Bge => flags.eq || flags.gt,
        };
        BranchDecisionOutput { kind, flags, taken }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ControlSignalGenerator;

impl ControlSignalGenerator {
    pub fn fetch_signals(self) -> ControlSignals {
        let mut sig = ControlSignals::inactive(Phase::Fetch);
        sig.ir_write = true;
        sig.mem_read = true;
        sig.addr_sel = MemAddrSel::Pc;
        sig.pc_sel = PcSel::PcPlus4;
        sig
    }

    pub fn execute_signals(
        self,
        decoded: DecodedInstruction,
        alu_op: Option<AluRKind>,
        branch_decision: Option<BranchDecisionOutput>,
    ) -> Result<ControlSignals, String> {
        let mut sig = ControlSignals::inactive(Phase::Execute);
        sig.instr_class = Some(decoded.instr_class);
        sig.imm_sel = decoded.imm_sel;
        sig.alu_op = alu_op;
        sig.branch_kind = decoded.branch_kind;
        sig.pc_write = true;

        match decoded.instr_class {
            InstrClass::Lui => {
                sig.reg_write = true;
                sig.wb_sel = WbSel::ImmUpper;
                sig.pc_sel = PcSel::PcPlus4;
            }
            InstrClass::AluI => {
                sig.reg_write = true;
                sig.opa_sel = OpASel::Rs1;
                sig.opb_sel = OpBSel::Imm;
                sig.wb_sel = WbSel::Alu;
                sig.pc_sel = PcSel::PcPlus4;
            }
            InstrClass::Load => {
                sig.reg_write = true;
                sig.mem_read = true;
                sig.addr_sel = MemAddrSel::AluOut;
                sig.opa_sel = OpASel::Rs1;
                sig.opb_sel = OpBSel::Imm;
                sig.wb_sel = WbSel::Mem;
                sig.pc_sel = PcSel::PcPlus4;
            }
            InstrClass::Store => {
                sig.mem_write = true;
                sig.addr_sel = MemAddrSel::AluOut;
                sig.mem_write_data_sel = MemWriteDataSel::Rs2;
                sig.opa_sel = OpASel::Rs1;
                sig.opb_sel = OpBSel::Imm;
                sig.pc_sel = PcSel::PcPlus4;
            }
            InstrClass::AluR => {
                sig.reg_write = true;
                sig.opa_sel = OpASel::Rs1;
                sig.opb_sel = OpBSel::Rs2;
                sig.wb_sel = WbSel::Alu;
                sig.pc_sel = PcSel::PcPlus4;
            }
            InstrClass::Branch => {
                let decision = branch_decision.ok_or_else(|| {
                    "branch instruction needs eq/lt/gt feedback from BranchComparator".to_string()
                })?;
                sig.opa_sel = OpASel::Pc;
                sig.opb_sel = OpBSel::Imm;
                sig.pc_sel = PcSel::Branch;
                sig.branch_flags = Some(decision.flags);
                sig.take_branch = Some(decision.taken);
            }
            InstrClass::Jal => {
                sig.reg_write = true;
                sig.opa_sel = OpASel::Pc;
                sig.opb_sel = OpBSel::Imm;
                sig.wb_sel = WbSel::PcPlus4;
                sig.pc_sel = PcSel::AluTarget;
            }
            InstrClass::Jalr => {
                sig.reg_write = true;
                sig.opa_sel = OpASel::Rs1;
                sig.opb_sel = OpBSel::Imm;
                sig.wb_sel = WbSel::PcPlus4;
                sig.pc_sel = PcSel::AluTarget;
            }
            InstrClass::Halt => {
                sig.pc_write = false;
                sig.halt_req = true;
            }
            InstrClass::Mret => {
                sig.pc_sel = PcSel::Mepc;
                sig.trap_exit = true;
            }
            InstrClass::VectorLoad | InstrClass::VectorStore | InstrClass::VectorR => {
                return Err("vector instructions are decoded, but execute path is reserved for vector milestone".to_string());
            }
        }

        Ok(sig)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlInternalSignals {
    pub state_q: Phase,
    pub state_d: Phase,
    pub reset: bool,
    pub decoded: Option<DecodedInstruction>,
    pub alu_decode_info: Option<AluDecodeInfo>,
    pub alu_op: Option<AluRKind>,
    pub branch_flags_in: Option<BranchCompareFlags>,
    pub branch_decision: Option<BranchDecisionOutput>,
    pub irq_pending_in: Option<bool>,
    pub mie_in: Option<bool>,
    pub in_trap_in: Option<bool>,
    pub irq_req: bool,
}

impl ControlInternalSignals {
    pub fn fetch(state_q: Phase, state_d: Phase) -> Self {
        Self {
            state_q,
            state_d,
            reset: false,
            decoded: None,
            alu_decode_info: None,
            alu_op: None,
            branch_flags_in: None,
            branch_decision: None,
            irq_pending_in: None,
            mie_in: None,
            in_trap_in: None,
            irq_req: false,
        }
    }

    pub fn execute(
        state_q: Phase,
        state_d: Phase,
        decoded: DecodedInstruction,
        alu_op: Option<AluRKind>,
        branch_flags_in: Option<BranchCompareFlags>,
        branch_decision: Option<BranchDecisionOutput>,
        irq_input: InterruptRequestInput,
        irq_req: bool,
    ) -> Self {
        Self {
            state_q,
            state_d,
            reset: false,
            decoded: Some(decoded),
            alu_decode_info: Some(decoded.alu_dec_info),
            alu_op,
            branch_flags_in,
            branch_decision,
            irq_pending_in: Some(irq_input.irq_pending),
            mie_in: Some(irq_input.mie),
            in_trap_in: Some(irq_input.in_trap),
            irq_req,
        }
    }

    pub fn signal_summary(&self) -> String {
        let decoded = self
            .decoded
            .map(|d| {
                format!(
                    "instr_class={:?} imm_sel={:?} branch_kind={:?} is_halt={}",
                    d.instr_class,
                    d.imm_sel,
                    d.branch_kind,
                    bit(d.is_halt),
                )
            })
            .unwrap_or_else(|| {
                "instr_class=None imm_sel=None branch_kind=None is_halt=0".to_string()
            });

        let branch = self
            .branch_decision
            .map(|decision| {
                format!(
                    "BranchDecision(kind={:?}, eq={}, lt={}, gt={}, take_branch={})",
                    decision.kind,
                    bit(decision.flags.eq),
                    bit(decision.flags.lt),
                    bit(decision.flags.gt),
                    bit(decision.taken),
                )
            })
            .unwrap_or_else(|| "BranchDecision(None)".to_string());

        let irq = match (self.irq_pending_in, self.mie_in, self.in_trap_in) {
            (Some(pending), Some(mie), Some(in_trap)) => format!(
                "InterruptRequestLogic(irq_pending={}, mie={}, in_trap={} -> irq_req={})",
                bit(pending),
                bit(mie),
                bit(in_trap),
                bit(self.irq_req),
            ),
            _ => format!("InterruptRequestLogic.irq_req={}", bit(self.irq_req)),
        };

        format!(
            "StateRegister.Q={:?} reset={} InstructionDecoder({}) AluDecoder(info={:?} -> alu_op={:?}) BranchFlagsIn={:?} {} {} NextStateLogic.D={:?}",
            self.state_q,
            bit(self.reset),
            decoded,
            self.alu_decode_info,
            self.alu_op,
            self.branch_flags_in,
            branch,
            irq,
            self.state_d,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlStep {
    pub signals: ControlSignals,
    pub internal: ControlInternalSignals,
}

impl ControlStep {
    pub fn trace_prefix(&self) -> String {
        format!("cu: {};", self.internal.signal_summary())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRegister {
    state: Phase,
}

impl Default for StateRegister {
    fn default() -> Self {
        Self {
            state: Phase::Fetch,
        }
    }
}

impl StateRegister {
    pub fn state(self) -> Phase {
        self.state
    }

    pub fn clock(&mut self, reset: bool, next_state: Phase) {
        if reset {
            self.state = Phase::Fetch;
        } else {
            self.state = next_state;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControlUnitState {
    pub state_register: StateRegister,
}

impl ControlUnitState {
    pub fn phase(self) -> Phase {
        self.state_register.state()
    }

    pub fn clock(&mut self, reset: bool, next_state: Phase) {
        self.state_register.clock(reset, next_state);
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NextStateLogic;

impl NextStateLogic {
    pub fn next_state(self, current: Phase, halt_req: bool, irq_req: bool) -> Phase {
        match (current, halt_req, irq_req) {
            (_, true, _) => Phase::Halt,
            (Phase::Fetch, false, _) => Phase::Execute,
            (Phase::Execute, false, true) => Phase::TrapEnter,
            (Phase::Execute, false, false) => Phase::Fetch,
            (Phase::TrapEnter, false, _) => Phase::Fetch,
            (Phase::Halt, false, _) => Phase::Halt,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ControlUnit {
    pub instruction_decoder: InstructionDecoder,
    pub alu_decoder: AluDecoder,
    pub branch_decision: BranchDecision,
    pub signal_generator: ControlSignalGenerator,
    pub next_state_logic: NextStateLogic,
    pub interrupt_request_logic: InterruptRequestLogic,
}

impl ControlUnit {
    pub fn decode(self, inst: &Instruction) -> DecodedInstruction {
        self.instruction_decoder.decode(inst)
    }

    pub fn fetch_signals(self) -> ControlSignals {
        self.signal_generator.fetch_signals()
    }

    pub fn fetch_step(self, state: &ControlUnitState) -> Result<ControlStep, String> {
        let current = state.phase();
        if current != Phase::Fetch {
            return Err(format!(
                "ControlUnit fetch_step expected StateRegister=Fetch, got {:?}",
                current
            ));
        }

        let signals = self.signal_generator.fetch_signals();
        let next_state = self
            .next_state_logic
            .next_state(current, signals.halt_req, false);
        let mut internal = ControlInternalSignals::fetch(current, next_state);
        internal.irq_req = false;
        Ok(ControlStep { signals, internal })
    }

    pub fn execute_step(
        self,
        state: &ControlUnitState,
        decoded: DecodedInstruction,
        branch_flags: Option<BranchCompareFlags>,
        irq_input: InterruptRequestInput,
    ) -> Result<ControlStep, String> {
        let current = state.phase();
        if current != Phase::Execute {
            return Err(format!(
                "ControlUnit execute_step expected StateRegister=Execute, got {:?}",
                current
            ));
        }

        let alu_op = self.alu_decoder.decode(decoded.alu_dec_info);
        let branch_decision = if decoded.is_branch {
            let flags = branch_flags.ok_or_else(|| {
                "ControlUnit expected BranchComparator eq/lt/gt flags".to_string()
            })?;
            let kind = decoded
                .branch_kind
                .ok_or_else(|| "decoded branch is missing branch_kind".to_string())?;
            Some(self.branch_decision.decide(kind, flags))
        } else {
            None
        };

        let signals = self
            .signal_generator
            .execute_signals(decoded, alu_op, branch_decision)?;
        let irq_req = self.interrupt_request_logic.eval(irq_input);
        let effective_irq = irq_req && !signals.halt_req;
        let next_state = self
            .next_state_logic
            .next_state(current, signals.halt_req, effective_irq);
        let internal = ControlInternalSignals::execute(
            current,
            next_state,
            decoded,
            alu_op,
            branch_flags,
            branch_decision,
            irq_input,
            effective_irq,
        );

        Ok(ControlStep { signals, internal })
    }

    pub fn trap_enter_step(self, state: &ControlUnitState) -> Result<ControlStep, String> {
        let current = state.phase();
        if current != Phase::TrapEnter {
            return Err(format!(
                "ControlUnit trap_enter_step expected StateRegister=TrapEnter, got {:?}",
                current
            ));
        }

        let mut signals = ControlSignals::inactive(Phase::TrapEnter);
        signals.trap_enter = true;
        signals.mem_read = true;
        signals.pc_write = true;
        signals.addr_sel = MemAddrSel::TrapVectorAddr;
        signals.pc_sel = PcSel::TrapVector;
        let next_state = self.next_state_logic.next_state(current, false, false);
        let mut internal = ControlInternalSignals::fetch(current, next_state);
        internal.irq_req = true;
        Ok(ControlStep { signals, internal })
    }

    pub fn execute_signals(
        self,
        decoded: DecodedInstruction,
        branch_flags: Option<BranchCompareFlags>,
    ) -> Result<ControlSignals, String> {
        let alu_op = self.alu_decoder.decode(decoded.alu_dec_info);
        let branch_decision = if decoded.is_branch {
            let flags = branch_flags.ok_or_else(|| {
                "ControlUnit expected BranchComparator eq/lt/gt flags".to_string()
            })?;
            let kind = decoded
                .branch_kind
                .ok_or_else(|| "decoded branch is missing branch_kind".to_string())?;
            Some(self.branch_decision.decide(kind, flags))
        } else {
            None
        };
        self.signal_generator
            .execute_signals(decoded, alu_op, branch_decision)
    }

    pub fn next_state(self, current: Phase, halt_req: bool) -> Phase {
        self.next_state_logic.next_state(current, halt_req, false)
    }
}

pub fn decode_instruction(inst: &Instruction) -> DecodedInstruction {
    ControlUnit::default().decode(inst)
}

pub fn fetch_signals() -> ControlSignals {
    ControlUnit::default().fetch_signals()
}

pub fn generate_execute_signals(
    decoded: DecodedInstruction,
    branch_flags: Option<BranchCompareFlags>,
) -> Result<ControlSignals, String> {
    ControlUnit::default().execute_signals(decoded, branch_flags)
}

pub fn select_next_pc(
    sig: &ControlSignals,
    pc_old: u32,
    alu_out: u32,
    trap_handler_addr: Option<u32>,
    mepc: Option<u32>,
) -> Result<u32, String> {
    match sig.pc_sel {
        PcSel::PcPlus4 => Ok(pc_old.wrapping_add(4)),
        PcSel::AluTarget => Ok(alu_out),
        PcSel::Branch => {
            if sig
                .take_branch
                .ok_or_else(|| "PC_MUX expected BranchDecision take_branch signal".to_string())?
            {
                Ok(alu_out)
            } else {
                Ok(pc_old.wrapping_add(4))
            }
        }
        PcSel::TrapVector => trap_handler_addr
            .ok_or_else(|| "PC_MUX TrapVector needs handler address from memory".to_string()),
        PcSel::Mepc => mepc.ok_or_else(|| "PC_MUX Mepc needs mepc from TrapBlock".to_string()),
    }
}

pub fn next_phase_after_fetch() -> Phase {
    ControlUnit::default().next_state(Phase::Fetch, false)
}

pub fn next_phase_after_execute(halted: bool) -> Phase {
    ControlUnit::default().next_state(Phase::Execute, halted)
}
