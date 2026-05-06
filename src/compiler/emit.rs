use super::*;

impl Compiler {
    pub(super) fn lookup_var(&self, name: &str, env: &Env) -> Option<VarInfo> {
        env.lookup(name)
            .or_else(|| self.global_vars.get(name).cloned())
    }

    pub(super) fn define_global(&mut self, name: &str, kind: ValueKind) -> VarInfo {
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

    pub(super) fn compile_store_value(
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

    pub(super) fn emit_load_from_info_pair(&mut self, lo: Reg, info: &VarInfo) {
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

    pub(super) fn emit_store_info_pair(&mut self, lo: Reg, hi: Reg, info: &VarInfo) {
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

    pub(super) fn intern_string(&mut self, text: &str) -> String {
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

    pub(super) fn next_label(&mut self, prefix: &str) -> String {
        let label = format!("__{}_{}", prefix, self.next_label_id);
        self.next_label_id += 1;
        label
    }

    pub(super) fn interrupt_context_regs() -> [Reg; 30] {
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

    pub(super) fn emit_interrupt_context_save(&mut self) {
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

    pub(super) fn emit_interrupt_context_restore(&mut self) {
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

    pub(super) fn emit_prologue(&mut self, frame_bytes: i32) {
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

    pub(super) fn emit_epilogue(&mut self, frame_bytes: i32) {
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

    pub(super) fn emit_epilogue_without_return(&mut self, frame_bytes: i32) {
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

    pub(super) fn emit_load_imm(&mut self, rd: Reg, value: i32) {
        load_u32(&mut self.program, rd, value);
    }

    pub(super) fn emit_r(&mut self, op: AluRKind, rd: Reg, rs1: Reg, rs2: Reg) {
        self.program
            .emit_inst(AsmSection::Text, Instruction::AluR { op, rd, rs1, rs2 });
    }

    pub(super) fn emit_load_addr(&mut self, rd: Reg, label: &str) {
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

    pub(super) fn emit_load_word(&mut self, rd: Reg, label: &str) {
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

    pub(super) fn emit_store_word(&mut self, rs: Reg, label: &str) {
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

    pub(super) fn emit_load_frame(&mut self, rd: Reg, offset: i32) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Lw {
                rd,
                rs1: Reg::S1,
                off: Expr::from_i32(offset),
            },
        );
    }

    pub(super) fn emit_store_frame(&mut self, rs: Reg, offset: i32) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Sw {
                rs2: rs,
                rs1: Reg::S1,
                off: Expr::from_i32(offset),
            },
        );
    }

    pub(super) fn emit_load_from_loc(&mut self, rd: Reg, loc: &VarLoc) {
        match loc {
            VarLoc::Global(label) => self.emit_load_word(rd, label),
            VarLoc::Frame(offset) => self.emit_load_frame(rd, *offset),
        }
    }

    pub(super) fn emit_store_to_loc(&mut self, rs: Reg, loc: &VarLoc) {
        match loc {
            VarLoc::Global(label) => self.emit_store_word(rs, label),
            VarLoc::Frame(offset) => self.emit_store_frame(rs, *offset),
        }
    }

    pub(super) fn emit_branch(&mut self, op: BranchKind, rs1: Reg, rs2: Reg, label: &str) {
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

    pub(super) fn emit_jump(&mut self, label: &str) {
        self.program.emit_inst(
            AsmSection::Text,
            Instruction::Jal {
                rd: Reg::Zero,
                off: Expr::pcrel(label),
            },
        );
    }

    pub(super) fn push_reg(&mut self, reg: Reg) {
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

    pub(super) fn pop_reg(&mut self, reg: Reg) {
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
