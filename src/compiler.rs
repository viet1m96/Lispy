use std::collections::BTreeMap;

use crate::asm::{AsmProgram, AsmSection, DataItem, Expr};
use crate::isa::{AluRKind, BranchKind, Instruction, Reg};
use crate::lisp::{
    parse_program, Binding, Callee, Defun, Expr as LExpr, Param, Program, TopForm, TypeName,
};
use crate::runtime::{
    emit_runtime, load_mmio_base, load_u32, mov, DEFAULT_INPUT_HANDLER_LABEL, PRINT_INT_LABEL,
    PRINT_PSTR_LABEL, PRINT_VALUE_LABEL, READ_CHAR_LABEL, READ_LINE_LABEL,
};
use crate::typecheck::typecheck_program;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Int,
    I64,
    String,
    Unknown,
}

impl ValueKind {
    fn from_type_name<T: std::borrow::Borrow<TypeName>>(ty: T) -> Self {
        match *ty.borrow() {
            TypeName::Int => Self::Int,
            TypeName::I64 => Self::I64,
            TypeName::Bool => Self::Int,
            TypeName::String => Self::String,
        }
    }

    fn width_words(self) -> usize {
        match self {
            Self::I64 => 2,
            Self::Int | Self::String | Self::Unknown => 1,
        }
    }
}

#[derive(Debug, Clone)]
struct FunctionSig {
    label: String,
    param_kinds: Vec<ValueKind>,
    param_offsets: Vec<i32>,
    param_word_count: usize,
    return_kind: ValueKind,
}

impl FunctionSig {
    fn param_count(&self) -> usize {
        self.param_kinds.len()
    }
}

#[derive(Debug, Clone)]
enum VarLoc {
    Global(String),
    Frame(i32), // offset from s1
}

#[derive(Debug, Clone)]
struct VarInfo {
    loc: VarLoc,
    kind: ValueKind,
}

#[derive(Debug, Clone)]
struct Env {
    scopes: Vec<BTreeMap<String, VarInfo>>,
    next_local_slot: usize,
}

impl Env {
    fn top() -> Self {
        Self {
            scopes: vec![BTreeMap::new()],
            next_local_slot: 0,
        }
    }

    fn function(params: &[Param], sig: &FunctionSig) -> Self {
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

    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn insert_current(&mut self, name: String, info: VarInfo) {
        self.scopes
            .last_mut()
            .expect("scope stack is never empty")
            .insert(name, info);
    }

    fn lookup(&self, name: &str) -> Option<VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.clone());
            }
        }
        None
    }

    fn alloc_frame_slots(&mut self, width_words: usize) -> i32 {
        let offset = (self.next_local_slot as i32) * 4;
        self.next_local_slot += width_words.max(1);
        offset
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
    needs_print_value: bool,
    needs_read_line: bool,
    needs_read_char: bool,
    has_custom_input_handler: bool,
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
        }
    }

    fn finish(mut self) -> AsmProgram {
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

    fn emit_trap_vector_table(&mut self) {
        let handler_label = self
            .function_sigs
            .get(DEFAULT_INPUT_HANDLER_LABEL)
            .map(|sig| sig.label.clone())
            .unwrap_or_else(|| DEFAULT_INPUT_HANDLER_LABEL.to_string());
        self.program.label(AsmSection::Text, "__trap_vector_table");
        self.program
            .emit_data(AsmSection::Text, DataItem::LabelAddr(handler_label));
    }

    fn compile_top_level(&mut self, ast: &Program) -> Result<(), String> {
        self.program.label(AsmSection::Text, "_start");

        let top_level_slot_count: usize = ast
            .forms
            .iter()
            .filter_map(|form| match form {
                TopForm::Expr(expr) => Some(count_let_slots_in_expr(expr, self)),
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

        let local_slot_count = sig.param_word_count + count_let_slots_in_body(&defun.body, self);
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

    fn compile_expr(&mut self, expr: &LExpr, target: Reg, env: &mut Env) -> Result<(), String> {
        if is_foldable_expr(expr) {
            if let Some(value) = const_i32(expr) {
                self.emit_load_imm(target, value);
                return Ok(());
            }
            if is_i64_expr(expr) {
                if let Some(value) = const_i64(expr) {
                    self.emit_load_const_i64(value, target);
                    return Ok(());
                }
                return Err("dynamic i64 expressions are not supported yet in this milestone; only constant i64 expressions are supported".to_string());
            }
        }

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
                TypeName::Int | TypeName::Bool => {
                    if self.infer_expr_kind_scoped(value, env) == ValueKind::I64
                        || is_i64_expr(value)
                    {
                        self.compile_i64_expr(value, env)?;
                        if *target_type == TypeName::Bool {
                            self.emit_r(AluRKind::Or, target, Reg::A0, Reg::A1);
                        } else if target != Reg::A0 {
                            mov(&mut self.program, target, Reg::A0);
                        }
                    } else {
                        self.compile_expr(value, target, env)?;
                    }
                }
                TypeName::String => self.compile_expr(value, target, env)?,
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
                let kind = self.infer_expr_kind_scoped(value, env);
                if kind == ValueKind::I64 || is_i64_expr(value) {
                    if let Some(const_value) = const_i64(value) {
                        self.emit_print_const_i64(const_value, target);
                    } else {
                        self.compile_i64_expr(value, env)?;
                        self.emit_print_dynamic_u64(target);
                    }
                    return Ok(());
                }

                let label = match kind {
                    ValueKind::Int => {
                        self.needs_print_int = true;
                        PRINT_INT_LABEL
                    }
                    ValueKind::String => {
                        self.needs_print_pstr = true;
                        PRINT_PSTR_LABEL
                    }
                    ValueKind::I64 => unreachable!("i64 print handled above"),
                    ValueKind::Unknown => {
                        self.needs_print_value = true;
                        PRINT_VALUE_LABEL
                    }
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
            LExpr::ReadInputData => self.compile_read_input_data(&[], target)?,
            LExpr::HandlerDone => self.compile_handler_done(&[], target)?,
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

    fn compile_i64_expr(&mut self, expr: &LExpr, env: &mut Env) -> Result<(), String> {
        if let Some(value) = const_i64(expr) {
            self.emit_load_const_i64(value, Reg::A0);
            return Ok(());
        }

        match expr {
            LExpr::Cast { target_type, value } => match target_type {
                TypeName::I64 => {
                    if self.infer_expr_kind_scoped(value, env) == ValueKind::I64
                        || is_i64_expr(value)
                    {
                        self.compile_i64_expr(value, env)
                    } else {
                        self.compile_expr(value, Reg::A0, env)?;
                        self.emit_sign_extend_a0_to_a1();
                        Ok(())
                    }
                }
                TypeName::Int | TypeName::Bool => {
                    self.compile_expr(value, Reg::A0, env)?;
                    self.emit_sign_extend_a0_to_a1();
                    Ok(())
                }
                TypeName::String => Err("cannot use string cast in an i64 expression".to_string()),
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

    fn compile_i64_mul(&mut self, args: &[LExpr], env: &mut Env) -> Result<(), String> {
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

    fn compile_i64_add(&mut self, args: &[LExpr], env: &mut Env) -> Result<(), String> {
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

    fn compile_i64_sub(&mut self, args: &[LExpr], env: &mut Env) -> Result<(), String> {
        if args.len() != 2 {
            return Err("dynamic i64 '-' currently supports exactly 2 arguments".to_string());
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

    fn compile_let(
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

    fn compile_builtin(
        &mut self,
        name: &str,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        match name {
            "+" => self.compile_fold_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Add, dst, lhs, rhs);
                Ok(())
            }),
            "-" => {
                if args.len() == 1 {
                    self.compile_expr(&args[0], Reg::T1, env)?;
                    self.emit_r(AluRKind::Sub, target, Reg::Zero, Reg::T1);
                    return Ok(());
                }
                self.compile_fold_binary(args, target, env, |this, dst, lhs, rhs| {
                    this.emit_r(AluRKind::Sub, dst, lhs, rhs);
                    Ok(())
                })
            }
            "*" => self.compile_fold_binary(args, target, env, |this, dst, lhs, rhs| {
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
            "bit-and" => self.compile_fold_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::And, dst, lhs, rhs);
                Ok(())
            }),
            "bit-or" => self.compile_fold_binary(args, target, env, |this, dst, lhs, rhs| {
                this.emit_r(AluRKind::Or, dst, lhs, rhs);
                Ok(())
            }),
            "bit-xor" => self.compile_fold_binary(args, target, env, |this, dst, lhs, rhs| {
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
            "strlen" => self.compile_strlen(args, target, env),
            "strget" => self.compile_strget(args, target, env),
            "strset" => self.compile_strset(args, target, env),
            other => Err(format!(
                "builtin '{other}' is reserved for a later milestone"
            )),
        }
    }

    fn compile_read_input_data(&mut self, args: &[LExpr], target: Reg) -> Result<(), String> {
        if !args.is_empty() {
            return Err("read-input-data expects no arguments".to_string());
        }
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

    fn compile_handler_done(&mut self, args: &[LExpr], target: Reg) -> Result<(), String> {
        if !args.is_empty() {
            return Err("handler-done expects no arguments".to_string());
        }
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

    fn compile_user_call(
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

    fn compile_exact_binary<F>(
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

    fn compile_fold_binary<F>(
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

    fn compile_compare(
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

        let lhs_kind = self.infer_expr_kind_scoped(&args[0], env);
        let rhs_kind = self.infer_expr_kind_scoped(&args[1], env);
        if lhs_kind == ValueKind::I64
            || rhs_kind == ValueKind::I64
            || is_i64_expr(&args[0])
            || is_i64_expr(&args[1])
        {
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

    fn compile_compare_i64(
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

    fn emit_i64_less_than_branch(
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

    fn compile_and(&mut self, args: &[LExpr], target: Reg, env: &mut Env) -> Result<(), String> {
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

    fn compile_or(&mut self, args: &[LExpr], target: Reg, env: &mut Env) -> Result<(), String> {
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

    fn compile_not(&mut self, args: &[LExpr], target: Reg, env: &mut Env) -> Result<(), String> {
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

    fn compile_strlen(&mut self, args: &[LExpr], target: Reg, env: &mut Env) -> Result<(), String> {
        if args.len() != 1 {
            return Err(format!(
                "strlen expects exactly 1 argument, got {}",
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

    fn compile_strget(&mut self, args: &[LExpr], target: Reg, env: &mut Env) -> Result<(), String> {
        if args.len() != 2 {
            return Err(format!(
                "strget expects exactly 2 arguments, got {}",
                args.len()
            ));
        }

        self.compile_expr(&args[0], Reg::T0, env)?;
        self.push_reg(Reg::T0);
        self.compile_expr(&args[1], Reg::T1, env)?;
        self.pop_reg(Reg::T0);

        self.emit_scale_string_index(Reg::T1, Reg::T1);
        self.emit_r(AluRKind::Add, Reg::T0, Reg::T0, Reg::T1);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: target,
                rs1: Reg::T0,
                off: Expr::from_i32(0),
            },
        );
        Ok(())
    }

    fn compile_strset(&mut self, args: &[LExpr], target: Reg, env: &mut Env) -> Result<(), String> {
        if args.len() != 3 {
            return Err(format!(
                "strset expects exactly 3 arguments, got {}",
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

        self.emit_scale_string_index(Reg::T1, Reg::T1);
        self.emit_r(AluRKind::Add, Reg::T0, Reg::T0, Reg::T1);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::T2,
                rs1: Reg::T0,
                off: Expr::from_i32(0),
            },
        );
        if target != Reg::T2 {
            mov(&mut self.program, target, Reg::T2);
        }
        Ok(())
    }

    fn emit_scale_string_index(&mut self, index_reg: Reg, out_reg: Reg) {
        self.emit_load_imm(Reg::T3, 2);
        self.emit_r(AluRKind::Sll, out_reg, index_reg, Reg::T3);
        self.emit_load_imm(Reg::T3, 4);
        self.emit_r(AluRKind::Add, out_reg, out_reg, Reg::T3);
    }

    fn emit_load_const_i64(&mut self, value: i64, target: Reg) {
        let lo = value as u32;
        let hi = ((value as u64) >> 32) as u32;
        self.emit_load_imm(target, lo as i32);
        self.emit_load_imm(Reg::A1, hi as i32);
    }

    fn emit_print_const_i64(&mut self, value: i64, target: Reg) {
        load_mmio_base(&mut self.program, Reg::T6);
        for byte in value.to_string().bytes() {
            self.emit_load_imm(Reg::T5, i32::from(byte));
            self.program.emit_inst(
                AsmSection::Text,
                Instruction::Sw {
                    rs2: Reg::T5,
                    rs1: Reg::T6,
                    off: Expr::from_i32(8),
                },
            );
        }
        self.emit_load_const_i64(value, Reg::A0);
        if target != Reg::A0 {
            mov(&mut self.program, target, Reg::A0);
        }
    }

    fn emit_zero_extend_a0_to_a1(&mut self) {
        self.emit_load_imm(Reg::A1, 0);
    }

    fn emit_sign_extend_a0_to_a1(&mut self) {
        self.emit_load_imm(Reg::T6, 31);
        self.emit_r(AluRKind::Sra, Reg::A1, Reg::A0, Reg::T6);
    }

    fn emit_mul_u64_pairs(&mut self) {
        // Input:
        //   lhs = t1:t0
        //   rhs = a1:a0
        // Output:
        //   product low 64 bits = a1:a0
        self.emit_r(AluRKind::Mulhu, Reg::T2, Reg::T0, Reg::A0);
        self.emit_r(AluRKind::Mul, Reg::T3, Reg::T1, Reg::A0);
        self.emit_r(AluRKind::Add, Reg::T2, Reg::T2, Reg::T3);
        self.emit_r(AluRKind::Mul, Reg::T3, Reg::T0, Reg::A1);
        self.emit_r(AluRKind::Add, Reg::A1, Reg::T2, Reg::T3);
        self.emit_r(AluRKind::Mul, Reg::A0, Reg::T0, Reg::A0);
    }

    fn emit_print_dynamic_u64(&mut self, target: Reg) {
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

    fn emit_divide_u64_by_10(&mut self) {
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

    fn lookup_var(&self, name: &str, env: &Env) -> Option<VarInfo> {
        env.lookup(name)
            .or_else(|| self.global_vars.get(name).cloned())
    }

    fn define_global(&mut self, name: &str, kind: ValueKind) -> VarInfo {
        if let Some(existing) = self.global_vars.get(name) {
            return existing.clone();
        }

        let label = format!("__g_{}_{}", sanitize(name), self.next_global_id);
        self.next_global_id += 1;
        self.program.label(AsmSection::Data, &label);
        for _ in 0..kind.width_words() {
            self.program.emit_data(AsmSection::Data, DataItem::Word(0));
        }
        let info = VarInfo {
            loc: VarLoc::Global(label),
            kind,
        };
        self.global_vars.insert(name.to_string(), info.clone());
        info
    }

    fn compile_store_value(
        &mut self,
        value: &LExpr,
        target: Reg,
        env: &mut Env,
        info: &VarInfo,
    ) -> Result<(), String> {
        if info.kind == ValueKind::I64 {
            self.compile_i64_expr(value, env)?;
            self.emit_store_info_pair(Reg::A0, Reg::A1, info);
            if target != Reg::A0 {
                mov(&mut self.program, target, Reg::A0);
            }
        } else {
            self.compile_expr(value, target, env)?;
            self.emit_store_to_loc(target, &info.loc);
        }
        Ok(())
    }

    fn emit_load_from_info_pair(&mut self, lo: Reg, info: &VarInfo) {
        match &info.loc {
            VarLoc::Global(label) => {
                self.emit_load_addr(Reg::T6, label);
                self.program.emit_inst(
                    AsmSection::Text,
                    Instruction::Lw {
                        rd: lo,
                        rs1: Reg::T6,
                        off: Expr::from_i32(0),
                    },
                );
                self.program.emit_inst(
                    AsmSection::Text,
                    Instruction::Lw {
                        rd: Reg::A1,
                        rs1: Reg::T6,
                        off: Expr::from_i32(4),
                    },
                );
            }
            VarLoc::Frame(offset) => {
                self.emit_load_frame(lo, *offset);
                self.emit_load_frame(Reg::A1, *offset + 4);
            }
        }
    }

    fn emit_store_info_pair(&mut self, lo: Reg, hi: Reg, info: &VarInfo) {
        match &info.loc {
            VarLoc::Global(label) => {
                self.emit_load_addr(Reg::T6, label);
                self.program.emit_inst(
                    AsmSection::Text,
                    Instruction::Sw {
                        rs2: lo,
                        rs1: Reg::T6,
                        off: Expr::from_i32(0),
                    },
                );
                self.program.emit_inst(
                    AsmSection::Text,
                    Instruction::Sw {
                        rs2: hi,
                        rs1: Reg::T6,
                        off: Expr::from_i32(4),
                    },
                );
            }
            VarLoc::Frame(offset) => {
                self.emit_store_frame(lo, *offset);
                self.emit_store_frame(hi, *offset + 4);
            }
        }
    }

    fn intern_string(&mut self, text: &str) -> String {
        if let Some(existing) = self.string_labels.get(text) {
            return existing.clone();
        }

        let label = format!("__str_{}", self.next_string_id);
        self.next_string_id += 1;
        self.program.label(AsmSection::Data, &label);
        self.program
            .emit_data(AsmSection::Data, DataItem::PStr(text.to_string()));
        self.string_labels.insert(text.to_string(), label.clone());
        label
    }

    fn next_label(&mut self, prefix: &str) -> String {
        let label = format!("__{}_{}", prefix, self.next_label_id);
        self.next_label_id += 1;
        label
    }

    fn interrupt_context_regs() -> [Reg; 30] {
        [
            Reg::Ra,
            Reg::Gp,
            Reg::Tp,
            Reg::T0,
            Reg::T1,
            Reg::T2,
            Reg::S0,
            Reg::S1,
            Reg::A0,
            Reg::A1,
            Reg::A2,
            Reg::A3,
            Reg::A4,
            Reg::A5,
            Reg::A6,
            Reg::A7,
            Reg::S2,
            Reg::S3,
            Reg::S4,
            Reg::S5,
            Reg::S6,
            Reg::S7,
            Reg::S8,
            Reg::S9,
            Reg::S10,
            Reg::S11,
            Reg::T3,
            Reg::T4,
            Reg::T5,
            Reg::T6,
        ]
    }

    fn emit_interrupt_context_save(&mut self) {
        let regs = Self::interrupt_context_regs();
        let bytes = (regs.len() as i32) * 4;
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::Sp,
                rs1: Reg::Sp,
                imm: Expr::from_i32(-bytes),
            },
        );
        for (index, reg) in regs.iter().copied().enumerate() {
            self.program.emit_inst(
                AsmSection::Text,
                Instruction::Sw {
                    rs2: reg,
                    rs1: Reg::Sp,
                    off: Expr::from_i32((index as i32) * 4),
                },
            );
        }
    }

    fn emit_interrupt_context_restore(&mut self) {
        let regs = Self::interrupt_context_regs();
        let bytes = (regs.len() as i32) * 4;
        for (index, reg) in regs.iter().copied().enumerate() {
            self.program.emit_inst(
                AsmSection::Text,
                Instruction::Lw {
                    rd: reg,
                    rs1: Reg::Sp,
                    off: Expr::from_i32((index as i32) * 4),
                },
            );
        }
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::Sp,
                rs1: Reg::Sp,
                imm: Expr::from_i32(bytes),
            },
        );
    }

    fn emit_prologue(&mut self, frame_bytes: i32) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::Sp,
                rs1: Reg::Sp,
                imm: Expr::from_i32(-(frame_bytes + 8)),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::Ra,
                rs1: Reg::Sp,
                off: Expr::from_i32(0),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: Reg::S1,
                rs1: Reg::Sp,
                off: Expr::from_i32(4),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::S1,
                rs1: Reg::Sp,
                imm: Expr::from_i32(8),
            },
        );
    }

    fn emit_epilogue(&mut self, frame_bytes: i32) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: Reg::Ra,
                rs1: Reg::S1,
                off: Expr::from_i32(-8),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: Reg::T0,
                rs1: Reg::S1,
                off: Expr::from_i32(-4),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::Sp,
                rs1: Reg::S1,
                imm: Expr::from_i32(frame_bytes),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::S1,
                rs1: Reg::T0,
                imm: Expr::from_i32(0),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Jalr {
                rd: Reg::Zero,
                rs1: Reg::Ra,
                off: Expr::from_i32(0),
            },
        );
    }

    fn emit_epilogue_without_return(&mut self, frame_bytes: i32) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: Reg::Ra,
                rs1: Reg::S1,
                off: Expr::from_i32(-8),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: Reg::T0,
                rs1: Reg::S1,
                off: Expr::from_i32(-4),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::Sp,
                rs1: Reg::S1,
                imm: Expr::from_i32(frame_bytes),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::S1,
                rs1: Reg::T0,
                imm: Expr::from_i32(0),
            },
        );
    }

    fn emit_load_imm(&mut self, rd: Reg, value: i32) {
        load_u32(&mut self.program, rd, value);
    }

    fn emit_r(&mut self, op: AluRKind, rd: Reg, rs1: Reg, rs2: Reg) {
        self.program
            .emit_inst(AsmSection::Text, Instruction::AluR { op, rd, rs1, rs2 });
    }

    fn emit_load_addr(&mut self, rd: Reg, label: &str) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lui {
                rd,
                imm20: Expr::hi20(label),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd,
                rs1: rd,
                imm: Expr::lo12(label),
            },
        );
    }

    fn emit_load_word(&mut self, rd: Reg, label: &str) {
        self.emit_load_addr(rd, label);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd,
                rs1: rd,
                off: Expr::from_i32(0),
            },
        );
    }

    fn emit_store_word(&mut self, rs: Reg, label: &str) {
        self.emit_load_addr(Reg::T6, label);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: rs,
                rs1: Reg::T6,
                off: Expr::from_i32(0),
            },
        );
    }

    fn emit_load_frame(&mut self, rd: Reg, offset: i32) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd,
                rs1: Reg::S1,
                off: Expr::from_i32(offset),
            },
        );
    }

    fn emit_store_frame(&mut self, rs: Reg, offset: i32) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: rs,
                rs1: Reg::S1,
                off: Expr::from_i32(offset),
            },
        );
    }

    fn emit_load_from_loc(&mut self, rd: Reg, loc: &VarLoc) {
        match loc {
            VarLoc::Global(label) => self.emit_load_word(rd, label),
            VarLoc::Frame(offset) => self.emit_load_frame(rd, *offset),
        }
    }

    fn emit_store_to_loc(&mut self, rs: Reg, loc: &VarLoc) {
        match loc {
            VarLoc::Global(label) => self.emit_store_word(rs, label),
            VarLoc::Frame(offset) => self.emit_store_frame(rs, *offset),
        }
    }

    fn emit_branch(&mut self, op: BranchKind, rs1: Reg, rs2: Reg, label: &str) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Branch {
                op,
                rs1,
                rs2,
                off: Expr::pcrel(label),
            },
        );
    }

    fn emit_jump(&mut self, label: &str) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Jal {
                rd: Reg::Zero,
                off: Expr::pcrel(label),
            },
        );
    }

    fn push_reg(&mut self, reg: Reg) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::Sp,
                rs1: Reg::Sp,
                imm: Expr::from_i32(-4),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: reg,
                rs1: Reg::Sp,
                off: Expr::from_i32(0),
            },
        );
    }

    fn pop_reg(&mut self, reg: Reg) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: reg,
                rs1: Reg::Sp,
                off: Expr::from_i32(0),
            },
        );
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Addi {
                rd: Reg::Sp,
                rs1: Reg::Sp,
                imm: Expr::from_i32(4),
            },
        );
    }
}

fn arg_reg(index: usize) -> Result<Reg, String> {
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

fn count_let_slots_in_body(body: &[LExpr], compiler: &Compiler) -> usize {
    body.iter()
        .map(|expr| count_let_slots_in_expr(expr, compiler))
        .sum()
}

fn count_let_slots_in_expr(expr: &LExpr, _compiler: &Compiler) -> usize {
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
        LExpr::Cast { value, .. } => count_let_slots_in_expr(value, _compiler),
        LExpr::Setq { value, .. } => count_let_slots_in_expr(value, _compiler),
        LExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            count_let_slots_in_expr(cond, _compiler)
                + count_let_slots_in_expr(then_branch, _compiler)
                + count_let_slots_in_expr(else_branch, _compiler)
        }
        LExpr::Begin(items) => items
            .iter()
            .map(|expr| count_let_slots_in_expr(expr, _compiler))
            .sum(),
        LExpr::Let { bindings, body } => {
            bindings
                .iter()
                .map(|binding| {
                    ValueKind::from_type_name(binding.type_ann).width_words()
                        + count_let_slots_in_expr(&binding.value, _compiler)
                })
                .sum::<usize>()
                + body
                    .iter()
                    .map(|expr| count_let_slots_in_expr(expr, _compiler))
                    .sum::<usize>()
        }
        LExpr::Loop {
            cond,
            body,
            finally,
        } => {
            count_let_slots_in_expr(cond, _compiler)
                + body
                    .iter()
                    .map(|expr| count_let_slots_in_expr(expr, _compiler))
                    .sum::<usize>()
                + count_let_slots_in_expr(finally, _compiler)
        }
        LExpr::Print(value) | LExpr::PrintStr(value) => count_let_slots_in_expr(value, _compiler),
        LExpr::Call { args, .. } => args
            .iter()
            .map(|arg| count_let_slots_in_expr(arg, _compiler))
            .sum(),
    }
}

impl Compiler {
    fn infer_expr_kind_scoped(&self, expr: &LExpr, env: &Env) -> ValueKind {
        match expr {
            LExpr::Ident(name) => self
                .lookup_var(name, env)
                .map(|info| info.kind)
                .unwrap_or(ValueKind::Unknown),
            LExpr::Cast { target_type, .. } => ValueKind::from_type_name(*target_type),
            LExpr::Setq { type_ann, .. } => ValueKind::from_type_name(*type_ann),
            LExpr::If {
                then_branch,
                else_branch,
                ..
            } => {
                let left = self.infer_expr_kind_scoped(then_branch, env);
                let right = self.infer_expr_kind_scoped(else_branch, env);
                if left == right {
                    left
                } else if left == ValueKind::I64 && right == ValueKind::Int
                    || left == ValueKind::Int && right == ValueKind::I64
                {
                    ValueKind::I64
                } else {
                    ValueKind::Unknown
                }
            }
            LExpr::Begin(items) => items
                .last()
                .map(|expr| self.infer_expr_kind_scoped(expr, env))
                .unwrap_or(ValueKind::Unknown),
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
                    .map(|expr| self.infer_expr_kind_scoped(expr, &scoped))
                    .unwrap_or(ValueKind::Unknown)
            }
            LExpr::Loop { finally, .. } => self.infer_expr_kind_scoped(finally, env),
            LExpr::Print(value) => self.infer_expr_kind_scoped(value, env),
            LExpr::PrintStr(_) => ValueKind::String,
            LExpr::Call { callee, args } => match callee {
                Callee::Builtin(name) => match name.as_str() {
                    "strlen" | "strget" | "strset" | "=" | "!=" | "<" | "<=" | ">" | ">="
                    | "and" | "or" | "not" => ValueKind::Int,
                    "+" | "-" | "*" | "/" | "%" | "bit-and" | "bit-or" | "bit-xor" | "shl"
                    | "shr" | "sar" => {
                        let mut saw_i64 = false;
                        for arg in args {
                            if self.infer_expr_kind_scoped(arg, env) == ValueKind::I64 {
                                saw_i64 = true;
                            }
                        }
                        if saw_i64 {
                            ValueKind::I64
                        } else {
                            ValueKind::Int
                        }
                    }
                    _ => ValueKind::Unknown,
                },
                Callee::Ident(name) => self
                    .function_sigs
                    .get(name)
                    .map(|sig| sig.return_kind)
                    .unwrap_or(ValueKind::Unknown),
            },
            other => self.infer_expr_kind(other),
        }
    }

    fn infer_expr_kind(&self, expr: &LExpr) -> ValueKind {
        match expr {
            LExpr::Number(_)
            | LExpr::Bool(_)
            | LExpr::Nil
            | LExpr::ReadChar
            | LExpr::ReadInputData
            | LExpr::HandlerDone
            | LExpr::Halt => ValueKind::Int,
            LExpr::Cast { target_type, .. } => ValueKind::from_type_name(*target_type),
            LExpr::String(_) | LExpr::ReadLine => ValueKind::String,
            LExpr::Ident(name) => self
                .global_vars
                .get(name)
                .map(|info| info.kind)
                .unwrap_or(ValueKind::Unknown),
            LExpr::Setq { type_ann, .. } => ValueKind::from_type_name(*type_ann),
            LExpr::If {
                then_branch,
                else_branch,
                ..
            } => {
                let left = self.infer_expr_kind(then_branch);
                let right = self.infer_expr_kind(else_branch);
                if left == right {
                    left
                } else if left == ValueKind::I64 && right == ValueKind::Int
                    || left == ValueKind::Int && right == ValueKind::I64
                {
                    ValueKind::I64
                } else {
                    ValueKind::Unknown
                }
            }
            LExpr::Begin(items) => items
                .last()
                .map(|expr| self.infer_expr_kind(expr))
                .unwrap_or(ValueKind::Unknown),
            LExpr::Let { body, .. } => body
                .last()
                .map(|expr| self.infer_expr_kind(expr))
                .unwrap_or(ValueKind::Unknown),
            LExpr::Loop { finally, .. } => self.infer_expr_kind(finally),
            LExpr::Print(value) => self.infer_expr_kind(value),
            LExpr::PrintStr(_) => ValueKind::String,
            LExpr::Call { callee, args } => match callee {
                Callee::Builtin(name) => match name.as_str() {
                    "strlen" | "strget" | "strset" | "=" | "!=" | "<" | "<=" | ">" | ">="
                    | "and" | "or" | "not" => ValueKind::Int,
                    "+" | "-" | "*" | "/" | "%" | "bit-and" | "bit-or" | "bit-xor" | "shl"
                    | "shr" | "sar" => {
                        let mut saw_i64 = false;
                        for arg in args {
                            if self.infer_expr_kind(arg) == ValueKind::I64 {
                                saw_i64 = true;
                            }
                        }
                        if saw_i64 {
                            ValueKind::I64
                        } else {
                            ValueKind::Int
                        }
                    }
                    _ => ValueKind::Unknown,
                },
                Callee::Ident(name) => self
                    .function_sigs
                    .get(name)
                    .map(|sig| sig.return_kind)
                    .unwrap_or(ValueKind::Unknown),
            },
        }
    }
}

fn expr_guarantees_halt(expr: &LExpr) -> bool {
    match expr {
        LExpr::Halt => true,
        LExpr::Begin(items) => items.last().map(expr_guarantees_halt).unwrap_or(false),
        LExpr::Let { body, .. } => body.last().map(expr_guarantees_halt).unwrap_or(false),
        LExpr::If {
            then_branch,
            else_branch,
            ..
        } => expr_guarantees_halt(then_branch) && expr_guarantees_halt(else_branch),
        LExpr::Loop { finally, .. } => expr_guarantees_halt(finally),
        _ => false,
    }
}

#[derive(Clone, Copy)]
enum CompareKind {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn is_foldable_expr(expr: &LExpr) -> bool {
    match expr {
        LExpr::Number(_) | LExpr::Bool(_) | LExpr::Nil => true,
        LExpr::Cast { value, .. } => is_foldable_expr(value),
        LExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            is_foldable_expr(cond) && is_foldable_expr(then_branch) && is_foldable_expr(else_branch)
        }
        LExpr::Call {
            callee: Callee::Builtin(_),
            args,
        } => args.iter().all(is_foldable_expr),
        _ => false,
    }
}

fn const_i32(expr: &LExpr) -> Option<i32> {
    if is_i64_expr(expr) {
        return None;
    }
    match expr {
        LExpr::Number(value) => i32::try_from(*value).ok(),
        LExpr::Cast {
            target_type: TypeName::Int,
            value,
        }
        | LExpr::Cast {
            target_type: TypeName::Bool,
            value,
        } => const_i32(value),
        LExpr::Cast {
            target_type: TypeName::I64,
            ..
        }
        | LExpr::Cast {
            target_type: TypeName::String,
            ..
        } => None,
        LExpr::Bool(value) => Some(if *value { 1 } else { 0 }),
        LExpr::Nil => Some(0),
        LExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if const_i32(cond)? != 0 {
                const_i32(then_branch)
            } else {
                const_i32(else_branch)
            }
        }
        LExpr::Begin(items) => items.last().and_then(const_i32),
        LExpr::Let { body, .. } => body.last().and_then(const_i32),
        LExpr::Loop { finally, .. } => const_i32(finally),
        LExpr::Call {
            callee: Callee::Builtin(name),
            args,
        } => {
            let vals: Option<Vec<i32>> = args.iter().map(const_i32).collect();
            let vals = vals?;
            match name.as_str() {
                "+" => Some(vals.into_iter().fold(0i32, |acc, v| acc.wrapping_add(v))),
                "-" => {
                    if vals.len() == 1 {
                        Some(0i32.wrapping_sub(vals[0]))
                    } else if vals.len() >= 2 {
                        let mut it = vals.into_iter();
                        let first = it.next()?;
                        Some(it.fold(first, |acc, v| acc.wrapping_sub(v)))
                    } else {
                        None
                    }
                }
                "*" => Some(vals.into_iter().fold(1i32, |acc, v| acc.wrapping_mul(v))),
                "/" if vals.len() == 2 && vals[1] != 0 => Some(vals[0].wrapping_div(vals[1])),
                "%" if vals.len() == 2 && vals[1] != 0 => Some(vals[0].wrapping_rem(vals[1])),
                "bit-and" => Some(vals.into_iter().fold(-1i32, |acc, v| acc & v)),
                "bit-or" => Some(vals.into_iter().fold(0i32, |acc, v| acc | v)),
                "bit-xor" => Some(vals.into_iter().fold(0i32, |acc, v| acc ^ v)),
                "shl" if vals.len() == 2 => Some(vals[0].wrapping_shl((vals[1] as u32) & 31)),
                "shr" if vals.len() == 2 => {
                    Some(((vals[0] as u32).wrapping_shr((vals[1] as u32) & 31)) as i32)
                }
                "sar" if vals.len() == 2 => Some(vals[0] >> ((vals[1] as u32) & 31)),
                "=" if vals.len() == 2 => Some((vals[0] == vals[1]) as i32),
                "!=" if vals.len() == 2 => Some((vals[0] != vals[1]) as i32),
                "<" if vals.len() == 2 => Some((vals[0] < vals[1]) as i32),
                "<=" if vals.len() == 2 => Some((vals[0] <= vals[1]) as i32),
                ">" if vals.len() == 2 => Some((vals[0] > vals[1]) as i32),
                ">=" if vals.len() == 2 => Some((vals[0] >= vals[1]) as i32),
                _ => None,
            }
        }
        _ => None,
    }
}

fn const_i64(expr: &LExpr) -> Option<i64> {
    match expr {
        LExpr::Number(value) => Some(*value),
        LExpr::Cast {
            target_type: TypeName::I64,
            value,
        }
        | LExpr::Cast {
            target_type: TypeName::Int,
            value,
        }
        | LExpr::Cast {
            target_type: TypeName::Bool,
            value,
        } => const_i64(value),
        LExpr::Cast {
            target_type: TypeName::String,
            ..
        } => None,
        LExpr::Bool(value) => Some(if *value { 1 } else { 0 }),
        LExpr::Nil => Some(0),
        LExpr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if const_i64(cond)? != 0 {
                const_i64(then_branch)
            } else {
                const_i64(else_branch)
            }
        }
        LExpr::Begin(items) => items.last().and_then(const_i64),
        LExpr::Let { body, .. } => body.last().and_then(const_i64),
        LExpr::Loop { finally, .. } => const_i64(finally),
        LExpr::Call {
            callee: Callee::Builtin(name),
            args,
        } => {
            let vals: Option<Vec<i64>> = args.iter().map(const_i64).collect();
            let vals = vals?;
            match name.as_str() {
                "+" => Some(vals.into_iter().fold(0i64, |acc, v| acc.wrapping_add(v))),
                "-" => {
                    if vals.len() == 1 {
                        Some(0i64.wrapping_sub(vals[0]))
                    } else if vals.len() >= 2 {
                        let mut it = vals.into_iter();
                        let first = it.next()?;
                        Some(it.fold(first, |acc, v| acc.wrapping_sub(v)))
                    } else {
                        None
                    }
                }
                "*" => Some(vals.into_iter().fold(1i64, |acc, v| acc.wrapping_mul(v))),
                "/" if vals.len() == 2 && vals[1] != 0 => Some(vals[0].wrapping_div(vals[1])),
                "%" if vals.len() == 2 && vals[1] != 0 => Some(vals[0].wrapping_rem(vals[1])),
                "bit-and" => Some(vals.into_iter().fold(-1i64, |acc, v| acc & v)),
                "bit-or" => Some(vals.into_iter().fold(0i64, |acc, v| acc | v)),
                "bit-xor" => Some(vals.into_iter().fold(0i64, |acc, v| acc ^ v)),
                "shl" if vals.len() == 2 => Some(vals[0].wrapping_shl((vals[1] as u32) & 63)),
                "shr" if vals.len() == 2 => {
                    Some(((vals[0] as u64).wrapping_shr((vals[1] as u32) & 63)) as i64)
                }
                "sar" if vals.len() == 2 => Some(vals[0] >> ((vals[1] as u32) & 63)),
                "=" if vals.len() == 2 => Some((vals[0] == vals[1]) as i64),
                "!=" if vals.len() == 2 => Some((vals[0] != vals[1]) as i64),
                "<" if vals.len() == 2 => Some((vals[0] < vals[1]) as i64),
                "<=" if vals.len() == 2 => Some((vals[0] <= vals[1]) as i64),
                ">" if vals.len() == 2 => Some((vals[0] > vals[1]) as i64),
                ">=" if vals.len() == 2 => Some((vals[0] >= vals[1]) as i64),
                _ => None,
            }
        }
        _ => None,
    }
}

fn is_i64_expr(expr: &LExpr) -> bool {
    match expr {
        LExpr::Cast {
            target_type: TypeName::I64,
            ..
        } => true,
        LExpr::Cast { .. } => false,
        LExpr::Setq { value, .. } => is_i64_expr(value),
        LExpr::If {
            then_branch,
            else_branch,
            ..
        } => is_i64_expr(then_branch) || is_i64_expr(else_branch),
        LExpr::Begin(items) => items.last().map(is_i64_expr).unwrap_or(false),
        LExpr::Let { body, .. } => body.last().map(is_i64_expr).unwrap_or(false),
        LExpr::Loop { finally, .. } => is_i64_expr(finally),
        LExpr::Print(value) => is_i64_expr(value),
        LExpr::PrintStr(_) => false,
        LExpr::Call {
            callee: Callee::Builtin(name),
            args,
        } => match name.as_str() {
            "+" | "-" | "*" | "/" | "%" | "bit-and" | "bit-or" | "bit-xor" | "shl" | "shr"
            | "sar" => args.iter().any(is_i64_expr),
            _ => false,
        },
        _ => false,
    }
}
