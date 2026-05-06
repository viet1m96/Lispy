use super::*;

impl Compiler {
    pub(super) fn compile_i64_expr(&mut self, expr: &LExpr, env: &mut Env) -> Result<(), String> {
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
                TypeName::Array => Err("cannot use array cast in an i64 expression".to_string()),
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
                if let Some(const_value) = const_i64(value) {
                    self.emit_print_const_i64(const_value, Reg::A0);
                } else {
                    self.compile_i64_expr(value, env)?;
                    self.emit_print_dynamic_i64(Reg::A0);
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

    pub(super) fn emit_print_const_i64(&mut self, value: i64, target: Reg) {
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
