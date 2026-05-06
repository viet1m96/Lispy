use super::*;

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
    pub(super) fn infer_expr_kind_scoped(&self, expr: &LExpr, env: &Env) -> ValueKind {
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
                    "strlen" | "strget" | "strset" | "array-get" | "array-set" | "array-size"
                    | "=" | "!=" | "<" | "<=" | ">" | ">=" | "and" | "or" | "not" => ValueKind::Int,
                    "array" | "vadd" | "vsub" | "vmul" | "vdiv" | "vcmp" => ValueKind::Array,
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

    pub(super) fn infer_expr_kind(&self, expr: &LExpr) -> ValueKind {
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
                    "strlen" | "strget" | "strset" | "array-get" | "array-set" | "array-size"
                    | "=" | "!=" | "<" | "<=" | ">" | ">=" | "and" | "or" | "not" => ValueKind::Int,
                    "array" | "vadd" | "vsub" | "vmul" | "vdiv" | "vcmp" => ValueKind::Array,
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

pub(super) fn expr_guarantees_halt(expr: &LExpr) -> bool {
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

pub(super) fn sanitize(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

pub(super) fn is_foldable_expr(expr: &LExpr) -> bool {
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

pub(super) fn const_i32(expr: &LExpr) -> Option<i32> {
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
            target_type: TypeName::String | TypeName::Array,
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

pub(super) fn const_i64(expr: &LExpr) -> Option<i64> {
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
            target_type: TypeName::String | TypeName::Array,
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

pub(super) fn is_i64_expr(expr: &LExpr) -> bool {
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
