use std::env;
use std::path::Path;

use lispy::asm::AsmProgram;
use lispy::compiler::compile_source;
use lispy::exec::run_to_halt;
use lispy::image::ProgramImage;
use lispy::lisp::parse_program;
use lispy::machine::Machine;
use lispy::trace::TraceRenderMode;

fn print_usage() {
    println!("lab4-rust");
    println!("  dump-image <input.bin>                         print image summary and listing");
    println!("  sim-image <input.bin> [schedule.txt] [max_ticks] [brief|full]  run tick engine");
    println!("  dump-ast <input.lisp>                          parse Lisp source and print AST");
    println!(
        "  compile-lisp <input.lisp> <out.bin>            compile Lisp source to binary image"
    );
    println!(
        "  run-lisp <input.lisp> [schedule.txt] [max_ticks] [brief|full]  compile and simulate"
    );
}

fn cmd_dump_image(path: &Path) -> Result<(), String> {
    let image = ProgramImage::read_from_file(path).map_err(|e| e.to_string())?;
    println!("entry   : 0x{:08x}", image.entry);
    println!(
        "text    : base=0x{:08x}, size={} bytes",
        image.layout.text_base,
        image.text.len()
    );
    println!(
        "rodata  : base=0x{:08x}, size={} bytes",
        image.layout.rodata_base,
        image.rodata.len()
    );
    println!(
        "data    : base=0x{:08x}, size={} bytes",
        image.layout.data_base,
        image.data.len()
    );
    println!();
    println!("{}", image.render_listing());
    Ok(())
}

fn cmd_sim_image(
    path: &Path,
    input_path: Option<&Path>,
    max_ticks: u64,
    trace_mode: TraceRenderMode,
) -> Result<(), String> {
    let image = ProgramImage::read_from_file(path).map_err(|e| e.to_string())?;
    run_image(&image, input_path, max_ticks, trace_mode)
}

fn cmd_dump_ast(path: &Path) -> Result<(), String> {
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let program = parse_program(&source)?;
    print!("{}", program.render_tree());
    Ok(())
}

fn cmd_compile_lisp(input: &Path, output: &Path) -> Result<(), String> {
    let source = std::fs::read_to_string(input).map_err(|e| e.to_string())?;
    let program = compile_source(&source)?;
    write_program_outputs(&program, output)
}

fn cmd_run_lisp(
    input: &Path,
    input_path: Option<&Path>,
    max_ticks: u64,
    trace_mode: TraceRenderMode,
) -> Result<(), String> {
    let source = std::fs::read_to_string(input).map_err(|e| e.to_string())?;
    let program = compile_source(&source)?;
    let assembled = program.assemble()?;
    let image = ProgramImage::from_assembled(&assembled);
    run_image(&image, input_path, max_ticks, trace_mode)
}

fn write_program_outputs(program: &AsmProgram, path: &Path) -> Result<(), String> {
    let assembled = program.assemble()?;
    let image = ProgramImage::from_assembled(&assembled);
    image.write_to_file(path).map_err(|e| e.to_string())?;
    let listing_path = path.with_extension("lst");
    std::fs::write(&listing_path, assembled.render_listing()).map_err(|e| e.to_string())?;
    println!("wrote image: {}", path.display());
    println!("wrote listing: {}", listing_path.display());
    Ok(())
}

fn load_input_into_machine(machine: &mut Machine, bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "trap input must be a UTF-8 schedule file, for example: 10 A".to_string())?;
    machine.load_input_schedule_text(text)
}

fn run_image(
    image: &ProgramImage,
    input_path: Option<&Path>,
    max_ticks: u64,
    trace_mode: TraceRenderMode,
) -> Result<(), String> {
    let mut machine = Machine::from_image(image)?;
    if let Some(path) = input_path {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        load_input_into_machine(&mut machine, &bytes)?;
    }
    let trace = run_to_halt(&mut machine, max_ticks)?;
    println!("halted : {}", machine.halted);
    println!("reason : {}", machine.halt_reason.as_deref().unwrap_or("-"));
    println!("ticks  : {}", machine.tick);
    println!("pc     : 0x{:08x}", machine.pc);
    println!("phase  : {}", machine.phase().name());
    println!("output : {:?}", machine.output_as_string());
    println!(
        "lost_input : {:?}",
        String::from_utf8_lossy(&machine.input_device.lost_input)
    );
    println!();
    println!("[trace:{}]", trace_mode.name());
    print!("{}", trace.render_mode(trace_mode));
    Ok(())
}

fn parse_run_args(
    args: &[String],
    start: usize,
) -> Result<(Option<&Path>, u64, TraceRenderMode), String> {
    let mut input_path = None;
    let mut max_ticks = 1_000_u64;
    let mut trace_mode = TraceRenderMode::Brief;

    for value in args.iter().skip(start) {
        if let Some(mode) = TraceRenderMode::parse(value) {
            trace_mode = mode;
            continue;
        }

        if let Ok(ticks) = value.parse::<u64>() {
            max_ticks = ticks;
            continue;
        }

        if input_path.is_none() {
            input_path = Some(Path::new(value));
            continue;
        }

        return Err(format!("unexpected extra argument: {value}"));
    }

    Ok((input_path, max_ticks, trace_mode))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        return;
    }

    let result = match args[1].as_str() {
        "dump-image" if args.len() == 3 => cmd_dump_image(Path::new(&args[2])),
        "sim-image" if (3..=6).contains(&args.len()) => {
            let (input_path, max_ticks, trace_mode) = match parse_run_args(&args, 3) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("error: {err}");
                    std::process::exit(1);
                }
            };
            cmd_sim_image(Path::new(&args[2]), input_path, max_ticks, trace_mode)
        }
        "dump-ast" if args.len() == 3 => cmd_dump_ast(Path::new(&args[2])),
        "compile-lisp" if args.len() == 4 => {
            cmd_compile_lisp(Path::new(&args[2]), Path::new(&args[3]))
        }
        "run-lisp" if (3..=6).contains(&args.len()) => {
            let (input_path, max_ticks, trace_mode) = match parse_run_args(&args, 3) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("error: {err}");
                    std::process::exit(1);
                }
            };
            cmd_run_lisp(Path::new(&args[2]), input_path, max_ticks, trace_mode)
        }
        _ => {
            print_usage();
            return;
        }
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
