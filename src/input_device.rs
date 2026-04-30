use std::collections::VecDeque;

use crate::interrupt::{InterruptLines, INPUT_IRQ_ID};
use crate::memory_state::MemoryState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledInputByte {
    pub tick: u64,
    pub byte: u8,
}

#[derive(Debug, Clone, Default)]
pub struct InputDevice {
    schedule: VecDeque<ScheduledInputByte>,
    pub lost_input: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceTickResult {
    pub interrupt_lines: InterruptLines,
    pub events: Vec<String>,
}

impl InputDevice {
    pub fn load_schedule_text(&mut self, text: &str) -> Result<(), String> {
        let mut events = Vec::new();

        for (line_no, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut parts = line.split_whitespace();

            let tick_text = parts
                .next()
                .ok_or_else(|| format!("missing tick on input line {}", line_no + 1))?;

            let byte_text = parts
                .next()
                .ok_or_else(|| format!("missing byte on input line {}", line_no + 1))?;

            if parts.next().is_some() {
                return Err(format!(
                    "too many fields on input line {}. Expected format: <tick> <byte>",
                    line_no + 1
                ));
            }

            let tick = tick_text.parse::<u64>().map_err(|_| {
                format!("invalid tick on input line {}: {}", line_no + 1, tick_text)
            })?;

            let byte = parse_byte(byte_text)
                .map_err(|err| format!("{} on input line {}", err, line_no + 1))?;

            events.push(ScheduledInputByte { tick, byte });
        }

        events.sort_by_key(|event| event.tick);
        self.schedule = VecDeque::from(events);
        self.lost_input.clear();

        Ok(())
    }

    pub fn tick(&mut self, tick: u64, memory: &mut MemoryState) -> DeviceTickResult {
        let mut events = Vec::new();

        while matches!(self.schedule.front(), Some(event) if event.tick == tick) {
            let event = self.schedule.pop_front().expect("front checked");

            if memory.input_data_pending() {
                self.lost_input.push(event.byte);
                events.push(format!(
                    "InputDevice: tick={} byte=0x{:02x} arrived while MMIO_IN_DATA is still pending; byte lost; irq_pending remains {}",
                    tick,
                    event.byte,
                    u8::from(memory.input_interrupt_pending()),
                ));
                continue;
            }

            memory.latch_input_byte(event.byte);
            events.push(format!(
                "InputDevice: tick={} byte=0x{:02x} latched to MMIO_IN_DATA; irq_pending=1 irq_id={}",
                tick,
                event.byte,
                INPUT_IRQ_ID,
            ));
        }

        let interrupt_lines = InterruptLines {
            pending: memory.input_interrupt_pending(),
            irq_id: if memory.input_interrupt_pending() {
                INPUT_IRQ_ID
            } else {
                0
            },
        };

        DeviceTickResult {
            interrupt_lines,
            events,
        }
    }
}

fn parse_byte(text: &str) -> Result<u8, String> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u8::from_str_radix(hex, 16)
            .map_err(|_| format!("invalid hex byte literal: {}", text));
    }

    match text {
        "\\n" => return Ok(b'\n'),
        "\\r" => return Ok(b'\r'),
        "\\t" => return Ok(b'\t'),
        "\\0" => return Ok(0),
        _ => {}
    }

    if let Ok(value) = text.parse::<u8>() {
        return Ok(value);
    }

    if text.len() == 1 {
        return Ok(text.as_bytes()[0]);
    }

    Err(format!(
        "invalid byte literal: {}. Use A, 65, 0x41, \\n, \\t, \\r, or \\0",
        text
    ))
}
