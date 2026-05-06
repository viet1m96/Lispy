use crate::control::VecWbSel;
use crate::isa::{VReg, VectorRKind};

pub const VECTOR_REG_COUNT: usize = 8;
pub const VECTOR_LANES: usize = 4;
pub const VECTOR_LANE_BYTES: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorMemoryOp {
    Load { vd: VReg },
    Store { vs: VReg },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorRegisterFile {
    regs: [[u32; VECTOR_LANES]; VECTOR_REG_COUNT],
}

impl Default for VectorRegisterFile {
    fn default() -> Self {
        Self {
            regs: [[0; VECTOR_LANES]; VECTOR_REG_COUNT],
        }
    }
}

impl VectorRegisterFile {
    pub fn read(&self, reg: VReg) -> [u32; VECTOR_LANES] {
        self.regs[reg.bits() as usize]
    }

    pub fn write(
        &mut self,
        reg: VReg,
        value: [u32; VECTOR_LANES],
        enable: bool,
    ) -> Option<(VReg, [u32; VECTOR_LANES])> {
        if !enable {
            return None;
        }
        self.regs[reg.bits() as usize] = value;
        Some((reg, value))
    }

    pub fn read_lane(&self, reg: VReg, lane: usize) -> Result<u32, String> {
        if lane >= VECTOR_LANES {
            return Err(format!("vector lane index out of range: {lane}"));
        }
        Ok(self.regs[reg.bits() as usize][lane])
    }

    pub fn write_lane(
        &mut self,
        reg: VReg,
        lane: usize,
        value: u32,
        enable: bool,
    ) -> Result<Option<(VReg, usize, u32)>, String> {
        if !enable {
            return Ok(None);
        }
        if lane >= VECTOR_LANES {
            return Err(format!("vector lane index out of range: {lane}"));
        }
        self.regs[reg.bits() as usize][lane] = value;
        Ok(Some((reg, lane, value)))
    }

    pub fn snapshot(&self) -> [[u32; VECTOR_LANES]; VECTOR_REG_COUNT] {
        self.regs
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VectorBaseRegister {
    value: u32,
}

impl VectorBaseRegister {
    pub fn read(self) -> u32 {
        self.value
    }

    pub fn write(&mut self, value: u32, enable: bool) -> Option<u32> {
        if enable {
            self.value = value;
            Some(value)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LaneCounterRegister {
    value: usize,
}

impl LaneCounterRegister {
    pub fn read(self) -> usize {
        self.value
    }

    pub fn reset(&mut self, enable: bool) -> Option<usize> {
        if enable {
            self.value = 0;
            Some(self.value)
        } else {
            None
        }
    }

    pub fn increment_or_clear(&mut self, lane_done: bool, enable: bool) -> Option<usize> {
        if !enable {
            return None;
        }
        if lane_done {
            self.value = 0;
        } else {
            self.value += 1;
        }
        Some(self.value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VectorMemOpRegister {
    value: Option<VectorMemoryOp>,
}

impl VectorMemOpRegister {
    pub fn read(self) -> Option<VectorMemoryOp> {
        self.value
    }

    pub fn write(&mut self, value: VectorMemoryOp, enable: bool) -> Option<VectorMemoryOp> {
        if enable {
            self.value = Some(value);
            self.value
        } else {
            None
        }
    }

    pub fn clear(&mut self, enable: bool) -> bool {
        if enable {
            self.value = None;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VectorState {
    pub register_file: VectorRegisterFile,
    pub base_register: VectorBaseRegister,
    pub lane_counter: LaneCounterRegister,
    pub mem_op_register: VectorMemOpRegister,
}

impl VectorState {
    pub fn lane_done(&self) -> bool {
        LaneComparator.eval(self.lane_counter.read())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LaneOffsetShifter;

impl LaneOffsetShifter {
    pub fn eval(self, lane: usize) -> u32 {
        (lane as u32).wrapping_mul(VECTOR_LANE_BYTES)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VectorLaneAddressAdder;

impl VectorLaneAddressAdder {
    pub fn eval(self, base: u32, lane_offset: u32) -> u32 {
        base.wrapping_add(lane_offset)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LaneComparator;

impl LaneComparator {
    pub fn eval(self, lane: usize) -> bool {
        lane + 1 >= VECTOR_LANES
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VectorWriteBackMux;

impl VectorWriteBackMux {
    pub fn select(
        self,
        sel: VecWbSel,
        mem_lane: Option<u32>,
        alu_result: Option<[u32; VECTOR_LANES]>,
    ) -> Result<VectorWriteBackValue, String> {
        match sel {
            VecWbSel::None => Err("VectorWriteBackMUX selected None".to_string()),
            VecWbSel::MemLane => mem_lane.map(VectorWriteBackValue::Lane).ok_or_else(|| {
                "VectorWriteBackMUX selected MemLane but mem_out is missing".to_string()
            }),
            VecWbSel::Alu => alu_result.map(VectorWriteBackValue::Full).ok_or_else(|| {
                "VectorWriteBackMUX selected Alu but VectorALU result is missing".to_string()
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorWriteBackValue {
    Lane(u32),
    Full([u32; VECTOR_LANES]),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VectorAlu;

impl VectorAlu {
    pub fn execute(
        self,
        op: VectorRKind,
        lhs: [u32; VECTOR_LANES],
        rhs: [u32; VECTOR_LANES],
    ) -> Result<[u32; VECTOR_LANES], String> {
        let mut out = [0_u32; VECTOR_LANES];
        for lane in 0..VECTOR_LANES {
            out[lane] = execute_lane(op, lhs[lane], rhs[lane])?;
        }
        Ok(out)
    }
}

fn execute_lane(op: VectorRKind, lhs: u32, rhs: u32) -> Result<u32, String> {
    match op {
        VectorRKind::Vadd => Ok(lhs.wrapping_add(rhs)),
        VectorRKind::Vsub => Ok(lhs.wrapping_sub(rhs)),
        VectorRKind::Vmul => Ok(lhs.wrapping_mul(rhs)),
        VectorRKind::Vdiv => {
            if rhs == 0 {
                Err("vector division by zero".to_string())
            } else {
                Ok(((lhs as i32) / (rhs as i32)) as u32)
            }
        }
        VectorRKind::Vcmpeq => Ok(u32::from(lhs == rhs)),
    }
}
