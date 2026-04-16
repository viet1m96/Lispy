use std::fs;
use std::io;
use std::path::Path;

use crate::asm::{bytes_to_hex, AsmSection, AssembledProgram};
use crate::isa::Instruction;

pub const IMAGE_MAGIC: &[u8; 4] = b"AKIM";
pub const IMAGE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryLayout {
    pub text_base: u32,
    pub rodata_base: u32,
    pub data_base: u32,
    pub heap_base: u32,
    pub stack_top: u32,
    pub mmio_base: u32,
}

pub const DEFAULT_MEMORY_LAYOUT: MemoryLayout = MemoryLayout {
    text_base: 0x0000_0000,
    rodata_base: 0x0001_0000,
    data_base: 0x0002_0000,
    heap_base: 0x0003_0000,
    stack_top: 0x000f_0000,
    mmio_base: 0x00ff_0000,
};

#[derive(Debug, Clone)]
pub struct ProgramImage {
    pub layout: MemoryLayout,
    pub entry: u32,
    pub text: Vec<u8>,
    pub rodata: Vec<u8>,
    pub data: Vec<u8>,
}

impl ProgramImage {
    pub fn from_assembled(program: &AssembledProgram) -> Self {
        Self {
            layout: program.layout,
            entry: program.entry,
            text: program.text.bytes.clone(),
            rodata: program.rodata.bytes.clone(),
            data: program.data.bytes.clone(),
        }
    }

    pub fn write_to_file(&self, path: &Path) -> io::Result<()> {
        fs::write(path, self.to_bytes())
    }

    pub fn read_from_file(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        Self::from_bytes(&bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(IMAGE_MAGIC);
        write_u32(&mut out, IMAGE_VERSION);
        write_u32(&mut out, self.entry);

        write_u32(&mut out, self.layout.text_base);
        write_u32(&mut out, self.text.len() as u32);
        write_u32(&mut out, self.layout.rodata_base);
        write_u32(&mut out, self.rodata.len() as u32);
        write_u32(&mut out, self.layout.data_base);
        write_u32(&mut out, self.data.len() as u32);
        write_u32(&mut out, self.layout.heap_base);
        write_u32(&mut out, self.layout.stack_top);
        write_u32(&mut out, self.layout.mmio_base);

        out.extend_from_slice(&self.text);
        out.extend_from_slice(&self.rodata);
        out.extend_from_slice(&self.data);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 48 {
            return Err("image is too small".to_string());
        }
        if &bytes[0..4] != IMAGE_MAGIC {
            return Err("invalid image magic".to_string());
        }

        let version = read_u32(bytes, 4)?;
        if version != IMAGE_VERSION {
            return Err(format!("unsupported image version: {version}"));
        }

        let entry = read_u32(bytes, 8)?;
        let text_base = read_u32(bytes, 12)?;
        let text_size = read_u32(bytes, 16)? as usize;
        let rodata_base = read_u32(bytes, 20)?;
        let rodata_size = read_u32(bytes, 24)? as usize;
        let data_base = read_u32(bytes, 28)?;
        let data_size = read_u32(bytes, 32)? as usize;
        let heap_base = read_u32(bytes, 36)?;
        let stack_top = read_u32(bytes, 40)?;
        let mmio_base = read_u32(bytes, 44)?;

        let mut cursor = 48;
        let text = read_blob(bytes, &mut cursor, text_size)?;
        let rodata = read_blob(bytes, &mut cursor, rodata_size)?;
        let data = read_blob(bytes, &mut cursor, data_size)?;

        Ok(Self {
            layout: MemoryLayout {
                text_base,
                rodata_base,
                data_base,
                heap_base,
                stack_top,
                mmio_base,
            },
            entry,
            text,
            rodata,
            data,
        })
    }

    pub fn render_listing(&self) -> String {
        let mut out = String::new();
        render_section_listing(
            &mut out,
            AsmSection::Text,
            self.layout.text_base,
            &self.text,
            true,
        );
        out.push('\n');
        render_section_listing(
            &mut out,
            AsmSection::Rodata,
            self.layout.rodata_base,
            &self.rodata,
            false,
        );
        out.push('\n');
        render_section_listing(
            &mut out,
            AsmSection::Data,
            self.layout.data_base,
            &self.data,
            false,
        );
        out
    }
}

fn render_section_listing(
    out: &mut String,
    section: AsmSection,
    base: u32,
    bytes: &[u8],
    try_decode: bool,
) {
    out.push_str(&format!("[{} @ 0x{base:08x}]\n", section.name()));

    if bytes.is_empty() {
        out.push_str("(empty)\n");
        return;
    }

    let mut address = base;
    let mut cursor = 0;
    while cursor < bytes.len() {
        let remaining = bytes.len() - cursor;
        let width = if try_decode && remaining >= 4 {
            4
        } else {
            remaining.min(16)
        };
        let chunk = &bytes[cursor..cursor + width];
        let text = if try_decode && width == 4 {
            let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            match Instruction::decode(word) {
                Ok(inst) => inst.mnemonic(),
                Err(_) => format!(".word 0x{word:08x}"),
            }
        } else {
            format!(".bytes {}", bytes_to_hex(chunk))
        };
        out.push_str(&format!(
            "{address:08x} - {:<16} - {text}\n",
            bytes_to_hex(chunk)
        ));
        cursor += width;
        address += width as u32;
    }
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn read_u32(bytes: &[u8], start: usize) -> Result<u32, String> {
    let slice = bytes
        .get(start..start + 4)
        .ok_or_else(|| "unexpected end of image".to_string())?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_blob(bytes: &[u8], cursor: &mut usize, size: usize) -> Result<Vec<u8>, String> {
    let end = cursor.saturating_add(size);
    let slice = bytes
        .get(*cursor..end)
        .ok_or_else(|| "unexpected end of image while reading section".to_string())?;
    *cursor = end;
    Ok(slice.to_vec())
}
