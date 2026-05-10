use std::fs;
use std::io;
use std::path::Path;

use crate::asm::AssembledProgram;

pub const IMAGE_MAGIC: &[u8; 4] = b"AKIM";
pub const IMAGE_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryLayout {
    pub text_base: u32,
    pub data_base: u32,
    pub heap_base: u32,
    pub stack_top: u32,
    pub mmio_base: u32,
}

pub const DEFAULT_MEMORY_LAYOUT: MemoryLayout = MemoryLayout {
    text_base: 0x0000_0000,
    data_base: 0x0001_0000,
    heap_base: 0x0003_0000,
    stack_top: 0x000f_0000,
    mmio_base: 0x00ff_0000,
};

#[derive(Debug, Clone)]
pub struct ProgramImage {
    pub layout: MemoryLayout,
    pub entry: u32,
    pub text: Vec<u8>,
    pub data: Vec<u8>,
}

impl ProgramImage {
    pub fn from_assembled(program: &AssembledProgram) -> Self {
        Self {
            layout: program.layout,
            entry: program.entry,
            text: program.text.bytes.clone(),
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
        write_u32(&mut out, self.layout.data_base);
        write_u32(&mut out, self.data.len() as u32);
        write_u32(&mut out, self.layout.heap_base);
        write_u32(&mut out, self.layout.stack_top);
        write_u32(&mut out, self.layout.mmio_base);

        out.extend_from_slice(&self.text);
        out.extend_from_slice(&self.data);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 40 {
            return Err("image is too small".to_string());
        }
        if &bytes[0..4] != IMAGE_MAGIC {
            return Err("invalid image magic".to_string());
        }

        let version = read_u32(bytes, 4)?;
        if version != IMAGE_VERSION {
            return Err(format!("invalid image version: {version}"));
        }

        let entry = read_u32(bytes, 8)?;
        let text_base = read_u32(bytes, 12)?;
        let text_size = read_u32(bytes, 16)? as usize;
        let data_base = read_u32(bytes, 20)?;
        let data_size = read_u32(bytes, 24)? as usize;
        let heap_base = read_u32(bytes, 28)?;
        let stack_top = read_u32(bytes, 32)?;
        let mmio_base = read_u32(bytes, 36)?;

        let mut cursor = 40;
        let text = read_blob(bytes, &mut cursor, text_size)?;
        let data = read_blob(bytes, &mut cursor, data_size)?;

        Ok(Self {
            layout: MemoryLayout {
                text_base,
                data_base,
                heap_base,
                stack_top,
                mmio_base,
            },
            entry,
            text,
            data,
        })
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
