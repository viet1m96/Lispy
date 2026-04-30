use crate::control::ControlUnit;
use crate::datapath;
use crate::interrupt::InterruptRequestInput;
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
    let device_events = machine.tick_devices();
    let device_note = if device_events.is_empty() {
        String::new()
    } else {
        format!("device: {}; ", device_events.join("; "))
    };

    let control_unit = ControlUnit::default();
    let phase = machine.phase();

    match phase {
        Phase::Fetch => {
            let pc = machine.pc;
            let control_step = control_unit.fetch_step(&machine.control_state)?;
            let datapath_note = datapath::tick_fetch(machine, &control_step.signals)?;
            let ir = machine.ir;
            trace.push(
                tick,
                phase,
                pc,
                ir,
                format!(
                    "{}{}",
                    device_note,
                    trace_note(&control_step, datapath_note)
                ),
            );
            machine.clock_control(false, control_step.internal.state_d);
        }
        Phase::Execute => {
            let pc_old = machine.pc;
            let ir = machine.ir;
            let inst = Instruction::decode(ir)?;
            let decoded = control_unit.decode(&inst);
            let branch_flags = datapath::branch_feedback(machine, &inst, &decoded)?;
            let irq_input = InterruptRequestInput {
                irq_pending: machine.interrupt_lines.pending,
                mie: machine.trap.mie(),
                in_trap: machine.trap.in_trap(),
            };
            let control_step = control_unit.execute_step(
                &machine.control_state,
                decoded,
                branch_flags,
                irq_input,
            )?;
            let datapath_note =
                datapath::apply_execute(machine, &inst, &control_step.signals, pc_old)?;
            trace.push(
                tick,
                phase,
                pc_old,
                ir,
                format!(
                    "{}{}",
                    device_note,
                    trace_note(&control_step, datapath_note)
                ),
            );
            machine.clock_control(false, control_step.internal.state_d);
        }
        Phase::TrapEnter => {
            let pc = machine.pc;
            let ir = machine.ir;
            let control_step = control_unit.trap_enter_step(&machine.control_state)?;
            let datapath_note = datapath::apply_trap_enter(machine, &control_step.signals)?;
            trace.push(
                tick,
                phase,
                pc,
                ir,
                format!(
                    "{}{}",
                    device_note,
                    trace_note(&control_step, datapath_note)
                ),
            );
            machine.clock_control(false, control_step.internal.state_d);
        }
        Phase::Halt => {
            trace.push(
                tick,
                phase,
                machine.pc,
                machine.ir,
                format!("{}cu: StateRegister.Q=Halt NextStateLogic.D=Halt; signals: pc_wr=0 ir_wr=0 reg_wr=0 mem_rd=0 mem_wr=0 halt_req=1; CPU stopped", device_note),
            );
            machine.set_halt("halt state");
        }
    }

    machine.refresh_interrupt_lines();
    machine.tick += 1;
    Ok(())
}

fn trace_note(control_step: &crate::control::ControlStep, datapath_note: String) -> String {
    format!("{} {}", control_step.trace_prefix(), datapath_note)
}
