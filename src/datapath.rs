use crate::asm::Expr;
use crate::branch::{self, BranchInput, BranchOutput};
use crate::control::{self, ControlSignals, OpASel, OpBSel, PcSel, WbSel};
use crate::isa::{AluRKind, Instruction, Reg};
use crate::machine::Machine;

pub fn fetch_into_ir(machine: &mut Machine) -> Result<(u32, u32), String> {
    let pc = machine.pc;
    let ir = machine.memory.load_u32(pc)?;
    machine.ir = ir;
    Ok((pc, ir))
}

pub fn branch_feedback(
    machine: &Machine,
    inst: &Instruction,
    sig: &ControlSignals,
    _pc_old: u32,
) -> Result<Option<BranchOutput>, String> {
    if sig.pc_sel != PcSel::Branch {
        return Ok(None);
    }

    let kind = sig
        .branch_kind
        .ok_or_else(|| format!("missing branch kind for instruction: {}", inst.mnemonic()))?;
    let rs1_value = read_rs1(machine, inst)?;
    let rs2_value = read_rs2(machine, inst)?;
    Ok(Some(branch::evaluate(BranchInput {
        kind,
        rs1_value,
        rs2_value,
    })))
}

pub fn apply_execute(
    machine: &mut Machine,
    inst: &Instruction,
    sig: &ControlSignals,
    branch_out: Option<BranchOutput>,
    pc_old: u32,
) -> Result<String, String> {
    if sig.halt {
        machine.set_halt("halt instruction");
        return Ok("halt".to_string());
    }

    match inst {
        Instruction::Branch { .. } => {
            let branch_out = branch_out.ok_or_else(|| {
                format!(
                    "branch instruction reached execute without branch-unit feedback: {}",
                    inst.mnemonic()
                )
            })?;
            let alu_out = compute_alu_out(machine, inst, sig, pc_old)?;
            machine.pc = control::select_next_pc(sig, pc_old, alu_out, Some(branch_out))?;
            machine.force_zero_reg();

            let status = if branch_out.taken {
                "taken"
            } else {
                "not taken"
            };
            Ok(format!(
                "branch unit: rs1=0x{:08x}, rs2=0x{:08x}, take_branch={}; alu_target=0x{:08x}; pc <- 0x{:08x}",
                branch_out.rs1_value,
                branch_out.rs2_value,
                status,
                alu_out,
                machine.pc,
            ))
        }
        Instruction::Halt => unreachable!("halt is handled at the start of apply_execute"),
        Instruction::Csrrw { .. }
        | Instruction::Csrrs { .. }
        | Instruction::Mret
        | Instruction::Vld { .. }
        | Instruction::Vst { .. }
        | Instruction::VectorR { .. } => {
            unreachable!("unsupported instructions are rejected by control")
        }
        _ => {
            let alu_out = if sig.alu_op.is_some() {
                Some(compute_alu_out(machine, inst, sig, pc_old)?)
            } else {
                None
            };

            let mem_addr = if sig.mem_read || sig.mem_write {
                Some(alu_out.ok_or_else(|| {
                    format!(
                        "memory access requested but ALU address is missing for instruction: {}",
                        inst.mnemonic()
                    )
                })?)
            } else {
                None
            };

            let mut note_parts: Vec<String> = Vec::new();

            if sig.mem_write {
                let addr = mem_addr.expect("mem_addr exists when mem_write is set");
                let value = read_rs2(machine, inst)?;
                machine.memory.store_u32(addr, value)?;
                note_parts.push(format!("mem[0x{addr:08x}] <- 0x{value:08x}"));
            }

            let mem_data = if sig.mem_read {
                let addr = mem_addr.expect("mem_addr exists when mem_read is set");
                let value = machine.memory.load_u32(addr)?;
                note_parts.push(format!("mem[0x{addr:08x}] -> 0x{value:08x}"));
                Some(value)
            } else {
                None
            };

            if sig.reg_write {
                let rd = writeback_dest(inst)?;
                let wb_value =
                    select_writeback_value(inst, sig, pc_old.wrapping_add(4), alu_out, mem_data)?;
                let wb_note = describe_reg_write(machine.write_reg(rd, wb_value));
                note_parts.push(wb_note);
            }

            machine.pc = control::select_next_pc(sig, pc_old, alu_out.unwrap_or(0), None)?;
            machine.force_zero_reg();
            note_parts.push(format!("pc <- 0x{:08x}", machine.pc));

            Ok(note_parts.join("; "))
        }
    }
}

fn compute_alu_out(
    machine: &Machine,
    inst: &Instruction,
    sig: &ControlSignals,
    pc_old: u32,
) -> Result<u32, String> {
    let lhs = match sig.opa_sel {
        OpASel::Pc => pc_old,
        OpASel::Rs1 => read_rs1(machine, inst)?,
    };

    let rhs = match sig.opb_sel {
        OpBSel::Rs2 => read_rs2(machine, inst)?,
        OpBSel::Imm => read_imm(inst)? as u32,
    };

    let op = sig
        .alu_op
        .ok_or_else(|| format!("missing ALU op for instruction: {}", inst.mnemonic()))?;
    execute_alu(op, lhs, rhs)
}

fn writeback_dest(inst: &Instruction) -> Result<Reg, String> {
    match inst {
        Instruction::Lui { rd, .. }
        | Instruction::Addi { rd, .. }
        | Instruction::Lw { rd, .. }
        | Instruction::AluR { rd, .. }
        | Instruction::Jal { rd, .. }
        | Instruction::Jalr { rd, .. }
        | Instruction::Csrrw { rd, .. }
        | Instruction::Csrrs { rd, .. } => Ok(*rd),
        Instruction::Sw { .. }
        | Instruction::Branch { .. }
        | Instruction::Mret
        | Instruction::Halt
        | Instruction::Vld { .. }
        | Instruction::Vst { .. }
        | Instruction::VectorR { .. } => Err(format!(
            "instruction has no writeback destination register: {}",
            inst.mnemonic()
        )),
    }
}

fn select_writeback_value(
    inst: &Instruction,
    sig: &ControlSignals,
    next_seq_pc: u32,
    alu_out: Option<u32>,
    mem_data: Option<u32>,
) -> Result<u32, String> {
    match sig.wb_sel {
        WbSel::None => Err(format!(
            "register write requested but wb_sel is None for instruction: {}",
            inst.mnemonic()
        )),
        WbSel::Alu => alu_out.ok_or_else(|| {
            format!(
                "wb_sel=Alu but ALU output is missing for instruction: {}",
                inst.mnemonic()
            )
        }),
        WbSel::Mem => mem_data.ok_or_else(|| {
            format!(
                "wb_sel=Mem but memory data is missing for instruction: {}",
                inst.mnemonic()
            )
        }),
        WbSel::PcPlus4 => Ok(next_seq_pc),
        WbSel::ImmUpper => match inst {
            Instruction::Lui { imm20, .. } => upper_u_imm(imm20),
            _ => Err(format!(
                "wb_sel=ImmUpper is only valid for lui, got: {}",
                inst.mnemonic()
            )),
        },
        WbSel::Csr => Err("CSR writeback is reserved for milestone 9".to_string()),
    }
}

fn read_rs1(machine: &Machine, inst: &Instruction) -> Result<u32, String> {
    let reg = match inst {
        Instruction::Addi { rs1, .. }
        | Instruction::Lw { rs1, .. }
        | Instruction::Sw { rs1, .. }
        | Instruction::AluR { rs1, .. }
        | Instruction::Branch { rs1, .. }
        | Instruction::Jalr { rs1, .. }
        | Instruction::Csrrw { rs1, .. }
        | Instruction::Csrrs { rs1, .. }
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
    Ok(machine.read_reg(reg))
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
        | Instruction::Csrrw { .. }
        | Instruction::Csrrs { .. }
        | Instruction::Mret
        | Instruction::Halt
        | Instruction::Vld { .. }
        | Instruction::Vst { .. }
        | Instruction::VectorR { .. } => {
            return Err(format!("instruction has no rs2 field: {}", inst.mnemonic()));
        }
    };
    Ok(machine.read_reg(reg))
}

fn read_imm(inst: &Instruction) -> Result<i32, String> {
    match inst {
        Instruction::Addi { imm, .. }
        | Instruction::Lw { off: imm, .. }
        | Instruction::Sw { off: imm, .. }
        | Instruction::Branch { off: imm, .. }
        | Instruction::Jal { off: imm, .. }
        | Instruction::Jalr { off: imm, .. } => resolved_i32(imm),
        Instruction::Lui { .. }
        | Instruction::AluR { .. }
        | Instruction::Csrrw { .. }
        | Instruction::Csrrs { .. }
        | Instruction::Mret
        | Instruction::Halt
        | Instruction::Vld { .. }
        | Instruction::Vst { .. }
        | Instruction::VectorR { .. } => Err(format!(
            "instruction has no immediate field: {}",
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

fn describe_reg_write(write: Option<(Reg, u32)>) -> String {
    match write {
        Some((reg, value)) => format!("{reg} <- 0x{value:08x}"),
        None => "x0 write discarded".to_string(),
    }
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
