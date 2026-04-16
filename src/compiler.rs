use std::collections::BTreeMap;

use crate::asm::{AsmProgram, AsmSection, DataItem, Expr};
use crate::isa::{AluRKind, BranchKind, Instruction, Reg};
use crate::lisp::{parse_program, Binding, Callee, Defun, Expr as LExpr, Program, TopForm};
use crate::runtime::{
    emit_runtime, load_mmio_base, load_u32, mov, PRINT_INT_LABEL, PRINT_PSTR_LABEL,
    PRINT_VALUE_LABEL, READ_LINE_LABEL,
};

#[derive(Debug, Clone)]
struct FunctionSig {
    label: String,
    param_count: usize,
}

#[derive(Debug, Clone)]
enum VarLoc {
    Global(String),
    Frame(i32), // offset from s1
}

#[derive(Debug, Clone)]
struct Env {
    scopes: Vec<BTreeMap<String, VarLoc>>,
    next_local_slot: usize,
}

impl Env {
    fn top() -> Self {
        Self {
            scopes: vec![BTreeMap::new()],
            next_local_slot: 0,
        }
    }

    fn function(param_names: &[String]) -> Self {
        let mut root = BTreeMap::new();
        for (index, name) in param_names.iter().enumerate() {
            root.insert(name.clone(), VarLoc::Frame((index as i32) * 4));
        }
        Self {
            scopes: vec![root],
            next_local_slot: param_names.len(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn insert_current(&mut self, name: String, loc: VarLoc) {
        self.scopes
            .last_mut()
            .expect("scope stack is never empty")
            .insert(name, loc);
    }

    fn lookup(&self, name: &str) -> Option<VarLoc> {
        for scope in self.scopes.iter().rev() {
            if let Some(loc) = scope.get(name) {
                return Some(loc.clone());
            }
        }
        None
    }

    fn alloc_frame_slot(&mut self) -> i32 {
        let offset = (self.next_local_slot as i32) * 4;
        self.next_local_slot += 1;
        offset
    }
}

pub fn compile_source(source: &str) -> Result<AsmProgram, String> {
    let ast = parse_program(source)?;
    compile_program(&ast)
}

pub fn compile_program(ast: &Program) -> Result<AsmProgram, String> {
    let mut compiler = Compiler::new();
    compiler.collect_function_signatures(ast)?;
    compiler.compile_top_level(ast)?;
    compiler.compile_functions(ast)?;
    Ok(compiler.finish())
}

struct Compiler {
    program: AsmProgram,
    global_vars: BTreeMap<String, String>,
    function_sigs: BTreeMap<String, FunctionSig>,
    string_labels: BTreeMap<String, String>,
    next_label_id: usize,
    next_global_id: usize,
    next_string_id: usize,
    needs_print_int: bool,
    needs_print_pstr: bool,
    needs_print_value: bool,
    needs_read_line: bool,
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
        }
    }

    fn finish(mut self) -> AsmProgram {
        emit_runtime(
            &mut self.program,
            self.needs_print_int,
            self.needs_print_pstr,
            self.needs_print_value,
            self.needs_read_line,
        );
        self.program
    }

    fn collect_function_signatures(&mut self, ast: &Program) -> Result<(), String> {
        for form in &ast.forms {
            if let TopForm::Defun(defun) = form {
                if self.function_sigs.contains_key(&defun.name) {
                    return Err(format!("duplicate function definition: {}", defun.name));
                }
                if defun.params.len() > 8 {
                    return Err(format!(
                        "function '{}' has {} parameters, but milestone 6 supports at most 8",
                        defun.name,
                        defun.params.len()
                    ));
                }
                self.function_sigs.insert(
                    defun.name.clone(),
                    FunctionSig {
                        label: format!("fn_{}", sanitize(&defun.name)),
                        param_count: defun.params.len(),
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

        let local_slot_count = defun.params.len() + count_let_slots_in_body(&defun.body);
        let frame_bytes = (local_slot_count as i32) * 4;

        self.program.label(AsmSection::Text, &sig.label);
        self.emit_prologue(frame_bytes);

        let mut env = Env::function(&defun.params);

        for (index, _name) in defun.params.iter().enumerate() {
            let arg = arg_reg(index)?;
            let offset = (index as i32) * 4;
            self.emit_store_frame(arg, offset);
        }

        for expr in &defun.body {
            self.compile_expr(expr, Reg::A0, &mut env)?;
        }

        self.emit_epilogue(frame_bytes);
        Ok(())
    }

    fn compile_expr(&mut self, expr: &LExpr, target: Reg, env: &mut Env) -> Result<(), String> {
        match expr {
            LExpr::Setq { value, .. } if is_i64_expr(value) => {
                return Err("i64 variables are not supported yet in this milestone; use i64 expressions directly".to_string())
            }
            LExpr::Let { bindings, .. } if bindings.iter().any(|binding| is_i64_expr(&binding.value)) => {
                return Err("i64 let bindings are not supported yet in this milestone; use i64 expressions directly".to_string())
            }
            _ => {}
        }

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
                    "32-bit number literal out of range; wrap it as (i64 <number>)".to_string()
                })?;
                self.emit_load_imm(target, value)
            }
            LExpr::I64(_) => unreachable!("i64 expressions are handled before scalar codegen"),
            LExpr::Bool(value) => self.emit_load_imm(target, if *value { 1 } else { 0 }),
            LExpr::Nil => self.emit_load_imm(target, 0),
            LExpr::String(text) => {
                let label = self.intern_string(text);
                self.emit_load_addr(target, &label);
            }
            LExpr::Ident(name) => {
                let loc = self
                    .lookup_var(name, env)
                    .ok_or_else(|| format!("unknown variable: {name}"))?;
                self.emit_load_from_loc(target, &loc);
            }
            LExpr::Setq { name, value } => {
                self.compile_expr(value, target, env)?;
                let loc = if let Some(existing) = self.lookup_var(name, env) {
                    existing
                } else {
                    VarLoc::Global(self.define_global(name))
                };
                self.emit_store_to_loc(target, &loc);
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
                if let Some(const_value) = const_i64(value) {
                    self.emit_print_const_i64(const_value, target);
                    return Ok(());
                }
                if is_i64_expr(value) {
                    return Err("dynamic i64 print is not supported yet in this milestone; only constant i64 expressions can be printed".to_string());
                }
                let kind = infer_expr_kind(value);
                self.compile_expr(value, Reg::A0, env)?;
                let label = match kind {
                    ValueKind::Int => {
                        self.needs_print_int = true;
                        PRINT_INT_LABEL
                    }
                    ValueKind::String => {
                        self.needs_print_pstr = true;
                        PRINT_PSTR_LABEL
                    }
                    ValueKind::I64 => {
                        return Err("dynamic i64 print is not supported yet in this milestone; only constant i64 expressions can be printed".to_string())
                    }
                    ValueKind::Unknown => {
                        self.needs_print_value = true;
                        PRINT_VALUE_LABEL
                    }
                };
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
            LExpr::ReadChar => {
                load_mmio_base(&mut self.program, Reg::T6);
                self.program.emit_inst(
                    AsmSection::Text,
                    Instruction::Lw {
                        rd: target,
                        rs1: Reg::T6,
                        off: Expr::from_i32(4),
                    },
                );
                self.emit_load_imm(Reg::T5, 1);
                self.program.emit_inst(
                    AsmSection::Text,
                    Instruction::Sw {
                        rs2: Reg::T5,
                        rs1: Reg::T6,
                        off: Expr::from_i32(16),
                    },
                );
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

    fn compile_let(
        &mut self,
        bindings: &[Binding],
        body: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        let mut slots = Vec::new();
        for binding in bindings {
            let offset = env.alloc_frame_slot();
            slots.push((binding.name.clone(), offset, binding.value.clone()));
        }

        for (_name, offset, value) in &slots {
            self.compile_expr(value, target, env)?;
            self.emit_store_frame(target, *offset);
        }

        env.push_scope();
        for (name, offset, _) in &slots {
            env.insert_current(name.clone(), VarLoc::Frame(*offset));
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
            "print-str" => self.compile_print_str(args, target, env),
            other => Err(format!(
                "builtin '{other}' is reserved for a later milestone"
            )),
        }
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

        if args.len() != sig.param_count {
            return Err(format!(
                "function '{}' expects {} arguments, got {}",
                name,
                sig.param_count,
                args.len()
            ));
        }
        if args.len() > 8 {
            return Err(format!(
                "function '{}' call uses {} arguments, but milestone 6 supports at most 8",
                name,
                args.len()
            ));
        }

        for arg in args {
            self.compile_expr(arg, Reg::T0, env)?;
            self.push_reg(Reg::T0);
        }

        for index in (0..args.len()).rev() {
            self.pop_reg(arg_reg(index)?);
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

        self.compile_expr(&args[0], Reg::T0, env)?;
        self.push_reg(Reg::T0);
        self.compile_expr(&args[1], Reg::T1, env)?;
        self.pop_reg(Reg::T0);

        let label_true = self.next_label("cmp_true");
        let label_end = self.next_label("cmp_end");

        self.emit_load_imm(target, 0);
        match kind {
            CompareKind::Eq => self.emit_branch(BranchKind::Beq, Reg::T0, Reg::T1, &label_true),
            CompareKind::Ne => self.emit_branch(BranchKind::Bne, Reg::T0, Reg::T1, &label_true),
            CompareKind::Lt => self.emit_branch(BranchKind::Blt, Reg::T0, Reg::T1, &label_true),
            CompareKind::Le => self.emit_branch(BranchKind::Bge, Reg::T1, Reg::T0, &label_true),
            CompareKind::Gt => self.emit_branch(BranchKind::Blt, Reg::T1, Reg::T0, &label_true),
            CompareKind::Ge => self.emit_branch(BranchKind::Bge, Reg::T0, Reg::T1, &label_true),
        }
        self.emit_jump(&label_end);
        self.program.label(AsmSection::Text, &label_true);
        self.emit_load_imm(target, 1);
        self.program.label(AsmSection::Text, &label_end);
        Ok(())
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

    fn compile_print_str(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
        if args.len() != 1 {
            return Err(format!(
                "print-str expects exactly 1 argument, got {}",
                args.len()
            ));
        }

        self.compile_expr(&args[0], Reg::A0, env)?;
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

    fn lookup_var(&self, name: &str, env: &Env) -> Option<VarLoc> {
        env.lookup(name).or_else(|| {
            self.global_vars
                .get(name)
                .map(|label| VarLoc::Global(label.clone()))
        })
    }

    fn define_global(&mut self, name: &str) -> String {
        if let Some(existing) = self.global_vars.get(name) {
            return existing.clone();
        }

        let label = format!("__g_{}_{}", sanitize(name), self.next_global_id);
        self.next_global_id += 1;
        self.program.label(AsmSection::Data, &label);
        self.program.emit_data(AsmSection::Data, DataItem::Word(0));
        self.global_vars.insert(name.to_string(), label.clone());
        label
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

fn count_let_slots_in_body(body: &[LExpr]) -> usize {
    body.iter().map(count_let_slots_in_expr).sum()
}

fn count_let_slots_in_expr(expr: &LExpr) -> usize {
    match expr {
        LExpr::Number(_)
        | LExpr::I64(_)
        | LExpr::String(_)
        | LExpr::Bool(_)
        | LExpr::Nil
        | LExpr::Ident(_)
        | LExpr::ReadChar
        | LExpr::ReadLine
        | LExpr::Halt => 0,
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
            bindings.len()
                + bindings
                    .iter()
                    .map(|binding| count_let_slots_in_expr(&binding.value))
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
        LExpr::Print(value) => count_let_slots_in_expr(value),
        LExpr::Call { args, .. } => args.iter().map(count_let_slots_in_expr).sum(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Int,
    I64,
    String,
    Unknown,
}

fn infer_expr_kind(expr: &LExpr) -> ValueKind {
    match expr {
        LExpr::Number(_) | LExpr::Bool(_) | LExpr::Nil | LExpr::ReadChar | LExpr::Halt => {
            ValueKind::Int
        }
        LExpr::I64(_) => ValueKind::I64,
        LExpr::String(_) | LExpr::ReadLine => ValueKind::String,
        LExpr::Ident(_) => ValueKind::Unknown,
        LExpr::Setq { value, .. } => infer_expr_kind(value),
        LExpr::If {
            then_branch,
            else_branch,
            ..
        } => {
            let left = infer_expr_kind(then_branch);
            let right = infer_expr_kind(else_branch);
            if left == right {
                left
            } else {
                ValueKind::Unknown
            }
        }
        LExpr::Begin(items) => items
            .last()
            .map(infer_expr_kind)
            .unwrap_or(ValueKind::Unknown),
        LExpr::Let { body, .. } => body
            .last()
            .map(infer_expr_kind)
            .unwrap_or(ValueKind::Unknown),
        LExpr::Loop { finally, .. } => infer_expr_kind(finally),
        LExpr::Print(value) => infer_expr_kind(value),
        LExpr::Call { callee, args } => match callee {
            Callee::Builtin(name) => match name.as_str() {
                "print-str" => ValueKind::String,
                "strlen" | "strget" | "strset" | "=" | "!=" | "<" | "<=" | ">" | ">=" | "and"
                | "or" | "not" => ValueKind::Int,
                "+" | "-" | "*" | "/" | "%" | "bit-and" | "bit-or" | "bit-xor" | "shl" | "shr"
                | "sar" => {
                    if args
                        .iter()
                        .any(|arg| infer_expr_kind(arg) == ValueKind::I64)
                    {
                        ValueKind::I64
                    } else {
                        ValueKind::Int
                    }
                }
                _ => ValueKind::Unknown,
            },
            Callee::Ident(_) => ValueKind::Unknown,
        },
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
        LExpr::Number(_) | LExpr::I64(_) | LExpr::Bool(_) | LExpr::Nil => true,
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
        LExpr::I64(value) => const_i64(value),
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
        LExpr::I64(_) => true,
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
