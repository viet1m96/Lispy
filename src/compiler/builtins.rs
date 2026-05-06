use super::*;

impl Compiler {
    pub(super) fn compile_builtin(
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

    pub(super) fn compile_fold_binary<F>(
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

    pub(super) fn compile_strlen(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
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

    pub(super) fn compile_strget(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
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

    pub(super) fn compile_strset(
        &mut self,
        args: &[LExpr],
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
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

    pub(super) fn emit_scale_string_index(&mut self, index_reg: Reg, out_reg: Reg) {
        self.emit_load_imm(Reg::T3, 2);
        self.emit_r(AluRKind::Sll, out_reg, index_reg, Reg::T3);
        self.emit_load_imm(Reg::T3, 4);
        self.emit_r(AluRKind::Add, out_reg, out_reg, Reg::T3);
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
        self.emit_array_bounds_check(Reg::T0, Reg::T1)?;
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
        self.emit_array_bounds_check(Reg::T0, Reg::T1)?;
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
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: Reg::T3,
                rs1: Reg::T1,
                off: Expr::from_i32(0),
            },
        );

        let label_lr_sizes_ok = self.next_label("array_vec_lr_sizes_ok");
        self.emit_branch(BranchKind::Beq, Reg::T2, Reg::T3, &label_lr_sizes_ok);
        self.emit_runtime_error("array size mismatch");
        self.program.label(AsmSection::Text, &label_lr_sizes_ok);

        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: Reg::T4,
                rs1: Reg::T5,
                off: Expr::from_i32(0),
            },
        );
        let label_dst_size_ok = self.next_label("array_vec_dst_size_ok");
        self.emit_branch(BranchKind::Beq, Reg::T2, Reg::T4, &label_dst_size_ok);
        self.emit_runtime_error("array destination size mismatch");
        self.program.label(AsmSection::Text, &label_dst_size_ok);

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
        let label_scalar_step = self.next_label("array_vec_scalar_step");
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
        self.program.label(AsmSection::Text, &label_scalar_step);
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
        let label_size_ok = self.next_label("array_size_ok");
        self.emit_branch(BranchKind::Bge, Reg::T0, Reg::Zero, &label_size_ok);
        self.emit_runtime_error("array size must be non-negative");
        self.program.label(AsmSection::Text, &label_size_ok);

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

    fn emit_array_bounds_check(&mut self, array_reg: Reg, index_reg: Reg) -> Result<(), String> {
        let label_non_negative = self.next_label("array_idx_non_negative");
        let label_in_bounds = self.next_label("array_idx_in_bounds");
        self.emit_branch(BranchKind::Bge, index_reg, Reg::Zero, &label_non_negative);
        self.emit_runtime_error("array index out of bounds");
        self.program.label(AsmSection::Text, &label_non_negative);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd: Reg::T4,
                rs1: array_reg,
                off: Expr::from_i32(0),
            },
        );
        self.emit_branch(BranchKind::Blt, index_reg, Reg::T4, &label_in_bounds);
        self.emit_runtime_error("array index out of bounds");
        self.program.label(AsmSection::Text, &label_in_bounds);
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

    fn emit_runtime_error(&mut self, message: &str) {
        let label = self.intern_string(message);
        self.needs_print_pstr = true;
        self.emit_load_addr(Reg::A0, &label);
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Jal {
                rd: Reg::Ra,
                off: Expr::pcrel(PRINT_PSTR_LABEL),
            },
        );
        self.program.emit_inst(AsmSection::Text, Instruction::Halt);
    }
}
