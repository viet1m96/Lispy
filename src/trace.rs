use crate::isa::Instruction;
use crate::machine::Phase;

#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub tick: u64,
    pub phase: Phase,
    pub pc: u32,
    pub ir: u32,
    pub note: String,
}

#[derive(Debug, Clone, Default)]
pub struct TraceLog {
    entries: Vec<TraceEntry>,
}

impl TraceLog {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn push(&mut self, tick: u64, phase: Phase, pc: u32, ir: u32, note: impl Into<String>) {
        self.entries.push(TraceEntry {
            tick,
            phase,
            pc,
            ir,
            note: note.into(),
        });
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            let decoded = match Instruction::decode(entry.ir) {
                Ok(inst) => inst.mnemonic(),
                Err(_) => format!("0x{:08x}", entry.ir),
            };
            out.push_str(&format!(
                "tick={:04} phase={:<7} pc=0x{:08x} ir=0x{:08x} {:<24} | {}\n",
                entry.tick,
                entry.phase.name(),
                entry.pc,
                entry.ir,
                decoded,
                entry.note,
            ));
        }
        out
    }
}
