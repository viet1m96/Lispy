use std::collections::BTreeMap;

use crate::asm::{AsmProgram, AsmSection};
use crate::image::DEFAULT_MEMORY_LAYOUT;
use crate::isa::{Instruction, Reg};
use crate::lisp::{parse_program, Defun, Program, TopForm};
use crate::runtime::{emit_runtime, DEFAULT_INPUT_HANDLER_LABEL};
use crate::typecheck::typecheck_program;

pub(super) use crate::asm::{DataItem, Expr};
pub(super) use crate::isa::{AluRKind, BranchKind, VReg, VectorRKind};
pub(super) use crate::lisp::{Binding, Callee, Expr as LExpr, Param, TypeName};
pub(super) use crate::runtime::{
    load_mmio_base, load_u32, mov, HEAP_PTR_LABEL, PRINT_INT_LABEL, PRINT_PSTR_LABEL,
    PRINT_VALUE_LABEL, READ_CHAR_LABEL, READ_LINE_LABEL,
};

mod analysis;
mod builtins;
mod context;
mod emit;
mod expr;
mod i64;
mod trap_io;

use analysis::*;
use context::*;

const VECTOR_WIDTH_BYTES: i32 = 16;

pub fn compile_source(source: &str) -> Result<AsmProgram, String> {
    let ast = parse_program(source)?;
    compile_program(&ast)
}

pub fn compile_program(ast: &Program) -> Result<AsmProgram, String> {
    typecheck_program(ast)?;
    let mut compiler = Compiler::new();
    compiler.collect_function_signatures(ast)?;
    compiler.emit_trap_vector_table();
    compiler.compile_top_level(ast)?;
    compiler.compile_functions(ast)?;
    Ok(compiler.finish())
}

struct Compiler {
    program: AsmProgram,
    global_vars: BTreeMap<String, VarInfo>,
    function_sigs: BTreeMap<String, FunctionSig>,
    string_labels: BTreeMap<String, String>,
    next_label_id: usize,
    next_global_id: usize,
    next_string_id: usize,
    needs_print_int: bool,
    needs_print_pstr: bool,
    needs_print_value: bool,
    needs_read_line: bool,
    needs_read_char: bool,
    has_custom_input_handler: bool,
    needs_array_heap: bool,
}

impl Compiler {
    fn new() -> Self {
        let mut program = AsmProgram::new();
        program.set_entry_label("_start");
        Self {
            program,
            global_vars: BTreeMap::new(),
            function_sigs: BTreeMap::new(),
            string_labels: BTreeMap::new(),
            next_label_id: 0,
            next_global_id: 0,
            next_string_id: 0,
            needs_print_int: false,
            needs_print_pstr: false,
            needs_print_value: false,
            needs_read_line: false,
            needs_read_char: false,
            has_custom_input_handler: false,
            needs_array_heap: false,
        }
    }

    fn finish(mut self) -> AsmProgram {
        if self.needs_array_heap || self.needs_read_line {
            self.program.label(AsmSection::Data, HEAP_PTR_LABEL);
            self.program.emit_data(
                AsmSection::Data,
                DataItem::Word(DEFAULT_MEMORY_LAYOUT.heap_base),
            );
        }
        emit_runtime(
            &mut self.program,
            self.needs_print_int,
            self.needs_print_pstr,
            self.needs_print_value,
            self.needs_read_line,
            self.needs_read_char,
            !self.has_custom_input_handler,
        );
        self.program
    }

    fn collect_function_signatures(&mut self, ast: &Program) -> Result<(), String> {
        for form in &ast.forms {
            if let TopForm::Defun(defun) = form {
                if self.function_sigs.contains_key(&defun.name) {
                    return Err(format!("duplicate function definition: {}", defun.name));
                }
                if defun.name == DEFAULT_INPUT_HANDLER_LABEL {
                    self.has_custom_input_handler = true;
                }

                let param_kinds = defun
                    .params
                    .iter()
                    .map(|param| ValueKind::from_type_name(param.type_ann))
                    .collect::<Vec<_>>();

                let mut param_offsets = Vec::new();
                let mut param_word_count = 0usize;
                for kind in &param_kinds {
                    param_offsets.push((param_word_count as i32) * 4);
                    param_word_count += kind.width_words();
                }
                if param_word_count > 8 {
                    return Err(format!(
                        "function '{}' uses {} argument words, but this ABI supports at most 8 a-register words",
                        defun.name,
                        param_word_count
                    ));
                }

                let return_kind = ValueKind::from_type_name(defun.return_type);

                self.function_sigs.insert(
                    defun.name.clone(),
                    FunctionSig {
                        label: if defun.name == DEFAULT_INPUT_HANDLER_LABEL {
                            DEFAULT_INPUT_HANDLER_LABEL.to_string()
                        } else {
                            format!("fn_{}", sanitize(&defun.name))
                        },
                        param_kinds,
                        param_offsets,
                        param_word_count,
                        return_kind,
                    },
                );
            }
        }
        Ok(())
    }

    fn compile_top_level(&mut self, ast: &Program) -> Result<(), String> {
        self.program.label(AsmSection::Text, "_start");

        let top_level_slot_count: usize = ast
            .forms
            .iter()
            .filter_map(|form| match form {
                TopForm::Expr(expr) => Some(count_let_slots_in_expr(expr)),
                TopForm::Defun(_) => None,
            })
            .sum();
        if top_level_slot_count > 0 {
            let frame_bytes = (top_level_slot_count as i32) * 4;
            self.emit_prologue(frame_bytes);
        }

        let mut env = Env::top();

        for form in &ast.forms {
            if let TopForm::Expr(expr) = form {
                self.compile_expr(expr, Reg::A0, &mut env)?;
            }
        }

        let ends_with_halt = ast
            .forms
            .iter()
            .rev()
            .find_map(|form| match form {
                TopForm::Expr(expr) => Some(expr_guarantees_halt(expr)),
                TopForm::Defun(_) => None,
            })
            .unwrap_or(false);

        if !ends_with_halt {
            self.program.emit_inst(AsmSection::Text, Instruction::Halt);
        }
        Ok(())
    }

    fn compile_functions(&mut self, ast: &Program) -> Result<(), String> {
        for form in &ast.forms {
            if let TopForm::Defun(defun) = form {
                self.compile_defun(defun)?;
            }
        }
        Ok(())
    }

    fn compile_defun(&mut self, defun: &Defun) -> Result<(), String> {
        let sig = self
            .function_sigs
            .get(&defun.name)
            .ok_or_else(|| format!("missing signature for function {}", defun.name))?
            .clone();

        let local_slot_count = sig.param_word_count + count_let_slots_in_body(&defun.body);
        let frame_bytes = (local_slot_count as i32) * 4;

        self.program.label(AsmSection::Text, &sig.label);
        if defun.name == DEFAULT_INPUT_HANDLER_LABEL {
            self.emit_interrupt_context_save();
        }
        self.emit_prologue(frame_bytes);

        let mut env = Env::function(&defun.params, &sig);

        let mut arg_word = 0usize;
        for (index, _param) in defun.params.iter().enumerate() {
            let kind = sig.param_kinds[index];
            let offset = sig.param_offsets[index];
            let lo = arg_reg(arg_word)?;
            self.emit_store_frame(lo, offset);
            arg_word += 1;
            if kind == ValueKind::I64 {
                let hi = arg_reg(arg_word)?;
                self.emit_store_frame(hi, offset + 4);
                arg_word += 1;
            }
        }

        for (index, expr) in defun.body.iter().enumerate() {
            let is_last = index + 1 == defun.body.len();
            if is_last && sig.return_kind == ValueKind::I64 {
                self.compile_i64_expr(expr, &mut env)?;
            } else {
                self.compile_expr(expr, Reg::A0, &mut env)?;
            }
        }

        if defun.name == DEFAULT_INPUT_HANDLER_LABEL {
            self.emit_epilogue_without_return(frame_bytes);
            self.emit_interrupt_context_restore();
            self.program.emit_inst(AsmSection::Text, Instruction::Mret);
        } else {
            self.emit_epilogue(frame_bytes);
        }
        Ok(())
    }
}
