use crate::datapath::BranchCompareFlags;
use crate::interrupt::{InterruptRequestInput, InterruptRequestLogic};
use crate::isa::{AluRKind, BranchKind, Instruction, VectorRKind};
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
    VectorLaneAddr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemWriteDataSel {
    Rs2,
    VecLane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedInstruction {
    pub instr_class: InstrClass,
    pub imm_sel: ImmSel,
    pub is_branch: bool,
    pub branch_kind: Option<BranchKind>,
    pub vector_op: Option<VectorRKind>,
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
    pub start_vec_op: bool,
    pub vector_base_write: bool,
    pub lane_counter_reset: bool,
    pub lane_counter_inc: bool,
    pub vector_lane_write: bool,
    pub vector_lane_read: bool,
    pub vector_full_write: bool,

    pub addr_sel: MemAddrSel,
    pub opa_sel: OpASel,
    pub opb_sel: OpBSel,
    pub wb_sel: WbSel,
    pub pc_sel: PcSel,
    pub imm_sel: ImmSel,
    pub mem_write_data_sel: MemWriteDataSel,

    pub alu_op: Option<AluRKind>,
    pub vector_alu_op: Option<VectorRKind>,
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
            start_vec_op: false,
            vector_base_write: false,
            lane_counter_reset: false,
            lane_counter_inc: false,
            vector_lane_write: false,
            vector_lane_read: false,
            vector_full_write: false,
            addr_sel: MemAddrSel::Pc,
            opa_sel: OpASel::Rs1,
            opb_sel: OpBSel::Rs2,
            wb_sel: WbSel::None,
            pc_sel: PcSel::PcPlus4,
            imm_sel: ImmSel::None,
            mem_write_data_sel: MemWriteDataSel::Rs2,
            alu_op: None,
            vector_alu_op: None,
            branch_kind: None,
            branch_flags: None,
            take_branch: None,
        }
    }

    pub fn signal_summary(&self) -> String {
        format!(
            "pc_wr={} ir_wr={} reg_wr={} mem_rd={} mem_wr={} addr_sel={:?} opa_sel={:?} opb_sel={:?} imm_sel={:?} alu_op={:?} wb_sel={:?} pc_sel={:?} halt_req={} trap_enter={} trap_exit={} start_vec_op={} vbase_wr={} lane_counter_rst={} lane_counter_inc={} vec_lane_wr={} vec_lane_rd={} vec_full_wr={} vec_alu_op={:?} take_branch={:?}",
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
            bit(self.start_vec_op),
            bit(self.vector_base_write),
            bit(self.lane_counter_reset),
            bit(self.lane_counter_inc),
            bit(self.vector_lane_write),
            bit(self.vector_lane_read),
            bit(self.vector_full_write),
            self.vector_alu_op,
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
                vector_op: None,
                alu_dec_info: AluDecodeInfo::NoAlu,
            },
            Instruction::Addi { .. } => DecodedInstruction {
                instr_class: InstrClass::AluI,
                imm_sel: ImmSel::I,
                is_branch: false,
                branch_kind: None,
                vector_op: None,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Lw { .. } => DecodedInstruction {
                instr_class: InstrClass::Load,
                imm_sel: ImmSel::I,
                is_branch: false,
                branch_kind: None,
                vector_op: None,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Sw { .. } => DecodedInstruction {
                instr_class: InstrClass::Store,
                imm_sel: ImmSel::S,
                is_branch: false,
                branch_kind: None,
                vector_op: None,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::AluR { op, .. } => DecodedInstruction {
                instr_class: InstrClass::AluR,
                imm_sel: ImmSel::None,
                is_branch: false,
                branch_kind: None,
                vector_op: None,
                alu_dec_info: AluDecodeInfo::UseRType(*op),
            },
            Instruction::Branch { op, .. } => DecodedInstruction {
                instr_class: InstrClass::Branch,
                imm_sel: ImmSel::B,
                is_branch: true,
                branch_kind: Some(*op),
                vector_op: None,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Jal { .. } => DecodedInstruction {
                instr_class: InstrClass::Jal,
                imm_sel: ImmSel::J,
                is_branch: false,
                branch_kind: None,
                vector_op: None,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Jalr { .. } => DecodedInstruction {
                instr_class: InstrClass::Jalr,
                imm_sel: ImmSel::I,
                is_branch: false,
                branch_kind: None,
                vector_op: None,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Mret => DecodedInstruction {
                instr_class: InstrClass::Mret,
                imm_sel: ImmSel::None,
                is_branch: false,
                branch_kind: None,
                vector_op: None,
                alu_dec_info: AluDecodeInfo::NoAlu,
            },
            Instruction::Halt => DecodedInstruction {
                instr_class: InstrClass::Halt,
                imm_sel: ImmSel::None,
                is_branch: false,
                branch_kind: None,
                vector_op: None,
                alu_dec_info: AluDecodeInfo::NoAlu,
            },
            Instruction::Vld { .. } => DecodedInstruction {
                instr_class: InstrClass::VectorLoad,
                imm_sel: ImmSel::I,
                is_branch: false,
                branch_kind: None,
                vector_op: None,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::Vst { .. } => DecodedInstruction {
                instr_class: InstrClass::VectorStore,
                imm_sel: ImmSel::S,
                is_branch: false,
                branch_kind: None,
                vector_op: None,
                alu_dec_info: AluDecodeInfo::UseFixed(AluRKind::Add),
            },
            Instruction::VectorR { op, .. } => DecodedInstruction {
                instr_class: InstrClass::VectorR,
                imm_sel: ImmSel::None,
                is_branch: false,
                branch_kind: None,
                vector_op: Some(*op),
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
            InstrClass::VectorLoad => {
                sig.pc_write = false;
                sig.opa_sel = OpASel::Rs1;
                sig.opb_sel = OpBSel::Imm;
                sig.vector_base_write = true;
                sig.lane_counter_reset = true;
                sig.start_vec_op = true;
            }
            InstrClass::VectorStore => {
                sig.pc_write = false;
                sig.opa_sel = OpASel::Rs1;
                sig.opb_sel = OpBSel::Imm;
                sig.vector_base_write = true;
                sig.lane_counter_reset = true;
                sig.start_vec_op = true;
            }
            InstrClass::VectorR => {
                sig.vector_full_write = true;
                sig.vector_alu_op = decoded.vector_op;
                sig.pc_sel = PcSel::PcPlus4;
            }
        }

        Ok(sig)
    }

    pub fn vec_op_signals(
        self,
        decoded: DecodedInstruction,
        lane_done: bool,
    ) -> Result<ControlSignals, String> {
        let mut sig = ControlSignals::inactive(Phase::VecOp);
        sig.instr_class = Some(decoded.instr_class);
        sig.addr_sel = MemAddrSel::VectorLaneAddr;
        sig.pc_sel = PcSel::PcPlus4;
        sig.pc_write = lane_done;
        sig.lane_counter_inc = true;

        match decoded.instr_class {
            InstrClass::VectorLoad => {
                sig.mem_read = true;
                sig.vector_lane_write = true;
            }
            InstrClass::VectorStore => {
                sig.mem_write = true;
                sig.vector_lane_read = true;
                sig.mem_write_data_sel = MemWriteDataSel::VecLane;
            }
            other => {
                return Err(format!(
                    "VEC_OP phase expected vector memory instruction, got {:?}",
                    other
                ));
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
    pub lane_done_in: Option<bool>,
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
            lane_done_in: None,
        }
    }
    #[allow(clippy::too_many_arguments)]
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
            lane_done_in: None,
        }
    }

    pub fn vec_op(
        state_q: Phase,
        state_d: Phase,
        decoded: DecodedInstruction,
        lane_done: bool,
        irq_input: InterruptRequestInput,
        irq_req: bool,
    ) -> Self {
        Self {
            state_q,
            state_d,
            reset: false,
            decoded: Some(decoded),
            alu_decode_info: Some(decoded.alu_dec_info),
            alu_op: None,
            branch_flags_in: None,
            branch_decision: None,
            irq_pending_in: Some(irq_input.irq_pending),
            mie_in: Some(irq_input.mie),
            in_trap_in: Some(irq_input.in_trap),
            irq_req,
            lane_done_in: Some(lane_done),
        }
    }

    pub fn signal_summary(&self) -> String {
        let decoded = self
            .decoded
            .map(|d| {
                format!(
                    "instr_class={:?} imm_sel={:?} branch_kind={:?} vector_op={:?}",
                    d.instr_class,
                    d.imm_sel,
                    d.branch_kind,
                    d.vector_op,
                )
            })
            .unwrap_or_else(|| {
                "instr_class=None imm_sel=None branch_kind=None vector_op=None".to_string()
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

        let lane = self.lane_done_in.map(|done| format!(" LaneComparator(lane_done={})", bit(done))).unwrap_or_default();

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
            "StateRegister.Q={:?} reset={} InstructionDecoder({}) AluDecoder(info={:?} -> alu_op={:?}) BranchFlagsIn={:?} {}{} {} NextStateLogic.D={:?}",
            self.state_q,
            bit(self.reset),
            decoded,
            self.alu_decode_info,
            self.alu_op,
            self.branch_flags_in,
            branch,
            lane,
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
    pub fn next_state(
        self,
        current: Phase,
        halt_req: bool,
        irq_req: bool,
        start_vec_op: bool,
        lane_done: bool,
    ) -> Phase {
        if halt_req {
            return Phase::Halt;
        }

        match current {
            Phase::Fetch => Phase::Execute,
            Phase::Execute if start_vec_op => Phase::VecOp,
            Phase::Execute if irq_req => Phase::TrapEnter,
            Phase::Execute => Phase::Fetch,
            Phase::VecOp if !lane_done => Phase::VecOp,
            Phase::VecOp if irq_req => Phase::TrapEnter,
            Phase::VecOp => Phase::Fetch,
            Phase::TrapEnter => Phase::Fetch,
            Phase::Halt => Phase::Halt,
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
            .next_state(current, signals.halt_req, false, false, false);
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
        let effective_irq = irq_req && !signals.halt_req && !signals.start_vec_op;
        let next_state = self
            .next_state_logic
            .next_state(current, signals.halt_req, effective_irq, signals.start_vec_op, false);
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

    pub fn vec_op_step(
        self,
        state: &ControlUnitState,
        decoded: DecodedInstruction,
        lane_done: bool,
        irq_input: InterruptRequestInput,
    ) -> Result<ControlStep, String> {
        let current = state.phase();
        if current != Phase::VecOp {
            return Err(format!(
                "ControlUnit vec_op_step expected StateRegister=VecOp, got {:?}",
                current
            ));
        }

        let signals = self.signal_generator.vec_op_signals(decoded, lane_done)?;
        let irq_req = self.interrupt_request_logic.eval(irq_input);
        let effective_irq = irq_req && lane_done;
        let next_state = self.next_state_logic.next_state(
            current,
            signals.halt_req,
            effective_irq,
            false,
            lane_done,
        );
        let internal = ControlInternalSignals::vec_op(
            current,
            next_state,
            decoded,
            lane_done,
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
        let next_state = self.next_state_logic.next_state(current, false, false, false, false);
        let mut internal = ControlInternalSignals::fetch(current, next_state);
        internal.irq_req = true;
        Ok(ControlStep { signals, internal })
    }
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
