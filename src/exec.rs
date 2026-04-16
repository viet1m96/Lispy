use crate::control;
use crate::datapath;
use crate::isa::Instruction;
use crate::machine::{Machine, Phase};
use crate::trace::TraceLog;

pub fn run_to_halt(machine: &mut Machine, max_ticks: u64) -> Result<TraceLog, String> {
    let mut trace = TraceLog::new();

    while !machine.halted {
        if machine.tick >= max_ticks {
            return Err(format!("simulation exceeded max tick budget ({max_ticks})"));
        }
        step_tick(machine, &mut trace)?;
    }

    Ok(trace)
}

pub fn step_tick(machine: &mut Machine, trace: &mut TraceLog) -> Result<(), String> {
    let tick = machine.tick;

    match machine.phase {
        Phase::Fetch => {
            let (pc, ir) = datapath::fetch_into_ir(machine)?;
            trace.push(tick, Phase::Fetch, pc, ir, format!("ir <- mem[0x{pc:08x}]"));
            machine.phase = control::next_phase_after_fetch();
        }
        Phase::Execute => {
            let pc_old = machine.pc;
            let ir = machine.ir;
            let inst = Instruction::decode(ir)?;
            let signals = control::generate_signals(&inst)?;
            let branch_out = datapath::branch_feedback(machine, &inst, &signals, pc_old)?;
            let note = datapath::apply_execute(machine, &inst, &signals, branch_out, pc_old)?;
            trace.push(tick, Phase::Execute, pc_old, ir, note);
            if !machine.halted {
                machine.phase = control::next_phase_after_execute(machine.halted);
            }
        }
    }

    machine.tick += 1;
    Ok(())
}
