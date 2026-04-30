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
        self.render_pretty()
    }

    pub fn render_pretty(&self) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            let decoded = match Instruction::decode(entry.ir) {
                Ok(inst) => inst.mnemonic(),
                Err(_) => format!("0x{:08x}", entry.ir),
            };

            let (cu_internal, signals, actions) = split_note(&entry.note);
            out.push_str(&format!(
                "T{:04}  phase={:<7} pc=0x{:08x} ir=0x{:08x}  {}\n",
                entry.tick,
                entry.phase.name(),
                entry.pc,
                entry.ir,
                decoded,
            ));

            if let Some(cu_internal) = cu_internal {
                out.push_str("       CU/in  : ");
                out.push_str(&format_signals(cu_internal));
                out.push('\n');
            }

            if let Some(signals) = signals {
                out.push_str("       CU/out  : ");
                out.push_str(&format_signals(signals));
                out.push('\n');
            }

            let actions = actions.trim();
            if !actions.is_empty() {
                out.push_str("       DP      :\n");
                for action in actions
                    .split(';')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                {
                    out.push_str("         - ");
                    out.push_str(action);
                    out.push('\n');
                }
            }
        }
        out
    }

    pub fn render_compact(&self) -> String {
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

fn split_note(note: &str) -> (Option<&str>, Option<&str>, &str) {
    let mut rest = note.trim();
    let mut cu_internal = None;

    if let Some(after_cu) = rest.strip_prefix("cu: ") {
        if let Some((internal, after_internal)) = after_cu.split_once("; signals: ") {
            cu_internal = Some(internal.trim());
            rest = after_internal.trim();
            rest = rest.strip_prefix("signals: ").unwrap_or(rest);
        } else {
            return (Some(after_cu.trim()), None, "");
        }
    } else if let Some(after_signals) = rest.strip_prefix("signals: ") {
        rest = after_signals;
    } else {
        return (None, None, rest);
    }

    match rest.split_once(';') {
        Some((signals, actions)) => (cu_internal, Some(signals.trim()), actions),
        None => (cu_internal, Some(rest.trim()), ""),
    }
}

fn format_signals(signals: &str) -> String {
    signals.split_whitespace().collect::<Vec<_>>().join("  ")
}
