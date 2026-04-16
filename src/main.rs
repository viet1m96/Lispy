mod asm;
mod branch;
mod compiler;
mod control;
mod datapath;
mod exec;
mod image;
mod isa;
mod lisp;
mod machine;
mod memory;
mod runtime;
mod trace;
mod trap;
mod vector;

use std::env;
use std::path::Path;

use asm::AsmProgram;
use compiler::compile_source;
use exec::run_to_halt;
use image::ProgramImage;
use lisp::parse_program;
use machine::Machine;

fn print_usage() {
    println!("lab4-rust");
    println!("  dump-image <input.bin>               print image summary and listing");
    println!("  sim-image <input.bin> [max_ticks]    run scalar tick engine and print trace");
    println!("  dump-ast <input.lisp>                parse Lisp source and print AST");
    println!("  compile-lisp <input.lisp> <out.bin>  compile Lisp source to binary image");
    println!("  run-lisp <input.lisp> [max_ticks]    compile Lisp source and simulate it");
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

fn cmd_sim_image(path: &Path, max_ticks: u64) -> Result<(), String> {
    let image = ProgramImage::read_from_file(path).map_err(|e| e.to_string())?;
    run_image(&image, max_ticks)
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

fn cmd_run_lisp(input: &Path, max_ticks: u64) -> Result<(), String> {
    let source = std::fs::read_to_string(input).map_err(|e| e.to_string())?;
    let program = compile_source(&source)?;
    let assembled = program.assemble()?;
    let image = ProgramImage::from_assembled(&assembled);
    run_image(&image, max_ticks)
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

fn run_image(image: &ProgramImage, max_ticks: u64) -> Result<(), String> {
    let mut machine = Machine::from_image(image)?;
    let trace = run_to_halt(&mut machine, max_ticks)?;
    println!("halted : {}", machine.halted);
    println!("reason : {}", machine.halt_reason.as_deref().unwrap_or("-"));
    println!("ticks  : {}", machine.tick);
    println!("pc     : 0x{:08x}", machine.pc);
    println!("phase  : {}", machine.phase.name());
    println!("output : {:?}", machine.output_as_string());
    println!();
    println!("[trace]");
    print!("{}", trace.render());
    Ok(())
}

fn parse_max_ticks(args: &[String], index: usize) -> Result<u64, String> {
    if let Some(value) = args.get(index) {
        value
            .parse::<u64>()
            .map_err(|_| format!("invalid max_ticks: {value}"))
    } else {
        Ok(1_000)
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        return;
    }

    let result = match args[1].as_str() {
        "dump-image" if args.len() == 3 => cmd_dump_image(Path::new(&args[2])),
        "sim-image" if args.len() == 3 || args.len() == 4 => {
            let max_ticks = match parse_max_ticks(&args, 3) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("error: {err}");
                    std::process::exit(1);
                }
            };
            cmd_sim_image(Path::new(&args[2]), max_ticks)
        }
        "dump-ast" if args.len() == 3 => cmd_dump_ast(Path::new(&args[2])),
        "compile-lisp" if args.len() == 4 => {
            cmd_compile_lisp(Path::new(&args[2]), Path::new(&args[3]))
        }
        "run-lisp" if args.len() == 3 || args.len() == 4 => {
            let max_ticks = match parse_max_ticks(&args, 3) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("error: {err}");
                    std::process::exit(1);
                }
            };
            cmd_run_lisp(Path::new(&args[2]), max_ticks)
        }
        _ => {
            print_usage();
            Ok(())
        }
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
