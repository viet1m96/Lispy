use crate::isa::BranchKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchInput {
    pub kind: BranchKind,
    pub rs1_value: u32,
    pub rs2_value: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchOutput {
    pub kind: BranchKind,
    pub rs1_value: u32,
    pub rs2_value: u32,
    pub taken: bool,
}

pub fn evaluate(input: BranchInput) -> BranchOutput {
    let taken = compare(input.kind, input.rs1_value, input.rs2_value);

    BranchOutput {
        kind: input.kind,
        rs1_value: input.rs1_value,
        rs2_value: input.rs2_value,
        taken,
    }
}

fn compare(kind: BranchKind, lhs: u32, rhs: u32) -> bool {
    match kind {
        BranchKind::Beq => lhs == rhs,
        BranchKind::Bne => lhs != rhs,
        BranchKind::Blt => (lhs as i32) < (rhs as i32),
        BranchKind::Bge => (lhs as i32) >= (rhs as i32),
    }
}
