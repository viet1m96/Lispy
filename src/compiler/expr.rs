use super::*;

impl Compiler {
    pub(super) fn compile_expr(
        &mut self,
        expr: &LExpr,
        target: Reg,
        env: &mut Env,
    ) -> Result<(), String> {
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
                return Err(
                    "dynamic :i64 expression must be compiled through the i64 lowering path"
                        .to_string(),
                );
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
                TypeName::String | TypeName::Array => self.compile_expr(value, target, env)?,
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
                        self.emit_print_dynamic_i64(target);
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
                    ValueKind::Array => {
                        self.needs_print_int = true;
                        PRINT_INT_LABEL
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
