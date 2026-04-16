use std::fmt;

use crate::asm::Expr;

pub const INSTRUCTION_SIZE: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Reg {
    Zero = 0,
    Ra = 1,
    Sp = 2,
    Gp = 3,
    Tp = 4,
    T0 = 5,
    T1 = 6,
    T2 = 7,
    S0 = 8,
    S1 = 9,
    A0 = 10,
    A1 = 11,
    A2 = 12,
    A3 = 13,
    A4 = 14,
    A5 = 15,
    A6 = 16,
    A7 = 17,
    S2 = 18,
    S3 = 19,
    S4 = 20,
    S5 = 21,
    S6 = 22,
    S7 = 23,
    S8 = 24,
    S9 = 25,
    S10 = 26,
    S11 = 27,
    T3 = 28,
    T4 = 29,
    T5 = 30,
    T6 = 31,
}

impl Reg {
    pub fn from_u8(value: u8) -> Result<Self, String> {
        match value {
            0 => Ok(Self::Zero),
            1 => Ok(Self::Ra),
            2 => Ok(Self::Sp),
            3 => Ok(Self::Gp),
            4 => Ok(Self::Tp),
            5 => Ok(Self::T0),
            6 => Ok(Self::T1),
            7 => Ok(Self::T2),
            8 => Ok(Self::S0),
            9 => Ok(Self::S1),
            10 => Ok(Self::A0),
            11 => Ok(Self::A1),
            12 => Ok(Self::A2),
            13 => Ok(Self::A3),
            14 => Ok(Self::A4),
            15 => Ok(Self::A5),
            16 => Ok(Self::A6),
            17 => Ok(Self::A7),
            18 => Ok(Self::S2),
            19 => Ok(Self::S3),
            20 => Ok(Self::S4),
            21 => Ok(Self::S5),
            22 => Ok(Self::S6),
            23 => Ok(Self::S7),
            24 => Ok(Self::S8),
            25 => Ok(Self::S9),
            26 => Ok(Self::S10),
            27 => Ok(Self::S11),
            28 => Ok(Self::T3),
            29 => Ok(Self::T4),
            30 => Ok(Self::T5),
            31 => Ok(Self::T6),
            _ => Err(format!("invalid scalar register x{value}")),
        }
    }

    pub fn bits(self) -> u32 {
        self as u32
    }
}

impl fmt::Display for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Reg::Zero => "x0",
            Reg::Ra => "ra",
            Reg::Sp => "sp",
            Reg::Gp => "gp",
            Reg::Tp => "tp",
            Reg::T0 => "t0",
            Reg::T1 => "t1",
            Reg::T2 => "t2",
            Reg::S0 => "s0",
            Reg::S1 => "s1",
            Reg::A0 => "a0",
            Reg::A1 => "a1",
            Reg::A2 => "a2",
            Reg::A3 => "a3",
            Reg::A4 => "a4",
            Reg::A5 => "a5",
            Reg::A6 => "a6",
            Reg::A7 => "a7",
            Reg::S2 => "s2",
            Reg::S3 => "s3",
            Reg::S4 => "s4",
            Reg::S5 => "s5",
            Reg::S6 => "s6",
            Reg::S7 => "s7",
            Reg::S8 => "s8",
            Reg::S9 => "s9",
            Reg::S10 => "s10",
            Reg::S11 => "s11",
            Reg::T3 => "t3",
            Reg::T4 => "t4",
            Reg::T5 => "t5",
            Reg::T6 => "t6",
        };
        write!(f, "{text}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VReg(pub u8);

impl VReg {
    pub fn new(index: u8) -> Result<Self, String> {
        if index < 8 {
            Ok(Self(index))
        } else {
            Err(format!("invalid vector register v{index}"))
        }
    }

    pub fn bits(self) -> u32 {
        self.0 as u32
    }

    pub fn from_u8(value: u8) -> Result<Self, String> {
        Self::new(value)
    }
}

impl fmt::Display for VReg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Csr {
    Mstatus,
    Mtvec,
    Mepc,
    Mcause,
    Raw(u16),
}

impl Csr {
    pub fn number(self) -> u16 {
        match self {
            Csr::Mstatus => 0x300,
            Csr::Mtvec => 0x305,
            Csr::Mepc => 0x341,
            Csr::Mcause => 0x342,
            Csr::Raw(n) => n,
        }
    }

    pub fn from_number(value: u16) -> Self {
        match value {
            0x300 => Self::Mstatus,
            0x305 => Self::Mtvec,
            0x341 => Self::Mepc,
            0x342 => Self::Mcause,
            other => Self::Raw(other),
        }
    }
}

impl fmt::Display for Csr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Csr::Mstatus => write!(f, "mstatus"),
            Csr::Mtvec => write!(f, "mtvec"),
            Csr::Mepc => write!(f, "mepc"),
            Csr::Mcause => write!(f, "mcause"),
            Csr::Raw(value) => write!(f, "csr(0x{value:03x})"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchKind {
    Beq,
    Bne,
    Blt,
    Bge,
}

impl BranchKind {
    fn funct3(self) -> u32 {
        match self {
            Self::Beq => 0b000,
            Self::Bne => 0b001,
            Self::Blt => 0b100,
            Self::Bge => 0b101,
        }
    }

    fn mnemonic(self) -> &'static str {
        match self {
            Self::Beq => "beq",
            Self::Bne => "bne",
            Self::Blt => "blt",
            Self::Bge => "bge",
        }
    }

    fn from_funct3(value: u32) -> Result<Self, String> {
        match value {
            0b000 => Ok(Self::Beq),
            0b001 => Ok(Self::Bne),
            0b100 => Ok(Self::Blt),
            0b101 => Ok(Self::Bge),
            _ => Err(format!("unsupported branch funct3: {value:#05b}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AluRKind {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Sll,
    Srl,
    Sra,
    Slt,
    Sltu,
    Mul,
    Mulh,
    Mulhsu,
    Mulhu,
    Div,
    Divu,
    Rem,
    Remu,
}

impl AluRKind {
    fn bits(self) -> (u32, u32) {
        match self {
            Self::Add => (0b000, 0b0000000),
            Self::Sub => (0b000, 0b0100000),
            Self::And => (0b111, 0b0000000),
            Self::Or => (0b110, 0b0000000),
            Self::Xor => (0b100, 0b0000000),
            Self::Sll => (0b001, 0b0000000),
            Self::Srl => (0b101, 0b0000000),
            Self::Sra => (0b101, 0b0100000),
            Self::Slt => (0b010, 0b0000000),
            Self::Sltu => (0b011, 0b0000000),
            Self::Mul => (0b000, 0b0000001),
            Self::Mulh => (0b001, 0b0000001),
            Self::Mulhsu => (0b010, 0b0000001),
            Self::Mulhu => (0b011, 0b0000001),
            Self::Div => (0b100, 0b0000001),
            Self::Divu => (0b101, 0b0000001),
            Self::Rem => (0b110, 0b0000001),
            Self::Remu => (0b111, 0b0000001),
        }
    }

    fn mnemonic(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Sub => "sub",
            Self::And => "and",
            Self::Or => "or",
            Self::Xor => "xor",
            Self::Sll => "sll",
            Self::Srl => "srl",
            Self::Sra => "sra",
            Self::Slt => "slt",
            Self::Sltu => "sltu",
            Self::Mul => "mul",
            Self::Mulh => "mulh",
            Self::Mulhsu => "mulhsu",
            Self::Mulhu => "mulhu",
            Self::Div => "div",
            Self::Divu => "divu",
            Self::Rem => "rem",
            Self::Remu => "remu",
        }
    }

    fn from_bits(funct3: u32, funct7: u32) -> Result<Self, String> {
        match (funct3, funct7) {
            (0b000, 0b0000000) => Ok(Self::Add),
            (0b000, 0b0100000) => Ok(Self::Sub),
            (0b111, 0b0000000) => Ok(Self::And),
            (0b110, 0b0000000) => Ok(Self::Or),
            (0b100, 0b0000000) => Ok(Self::Xor),
            (0b001, 0b0000000) => Ok(Self::Sll),
            (0b101, 0b0000000) => Ok(Self::Srl),
            (0b101, 0b0100000) => Ok(Self::Sra),
            (0b010, 0b0000000) => Ok(Self::Slt),
            (0b011, 0b0000000) => Ok(Self::Sltu),
            (0b000, 0b0000001) => Ok(Self::Mul),
            (0b001, 0b0000001) => Ok(Self::Mulh),
            (0b010, 0b0000001) => Ok(Self::Mulhsu),
            (0b011, 0b0000001) => Ok(Self::Mulhu),
            (0b100, 0b0000001) => Ok(Self::Div),
            (0b101, 0b0000001) => Ok(Self::Divu),
            (0b110, 0b0000001) => Ok(Self::Rem),
            (0b111, 0b0000001) => Ok(Self::Remu),
            _ => Err(format!(
                "unsupported scalar R-type combination funct3={funct3:#05b}, funct7={funct7:#09b}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorRKind {
    Vadd,
    Vsub,
    Vmul,
    Vdiv,
    Vcmpeq,
}

impl VectorRKind {
    fn bits(self) -> (u32, u32) {
        match self {
            Self::Vadd => (0b000, 0b0000000),
            Self::Vsub => (0b000, 0b0100000),
            Self::Vmul => (0b001, 0b0000001),
            Self::Vdiv => (0b100, 0b0000001),
            Self::Vcmpeq => (0b010, 0b0000000),
        }
    }

    fn mnemonic(self) -> &'static str {
        match self {
            Self::Vadd => "vadd",
            Self::Vsub => "vsub",
            Self::Vmul => "vmul",
            Self::Vdiv => "vdiv",
            Self::Vcmpeq => "vcmpeq",
        }
    }

    fn from_bits(funct3: u32, funct7: u32) -> Result<Self, String> {
        match (funct3, funct7) {
            (0b000, 0b0000000) => Ok(Self::Vadd),
            (0b000, 0b0100000) => Ok(Self::Vsub),
            (0b001, 0b0000001) => Ok(Self::Vmul),
            (0b100, 0b0000001) => Ok(Self::Vdiv),
            (0b010, 0b0000000) => Ok(Self::Vcmpeq),
            _ => Err(format!(
                "unsupported vector R-type combination funct3={funct3:#05b}, funct7={funct7:#09b}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Lui {
        rd: Reg,
        imm20: Expr,
    },
    Addi {
        rd: Reg,
        rs1: Reg,
        imm: Expr,
    },
    Lw {
        rd: Reg,
        rs1: Reg,
        off: Expr,
    },
    Sw {
        rs2: Reg,
        rs1: Reg,
        off: Expr,
    },
    AluR {
        op: AluRKind,
        rd: Reg,
        rs1: Reg,
        rs2: Reg,
    },
    Branch {
        op: BranchKind,
        rs1: Reg,
        rs2: Reg,
        off: Expr,
    },
    Jal {
        rd: Reg,
        off: Expr,
    },
    Jalr {
        rd: Reg,
        rs1: Reg,
        off: Expr,
    },
    Csrrw {
        rd: Reg,
        csr: Csr,
        rs1: Reg,
    },
    Csrrs {
        rd: Reg,
        csr: Csr,
        rs1: Reg,
    },
    Mret,
    Halt,
    Vld {
        vd: VReg,
        rs1: Reg,
        off: Expr,
    },
    Vst {
        vs: VReg,
        rs1: Reg,
        off: Expr,
    },
    VectorR {
        op: VectorRKind,
        vd: VReg,
        vs1: VReg,
        vs2: VReg,
    },
}

impl Instruction {
    pub fn encode_resolved(&self) -> Result<u32, String> {
        match self {
            Instruction::Lui { rd, imm20 } => {
                let imm = expr_as_u20(imm20)?;
                Ok(pack_u(imm, rd.bits(), 0b0110111))
            }
            Instruction::Addi { rd, rs1, imm } => {
                let imm = expr_as_i12(imm)?;
                Ok(pack_i(imm, rs1.bits(), 0b000, rd.bits(), 0b0010011))
            }
            Instruction::Lw { rd, rs1, off } => {
                let imm = expr_as_i12(off)?;
                Ok(pack_i(imm, rs1.bits(), 0b010, rd.bits(), 0b0000011))
            }
            Instruction::Sw { rs2, rs1, off } => {
                let imm = expr_as_i12(off)?;
                Ok(pack_s(imm, rs2.bits(), rs1.bits(), 0b010, 0b0100011))
            }
            Instruction::AluR { op, rd, rs1, rs2 } => {
                let (funct3, funct7) = op.bits();
                Ok(pack_r(
                    funct7,
                    rs2.bits(),
                    rs1.bits(),
                    funct3,
                    rd.bits(),
                    0b0110011,
                ))
            }
            Instruction::Branch { op, rs1, rs2, off } => {
                let imm = expr_as_i13(off)?;
                if imm % 2 != 0 {
                    return Err(format!("branch offset must be 2-byte aligned, got {imm}"));
                }
                Ok(pack_b(imm, rs2.bits(), rs1.bits(), op.funct3(), 0b1100011))
            }
            Instruction::Jal { rd, off } => {
                let imm = expr_as_i21(off)?;
                if imm % 2 != 0 {
                    return Err(format!("jal offset must be 2-byte aligned, got {imm}"));
                }
                Ok(pack_j(imm, rd.bits(), 0b1101111))
            }
            Instruction::Jalr { rd, rs1, off } => {
                let imm = expr_as_i12(off)?;
                Ok(pack_i(imm, rs1.bits(), 0b000, rd.bits(), 0b1100111))
            }
            Instruction::Csrrw { rd, csr, rs1 } => Ok(pack_i(
                i32::from(csr.number()),
                rs1.bits(),
                0b001,
                rd.bits(),
                0b1110011,
            )),
            Instruction::Csrrs { rd, csr, rs1 } => Ok(pack_i(
                i32::from(csr.number()),
                rs1.bits(),
                0b010,
                rd.bits(),
                0b1110011,
            )),
            Instruction::Mret => Ok(pack_i(0x302, 0, 0b000, 0, 0b1110011)),
            Instruction::Halt => Ok(pack_i(0x0fff, 0, 0b000, 0, 0b1110011)),
            Instruction::Vld { vd, rs1, off } => {
                let imm = expr_as_i12(off)?;
                Ok(pack_i(imm, rs1.bits(), 0b000, vd.bits(), 0b0000111))
            }
            Instruction::Vst { vs, rs1, off } => {
                let imm = expr_as_i12(off)?;
                Ok(pack_s(imm, vs.bits(), rs1.bits(), 0b000, 0b0100111))
            }
            Instruction::VectorR { op, vd, vs1, vs2 } => {
                let (funct3, funct7) = op.bits();
                Ok(pack_r(
                    funct7,
                    vs2.bits(),
                    vs1.bits(),
                    funct3,
                    vd.bits(),
                    0b1010111,
                ))
            }
        }
    }

    pub fn decode(word: u32) -> Result<Self, String> {
        let opcode = word & 0x7f;
        let rd = Reg::from_u8(field5(word, 7) as u8)?;
        let funct3 = field3(word, 12);
        let rs1 = Reg::from_u8(field5(word, 15) as u8)?;
        let rs2 = Reg::from_u8(field5(word, 20) as u8)?;
        let funct7 = field7(word, 25);

        match opcode {
            0b0110111 => Ok(Self::Lui {
                rd,
                imm20: Expr::from_i32((word >> 12) as i32),
            }),
            0b0010011 => Ok(Self::Addi {
                rd,
                rs1,
                imm: Expr::from_i32(extract_i_imm(word)),
            }),
            0b0000011 if funct3 == 0b010 => Ok(Self::Lw {
                rd,
                rs1,
                off: Expr::from_i32(extract_i_imm(word)),
            }),
            0b0100011 if funct3 == 0b010 => Ok(Self::Sw {
                rs2,
                rs1,
                off: Expr::from_i32(extract_s_imm(word)),
            }),
            0b0110011 => Ok(Self::AluR {
                op: AluRKind::from_bits(funct3, funct7)?,
                rd,
                rs1,
                rs2,
            }),
            0b1100011 => Ok(Self::Branch {
                op: BranchKind::from_funct3(funct3)?,
                rs1,
                rs2,
                off: Expr::from_i32(extract_b_imm(word)),
            }),
            0b1101111 => Ok(Self::Jal {
                rd,
                off: Expr::from_i32(extract_j_imm(word)),
            }),
            0b1100111 if funct3 == 0b000 => Ok(Self::Jalr {
                rd,
                rs1,
                off: Expr::from_i32(extract_i_imm(word)),
            }),
            0b1110011 if funct3 == 0b001 => Ok(Self::Csrrw {
                rd,
                csr: Csr::from_number(field12(word, 20) as u16),
                rs1,
            }),
            0b1110011 if funct3 == 0b010 => Ok(Self::Csrrs {
                rd,
                csr: Csr::from_number(field12(word, 20) as u16),
                rs1,
            }),
            0b1110011 if funct3 == 0b000 && field12(word, 20) == 0x302 => Ok(Self::Mret),
            0b1110011 if funct3 == 0b000 && field12(word, 20) == 0x0fff => Ok(Self::Halt),
            0b0000111 if funct3 == 0b000 => Ok(Self::Vld {
                vd: VReg::from_u8(field5(word, 7) as u8)?,
                rs1,
                off: Expr::from_i32(extract_i_imm(word)),
            }),
            0b0100111 if funct3 == 0b000 => Ok(Self::Vst {
                vs: VReg::from_u8(field5(word, 20) as u8)?,
                rs1,
                off: Expr::from_i32(extract_s_imm(word)),
            }),
            0b1010111 => Ok(Self::VectorR {
                op: VectorRKind::from_bits(funct3, funct7)?,
                vd: VReg::from_u8(field5(word, 7) as u8)?,
                vs1: VReg::from_u8(field5(word, 15) as u8)?,
                vs2: VReg::from_u8(field5(word, 20) as u8)?,
            }),
            _ => Err(format!("unsupported instruction word 0x{word:08x}")),
        }
    }

    pub fn mnemonic(&self) -> String {
        match self {
            Instruction::Lui { rd, imm20 } => format!("lui {rd}, {imm20}"),
            Instruction::Addi { rd, rs1, imm } => format!("addi {rd}, {rs1}, {imm}"),
            Instruction::Lw { rd, rs1, off } => format!("lw {rd}, {off}({rs1})"),
            Instruction::Sw { rs2, rs1, off } => format!("sw {rs2}, {off}({rs1})"),
            Instruction::AluR { op, rd, rs1, rs2 } => {
                format!("{} {rd}, {rs1}, {rs2}", op.mnemonic())
            }
            Instruction::Branch { op, rs1, rs2, off } => {
                format!("{} {rs1}, {rs2}, {off}", op.mnemonic())
            }
            Instruction::Jal { rd, off } => format!("jal {rd}, {off}"),
            Instruction::Jalr { rd, rs1, off } => format!("jalr {rd}, {off}({rs1})"),
            Instruction::Csrrw { rd, csr, rs1 } => format!("csrrw {rd}, {csr}, {rs1}"),
            Instruction::Csrrs { rd, csr, rs1 } => format!("csrrs {rd}, {csr}, {rs1}"),
            Instruction::Mret => "mret".to_string(),
            Instruction::Halt => "halt".to_string(),
            Instruction::Vld { vd, rs1, off } => format!("vld {vd}, {off}({rs1})"),
            Instruction::Vst { vs, rs1, off } => format!("vst {vs}, {off}({rs1})"),
            Instruction::VectorR { op, vd, vs1, vs2 } => {
                format!("{} {vd}, {vs1}, {vs2}", op.mnemonic())
            }
        }
    }
}

fn expr_as_u20(expr: &Expr) -> Result<u32, String> {
    let value = expr
        .resolved_i32()
        .ok_or_else(|| format!("instruction still has unresolved expression: {expr}"))?;
    let masked = value as i64;
    if !(0..=(0x000f_ffff_i64)).contains(&masked) {
        return Err(format!(
            "value does not fit U-immediate upper 20 bits: {value}"
        ));
    }
    Ok(value as u32)
}

fn expr_as_i12(expr: &Expr) -> Result<i32, String> {
    fit_signed(expr, 12)
}

fn expr_as_i13(expr: &Expr) -> Result<i32, String> {
    fit_signed(expr, 13)
}

fn expr_as_i21(expr: &Expr) -> Result<i32, String> {
    fit_signed(expr, 21)
}

fn fit_signed(expr: &Expr, bits: u32) -> Result<i32, String> {
    let value = expr
        .resolved_i32()
        .ok_or_else(|| format!("instruction still has unresolved expression: {expr}"))?;
    let min = -(1_i64 << (bits - 1));
    let max = (1_i64 << (bits - 1)) - 1;
    let signed = i64::from(value);
    if signed < min || signed > max {
        return Err(format!(
            "value {value} does not fit signed {bits}-bit immediate"
        ));
    }
    Ok(value)
}

fn pack_r(funct7: u32, rs2: u32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    (funct7 << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode
}

fn pack_i(imm: i32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    (((imm as u32) & 0x0fff) << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode
}

fn pack_s(imm: i32, rs2: u32, rs1: u32, funct3: u32, opcode: u32) -> u32 {
    let imm = (imm as u32) & 0x0fff;
    let imm_hi = (imm >> 5) & 0x7f;
    let imm_lo = imm & 0x1f;
    (imm_hi << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (imm_lo << 7) | opcode
}

fn pack_b(imm: i32, rs2: u32, rs1: u32, funct3: u32, opcode: u32) -> u32 {
    let imm = (imm as u32) & 0x1fff;
    let bit12 = (imm >> 12) & 0x1;
    let bit11 = (imm >> 11) & 0x1;
    let bits10_5 = (imm >> 5) & 0x3f;
    let bits4_1 = (imm >> 1) & 0x0f;
    (bit12 << 31)
        | (bits10_5 << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (funct3 << 12)
        | (bits4_1 << 8)
        | (bit11 << 7)
        | opcode
}

fn pack_u(imm20: u32, rd: u32, opcode: u32) -> u32 {
    (imm20 << 12) | (rd << 7) | opcode
}

fn pack_j(imm: i32, rd: u32, opcode: u32) -> u32 {
    let imm = (imm as u32) & 0x1f_ffff;
    let bit20 = (imm >> 20) & 0x1;
    let bits10_1 = (imm >> 1) & 0x03ff;
    let bit11 = (imm >> 11) & 0x1;
    let bits19_12 = (imm >> 12) & 0x00ff;
    (bit20 << 31) | (bits10_1 << 21) | (bit11 << 20) | (bits19_12 << 12) | (rd << 7) | opcode
}

fn extract_i_imm(word: u32) -> i32 {
    sign_extend(field12(word, 20), 12)
}

fn extract_s_imm(word: u32) -> i32 {
    let imm = (field7(word, 25) << 5) | field5(word, 7);
    sign_extend(imm, 12)
}

fn extract_b_imm(word: u32) -> i32 {
    let imm = (bit(word, 31) << 12)
        | (bit(word, 7) << 11)
        | (field6(word, 25) << 5)
        | (field4(word, 8) << 1);
    sign_extend(imm, 13)
}

fn extract_j_imm(word: u32) -> i32 {
    let imm = (bit(word, 31) << 20)
        | (field8(word, 12) << 12)
        | (bit(word, 20) << 11)
        | (field10(word, 21) << 1);
    sign_extend(imm, 21)
}

fn sign_extend(value: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

fn bit(word: u32, shift: u32) -> u32 {
    (word >> shift) & 0x1
}

fn field3(word: u32, shift: u32) -> u32 {
    (word >> shift) & 0x7
}

fn field4(word: u32, shift: u32) -> u32 {
    (word >> shift) & 0x0f
}

fn field5(word: u32, shift: u32) -> u32 {
    (word >> shift) & 0x1f
}

fn field6(word: u32, shift: u32) -> u32 {
    (word >> shift) & 0x3f
}

fn field7(word: u32, shift: u32) -> u32 {
    (word >> shift) & 0x7f
}

fn field8(word: u32, shift: u32) -> u32 {
    (word >> shift) & 0xff
}

fn field10(word: u32, shift: u32) -> u32 {
    (word >> shift) & 0x03ff
}

fn field12(word: u32, shift: u32) -> u32 {
    (word >> shift) & 0x0fff
}
