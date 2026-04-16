use std::collections::VecDeque;

use crate::image::{MemoryLayout, ProgramImage};

pub const MMIO_SIZE: u32 = 0x100;

pub const MMIO_IN_STATUS: u32 = 0x00;
pub const MMIO_IN_DATA: u32 = 0x04;
pub const MMIO_OUT_DATA: u32 = 0x08;
pub const MMIO_OUT_STATUS: u32 = 0x0c;
pub const MMIO_IRQ_ACK: u32 = 0x10;

pub const IN_STATUS_HAS_DATA: u32 = 0x1;
pub const OUT_STATUS_READY: u32 = 0x1;

#[derive(Debug, Clone)]
pub struct MmioState {
    pub in_status: u32,
    pub in_data: u32,
    pub out_status: u32,
    pub irq_ack: u32,
    input_queue: VecDeque<u8>,
    pub output: Vec<u8>,
}

impl Default for MmioState {
    fn default() -> Self {
        Self {
            in_status: 0,
            in_data: 0,
            out_status: OUT_STATUS_READY,
            irq_ack: 0,
            input_queue: VecDeque::new(),
            output: Vec::new(),
        }
    }
}

impl MmioState {
    pub fn output_as_string(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    fn refresh_input_latch(&mut self) {
        if self.in_status & IN_STATUS_HAS_DATA != 0 {
            return;
        }

        if let Some(byte) = self.input_queue.front().copied() {
            self.in_data = u32::from(byte);
            self.in_status = IN_STATUS_HAS_DATA;
        } else {
            self.in_data = 0;
            self.in_status = 0;
        }
    }

    fn ack_input(&mut self) {
        self.irq_ack = 1;
        if self.in_status & IN_STATUS_HAS_DATA != 0 {
            let _ = self.input_queue.pop_front();
        }
        self.in_status = 0;
        self.in_data = 0;
        self.refresh_input_latch();
    }
}

#[derive(Debug, Clone)]
pub struct Memory {
    pub layout: MemoryLayout,
    text: Vec<u8>,
    rodata: Vec<u8>,
    data_ram: Vec<u8>,
    pub mmio: MmioState,
}

impl Memory {
    pub fn from_image(image: &ProgramImage) -> Result<Self, String> {
        if image.layout.data_base > image.layout.heap_base {
            return Err("data_base must be <= heap_base".to_string());
        }
        if image.layout.heap_base > image.layout.stack_top {
            return Err("heap_base must be <= stack_top".to_string());
        }
        if image.layout.stack_top > image.layout.mmio_base {
            return Err("stack_top must be <= mmio_base".to_string());
        }

        let mutable_size = image
            .layout
            .mmio_base
            .checked_sub(image.layout.data_base)
            .ok_or_else(|| "invalid mutable memory range".to_string())?;

        let mut data_ram = vec![0; mutable_size as usize];
        if image.data.len() > data_ram.len() {
            return Err("data section does not fit in mutable memory range".to_string());
        }
        data_ram[..image.data.len()].copy_from_slice(&image.data);

        Ok(Self {
            layout: image.layout,
            text: image.text.clone(),
            rodata: image.rodata.clone(),
            data_ram,
            mmio: MmioState::default(),
        })
    }

    pub fn output_as_string(&self) -> String {
        self.mmio.output_as_string()
    }

    pub fn load_u8(&self, address: u32) -> Result<u8, String> {
        if let Some(value) = read_from_region(address, self.layout.text_base, &self.text) {
            return Ok(value);
        }
        if let Some(value) = read_from_region(address, self.layout.rodata_base, &self.rodata) {
            return Ok(value);
        }
        if let Some(value) = read_from_region(address, self.layout.data_base, &self.data_ram) {
            return Ok(value);
        }
        Err(format!("read out of mapped memory at 0x{address:08x}"))
    }

    pub fn store_u8(&mut self, address: u32, value: u8) -> Result<(), String> {
        if write_to_region(address, self.layout.data_base, &mut self.data_ram, value) {
            return Ok(());
        }
        if self.is_mmio_address(address) {
            return Err(format!(
                "byte MMIO access is not supported at 0x{address:08x}; use word access"
            ));
        }
        Err(format!(
            "write out of mapped writable memory at 0x{address:08x}"
        ))
    }

    pub fn load_u32(&self, address: u32) -> Result<u32, String> {
        ensure_word_aligned(address)?;

        if self.is_mmio_address(address) {
            return self.load_mmio_u32(address);
        }

        let b0 = self.load_u8(address)?;
        let b1 = self.load_u8(address + 1)?;
        let b2 = self.load_u8(address + 2)?;
        let b3 = self.load_u8(address + 3)?;
        Ok(u32::from_le_bytes([b0, b1, b2, b3]))
    }

    pub fn store_u32(&mut self, address: u32, value: u32) -> Result<(), String> {
        ensure_word_aligned(address)?;

        if self.is_mmio_address(address) {
            return self.store_mmio_u32(address, value);
        }

        let bytes = value.to_le_bytes();
        self.store_u8(address, bytes[0])?;
        self.store_u8(address + 1, bytes[1])?;
        self.store_u8(address + 2, bytes[2])?;
        self.store_u8(address + 3, bytes[3])?;
        Ok(())
    }

    fn is_mmio_address(&self, address: u32) -> bool {
        let start = self.layout.mmio_base;
        let end = start.saturating_add(MMIO_SIZE);
        (start..end).contains(&address)
    }

    fn load_mmio_u32(&self, address: u32) -> Result<u32, String> {
        let offset = address - self.layout.mmio_base;
        match offset {
            MMIO_IN_STATUS => Ok(self.mmio.in_status),
            MMIO_IN_DATA => Ok(self.mmio.in_data),
            MMIO_OUT_DATA => Ok(0),
            MMIO_OUT_STATUS => Ok(self.mmio.out_status),
            MMIO_IRQ_ACK => Ok(self.mmio.irq_ack),
            _ => Err(format!("unknown MMIO register at 0x{address:08x}")),
        }
    }

    fn store_mmio_u32(&mut self, address: u32, value: u32) -> Result<(), String> {
        let offset = address - self.layout.mmio_base;
        match offset {
            MMIO_OUT_DATA => {
                self.mmio.output.push((value & 0xff) as u8);
                Ok(())
            }
            MMIO_IRQ_ACK => {
                if value != 0 {
                    self.mmio.ack_input();
                }
                Ok(())
            }
            MMIO_IN_STATUS | MMIO_IN_DATA | MMIO_OUT_STATUS => Err(format!(
                "MMIO register at 0x{address:08x} is read-only in milestone 2"
            )),
            _ => Err(format!("unknown MMIO register at 0x{address:08x}")),
        }
    }
}

fn ensure_word_aligned(address: u32) -> Result<(), String> {
    if address % 4 != 0 {
        Err(format!("unaligned word access at 0x{address:08x}"))
    } else {
        Ok(())
    }
}

fn read_from_region(address: u32, base: u32, region: &[u8]) -> Option<u8> {
    let offset = address.checked_sub(base)? as usize;
    region.get(offset).copied()
}

fn write_to_region(address: u32, base: u32, region: &mut [u8], value: u8) -> bool {
    let Some(offset) = address.checked_sub(base) else {
        return false;
    };
    let offset = offset as usize;
    if let Some(slot) = region.get_mut(offset) {
        *slot = value;
        true
    } else {
        false
    }
}
