use crate::asm::{AsmProgram, AsmSection, DataItem, Expr};
use crate::image::DEFAULT_MEMORY_LAYOUT;
use crate::isa::{AluRKind, BranchKind, Instruction, Reg};

pub const PRINT_INT_LABEL: &str = "__rt_print_int";
pub const PRINT_PSTR_LABEL: &str = "__rt_print_pstr";
pub const PRINT_VALUE_LABEL: &str = "__rt_print_value";
pub const READ_LINE_LABEL: &str = "__rt_read_line";

const HEAP_PTR_LABEL: &str = "__rt_heap_ptr";

pub fn emit_runtime(
    program: &mut AsmProgram,
    needs_print_int: bool,
    needs_print_pstr: bool,
    needs_print_value: bool,
    needs_read_line: bool,
) {
    let needs_print_int = needs_print_int || needs_print_value;
    let needs_print_pstr = needs_print_pstr || needs_print_value;

    if needs_print_int {
        emit_print_int(program);
    }
    if needs_print_pstr {
        emit_print_pstr(program);
    }
    if needs_print_value {
        emit_print_value(program);
    }
    if needs_read_line {
        emit_read_line(program);
        program.label(AsmSection::Data, HEAP_PTR_LABEL);
        program.emit_data(
            AsmSection::Data,
            DataItem::Word(DEFAULT_MEMORY_LAYOUT.heap_base),
        );
    }
}

fn emit_print_int(program: &mut AsmProgram) {
    program.label(AsmSection::Text, PRINT_INT_LABEL);

    // Save original value so print returns the same value.
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::Sp,
            rs1: Reg::Sp,
            imm: Expr::from_i32(-4),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: Reg::A0,
            rs1: Reg::Sp,
            off: Expr::from_i32(0),
        },
    );

    let label_negative = "__rt_print_int_negative";
    let label_after_sign = "__rt_print_int_after_sign";
    let label_zero = "__rt_print_int_zero";
    let label_digit_loop = "__rt_print_int_digit_loop";
    let label_output_loop = "__rt_print_int_output_loop";
    let label_done = "__rt_print_int_done";

    program.emit_inst(
        AsmSection::Text,
        Instruction::Branch {
            op: BranchKind::Blt,
            rs1: Reg::A0,
            rs2: Reg::Zero,
            off: Expr::pcrel(label_negative),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Jal {
            rd: Reg::Zero,
            off: Expr::pcrel(label_after_sign),
        },
    );

    program.label(AsmSection::Text, label_negative);
    load_mmio_out_addr(program, Reg::T5);
    load_small(program, Reg::T6, 45);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: Reg::T6,
            rs1: Reg::T5,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::AluR {
            op: AluRKind::Sub,
            rd: Reg::A0,
            rs1: Reg::Zero,
            rs2: Reg::A0,
        },
    );

    program.label(AsmSection::Text, label_after_sign);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Branch {
            op: BranchKind::Beq,
            rs1: Reg::A0,
            rs2: Reg::Zero,
            off: Expr::pcrel(label_zero),
        },
    );

    load_small(program, Reg::T1, 0);
    load_small(program, Reg::T2, 10);

    program.label(AsmSection::Text, label_digit_loop);
    program.emit_inst(
        AsmSection::Text,
        Instruction::AluR {
            op: AluRKind::Div,
            rd: Reg::T3,
            rs1: Reg::A0,
            rs2: Reg::T2,
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::AluR {
            op: AluRKind::Rem,
            rd: Reg::T4,
            rs1: Reg::A0,
            rs2: Reg::T2,
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::Sp,
            rs1: Reg::Sp,
            imm: Expr::from_i32(-4),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: Reg::T4,
            rs1: Reg::Sp,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::T1,
            rs1: Reg::T1,
            imm: Expr::from_i32(1),
        },
    );
    mov(program, Reg::A0, Reg::T3);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Branch {
            op: BranchKind::Bne,
            rs1: Reg::A0,
            rs2: Reg::Zero,
            off: Expr::pcrel(label_digit_loop),
        },
    );

    load_mmio_out_addr(program, Reg::T5);
    load_small(program, Reg::T6, 48);
    program.label(AsmSection::Text, label_output_loop);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Lw {
            rd: Reg::A0,
            rs1: Reg::Sp,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::Sp,
            rs1: Reg::Sp,
            imm: Expr::from_i32(4),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::AluR {
            op: AluRKind::Add,
            rd: Reg::A0,
            rs1: Reg::A0,
            rs2: Reg::T6,
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: Reg::A0,
            rs1: Reg::T5,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::T1,
            rs1: Reg::T1,
            imm: Expr::from_i32(-1),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Branch {
            op: BranchKind::Bne,
            rs1: Reg::T1,
            rs2: Reg::Zero,
            off: Expr::pcrel(label_output_loop),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Jal {
            rd: Reg::Zero,
            off: Expr::pcrel(label_done),
        },
    );

    program.label(AsmSection::Text, label_zero);
    load_mmio_out_addr(program, Reg::T5);
    load_small(program, Reg::A0, 48);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: Reg::A0,
            rs1: Reg::T5,
            off: Expr::from_i32(0),
        },
    );

    program.label(AsmSection::Text, label_done);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Lw {
            rd: Reg::A0,
            rs1: Reg::Sp,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::Sp,
            rs1: Reg::Sp,
            imm: Expr::from_i32(4),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Jalr {
            rd: Reg::Zero,
            rs1: Reg::Ra,
            off: Expr::from_i32(0),
        },
    );
}

fn emit_print_pstr(program: &mut AsmProgram) {
    program.label(AsmSection::Text, PRINT_PSTR_LABEL);

    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::Sp,
            rs1: Reg::Sp,
            imm: Expr::from_i32(-4),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: Reg::A0,
            rs1: Reg::Sp,
            off: Expr::from_i32(0),
        },
    );

    program.emit_inst(
        AsmSection::Text,
        Instruction::Lw {
            rd: Reg::T0,
            rs1: Reg::A0,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::T1,
            rs1: Reg::A0,
            imm: Expr::from_i32(4),
        },
    );
    load_mmio_out_addr(program, Reg::T5);

    let label_loop = "__rt_print_pstr_loop";
    let label_done = "__rt_print_pstr_done";

    program.label(AsmSection::Text, label_loop);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Branch {
            op: BranchKind::Beq,
            rs1: Reg::T0,
            rs2: Reg::Zero,
            off: Expr::pcrel(label_done),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Lw {
            rd: Reg::T6,
            rs1: Reg::T1,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: Reg::T6,
            rs1: Reg::T5,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::T1,
            rs1: Reg::T1,
            imm: Expr::from_i32(4),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::T0,
            rs1: Reg::T0,
            imm: Expr::from_i32(-1),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Jal {
            rd: Reg::Zero,
            off: Expr::pcrel(label_loop),
        },
    );

    program.label(AsmSection::Text, label_done);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Lw {
            rd: Reg::A0,
            rs1: Reg::Sp,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::Sp,
            rs1: Reg::Sp,
            imm: Expr::from_i32(4),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Jalr {
            rd: Reg::Zero,
            rs1: Reg::Ra,
            off: Expr::from_i32(0),
        },
    );
}

fn emit_print_value(program: &mut AsmProgram) {
    program.label(AsmSection::Text, PRINT_VALUE_LABEL);

    let label_int = "__rt_print_value_int";

    load_small(program, Reg::T0, 3);
    program.emit_inst(
        AsmSection::Text,
        Instruction::AluR {
            op: AluRKind::And,
            rd: Reg::T1,
            rs1: Reg::A0,
            rs2: Reg::T0,
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Branch {
            op: BranchKind::Bne,
            rs1: Reg::T1,
            rs2: Reg::Zero,
            off: Expr::pcrel(label_int),
        },
    );

    load_u32(program, Reg::T0, DEFAULT_MEMORY_LAYOUT.data_base as i32);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Branch {
            op: BranchKind::Blt,
            rs1: Reg::A0,
            rs2: Reg::T0,
            off: Expr::pcrel(label_int),
        },
    );

    load_u32(program, Reg::T0, DEFAULT_MEMORY_LAYOUT.mmio_base as i32);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Branch {
            op: BranchKind::Bge,
            rs1: Reg::A0,
            rs2: Reg::T0,
            off: Expr::pcrel(label_int),
        },
    );

    program.emit_inst(
        AsmSection::Text,
        Instruction::Jal {
            rd: Reg::Zero,
            off: Expr::pcrel(PRINT_PSTR_LABEL),
        },
    );

    program.label(AsmSection::Text, label_int);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Jal {
            rd: Reg::Zero,
            off: Expr::pcrel(PRINT_INT_LABEL),
        },
    );
}

fn emit_read_line(program: &mut AsmProgram) {
    program.label(AsmSection::Text, READ_LINE_LABEL);

    load_label_word(program, Reg::A0, HEAP_PTR_LABEL);
    mov(program, Reg::T0, Reg::A0);
    load_small(program, Reg::T1, 0);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::T2,
            rs1: Reg::T0,
            imm: Expr::from_i32(4),
        },
    );
    load_mmio_base(program, Reg::T6);

    let label_loop = "__rt_read_line_loop";
    let label_done = "__rt_read_line_done";
    let label_store = "__rt_read_line_store";

    program.label(AsmSection::Text, label_loop);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Lw {
            rd: Reg::T3,
            rs1: Reg::T6,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Branch {
            op: BranchKind::Beq,
            rs1: Reg::T3,
            rs2: Reg::Zero,
            off: Expr::pcrel(label_done),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Lw {
            rd: Reg::T4,
            rs1: Reg::T6,
            off: Expr::from_i32(4),
        },
    );
    load_small(program, Reg::T5, 1);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: Reg::T5,
            rs1: Reg::T6,
            off: Expr::from_i32(16),
        },
    );
    load_small(program, Reg::T5, 10);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Branch {
            op: BranchKind::Bne,
            rs1: Reg::T4,
            rs2: Reg::T5,
            off: Expr::pcrel(label_store),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Jal {
            rd: Reg::Zero,
            off: Expr::pcrel(label_done),
        },
    );

    program.label(AsmSection::Text, label_store);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: Reg::T4,
            rs1: Reg::T2,
            off: Expr::from_i32(0),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::T2,
            rs1: Reg::T2,
            imm: Expr::from_i32(4),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd: Reg::T1,
            rs1: Reg::T1,
            imm: Expr::from_i32(1),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Jal {
            rd: Reg::Zero,
            off: Expr::pcrel(label_loop),
        },
    );

    program.label(AsmSection::Text, label_done);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: Reg::T1,
            rs1: Reg::T0,
            off: Expr::from_i32(0),
        },
    );
    store_label_word(program, Reg::T2, HEAP_PTR_LABEL);
    mov(program, Reg::A0, Reg::T0);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Jalr {
            rd: Reg::Zero,
            rs1: Reg::Ra,
            off: Expr::from_i32(0),
        },
    );
}

fn load_label_addr(program: &mut AsmProgram, rd: Reg, label: &str) {
    program.emit_inst(
        AsmSection::Text,
        Instruction::Lui {
            rd,
            imm20: Expr::hi20(label),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd,
            rs1: rd,
            imm: Expr::lo12(label),
        },
    );
}

fn load_label_word(program: &mut AsmProgram, rd: Reg, label: &str) {
    load_label_addr(program, rd, label);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Lw {
            rd,
            rs1: rd,
            off: Expr::from_i32(0),
        },
    );
}

fn store_label_word(program: &mut AsmProgram, rs: Reg, label: &str) {
    load_label_addr(program, Reg::T5, label);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Sw {
            rs2: rs,
            rs1: Reg::T5,
            off: Expr::from_i32(0),
        },
    );
}

pub fn load_mmio_out_addr(program: &mut AsmProgram, rd: Reg) {
    load_u32(program, rd, 0x00ff_0008_u32 as i32);
}

pub fn load_mmio_base(program: &mut AsmProgram, rd: Reg) {
    load_u32(program, rd, 0x00ff_0000_u32 as i32);
}

pub fn load_small(program: &mut AsmProgram, rd: Reg, value: i32) {
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd,
            rs1: Reg::Zero,
            imm: Expr::from_i32(value),
        },
    );
}

pub fn load_u32(program: &mut AsmProgram, rd: Reg, value: i32) {
    if (-2048..=2047).contains(&value) {
        load_small(program, rd, value);
        return;
    }

    let bits = value as u32;
    let hi = (((bits as i64) + 0x800) >> 12) as i32;
    let lo = value.wrapping_sub(hi << 12);
    program.emit_inst(
        AsmSection::Text,
        Instruction::Lui {
            rd,
            imm20: Expr::from_i32(hi),
        },
    );
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd,
            rs1: rd,
            imm: Expr::from_i32(lo),
        },
    );
}

pub fn mov(program: &mut AsmProgram, rd: Reg, rs: Reg) {
    if rd == rs {
        return;
    }
    program.emit_inst(
        AsmSection::Text,
        Instruction::Addi {
            rd,
            rs1: rs,
            imm: Expr::from_i32(0),
        },
    );
}
