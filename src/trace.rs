use crate::isa::Instruction;
use crate::machine::Phase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceRenderMode {
    Brief,
    Full,
}

impl TraceRenderMode {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "brief" => Some(Self::Brief),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Brief => "brief",
            Self::Full => "full",
        }
    }
}

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

    pub fn render_mode(&self, mode: TraceRenderMode) -> String {
        match mode {
            TraceRenderMode::Brief => self.render_brief(),
            TraceRenderMode::Full => self.render_full(),
        }
    }

    pub fn render_brief(&self) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            let decoded = decode_instruction_text(entry.ir);
            let tags = brief_tags(entry);
            let suffix = if tags.is_empty() {
                String::new()
            } else {
                format!("  [{}]", tags.join(", "))
            };

            out.push_str(&format!(
                "T{:04}  phase={:<10} pc=0x{:08x} ir=0x{:08x}  {}{}\n",
                entry.tick,
                entry.phase.name(),
                entry.pc,
                entry.ir,
                decoded,
                suffix,
            ));
        }
        out
    }

    pub fn render_full(&self) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            let decoded = decode_instruction_text(entry.ir);

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
}

fn decode_instruction_text(ir: u32) -> String {
    match Instruction::decode(ir) {
        Ok(inst) => inst.mnemonic(),
        Err(_) => format!("0x{ir:08x}"),
    }
}

fn brief_tags(entry: &TraceEntry) -> Vec<String> {
    let mut tags = Vec::new();
    let note = entry.note.as_str();

    if let Some(byte) = find_hex_value_after(note, "byte=") {
        if note.contains("byte lost") {
            tags.push(format!("lost_input={}", format_byte(byte as u8)));
        } else {
            tags.push(format!("input={}", format_byte(byte as u8)));
        }
    }

    if let Some(value) = find_hex_value_after(note, "Memory[0x00ff0008] <- ") {
        tags.push(format!("out={}", format_byte(value as u8)));
    }

    if note.contains("Memory[0x00ff0010] <- 0x00000001") {
        tags.push("ack".to_string());
    }

    if entry.phase == Phase::TrapEnter || note.contains("trap_enter=1") {
        tags.push("trap".to_string());
    }

    if entry.phase == Phase::VecOp || note.contains("start_vec_op=1") {
        tags.push("vector".to_string());
    }

    if note.contains("trap_exit=1") {
        tags.push("mret".to_string());
    }

    if note.contains("take_branch=Some(true)") {
        tags.push("branch=taken".to_string());
    }

    if note.contains("halt_req=1") {
        tags.push("halt".to_string());
    }

    tags
}

fn find_hex_value_after(text: &str, marker: &str) -> Option<u32> {
    let start = text.find(marker)? + marker.len();
    let rest = text.get(start..)?.strip_prefix("0x")?;
    let hex_len = rest
        .chars()
        .take_while(|ch| ch.is_ascii_hexdigit())
        .take(8)
        .count();
    if hex_len == 0 {
        return None;
    }
    u32::from_str_radix(&rest[..hex_len], 16).ok()
}

fn format_byte(byte: u8) -> String {
    match byte {
        b'\n' => "'\\n'".to_string(),
        b'\r' => "'\\r'".to_string(),
        b'\t' => "'\\t'".to_string(),
        b'\\' => "'\\\\'".to_string(),
        b'\'' => "'\\''".to_string(),
        0x20..=0x7e => format!("'{}'", byte as char),
        _ => format!("0x{byte:02x}"),
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
