use std::collections::BTreeMap;
use std::fmt;

use crate::image::{MemoryLayout, DEFAULT_MEMORY_LAYOUT};
use crate::isa::{Instruction, INSTRUCTION_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum AsmSection {
    Text,
    Rodata,
    Data,
}

impl AsmSection {
    pub fn name(self) -> &'static str {
        match self {
            Self::Text => ".text",
            Self::Rodata => ".rodata",
            Self::Data => ".data",
        }
    }

    pub fn base(self, layout: &MemoryLayout) -> u32 {
        match self {
            Self::Text => layout.text_base,
            Self::Rodata => layout.rodata_base,
            Self::Data => layout.data_base,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Resolved(i32),
    LabelPcRel(String),
    LabelHi20(String),
    LabelLo12(String),
}

impl Expr {
    pub fn from_i32(value: i32) -> Self {
        Self::Resolved(value)
    }

    pub fn pcrel(name: &str) -> Self {
        Self::LabelPcRel(name.to_string())
    }

    pub fn hi20(name: &str) -> Self {
        Self::LabelHi20(name.to_string())
    }

    pub fn lo12(name: &str) -> Self {
        Self::LabelLo12(name.to_string())
    }

    pub fn resolved_i32_ref(&self) -> Option<i32> {
        match self {
            Self::Resolved(value) => Some(*value),
            _ => None,
        }
    }

    pub fn resolved_i32(&self) -> Option<i32> {
        self.resolved_i32_ref()
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Resolved(value) => write!(f, "{value}"),
            Expr::LabelPcRel(name) => write!(f, "%pcrel({name})"),
            Expr::LabelHi20(name) => write!(f, "%hi({name})"),
            Expr::LabelLo12(name) => write!(f, "%lo({name})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataItem {
    Word(u32),
    PStr(String),
}

impl DataItem {
    pub fn size(&self) -> u32 {
        match self {
            Self::Word(_) => 4,
            Self::PStr(text) => 4 + (text.chars().count() as u32) * 4,
        }
    }

    pub fn render(&self) -> String {
        match self {
            Self::Word(value) => format!(".word 0x{value:08x}"),
            Self::PStr(text) => format!(".pstr \"{}\"", escape_text(text)),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Word(value) => value.to_le_bytes().to_vec(),
            Self::PStr(text) => encode_pstr(text),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionItem {
    Label(String),
    Inst(Instruction),
    Data(DataItem),
}

#[derive(Debug, Clone)]
pub struct AsmProgram {
    pub layout: MemoryLayout,
    pub entry_label: String,
    items: BTreeMap<AsmSection, Vec<SectionItem>>,
}

impl AsmProgram {
    pub fn new() -> Self {
        let mut items = BTreeMap::new();
        items.insert(AsmSection::Text, Vec::new());
        items.insert(AsmSection::Rodata, Vec::new());
        items.insert(AsmSection::Data, Vec::new());
        Self {
            layout: DEFAULT_MEMORY_LAYOUT,
            entry_label: "_start".to_string(),
            items,
        }
    }

    pub fn set_entry_label(&mut self, label: &str) {
        self.entry_label = label.to_string();
    }

    pub fn label(&mut self, section: AsmSection, name: &str) {
        self.items
            .get_mut(&section)
            .expect("known section")
            .push(SectionItem::Label(name.to_string()));
    }

    pub fn emit_inst(&mut self, section: AsmSection, inst: Instruction) {
        self.items
            .get_mut(&section)
            .expect("known section")
            .push(SectionItem::Inst(inst));
    }

    pub fn emit_data(&mut self, section: AsmSection, data: DataItem) {
        self.items
            .get_mut(&section)
            .expect("known section")
            .push(SectionItem::Data(data));
    }

    pub fn assemble(&self) -> Result<AssembledProgram, String> {
        let symbols = self.build_symbol_table()?;
        let entry = *symbols
            .get(&self.entry_label)
            .ok_or_else(|| format!("missing entry label: {}", self.entry_label))?;

        let text = self.assemble_section(AsmSection::Text, &symbols)?;
        let rodata = self.assemble_section(AsmSection::Rodata, &symbols)?;
        let data = self.assemble_section(AsmSection::Data, &symbols)?;

        Ok(AssembledProgram {
            layout: self.layout,
            entry,
            symbols,
            text,
            rodata,
            data,
        })
    }

    fn build_symbol_table(&self) -> Result<BTreeMap<String, u32>, String> {
        let mut table = BTreeMap::new();
        for section in [AsmSection::Text, AsmSection::Rodata, AsmSection::Data] {
            let items = self.items.get(&section).expect("known section");
            let mut cursor = section.base(&self.layout);
            for item in items {
                match item {
                    SectionItem::Label(name) => {
                        if table.insert(name.clone(), cursor).is_some() {
                            return Err(format!("duplicate label: {name}"));
                        }
                    }

                    SectionItem::Inst(_) => cursor += INSTRUCTION_SIZE,
                    SectionItem::Data(data) => cursor += data.size(),
                }
            }
        }
        Ok(table)
    }

    fn assemble_section(
        &self,
        section: AsmSection,
        symbols: &BTreeMap<String, u32>,
    ) -> Result<AssembledSection, String> {
        let mut bytes = Vec::new();
        let mut listing = Vec::new();
        let mut cursor = section.base(&self.layout);
        let items = self.items.get(&section).expect("known section");

        for item in items {
            match item {
                SectionItem::Label(_) => {}

                SectionItem::Inst(inst) => {
                    let resolved = resolve_instruction(inst, cursor, symbols)?;
                    let word = resolved.encode_resolved()?;
                    let entry_bytes = word.to_le_bytes().to_vec();
                    listing.push(ListingEntry {
                        address: cursor,
                        bytes: entry_bytes.clone(),
                        text: resolved.mnemonic(),
                    });
                    bytes.extend_from_slice(&entry_bytes);
                    cursor += INSTRUCTION_SIZE;
                }
                SectionItem::Data(data) => {
                    let entry_bytes = data.encode();
                    listing.push(ListingEntry {
                        address: cursor,
                        bytes: entry_bytes.clone(),
                        text: data.render(),
                    });
                    bytes.extend_from_slice(&entry_bytes);
                    cursor += data.size();
                }
            }
        }

        Ok(AssembledSection {
            section,
            base: section.base(&self.layout),
            bytes,
            listing,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AssembledProgram {
    pub layout: MemoryLayout,
    pub entry: u32,
    pub symbols: BTreeMap<String, u32>,
    pub text: AssembledSection,
    pub rodata: AssembledSection,
    pub data: AssembledSection,
}

impl AssembledProgram {
    pub fn render_listing(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("entry = 0x{:08x}\n", self.entry));
        out.push_str("\n[symbols]\n");
        for (name, address) in &self.symbols {
            out.push_str(&format!("{name:<20} 0x{address:08x}\n"));
        }
        out.push_str("\n");
        out.push_str(&self.text.render_listing());
        out.push('\n');
        out.push_str(&self.rodata.render_listing());
        out.push('\n');
        out.push_str(&self.data.render_listing());
        out
    }
}

#[derive(Debug, Clone)]
pub struct AssembledSection {
    pub section: AsmSection,
    pub base: u32,
    pub bytes: Vec<u8>,
    pub listing: Vec<ListingEntry>,
}

impl AssembledSection {
    pub fn render_listing(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "[{} @ 0x{:08x}]\n",
            self.section.name(),
            self.base
        ));
        for entry in &self.listing {
            out.push_str(&format!(
                "{:08x} - {:<16} - {}\n",
                entry.address,
                bytes_to_hex(&entry.bytes),
                entry.text
            ));
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct ListingEntry {
    pub address: u32,
    pub bytes: Vec<u8>,
    pub text: String,
}

fn resolve_instruction(
    inst: &Instruction,
    address: u32,
    symbols: &BTreeMap<String, u32>,
) -> Result<Instruction, String> {
    match inst {
        Instruction::Lui { rd, imm20 } => Ok(Instruction::Lui {
            rd: *rd,
            imm20: resolve_expr(imm20, address, symbols)?,
        }),
        Instruction::Addi { rd, rs1, imm } => Ok(Instruction::Addi {
            rd: *rd,
            rs1: *rs1,
            imm: resolve_expr(imm, address, symbols)?,
        }),
        Instruction::Lw { rd, rs1, off } => Ok(Instruction::Lw {
            rd: *rd,
            rs1: *rs1,
            off: resolve_expr(off, address, symbols)?,
        }),
        Instruction::Sw { rs2, rs1, off } => Ok(Instruction::Sw {
            rs2: *rs2,
            rs1: *rs1,
            off: resolve_expr(off, address, symbols)?,
        }),
        Instruction::AluR { op, rd, rs1, rs2 } => Ok(Instruction::AluR {
            op: *op,
            rd: *rd,
            rs1: *rs1,
            rs2: *rs2,
        }),
        Instruction::Branch { op, rs1, rs2, off } => Ok(Instruction::Branch {
            op: *op,
            rs1: *rs1,
            rs2: *rs2,
            off: resolve_expr(off, address, symbols)?,
        }),
        Instruction::Jal { rd, off } => Ok(Instruction::Jal {
            rd: *rd,
            off: resolve_expr(off, address, symbols)?,
        }),
        Instruction::Jalr { rd, rs1, off } => Ok(Instruction::Jalr {
            rd: *rd,
            rs1: *rs1,
            off: resolve_expr(off, address, symbols)?,
        }),
        Instruction::Csrrw { rd, csr, rs1 } => Ok(Instruction::Csrrw {
            rd: *rd,
            csr: *csr,
            rs1: *rs1,
        }),
        Instruction::Csrrs { rd, csr, rs1 } => Ok(Instruction::Csrrs {
            rd: *rd,
            csr: *csr,
            rs1: *rs1,
        }),
        Instruction::Mret => Ok(Instruction::Mret),
        Instruction::Halt => Ok(Instruction::Halt),
        Instruction::Vld { vd, rs1, off } => Ok(Instruction::Vld {
            vd: *vd,
            rs1: *rs1,
            off: resolve_expr(off, address, symbols)?,
        }),
        Instruction::Vst { vs, rs1, off } => Ok(Instruction::Vst {
            vs: *vs,
            rs1: *rs1,
            off: resolve_expr(off, address, symbols)?,
        }),
        Instruction::VectorR { op, vd, vs1, vs2 } => Ok(Instruction::VectorR {
            op: *op,
            vd: *vd,
            vs1: *vs1,
            vs2: *vs2,
        }),
    }
}

fn resolve_expr(
    expr: &Expr,
    address: u32,
    symbols: &BTreeMap<String, u32>,
) -> Result<Expr, String> {
    let value = match expr {
        Expr::Resolved(value) => *value,

        Expr::LabelPcRel(name) => {
            let target = *symbols
                .get(name)
                .ok_or_else(|| format!("unknown label: {name}"))? as i32;
            target - address as i32
        }
        Expr::LabelHi20(name) => {
            let address = *symbols
                .get(name)
                .ok_or_else(|| format!("unknown label: {name}"))?;
            hi20_for_address(address)
        }
        Expr::LabelLo12(name) => {
            let address = *symbols
                .get(name)
                .ok_or_else(|| format!("unknown label: {name}"))?;
            lo12_for_address(address)
        }
    };
    Ok(Expr::Resolved(value))
}

pub fn hi20_for_address(address: u32) -> i32 {
    (((address as i64) + 0x800) >> 12) as i32
}

pub fn lo12_for_address(address: u32) -> i32 {
    let hi = hi20_for_address(address);
    address as i32 - (hi << 12)
}

fn encode_pstr(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let chars: Vec<u32> = text.chars().map(u32::from).collect();
    out.extend_from_slice(&(chars.len() as u32).to_le_bytes());
    for ch in chars {
        out.extend_from_slice(&ch.to_le_bytes());
    }
    out
}

pub fn bytes_to_hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".to_string();
    }
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

fn escape_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}
