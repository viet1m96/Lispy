use super::*;

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

    pub(super) fn compile_read_input_data(
        &mut self,
        args: &[LExpr],
        target: Reg,
    ) -> Result<(), String> {
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

    pub(super) fn compile_handler_done(
        &mut self,
        args: &[LExpr],
        target: Reg,
    ) -> Result<(), String> {
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
}
