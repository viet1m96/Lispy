use crate::branch::BranchOutput;
use crate::isa::{AluRKind, BranchKind, Instruction};
use crate::machine::Phase;

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
    Csr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcSel {
    PcPlus4,
    AluTarget,
    Branch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlSignals {
    pub reg_write: bool,
    pub mem_read: bool,
    pub mem_write: bool,
    pub opa_sel: OpASel,
    pub opb_sel: OpBSel,
    pub wb_sel: WbSel,
    pub pc_sel: PcSel,
    pub alu_op: Option<AluRKind>,
    pub branch_kind: Option<BranchKind>,
    pub halt: bool,
}

impl ControlSignals {
    fn scalar_default() -> Self {
        Self {
            reg_write: false,
            mem_read: false,
            mem_write: false,
            opa_sel: OpASel::Rs1,
            opb_sel: OpBSel::Rs2,
            wb_sel: WbSel::None,
            pc_sel: PcSel::PcPlus4,
            alu_op: None,
            branch_kind: None,
            halt: false,
        }
    }
}

pub fn generate_signals(inst: &Instruction) -> Result<ControlSignals, String> {
    let mut sig = ControlSignals::scalar_default();

    match inst {
        Instruction::Lui { .. } => {
            sig.reg_write = true;
            sig.wb_sel = WbSel::ImmUpper;
        }
        Instruction::Addi { .. } => {
            sig.reg_write = true;
            sig.opa_sel = OpASel::Rs1;
            sig.opb_sel = OpBSel::Imm;
            sig.wb_sel = WbSel::Alu;
            sig.alu_op = Some(AluRKind::Add);
        }
        Instruction::Lw { .. } => {
            sig.reg_write = true;
            sig.mem_read = true;
            sig.opa_sel = OpASel::Rs1;
            sig.opb_sel = OpBSel::Imm;
            sig.wb_sel = WbSel::Mem;
            sig.alu_op = Some(AluRKind::Add);
        }
        Instruction::Sw { .. } => {
            sig.mem_write = true;
            sig.opa_sel = OpASel::Rs1;
            sig.opb_sel = OpBSel::Imm;
            sig.alu_op = Some(AluRKind::Add);
        }
        Instruction::AluR { op, .. } => {
            sig.reg_write = true;
            sig.opa_sel = OpASel::Rs1;
            sig.opb_sel = OpBSel::Rs2;
            sig.wb_sel = WbSel::Alu;
            sig.alu_op = Some(*op);
        }
        Instruction::Branch { op, .. } => {
            sig.opa_sel = OpASel::Pc;
            sig.opb_sel = OpBSel::Imm;
            sig.pc_sel = PcSel::Branch;
            sig.alu_op = Some(AluRKind::Add);
            sig.branch_kind = Some(*op);
        }
        Instruction::Jal { .. } => {
            sig.reg_write = true;
            sig.opa_sel = OpASel::Pc;
            sig.opb_sel = OpBSel::Imm;
            sig.wb_sel = WbSel::PcPlus4;
            sig.pc_sel = PcSel::AluTarget;
            sig.alu_op = Some(AluRKind::Add);
        }
        Instruction::Jalr { .. } => {
            sig.reg_write = true;
            sig.opa_sel = OpASel::Rs1;
            sig.opb_sel = OpBSel::Imm;
            sig.wb_sel = WbSel::PcPlus4;
            sig.pc_sel = PcSel::AluTarget;
            sig.alu_op = Some(AluRKind::Add);
        }
        Instruction::Halt => {
            sig.halt = true;
        }
        Instruction::Csrrw { .. } | Instruction::Csrrs { .. } | Instruction::Mret => {
            return Err(format!(
                "system/trap instruction is reserved for milestone 9: {}",
                inst.mnemonic()
            ));
        }
        Instruction::Vld { .. } | Instruction::Vst { .. } | Instruction::VectorR { .. } => {
            return Err(format!(
                "vector instruction is reserved for milestone 10: {}",
                inst.mnemonic()
            ));
        }
    }

    Ok(sig)
}

pub fn select_next_pc(
    sig: &ControlSignals,
    pc_old: u32,
    alu_out: u32,
    branch_out: Option<BranchOutput>,
) -> Result<u32, String> {
    match sig.pc_sel {
        PcSel::PcPlus4 => Ok(pc_old.wrapping_add(4)),
        PcSel::AluTarget => Ok(alu_out),
        PcSel::Branch => {
            let branch_out = branch_out.ok_or_else(|| {
                "control expected branch-unit feedback but none was provided".to_string()
            })?;
            if branch_out.taken {
                Ok(alu_out)
            } else {
                Ok(pc_old.wrapping_add(4))
            }
        }
    }
}

pub fn next_phase_after_fetch() -> Phase {
    Phase::Execute
}

pub fn next_phase_after_execute(halted: bool) -> Phase {
    if halted {
        Phase::Execute
    } else {
        Phase::Fetch
    }
}
