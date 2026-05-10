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
    READ_CHAR_LABEL, READ_LINE_LABEL,
};

#[path = "emit.rs"]
mod emit;

const VECTOR_WIDTH_BYTES: i32 = 16;

// -----------------------------------------------------------------------------
// Small compiler data model: value kinds, function ABI metadata, and scopes.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueKind {
    Int,
    I64,
    String,
    Array,
}

impl ValueKind {
    pub(super) fn from_type_name<T: std::borrow::Borrow<TypeName>>(ty: T) -> Self {
        match *ty.borrow() {
            TypeName::Int => Self::Int,
            TypeName::I64 => Self::I64,
            TypeName::Bool => Self::Int,
            TypeName::String => Self::String,
            TypeName::Array => Self::Array,
        }
    }

    pub(super) fn width_words(self) -> usize {
        match self {
            Self::I64 => 2,
            Self::Int | Self::String | Self::Array => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct FunctionSig {
    pub(super) label: String,
    pub(super) param_kinds: Vec<ValueKind>,
    pub(super) param_offsets: Vec<i32>,
    pub(super) param_word_count: usize,
    pub(super) return_kind: ValueKind,
}

impl FunctionSig {
    pub(super) fn param_count(&self) -> usize {
        self.param_kinds.len()
    }
}

#[derive(Debug, Clone)]
pub(super) enum VarLoc {
    Global(String),
    Frame(i32),
}

#[derive(Debug, Clone)]
pub(super) struct VarInfo {
    pub(super) loc: VarLoc,
    pub(super) kind: ValueKind,
}

#[derive(Debug, Clone)]
pub(super) struct Env {
    scopes: Vec<BTreeMap<String, VarInfo>>,
    pub(super) next_local_slot: usize,
}

impl Env {
    pub(super) fn top() -> Self {
        Self {
            scopes: vec![BTreeMap::new()],
            next_local_slot: 0,
        }
    }

    pub(super) fn function(params: &[Param], sig: &FunctionSig) -> Self {
        let mut root = BTreeMap::new();
        for (index, param) in params.iter().enumerate() {
            let kind = sig.param_kinds[index];
            root.insert(
                param.name.clone(),
                VarInfo {
                    loc: VarLoc::Frame(sig.param_offsets[index]),
                    kind,
                },
            );
        }
        Self {
            scopes: vec![root],
            next_local_slot: sig.param_word_count,
        }
    }

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub(super) fn insert_current(&mut self, name: String, info: VarInfo) {
        self.scopes
            .last_mut()
            .expect("scope stack is never empty")
            .insert(name, info);
    }

    pub(super) fn lookup(&self, name: &str) -> Option<VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.clone());
            }
        }
        None
    }

    pub(super) fn alloc_frame_slots(&mut self, width_words: usize) -> i32 {
        let offset = (self.next_local_slot as i32) * 4;
        self.next_local_slot += width_words.max(1);
        offset
    }
}

#[derive(Clone, Copy)]
pub(super) enum CompareKind {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

pub(super) fn arg_reg(index: usize) -> Result<Reg, String> {
    match index {
        0 => Ok(Reg::A0),
        1 => Ok(Reg::A1),
        2 => Ok(Reg::A2),
        3 => Ok(Reg::A3),
        4 => Ok(Reg::A4),
        5 => Ok(Reg::A5),
        6 => Ok(Reg::A6),
        7 => Ok(Reg::A7),
        _ => Err(format!("argument register a{index} is not available")),
    }
}

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
        self.program.emit_inst(AsmSection::Text, Instruction::Halt);
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

// -----------------------------------------------------------------------------
// Analysis helpers
// -----------------------------------------------------------------------------

pub(super) fn count_let_slots_in_body(body: &[LExpr]) -> usize {
    body.iter().map(count_let_slots_in_expr).sum()
}

pub(super) fn count_let_slots_in_expr(expr: &LExpr) -> usize {
    match expr {
        LExpr::Number(_)
        | LExpr::String(_)
        | LExpr::Bool(_)
        | LExpr::Nil
        | LExpr::Ident(_)
        | LExpr::ReadChar
        | LExpr::ReadLine
        | LExpr::ReadInputData
        | LExpr::HandlerDone
        | LExpr::Halt => 0,
        LExpr::Cast { value, .. } => count_let_slots_in_expr(value),
        LExpr::Setq { value, .. } => count_let_slots_in_expr(value),
        LExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            count_let_slots_in_expr(cond)
                + count_let_slots_in_expr(then_branch)
                + count_let_slots_in_expr(else_branch)
        }
        LExpr::Begin(items) => items.iter().map(count_let_slots_in_expr).sum(),
        LExpr::Let { bindings, body } => {
            bindings
                .iter()
                .map(|binding| {
                    ValueKind::from_type_name(binding.type_ann).width_words()
                        + count_let_slots_in_expr(&binding.value)
                })
                .sum::<usize>()
                + body.iter().map(count_let_slots_in_expr).sum::<usize>()
        }
        LExpr::Loop {
            cond,
            body,
            finally,
        } => {
            count_let_slots_in_expr(cond)
                + body.iter().map(count_let_slots_in_expr).sum::<usize>()
                + count_let_slots_in_expr(finally)
        }
        LExpr::Print(value) | LExpr::PrintStr(value) => count_let_slots_in_expr(value),
        LExpr::Call { args, .. } => args.iter().map(count_let_slots_in_expr).sum(),
    }
}

impl Compiler {
    pub(super) fn expr_kind(&self, expr: &LExpr, env: &Env) -> Result<ValueKind, String> {
        match expr {
            LExpr::Number(_)
            | LExpr::Bool(_)
            | LExpr::Nil
            | LExpr::ReadChar
            | LExpr::ReadInputData
            | LExpr::HandlerDone
            | LExpr::Halt => Ok(ValueKind::Int),
            LExpr::String(_) | LExpr::ReadLine => Ok(ValueKind::String),
            LExpr::Ident(name) => self
                .lookup_var(name, env)
                .map(|info| info.kind)
                .ok_or_else(|| format!("unknown variable: {name}")),
            LExpr::Cast { target_type, .. } => Ok(ValueKind::from_type_name(*target_type)),
            LExpr::Setq { type_ann, .. } => Ok(ValueKind::from_type_name(*type_ann)),
            LExpr::If {
                then_branch,
                else_branch,
                ..
            } => {
                let left = self.expr_kind(then_branch, env)?;
                let right = self.expr_kind(else_branch, env)?;
                if left == right {
                    Ok(left)
                } else if matches!(
                    (left, right),
                    (ValueKind::I64, ValueKind::Int) | (ValueKind::Int, ValueKind::I64)
                ) {
                    Ok(ValueKind::I64)
                } else {
                    Err(format!(
                        "cannot infer common value kind for if branches: {:?} and {:?}",
                        left, right
                    ))
                }
            }
            LExpr::Begin(items) => items
                .last()
                .ok_or_else(|| "begin expression must contain at least one item".to_string())
                .and_then(|expr| self.expr_kind(expr, env)),
            LExpr::Let { bindings, body } => {
                let mut scoped = env.clone();
                scoped.push_scope();
                for binding in bindings {
                    let kind = ValueKind::from_type_name(binding.type_ann);
                    let offset = scoped.alloc_frame_slots(kind.width_words());
                    scoped.insert_current(
                        binding.name.clone(),
                        VarInfo {
                            loc: VarLoc::Frame(offset),
                            kind,
                        },
                    );
                }
                body.last()
                    .ok_or_else(|| "let expression must contain at least one body item".to_string())
                    .and_then(|expr| self.expr_kind(expr, &scoped))
            }
            LExpr::Loop { finally, .. } => self.expr_kind(finally, env),
            LExpr::Print(value) => self.expr_kind(value, env),
            LExpr::PrintStr(_) => Ok(ValueKind::String),
            LExpr::Call { callee, args } => match callee {
                Callee::Builtin(name) => match name.as_str() {
                    "array-get" | "array-set" | "array-size" | "=" | "!=" | "<" | "<=" | ">"
                    | ">=" | "and" | "or" | "not" => Ok(ValueKind::Int),
                    "array" | "vadd" | "vsub" | "vmul" | "vdiv" | "vcmp" => Ok(ValueKind::Array),
                    "+" | "-" | "*" | "/" | "%" | "bit-and" | "bit-or" | "bit-xor" | "shl"
                    | "shr" | "sar" => {
                        let mut saw_i64 = false;
                        for arg in args {
                            if self.expr_kind(arg, env)? == ValueKind::I64 {
                                saw_i64 = true;
                            }
                        }
                        Ok(if saw_i64 {
                            ValueKind::I64
                        } else {
                            ValueKind::Int
                        })
                    }
                    other => Err(format!("unknown builtin '{other}'")),
                },
                Callee::Ident(name) => self
                    .function_sigs
                    .get(name)
                    .map(|sig| sig.return_kind)
                    .ok_or_else(|| format!("unknown function: {name}")),
            },
        }
    }
}
pub(super) fn sanitize(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}
// -----------------------------------------------------------------------------
// Expression lowering
// -----------------------------------------------------------------------------

impl Compiler {
    pub(super) fn compile_expr(
        &mut self,
        expr: &LExpr,
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        match expr {
            LExpr::Number(value) => {
                let value = i32::try_from(*value).map_err(|_| {
                    "32-bit number literal out of range; cast it as (as-i64 <number>) when a 64-bit value is expected".to_string()
                })?;
                self.emit_load_imm(target, value)
            }
            LExpr::Cast { target_type, value } => match target_type {
                TypeName::I64 => {
                    self.compile_i64_expr(value, env)?;
                    if target != Reg::A0 {
                        mov(&mut self.program, target, Reg::A0);
                    }
                }
                TypeName::Int => {
                    if self.expr_kind(value, env)? == ValueKind::I64 {
                        self.compile_i64_expr(value, env)?;
                        if target != Reg::A0 {
                            mov(&mut self.program, target, Reg::A0);
                        }
                    } else {
                        self.compile_expr(value, target, env)?;
                    }
                }
                TypeName::Bool | TypeName::String | TypeName::Array => {
                    return Err("unsupported cast target reached code generator".to_string());
                }
            },
            LExpr::Bool(value) => self.emit_load_imm(target, if *value { 1 } else { 0 }),
            LExpr::Nil => self.emit_load_imm(target, 0),
            LExpr::String(text) => {
                let label = self.intern_string(text);
                self.emit_load_addr(target, &label);
            }
            LExpr::Ident(name) => {
                let info = self
                    .lookup_var(name, env)
                    .ok_or_else(|| format!("unknown variable: {name}"))?;
                if info.kind == ValueKind::I64 {
                    self.emit_load_from_info_pair(target, &info);
                } else {
                    self.emit_load_from_loc(target, &info.loc);
                }
            }
            LExpr::Setq {
                name,
                type_ann,
                value,
            } => {
                let declared_kind = ValueKind::from_type_name(*type_ann);

                let info = if let Some(existing) = self.lookup_var(name, env) {
                    if existing.kind != declared_kind {
                        return Err(format!(
                            "cannot redeclare variable '{name}' as {:?}; existing kind is {:?}",
                            declared_kind, existing.kind
                        ));
                    }
                    existing
                } else {
                    self.define_global(name, declared_kind)
                };

                self.compile_store_value(value, target, env, &info)?;
            }
            LExpr::Begin(items) => {
                for item in items {
                    self.compile_expr(item, target, env)?;
                }
            }
            LExpr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let label_else = self.next_label("if_else");
                let label_end = self.next_label("if_end");
                self.compile_expr(cond, target, env)?;
                self.emit_branch(BranchKind::Beq, target, Reg::Zero, &label_else);
                self.compile_expr(then_branch, target, env)?;
                self.emit_jump(&label_end);
                self.program.label(AsmSection::Text, &label_else);
                self.compile_expr(else_branch, target, env)?;
                self.program.label(AsmSection::Text, &label_end);
            }
            LExpr::Let { bindings, body } => self.compile_let(bindings, body, target, env)?,
            LExpr::Loop {
                cond,
                body,
                finally,
            } => {
                let label_loop = self.next_label("loop_begin");
                let label_exit = self.next_label("loop_exit");
                self.program.label(AsmSection::Text, &label_loop);
                self.compile_expr(cond, target, env)?;
                self.emit_branch(BranchKind::Beq, target, Reg::Zero, &label_exit);
                for item in body {
                    self.compile_expr(item, target, env)?;
                }
                self.emit_jump(&label_loop);
                self.program.label(AsmSection::Text, &label_exit);
                self.compile_expr(finally, target, env)?;
            }
            LExpr::Print(value) => {
                let kind = self.expr_kind(value, env)?;
                if kind == ValueKind::I64 {
                    self.compile_i64_expr(value, env)?;
                    self.emit_print_dynamic_i64(target);
                    return Ok(());
                }

                let label = match kind {
                    ValueKind::Int | ValueKind::Array => {
                        self.needs_print_int = true;
                        PRINT_INT_LABEL
                    }
                    ValueKind::String => {
                        self.needs_print_pstr = true;
                        PRINT_PSTR_LABEL
                    }
                    ValueKind::I64 => unreachable!("i64 print handled above"),
                };
                self.compile_expr(value, Reg::A0, env)?;
                self.program.emit_inst(
                    AsmSection::Text,
                    Instruction::Jal {
                        rd: Reg::Ra,
                        off: Expr::pcrel(label),
                    },
                );
                if target != Reg::A0 {
                    mov(&mut self.program, target, Reg::A0);
                }
            }
            LExpr::PrintStr(value) => {
                self.compile_expr(value, Reg::A0, env)?;
                self.needs_print_pstr = true;
                self.program.emit_inst(
                    AsmSection::Text,
                    Instruction::Jal {
                        rd: Reg::Ra,
                        off: Expr::pcrel(PRINT_PSTR_LABEL),
                    },
                );
                if target != Reg::A0 {
                    mov(&mut self.program, target, Reg::A0);
                }
            }
            LExpr::ReadChar => {
                self.needs_read_char = true;
                self.program.emit_inst(
                    AsmSection::Text,
                    Instruction::Jal {
                        rd: Reg::Ra,
                        off: Expr::pcrel(READ_CHAR_LABEL),
                    },
                );
                if target != Reg::A0 {
                    mov(&mut self.program, target, Reg::A0);
                }
            }
            LExpr::ReadLine => {
                self.needs_read_line = true;
                self.program.emit_inst(
                    AsmSection::Text,
                    Instruction::Jal {
                        rd: Reg::Ra,
                        off: Expr::pcrel(READ_LINE_LABEL),
                    },
                );
                if target != Reg::A0 {
                    mov(&mut self.program, target, Reg::A0);
                }
            }
            LExpr::ReadInputData => self.compile_read_input_data(target)?,
            LExpr::HandlerDone => self.compile_handler_done(target)?,
            LExpr::Halt => {
                self.program.emit_inst(AsmSection::Text, Instruction::Halt);
            }
            LExpr::Call { callee, args } => match callee {
                Callee::Builtin(name) => self.compile_builtin(name, args, target, env)?,
                Callee::Ident(name) => self.compile_user_call(name, args, target, env)?,
            },
        }
        Ok(())
    }

    pub(super) fn compile_let(
        &mut self,
        bindings: &[Binding],
        body: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        let mut slots = Vec::new();
        for binding in bindings {
            let kind = ValueKind::from_type_name(binding.type_ann);
            let offset = env.alloc_frame_slots(kind.width_words());
            slots.push((binding.name.clone(), offset, kind, binding.value.clone()));
        }

        for (_name, offset, kind, value) in &slots {
            let info = VarInfo {
                loc: VarLoc::Frame(*offset),
                kind: *kind,
            };
            self.compile_store_value(value, target, env, &info)?;
        }

        env.push_scope();
        for (name, offset, kind, _) in &slots {
            env.insert_current(
                name.clone(),
                VarInfo {
                    loc: VarLoc::Frame(*offset),
                    kind: *kind,
                },
            );
        }
        for expr in body {
            self.compile_expr(expr, target, env)?;
        }
        env.pop_scope();
        Ok(())
    }

    pub(super) fn compile_user_call(
        &mut self,
        name: &str,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        let sig = self
            .function_sigs
            .get(name)
            .ok_or_else(|| format!("unknown function: {name}"))?
            .clone();

        if args.len() != sig.param_count() {
            return Err(format!(
                "function '{}' expects {} arguments, got {}",
                name,
                sig.param_count(),
                args.len()
            ));
        }
        if sig.param_word_count > 8 {
            return Err(format!(
                "function '{}' call uses {} argument words, but this ABI supports at most 8",
                name, sig.param_word_count
            ));
        }

        for (arg, kind) in args.iter().zip(sig.param_kinds.iter()) {
            match kind {
                ValueKind::I64 => {
                    self.compile_i64_expr(arg, env)?;
                    self.push_reg(Reg::A0);
                    self.push_reg(Reg::A1);
                }
                _ => {
                    self.compile_expr(arg, Reg::T0, env)?;
                    self.push_reg(Reg::T0);
                }
            }
        }

        let mut arg_word = sig.param_word_count;
        for kind in sig.param_kinds.iter().rev() {
            match kind {
                ValueKind::I64 => {
                    arg_word -= 1;
                    self.pop_reg(arg_reg(arg_word)?);
                    arg_word -= 1;
                    self.pop_reg(arg_reg(arg_word)?);
                }
                _ => {
                    arg_word -= 1;
                    self.pop_reg(arg_reg(arg_word)?);
                }
            }
        }

        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Jal {
                rd: Reg::Ra,
                off: Expr::pcrel(&sig.label),
            },
        );

        if target != Reg::A0 {
            mov(&mut self.program, target, Reg::A0);
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Builtin lowering
// -----------------------------------------------------------------------------

impl Compiler {
    pub(super) fn compile_builtin(
        &mut self,
        name: &str,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        match name {
            "+" => self.compile_variadic_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Add, dst, lhs, rhs);
                Ok(())
            }),
            "-" => {
                if args.len() == 1 {
                    self.compile_expr(&args[0], Reg::T1, env)?;
                    self.emit_r(AluRKind::Sub, target, Reg::Zero, Reg::T1);
                    return Ok(());
                }
                self.compile_variadic_binary(args, target, env, |this, dst, lhs, rhs| {
                    this.emit_r(AluRKind::Sub, dst, lhs, rhs);
                    Ok(())
                })
            }
            "*" => self.compile_variadic_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Mul, dst, lhs, rhs);
                Ok(())
            }),
            "/" => self.compile_exact_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Div, dst, lhs, rhs);
                Ok(())
            }),
            "%" => self.compile_exact_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Rem, dst, lhs, rhs);
                Ok(())
            }),
            "bit-and" => self.compile_variadic_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::And, dst, lhs, rhs);
                Ok(())
            }),
            "bit-or" => self.compile_variadic_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Or, dst, lhs, rhs);
                Ok(())
            }),
            "bit-xor" => self.compile_variadic_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Xor, dst, lhs, rhs);
                Ok(())
            }),
            "shl" => self.compile_exact_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Sll, dst, lhs, rhs);
                Ok(())
            }),
            "shr" => self.compile_exact_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Srl, dst, lhs, rhs);
                Ok(())
            }),
            "sar" => self.compile_exact_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Sra, dst, lhs, rhs);
                Ok(())
            }),
            "=" => self.compile_compare(args, target, env, CompareKind::Eq),
            "!=" => self.compile_compare(args, target, env, CompareKind::Ne),
            "<" => self.compile_compare(args, target, env, CompareKind::Lt),
            "<=" => self.compile_compare(args, target, env, CompareKind::Le),
            ">" => self.compile_compare(args, target, env, CompareKind::Gt),
            ">=" => self.compile_compare(args, target, env, CompareKind::Ge),
            "and" => self.compile_and(args, target, env),
            "or" => self.compile_or(args, target, env),
            "not" => self.compile_not(args, target, env),
            "array" => self.compile_array_new(args, target, env),
            "array-get" => self.compile_array_get(args, target, env),
            "array-set" => self.compile_array_set(args, target, env),
            "array-size" => self.compile_array_size(args, target, env),
            "vadd" => self.compile_array_vector_op(VectorRKind::Vadd, args, target, env),
            "vsub" => self.compile_array_vector_op(VectorRKind::Vsub, args, target, env),
            "vmul" => self.compile_array_vector_op(VectorRKind::Vmul, args, target, env),
            "vdiv" => self.compile_array_vector_op(VectorRKind::Vdiv, args, target, env),
            "vcmp" => self.compile_array_vector_op(VectorRKind::Vcmpeq, args, target, env),
            other => Err(format!("unknown builtin '{other}' reached code generator")),
        }
    }

    pub(super) fn compile_exact_binary<F>(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
        mut emit: F,
    ) -> Result<(), String>
    where
        F: FnMut(&mut Compiler, Reg, Reg, Reg) -> Result<(), String>,
    {
        if args.len() != 2 {
            return Err(format!(
                "builtin expects exactly 2 arguments, got {}",
                args.len()
            ));
        }

        self.compile_expr(&args[0], Reg::T0, env)?;
        self.push_reg(Reg::T0);
        self.compile_expr(&args[1], Reg::T1, env)?;
        self.pop_reg(Reg::T0);
        emit(self, target, Reg::T0, Reg::T1)
    }

    pub(super) fn compile_variadic_binary<F>(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
        mut emit: F,
    ) -> Result<(), String>
    where
        F: FnMut(&mut Compiler, Reg, Reg, Reg) -> Result<(), String>,
    {
        if args.is_empty() {
            return Err("builtin expects at least 1 argument".to_string());
        }

        self.compile_expr(&args[0], target, env)?;
        for arg in &args[1..] {
            self.push_reg(target);
            self.compile_expr(arg, Reg::T1, env)?;
            self.pop_reg(Reg::T0);
            emit(self, target, Reg::T0, Reg::T1)?;
        }
        Ok(())
    }

    pub(super) fn compile_compare(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
        kind: CompareKind,
    ) -> Result<(), String> {
        if args.len() != 2 {
            return Err(format!(
                "comparison expects exactly 2 arguments, got {}",
                args.len()
            ));
        }

        let lhs_kind = self.expr_kind(&args[0], env)?;
        let rhs_kind = self.expr_kind(&args[1], env)?;
        if lhs_kind == ValueKind::I64 || rhs_kind == ValueKind::I64 {
            return self.compile_compare_i64(args, target, env, kind);
        }

        self.compile_expr(&args[0], Reg::T0, env)?;
        self.push_reg(Reg::T0);
        self.compile_expr(&args[1], Reg::T1, env)?;
        self.pop_reg(Reg::T0);

        let label_true = self.next_label("cmp_true");
        let label_false = self.next_label("cmp_false");
        let label_end = self.next_label("cmp_end");

        match kind {
            CompareKind::Eq => self.emit_branch(BranchKind::Beq, Reg::T0, Reg::T1, &label_true),
            CompareKind::Ne => self.emit_branch(BranchKind::Bne, Reg::T0, Reg::T1, &label_true),
            CompareKind::Lt => self.emit_branch(BranchKind::Blt, Reg::T0, Reg::T1, &label_true),
            CompareKind::Le => self.emit_branch(BranchKind::Bge, Reg::T1, Reg::T0, &label_true),
            CompareKind::Gt => self.emit_branch(BranchKind::Blt, Reg::T1, Reg::T0, &label_true),
            CompareKind::Ge => self.emit_branch(BranchKind::Bge, Reg::T0, Reg::T1, &label_true),
        }
        self.emit_jump(&label_false);
        self.program.label(AsmSection::Text, &label_true);
        self.emit_load_imm(target, 1);
        self.emit_jump(&label_end);
        self.program.label(AsmSection::Text, &label_false);
        self.emit_load_imm(target, 0);
        self.program.label(AsmSection::Text, &label_end);
        Ok(())
    }

    pub(super) fn compile_compare_i64(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
        kind: CompareKind,
    ) -> Result<(), String> {
        self.compile_i64_expr(&args[0], env)?;
        self.push_reg(Reg::A0);
        self.push_reg(Reg::A1);
        self.compile_i64_expr(&args[1], env)?;
        self.pop_reg(Reg::T1);
        self.pop_reg(Reg::T0);

        let label_true = self.next_label("cmp64_true");
        let label_false = self.next_label("cmp64_false");
        let label_end = self.next_label("cmp64_end");

        match kind {
            CompareKind::Eq => {
                self.emit_r(AluRKind::Xor, Reg::T2, Reg::T0, Reg::A0);
                self.emit_r(AluRKind::Xor, Reg::T3, Reg::T1, Reg::A1);
                self.emit_r(AluRKind::Or, Reg::T2, Reg::T2, Reg::T3);
                self.emit_branch(BranchKind::Beq, Reg::T2, Reg::Zero, &label_true);
                self.emit_jump(&label_false);
            }
            CompareKind::Ne => {
                self.emit_r(AluRKind::Xor, Reg::T2, Reg::T0, Reg::A0);
                self.emit_r(AluRKind::Xor, Reg::T3, Reg::T1, Reg::A1);
                self.emit_r(AluRKind::Or, Reg::T2, Reg::T2, Reg::T3);
                self.emit_branch(BranchKind::Bne, Reg::T2, Reg::Zero, &label_true);
                self.emit_jump(&label_false);
            }
            CompareKind::Lt => self.emit_i64_less_than_branch(true, &label_true, &label_false),
            CompareKind::Le => self.emit_i64_less_than_branch(false, &label_false, &label_true),
            CompareKind::Gt => self.emit_i64_less_than_branch(false, &label_true, &label_false),
            CompareKind::Ge => self.emit_i64_less_than_branch(true, &label_false, &label_true),
        }

        self.program.label(AsmSection::Text, &label_true);
        self.emit_load_imm(target, 1);
        self.emit_jump(&label_end);
        self.program.label(AsmSection::Text, &label_false);
        self.emit_load_imm(target, 0);
        self.program.label(AsmSection::Text, &label_end);
        Ok(())
    }

    pub(super) fn emit_i64_less_than_branch(
        &mut self,
        lhs_is_left: bool,
        label_true: &str,
        label_false: &str,
    ) {
        if lhs_is_left {
            self.emit_branch(BranchKind::Blt, Reg::T1, Reg::A1, label_true);
            self.emit_branch(BranchKind::Blt, Reg::A1, Reg::T1, label_false);
            self.emit_r(AluRKind::Sltu, Reg::T2, Reg::T0, Reg::A0);
        } else {
            self.emit_branch(BranchKind::Blt, Reg::A1, Reg::T1, label_true);
            self.emit_branch(BranchKind::Blt, Reg::T1, Reg::A1, label_false);
            self.emit_r(AluRKind::Sltu, Reg::T2, Reg::A0, Reg::T0);
        }
        self.emit_branch(BranchKind::Bne, Reg::T2, Reg::Zero, label_true);
        self.emit_jump(label_false);
    }

    pub(super) fn compile_and(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        if args.is_empty() {
            self.emit_load_imm(target, 1);
            return Ok(());
        }

        let label_false = self.next_label("and_false");
        let label_end = self.next_label("and_end");
        for arg in args {
            self.compile_expr(arg, target, env)?;
            self.emit_branch(BranchKind::Beq, target, Reg::Zero, &label_false);
        }
        self.emit_load_imm(target, 1);
        self.emit_jump(&label_end);
        self.program.label(AsmSection::Text, &label_false);
        self.emit_load_imm(target, 0);
        self.program.label(AsmSection::Text, &label_end);
        Ok(())
    }

    pub(super) fn compile_or(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        if args.is_empty() {
            self.emit_load_imm(target, 0);
            return Ok(());
        }

        let label_true = self.next_label("or_true");
        let label_end = self.next_label("or_end");
        for arg in args {
            self.compile_expr(arg, target, env)?;
            self.emit_branch(BranchKind::Bne, target, Reg::Zero, &label_true);
        }
        self.emit_load_imm(target, 0);
        self.emit_jump(&label_end);
        self.program.label(AsmSection::Text, &label_true);
        self.emit_load_imm(target, 1);
        self.program.label(AsmSection::Text, &label_end);
        Ok(())
    }

    pub(super) fn compile_not(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        if args.len() != 1 {
            return Err(format!(
                "not expects exactly 1 argument, got {}",
                args.len()
            ));
        }

        let label_true = self.next_label("not_true");
        let label_end = self.next_label("not_end");
        self.compile_expr(&args[0], target, env)?;
        self.emit_branch(BranchKind::Beq, target, Reg::Zero, &label_true);
        self.emit_load_imm(target, 0);
        self.emit_jump(&label_end);
        self.program.label(AsmSection::Text, &label_true);
        self.emit_load_imm(target, 1);
        self.program.label(AsmSection::Text, &label_end);
        Ok(())
    }
    pub(super) fn compile_array_new(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        if args.len() != 1 {
            return Err(format!(
                "array expects exactly 1 size argument, got {}",
                args.len()
            ));
        }
        self.compile_expr(&args[0], Reg::T0, env)?;
        self.emit_array_alloc_from_size(Reg::T0, Reg::A0)?;
        if target != Reg::A0 {
            mov(&mut self.program, target, Reg::A0);
        }
        Ok(())
    }

    pub(super) fn compile_array_get(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        if args.len() != 2 {
            return Err(format!(
                "array-get expects exactly 2 arguments, got {}",
                args.len()
            ));
        }
        self.compile_expr(&args[0], Reg::T0, env)?;
        self.push_reg(Reg::T0);
        self.compile_expr(&args[1], Reg::T1, env)?;
        self.pop_reg(Reg::T0);
        self.emit_array_element_addr(Reg::T0, Reg::T1, Reg::T2);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: target,
                rs1: Reg::T2,
                off: Expr::from_i32(0),
            },
        );
        Ok(())
    }

    pub(super) fn compile_array_set(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        if args.len() != 3 {
            return Err(format!(
                "array-set expects exactly 3 arguments, got {}",
                args.len()
            ));
        }
        self.compile_expr(&args[0], Reg::T0, env)?;
        self.push_reg(Reg::T0);
        self.compile_expr(&args[1], Reg::T1, env)?;
        self.push_reg(Reg::T1);
        self.compile_expr(&args[2], Reg::T2, env)?;
        self.pop_reg(Reg::T1);
        self.pop_reg(Reg::T0);
        self.emit_array_element_addr(Reg::T0, Reg::T1, Reg::T3);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::T2,
                rs1: Reg::T3,
                off: Expr::from_i32(0),
            },
        );
        if target != Reg::T2 {
            mov(&mut self.program, target, Reg::T2);
        }
        Ok(())
    }

    pub(super) fn compile_array_size(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        if args.len() != 1 {
            return Err(format!(
                "array-size expects exactly 1 argument, got {}",
                args.len()
            ));
        }
        self.compile_expr(&args[0], target, env)?;
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: target,
                rs1: target,
                off: Expr::from_i32(0),
            },
        );
        Ok(())
    }

    pub(super) fn compile_array_vector_op(
        &mut self,
        op: VectorRKind,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        if args.len() != 3 {
            return Err(format!(
                "array vector op expects exactly 3 arguments: destination array, left array, right array; got {}",
                args.len()
            ));
        }

        self.compile_expr(&args[0], Reg::T5, env)?;
        self.push_reg(Reg::T5);
        self.compile_expr(&args[1], Reg::T0, env)?;
        self.push_reg(Reg::T0);
        self.compile_expr(&args[2], Reg::T1, env)?;
        self.pop_reg(Reg::T0);
        self.pop_reg(Reg::T5);

        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: Reg::T2,
                rs1: Reg::T0,
                off: Expr::from_i32(0),
            },
        );

        mov(&mut self.program, Reg::A0, Reg::T5);

        self.emit_load_imm(Reg::T4, 4);
        self.emit_r(AluRKind::Add, Reg::T0, Reg::T0, Reg::T4);
        self.emit_r(AluRKind::Add, Reg::T1, Reg::T1, Reg::T4);
        self.emit_r(AluRKind::Add, Reg::T5, Reg::T5, Reg::T4);

        self.emit_load_imm(Reg::T4, 2);
        self.emit_r(AluRKind::Srl, Reg::T3, Reg::T2, Reg::T4);
        self.emit_load_imm(Reg::T4, 3);
        self.emit_r(AluRKind::And, Reg::T6, Reg::T2, Reg::T4);

        let label_vec_loop = self.next_label("array_vec_loop");
        let label_scalar_loop = self.next_label("array_vec_scalar_loop");
        let label_scalar_true = self.next_label("array_vec_cmp_true");
        let label_scalar_store = self.next_label("array_vec_cmp_store");
        let label_done = self.next_label("array_vec_done");

        self.program.label(AsmSection::Text, &label_vec_loop);
        self.emit_branch(BranchKind::Beq, Reg::T3, Reg::Zero, &label_scalar_loop);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Vld {
                vd: VReg::new(0)?,
                rs1: Reg::T0,
                off: Expr::from_i32(0),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Vld {
                vd: VReg::new(1)?,
                rs1: Reg::T1,
                off: Expr::from_i32(0),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::VectorR {
                op,
                vd: VReg::new(2)?,
                vs1: VReg::new(0)?,
                vs2: VReg::new(1)?,
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Vst {
                vs: VReg::new(2)?,
                rs1: Reg::T5,
                off: Expr::from_i32(0),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T0,
                rs1: Reg::T0,
                imm: Expr::from_i32(VECTOR_WIDTH_BYTES),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T1,
                rs1: Reg::T1,
                imm: Expr::from_i32(VECTOR_WIDTH_BYTES),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T5,
                rs1: Reg::T5,
                imm: Expr::from_i32(VECTOR_WIDTH_BYTES),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T3,
                rs1: Reg::T3,
                imm: Expr::from_i32(-1),
            },
        );
        self.emit_jump(&label_vec_loop);

        self.program.label(AsmSection::Text, &label_scalar_loop);
        self.emit_branch(BranchKind::Beq, Reg::T6, Reg::Zero, &label_done);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: Reg::A1,
                rs1: Reg::T0,
                off: Expr::from_i32(0),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: Reg::A2,
                rs1: Reg::T1,
                off: Expr::from_i32(0),
            },
        );
        match op {
            VectorRKind::Vadd => self.emit_r(AluRKind::Add, Reg::A3, Reg::A1, Reg::A2),
            VectorRKind::Vsub => self.emit_r(AluRKind::Sub, Reg::A3, Reg::A1, Reg::A2),
            VectorRKind::Vmul => self.emit_r(AluRKind::Mul, Reg::A3, Reg::A1, Reg::A2),
            VectorRKind::Vdiv => self.emit_r(AluRKind::Div, Reg::A3, Reg::A1, Reg::A2),
            VectorRKind::Vcmpeq => {
                self.emit_branch(BranchKind::Beq, Reg::A1, Reg::A2, &label_scalar_true);
                self.emit_load_imm(Reg::A3, 0);
                self.emit_jump(&label_scalar_store);
                self.program.label(AsmSection::Text, &label_scalar_true);
                self.emit_load_imm(Reg::A3, 1);
                self.program.label(AsmSection::Text, &label_scalar_store);
            }
        }
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::A3,
                rs1: Reg::T5,
                off: Expr::from_i32(0),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T0,
                rs1: Reg::T0,
                imm: Expr::from_i32(4),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T1,
                rs1: Reg::T1,
                imm: Expr::from_i32(4),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T5,
                rs1: Reg::T5,
                imm: Expr::from_i32(4),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T6,
                rs1: Reg::T6,
                imm: Expr::from_i32(-1),
            },
        );
        self.emit_jump(&label_scalar_loop);

        self.program.label(AsmSection::Text, &label_done);
        if target != Reg::A0 {
            mov(&mut self.program, target, Reg::A0);
        }
        Ok(())
    }

    fn emit_array_alloc_from_size(&mut self, size_reg: Reg, out: Reg) -> Result<(), String> {
        self.needs_array_heap = true;
        if size_reg != Reg::T0 {
            mov(&mut self.program, Reg::T0, size_reg);
        }

        self.emit_load_addr(Reg::T6, HEAP_PTR_LABEL);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: out,
                rs1: Reg::T6,
                off: Expr::from_i32(0),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::T0,
                rs1: out,
                off: Expr::from_i32(0),
            },
        );

        self.emit_load_imm(Reg::T4, 2);
        self.emit_r(AluRKind::Sll, Reg::T3, Reg::T0, Reg::T4);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T3,
                rs1: Reg::T3,
                imm: Expr::from_i32(4),
            },
        );
        self.emit_r(AluRKind::Add, Reg::T5, out, Reg::T3);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::T5,
                rs1: Reg::T6,
                off: Expr::from_i32(0),
            },
        );

        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T1,
                rs1: out,
                imm: Expr::from_i32(4),
            },
        );
        self.emit_load_imm(Reg::T2, 0);
        let label_loop = self.next_label("array_zero_loop");
        let label_done = self.next_label("array_zero_done");
        self.program.label(AsmSection::Text, &label_loop);
        self.emit_branch(BranchKind::Beq, Reg::T2, Reg::T0, &label_done);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::Zero,
                rs1: Reg::T1,
                off: Expr::from_i32(0),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T1,
                rs1: Reg::T1,
                imm: Expr::from_i32(4),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::T2,
                rs1: Reg::T2,
                imm: Expr::from_i32(1),
            },
        );
        self.emit_jump(&label_loop);
        self.program.label(AsmSection::Text, &label_done);
        Ok(())
    }

    fn emit_array_element_addr(&mut self, array_reg: Reg, index_reg: Reg, out_reg: Reg) {
        self.emit_load_imm(Reg::T4, 2);
        self.emit_r(AluRKind::Sll, out_reg, index_reg, Reg::T4);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: out_reg,
                rs1: out_reg,
                imm: Expr::from_i32(4),
            },
        );
        self.emit_r(AluRKind::Add, out_reg, array_reg, out_reg);
    }
}

// -----------------------------------------------------------------------------
// i64 lowering
// -----------------------------------------------------------------------------

impl Compiler {
    pub(super) fn compile_i64_expr(&mut self, expr: &LExpr, env: &mut Env) -> Result<(), String> {
        match expr {
            LExpr::Cast { target_type, value } => match target_type {
                TypeName::I64 => {
                    if self.expr_kind(value, env)? == ValueKind::I64 {
                        self.compile_i64_expr(value, env)
                    } else {
                        self.compile_expr(value, Reg::A0, env)?;
                        self.emit_sign_extend_a0_to_a1();
                        Ok(())
                    }
                }
                TypeName::Int => {
                    self.compile_expr(value, Reg::A0, env)?;
                    self.emit_sign_extend_a0_to_a1();
                    Ok(())
                }
                TypeName::Bool | TypeName::String | TypeName::Array => {
                    Err("unsupported cast target in an i64 expression".to_string())
                }
            },
            LExpr::Number(value) => {
                self.emit_load_const_i64(*value, Reg::A0);
                Ok(())
            }
            LExpr::Bool(value) => {
                self.emit_load_const_i64(if *value { 1 } else { 0 }, Reg::A0);
                Ok(())
            }
            LExpr::Nil => {
                self.emit_load_const_i64(0, Reg::A0);
                Ok(())
            }
            LExpr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let label_else = self.next_label("i64_if_else");
                let label_end = self.next_label("i64_if_end");
                self.compile_expr(cond, Reg::T0, env)?;
                self.emit_branch(BranchKind::Beq, Reg::T0, Reg::Zero, &label_else);
                self.compile_i64_expr(then_branch, env)?;
                self.emit_jump(&label_end);
                self.program.label(AsmSection::Text, &label_else);
                self.compile_i64_expr(else_branch, env)?;
                self.program.label(AsmSection::Text, &label_end);
                Ok(())
            }
            LExpr::Begin(items) => {
                for (index, item) in items.iter().enumerate() {
                    if index + 1 == items.len() {
                        self.compile_i64_expr(item, env)?;
                    } else {
                        self.compile_expr(item, Reg::A0, env)?;
                    }
                }
                Ok(())
            }
            LExpr::Ident(name) => {
                let info = self
                    .lookup_var(name, env)
                    .ok_or_else(|| format!("unknown variable: {name}"))?;
                if info.kind == ValueKind::I64 {
                    self.emit_load_from_info_pair(Reg::A0, &info);
                } else {
                    self.emit_load_from_loc(Reg::A0, &info.loc);
                    self.emit_sign_extend_a0_to_a1();
                }
                Ok(())
            }
            LExpr::Setq {
                name,
                type_ann,
                value,
            } => {
                let declared_kind = ValueKind::from_type_name(*type_ann);
                let info = if let Some(existing) = self.lookup_var(name, env) {
                    if existing.kind != declared_kind {
                        return Err(format!(
                            "cannot redeclare variable '{name}' as {:?}; existing kind is {:?}",
                            declared_kind, existing.kind
                        ));
                    }
                    existing
                } else {
                    self.define_global(name, declared_kind)
                };
                self.compile_store_value(value, Reg::A0, env, &info)?;
                if info.kind == ValueKind::I64 {
                    self.emit_load_from_info_pair(Reg::A0, &info);
                } else {
                    self.emit_load_from_loc(Reg::A0, &info.loc);
                    self.emit_sign_extend_a0_to_a1();
                }
                Ok(())
            }
            LExpr::Print(value) => {
                self.compile_i64_expr(value, env)?;
                self.emit_print_dynamic_i64(Reg::A0);
                Ok(())
            }

            LExpr::Call { callee, args } => match callee {
                Callee::Ident(name) => {
                    let sig = self
                        .function_sigs
                        .get(name)
                        .ok_or_else(|| format!("unknown function: {name}"))?
                        .clone();
                    self.compile_user_call(name, args, Reg::A0, env)?;
                    if sig.return_kind == ValueKind::I64 {
                        Ok(())
                    } else {
                        self.emit_sign_extend_a0_to_a1();
                        Ok(())
                    }
                }
                Callee::Builtin(name) if name == "*" => self.compile_i64_mul(args, env),
                Callee::Builtin(name) if name == "+" => self.compile_i64_add(args, env),
                Callee::Builtin(name) if name == "-" => self.compile_i64_sub(args, env),
                Callee::Builtin(_) => {
                    self.compile_expr(expr, Reg::A0, env)?;
                    self.emit_zero_extend_a0_to_a1();
                    Ok(())
                }
            },
            _ => {
                self.compile_expr(expr, Reg::A0, env)?;
                self.emit_sign_extend_a0_to_a1();
                Ok(())
            }
        }
    }

    pub(super) fn compile_i64_mul(&mut self, args: &[LExpr], env: &mut Env) -> Result<(), String> {
        if args.is_empty() {
            return Err("'*' expects at least 1 argument".to_string());
        }
        if args.len() == 1 {
            self.compile_i64_expr(&args[0], env)?;
            return Ok(());
        }

        self.compile_i64_expr(&args[0], env)?;
        for arg in &args[1..] {
            self.push_reg(Reg::A0);
            self.push_reg(Reg::A1);
            self.compile_i64_expr(arg, env)?;
            self.pop_reg(Reg::T1);
            self.pop_reg(Reg::T0);
            self.emit_mul_u64_pairs();
        }
        Ok(())
    }

    pub(super) fn compile_i64_add(&mut self, args: &[LExpr], env: &mut Env) -> Result<(), String> {
        if args.is_empty() {
            self.emit_load_const_i64(0, Reg::A0);
            return Ok(());
        }
        self.compile_i64_expr(&args[0], env)?;
        for arg in &args[1..] {
            self.push_reg(Reg::A0);
            self.push_reg(Reg::A1);
            self.compile_i64_expr(arg, env)?;
            self.pop_reg(Reg::T1);
            self.pop_reg(Reg::T0);
            self.emit_r(AluRKind::Add, Reg::T2, Reg::T0, Reg::A0);
            self.emit_r(AluRKind::Sltu, Reg::T3, Reg::T2, Reg::T0);
            self.emit_r(AluRKind::Add, Reg::A1, Reg::T1, Reg::A1);
            self.emit_r(AluRKind::Add, Reg::A1, Reg::A1, Reg::T3);
            mov(&mut self.program, Reg::A0, Reg::T2);
        }
        Ok(())
    }

    pub(super) fn compile_i64_sub(&mut self, args: &[LExpr], env: &mut Env) -> Result<(), String> {
        if args.len() != 2 {
            return Err("dynamic :i64 '-' expects exactly 2 arguments".to_string());
        }
        self.compile_i64_expr(&args[0], env)?;
        self.push_reg(Reg::A0);
        self.push_reg(Reg::A1);
        self.compile_i64_expr(&args[1], env)?;
        self.pop_reg(Reg::T1);
        self.pop_reg(Reg::T0);
        self.emit_r(AluRKind::Sltu, Reg::T3, Reg::T0, Reg::A0);
        self.emit_r(AluRKind::Sub, Reg::A0, Reg::T0, Reg::A0);
        self.emit_r(AluRKind::Sub, Reg::A1, Reg::T1, Reg::A1);
        self.emit_r(AluRKind::Sub, Reg::A1, Reg::A1, Reg::T3);
        Ok(())
    }

    pub(super) fn emit_load_const_i64(&mut self, value: i64, target: Reg) {
        let lo = value as u32;
        let hi = ((value as u64) >> 32) as u32;
        self.emit_load_imm(target, lo as i32);
        self.emit_load_imm(Reg::A1, hi as i32);
    }
    pub(super) fn emit_zero_extend_a0_to_a1(&mut self) {
        self.emit_load_imm(Reg::A1, 0);
    }

    pub(super) fn emit_sign_extend_a0_to_a1(&mut self) {
        self.emit_load_imm(Reg::T6, 31);
        self.emit_r(AluRKind::Sra, Reg::A1, Reg::A0, Reg::T6);
    }

    pub(super) fn emit_mul_u64_pairs(&mut self) {
        self.emit_r(AluRKind::Mulhu, Reg::T2, Reg::T0, Reg::A0);
        self.emit_r(AluRKind::Mul, Reg::T3, Reg::T1, Reg::A0);
        self.emit_r(AluRKind::Add, Reg::T2, Reg::T2, Reg::T3);
        self.emit_r(AluRKind::Mul, Reg::T3, Reg::T0, Reg::A1);
        self.emit_r(AluRKind::Add, Reg::A1, Reg::T2, Reg::T3);
        self.emit_r(AluRKind::Mul, Reg::A0, Reg::T0, Reg::A0);
    }

    pub(super) fn emit_print_dynamic_i64(&mut self, target: Reg) {
        let label_negative = self.next_label("print_i64_negative");
        let label_print_abs = self.next_label("print_i64_print_abs");
        let label_restore = self.next_label("print_i64_restore");

        self.push_reg(Reg::A0);
        self.push_reg(Reg::A1);

        self.emit_branch(BranchKind::Blt, Reg::A1, Reg::Zero, &label_negative);
        self.emit_jump(&label_print_abs);

        self.program.label(AsmSection::Text, &label_negative);
        load_mmio_base(&mut self.program, Reg::T6);
        self.emit_load_imm(Reg::T5, 45); // '-'
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::T5,
                rs1: Reg::T6,
                off: Expr::from_i32(8),
            },
        );

        self.emit_r(AluRKind::Sltu, Reg::T1, Reg::Zero, Reg::A0);
        self.emit_r(AluRKind::Sub, Reg::A1, Reg::Zero, Reg::A1);
        self.emit_r(AluRKind::Sub, Reg::A1, Reg::A1, Reg::T1);
        mov(&mut self.program, Reg::A0, Reg::T0);

        self.program.label(AsmSection::Text, &label_print_abs);
        self.emit_print_dynamic_u64(Reg::A0);
        self.emit_jump(&label_restore);

        self.program.label(AsmSection::Text, &label_restore);
        self.pop_reg(Reg::A1);
        self.pop_reg(Reg::A0);
        if target != Reg::A0 {
            mov(&mut self.program, target, Reg::A0);
        }
    }

    pub(super) fn emit_print_dynamic_u64(&mut self, target: Reg) {
        let label_loop = self.next_label("print_u64_loop");
        let label_nonzero = self.next_label("print_u64_nonzero");
        let label_zero = self.next_label("print_u64_zero");
        let label_emit = self.next_label("print_u64_emit");
        let label_restore = self.next_label("print_u64_restore");

        self.push_reg(Reg::S2);
        self.push_reg(Reg::A0);
        self.push_reg(Reg::A1);
        self.emit_load_imm(Reg::S2, 0);

        self.emit_branch(BranchKind::Bne, Reg::A1, Reg::Zero, &label_nonzero);
        self.emit_branch(BranchKind::Bne, Reg::A0, Reg::Zero, &label_nonzero);
        self.program.label(AsmSection::Text, &label_zero);
        load_mmio_base(&mut self.program, Reg::T6);
        self.emit_load_imm(Reg::T5, 48);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::T5,
                rs1: Reg::T6,
                off: Expr::from_i32(8),
            },
        );
        self.emit_jump(&label_restore);

        self.program.label(AsmSection::Text, &label_nonzero);
        self.program.label(AsmSection::Text, &label_loop);
        self.emit_divide_u64_by_10(); // qlo=t0, qhi=t1, rem=t2
        self.emit_load_imm(Reg::T3, 48);
        self.emit_r(AluRKind::Add, Reg::T2, Reg::T2, Reg::T3);
        self.push_reg(Reg::T2);
        self.emit_load_imm(Reg::T3, 1);
        self.emit_r(AluRKind::Add, Reg::S2, Reg::S2, Reg::T3);
        mov(&mut self.program, Reg::A0, Reg::T0);
        mov(&mut self.program, Reg::A1, Reg::T1);
        self.emit_branch(BranchKind::Bne, Reg::A1, Reg::Zero, &label_loop);
        self.emit_branch(BranchKind::Bne, Reg::A0, Reg::Zero, &label_loop);

        self.program.label(AsmSection::Text, &label_emit);
        self.emit_branch(BranchKind::Beq, Reg::S2, Reg::Zero, &label_restore);
        self.pop_reg(Reg::T5);
        load_mmio_base(&mut self.program, Reg::T6);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::T5,
                rs1: Reg::T6,
                off: Expr::from_i32(8),
            },
        );
        self.emit_load_imm(Reg::T3, -1);
        self.emit_r(AluRKind::Add, Reg::S2, Reg::S2, Reg::T3);
        self.emit_jump(&label_emit);

        self.program.label(AsmSection::Text, &label_restore);
        self.pop_reg(Reg::A1);
        self.pop_reg(Reg::A0);
        self.pop_reg(Reg::S2);
        if target != Reg::A0 {
            mov(&mut self.program, target, Reg::A0);
        }
    }

    pub(super) fn emit_divide_u64_by_10(&mut self) {
        self.emit_load_imm(Reg::T6, 10);
        self.emit_r(AluRKind::Divu, Reg::T1, Reg::A1, Reg::T6);
        self.emit_r(AluRKind::Remu, Reg::T3, Reg::A1, Reg::T6);
        self.emit_r(AluRKind::Divu, Reg::T0, Reg::A0, Reg::T6);
        self.emit_r(AluRKind::Remu, Reg::T2, Reg::A0, Reg::T6);

        self.emit_load_imm(Reg::T6, 429_496_729);
        self.emit_r(AluRKind::Mul, Reg::T4, Reg::T3, Reg::T6);
        self.emit_r(AluRKind::Add, Reg::T0, Reg::T0, Reg::T4);

        self.emit_load_imm(Reg::T6, 6);
        self.emit_r(AluRKind::Mul, Reg::T4, Reg::T3, Reg::T6);
        self.emit_r(AluRKind::Add, Reg::T2, Reg::T2, Reg::T4);

        self.emit_load_imm(Reg::T6, 10);
        self.emit_r(AluRKind::Divu, Reg::T4, Reg::T2, Reg::T6);
        self.emit_r(AluRKind::Remu, Reg::T2, Reg::T2, Reg::T6);
        self.emit_r(AluRKind::Add, Reg::T0, Reg::T0, Reg::T4);
    }
}

// -----------------------------------------------------------------------------
// Trap/MMIO lowering
// -----------------------------------------------------------------------------

impl Compiler {
    pub(super) fn emit_trap_vector_table(&mut self) {
        let handler_label = self
            .function_sigs
            .get(DEFAULT_INPUT_HANDLER_LABEL)
            .map(|sig| sig.label.clone())
            .unwrap_or_else(|| DEFAULT_INPUT_HANDLER_LABEL.to_string());
        self.program.label(AsmSection::Text, "__trap_vector_table");
        self.program
            .emit_data(AsmSection::Text, DataItem::LabelAddr(handler_label));
    }

    pub(super) fn compile_read_input_data(&mut self, target: Reg) -> Result<(), String> {
        load_mmio_base(&mut self.program, Reg::T6);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: target,
                rs1: Reg::T6,
                off: Expr::from_i32(4),
            },
        );
        Ok(())
    }

    pub(super) fn compile_handler_done(&mut self, target: Reg) -> Result<(), String> {
        load_mmio_base(&mut self.program, Reg::T6);
        self.emit_load_imm(Reg::T5, 1);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::T5,
                rs1: Reg::T6,
                off: Expr::from_i32(16),
            },
        );
        self.emit_load_imm(target, 0);
        Ok(())
    }
}
