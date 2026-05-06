use std::collections::BTreeMap;

use crate::lisp::{Binding, Callee, Defun, Expr, Program, TopForm, TypeName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type {
    Int,
    I64,
    Bool,
    String,
    Array,
    Nil,
}

impl Type {
    fn from_ann(ann: TypeName) -> Self {
        match ann {
            TypeName::Int => Self::Int,
            TypeName::I64 => Self::I64,
            TypeName::Bool => Self::Bool,
            TypeName::String => Self::String,
            TypeName::Array => Self::Array,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Int => ":int",
            Self::I64 => ":i64",
            Self::Bool => ":bool",
            Self::String => ":string",
            Self::Array => ":array",
            Self::Nil => ":nil",
        }
    }

    fn is_numeric(self) -> bool {
        matches!(self, Self::Int | Self::I64)
    }

    fn is_truthy_compatible(self) -> bool {
        matches!(self, Self::Int | Self::I64 | Self::Bool | Self::Nil)
    }
}

#[derive(Debug, Clone)]
struct FunctionType {
    params: Vec<Type>,
    ret: Type,
}

#[derive(Debug, Clone, Default)]
struct TypeEnv {
    scopes: Vec<BTreeMap<String, Type>>,
}

impl TypeEnv {
    fn new() -> Self {
        Self {
            scopes: vec![BTreeMap::new()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn insert(&mut self, name: String, ty: Type) -> Result<(), String> {
        let scope = self.scopes.last_mut().expect("scope exists");
        if let Some(existing) = scope.get(&name).copied() {
            if existing != ty {
                return Err(format!(
                    "cannot redeclare '{}' as {}; existing type is {}",
                    name,
                    ty.name(),
                    existing.name()
                ));
            }
        }
        scope.insert(name, ty);
        Ok(())
    }

    fn assign_or_check(&mut self, name: &str, ty: Type) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(existing) = scope.get_mut(name) {
                if *existing != ty {
                    return Err(format!(
                        "type mismatch for '{}': expected {}, got {}",
                        name,
                        existing.name(),
                        ty.name()
                    ));
                }
                return Ok(());
            }
        }
        self.insert(name.to_string(), ty)
    }

    fn lookup(&self, name: &str) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(*ty);
            }
        }
        None
    }
}

pub fn typecheck_program(program: &Program) -> Result<(), String> {
    let mut checker = TypeChecker::new();
    checker.collect_function_headers(program)?;
    checker.check_functions(program)?;
    checker.check_top_level(program)
}

struct TypeChecker {
    globals: BTreeMap<String, Type>,
    functions: BTreeMap<String, FunctionType>,
}

impl TypeChecker {
    fn new() -> Self {
        Self {
            globals: BTreeMap::new(),
            functions: BTreeMap::new(),
        }
    }

    fn collect_function_headers(&mut self, program: &Program) -> Result<(), String> {
        for form in &program.forms {
            let TopForm::Defun(defun) = form else {
                continue;
            };
            if self.functions.contains_key(&defun.name) {
                return Err(format!("duplicate function definition: {}", defun.name));
            }

            let params = defun
                .params
                .iter()
                .map(|param| Type::from_ann(param.type_ann))
                .collect();
            let ret = Type::from_ann(defun.return_type);

            self.functions
                .insert(defun.name.clone(), FunctionType { params, ret });
        }
        Ok(())
    }

    fn check_functions(&mut self, program: &Program) -> Result<(), String> {
        for form in &program.forms {
            let TopForm::Defun(defun) = form else {
                continue;
            };
            self.check_function(defun)?;
        }
        Ok(())
    }

    fn check_function(&mut self, defun: &Defun) -> Result<Type, String> {
        let sig = self
            .functions
            .get(&defun.name)
            .ok_or_else(|| format!("missing function signature: {}", defun.name))?
            .clone();

        let mut env = TypeEnv::new();
        for (param, ty) in defun.params.iter().zip(sig.params.iter()) {
            env.insert(param.name.clone(), *ty)?;
        }

        let expected_return = sig.ret;

        let mut last_ty = Type::Nil;
        for (index, expr) in defun.body.iter().enumerate() {
            let is_last = index + 1 == defun.body.len();
            last_ty = self.infer_expr(
                expr,
                &mut env,
                if is_last { Some(expected_return) } else { None },
            )?;
        }

        if !compatible_exact_or_literal_context(last_ty, expected_return) {
            return Err(format!(
                "return type mismatch in function '{}': expected {}, got {}",
                defun.name,
                expected_return.name(),
                last_ty.name()
            ));
        }
        Ok(expected_return)
    }

    fn check_top_level(&mut self, program: &Program) -> Result<(), String> {
        let mut env = TypeEnv::new();
        for (name, ty) in &self.globals {
            env.insert(name.clone(), *ty)?;
        }

        for form in &program.forms {
            if let TopForm::Expr(expr) = form {
                self.infer_expr(expr, &mut env, None)?;
            }
        }
        Ok(())
    }

    fn infer_expr(
        &mut self,
        expr: &Expr,
        env: &mut TypeEnv,
        expected: Option<Type>,
    ) -> Result<Type, String> {
        match expr {
            Expr::Number(value) => {
                let ty = expected.filter(|ty| ty.is_numeric()).unwrap_or_else(|| {
                    if i32::try_from(*value).is_ok() {
                        Type::Int
                    } else {
                        Type::I64
                    }
                });
                Ok(ty)
            }
            Expr::Cast { target_type, value } => {
                let target = Type::from_ann(*target_type);
                self.ensure_cast_allowed(value, env, target)?;
                Ok(target)
            }
            Expr::String(_) => Ok(Type::String),
            Expr::Bool(_) => Ok(Type::Bool),
            Expr::Nil => Ok(Type::Nil),
            Expr::Ident(name) => env
                .lookup(name)
                .or_else(|| self.globals.get(name).copied())
                .ok_or_else(|| format!("unknown variable: {name}")),
            Expr::Setq {
                name,
                type_ann,
                value,
            } => self.check_setq(name, *type_ann, value, env),
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.infer_expr(cond, env, Some(Type::Bool))?;
                if !cond_ty.is_truthy_compatible() {
                    return Err(format!(
                        "if condition must be truthy-compatible, got {}",
                        cond_ty.name()
                    ));
                }

                if let Some(expected_ty) = expected {
                    let then_ty = self.infer_expr(then_branch, env, Some(expected_ty))?;
                    let else_ty = self.infer_expr(else_branch, env, Some(expected_ty))?;
                    self.require_type(then_branch, then_ty, expected_ty, "then branch")?;
                    self.require_type(else_branch, else_ty, expected_ty, "else branch")?;
                    Ok(expected_ty)
                } else {
                    let then_ty = self.infer_expr(then_branch, env, None)?;
                    let else_ty = self.infer_expr(else_branch, env, None)?;
                    if then_ty == else_ty {
                        Ok(then_ty)
                    } else if is_numeric_literal(then_branch) && else_ty.is_numeric() {
                        let coerced = self.infer_expr(then_branch, env, Some(else_ty))?;
                        self.require_type(then_branch, coerced, else_ty, "then branch")?;
                        Ok(else_ty)
                    } else if is_numeric_literal(else_branch) && then_ty.is_numeric() {
                        let coerced = self.infer_expr(else_branch, env, Some(then_ty))?;
                        self.require_type(else_branch, coerced, then_ty, "else branch")?;
                        Ok(then_ty)
                    } else {
                        Err(format!(
                            "type mismatch in if branches: then has {}, else has {}",
                            then_ty.name(),
                            else_ty.name()
                        ))
                    }
                }
            }
            Expr::Begin(items) => {
                let mut last = Type::Nil;
                for (index, item) in items.iter().enumerate() {
                    let is_last = index + 1 == items.len();
                    last = self.infer_expr(item, env, if is_last { expected } else { None })?;
                }
                Ok(last)
            }
            Expr::Let { bindings, body } => {
                env.push_scope();
                for binding in bindings {
                    self.check_binding(binding, env)?;
                }

                let mut last = Type::Nil;
                for (index, expr) in body.iter().enumerate() {
                    let is_last = index + 1 == body.len();
                    last = self.infer_expr(expr, env, if is_last { expected } else { None })?;
                }
                env.pop_scope();
                Ok(last)
            }
            Expr::Loop {
                cond,
                body,
                finally,
            } => {
                let cond_ty = self.infer_expr(cond, env, Some(Type::Bool))?;
                if !cond_ty.is_truthy_compatible() {
                    return Err(format!(
                        "loop condition must be truthy-compatible, got {}",
                        cond_ty.name()
                    ));
                }
                for expr in body {
                    self.infer_expr(expr, env, None)?;
                }
                self.infer_expr(finally, env, expected)
            }
            Expr::Print(value) => self.infer_expr(value, env, expected),
            Expr::PrintStr(value) => {
                let ty = self.infer_expr(value, env, Some(Type::String))?;
                self.require_type(value, ty, Type::String, "print-str argument")?;
                Ok(Type::String)
            }
            Expr::ReadChar => Ok(Type::Int),
            Expr::ReadLine => Ok(Type::String),
            Expr::ReadInputData | Expr::HandlerDone => Ok(Type::Int),
            Expr::Halt => Ok(Type::Nil),
            Expr::Call { callee, args } => self.infer_call(callee, args, env, expected),
        }
    }

    fn check_setq(
        &mut self,
        name: &str,
        type_ann: TypeName,
        value: &Expr,
        env: &mut TypeEnv,
    ) -> Result<Type, String> {
        let declared = Type::from_ann(type_ann);

        let existing = env.lookup(name).or_else(|| self.globals.get(name).copied());
        if let Some(existing) = existing {
            if existing != declared {
                return Err(format!(
                    "type mismatch for '{}': existing type is {}, but setq declares {}",
                    name,
                    existing.name(),
                    declared.name()
                ));
            }
        }

        let value_ty = self.infer_expr(value, env, Some(declared))?;
        self.require_type(value, value_ty, declared, "setq")?;

        if env.lookup(name).is_none() && !self.globals.contains_key(name) {
            self.globals.insert(name.to_string(), declared);
        }
        env.assign_or_check(name, declared)?;
        Ok(declared)
    }

    fn check_binding(&mut self, binding: &Binding, env: &mut TypeEnv) -> Result<Type, String> {
        let declared = Type::from_ann(binding.type_ann);
        let value_ty = self.infer_expr(&binding.value, env, Some(declared))?;
        self.require_type(&binding.value, value_ty, declared, "let binding")?;
        env.insert(binding.name.clone(), declared)?;
        Ok(declared)
    }

    fn infer_call(
        &mut self,
        callee: &Callee,
        args: &[Expr],
        env: &mut TypeEnv,
        expected: Option<Type>,
    ) -> Result<Type, String> {
        match callee {
            Callee::Ident(name) => {
                let sig = self
                    .functions
                    .get(name)
                    .ok_or_else(|| format!("unknown function: {name}"))?
                    .clone();

                if args.len() != sig.params.len() {
                    return Err(format!(
                        "function '{}' expects {} arguments, got {}",
                        name,
                        sig.params.len(),
                        args.len()
                    ));
                }

                for (index, (arg, expected_ty)) in args.iter().zip(sig.params.iter()).enumerate() {
                    let actual = self.infer_expr(arg, env, Some(*expected_ty))?;
                    self.require_type(
                        arg,
                        actual,
                        *expected_ty,
                        &format!("argument {} of function '{}'", index + 1, name),
                    )?;
                }

                if let Some(expected_ty) = expected {
                    self.require_named(sig.ret, expected_ty, &format!("call to '{}'", name))?;
                }
                Ok(sig.ret)
            }
            Callee::Builtin(name) => self.infer_builtin(name, args, env, expected),
        }
    }

    fn infer_builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &mut TypeEnv,
        expected: Option<Type>,
    ) -> Result<Type, String> {
        match name {
            "+" | "-" | "*" | "/" | "%" | "bit-and" | "bit-or" | "bit-xor" => {
                self.numeric_operands(name, args, env, expected)
            }
            "shl" | "shr" | "sar" => {
                if args.len() != 2 {
                    return Err(format!("builtin '{}' expects exactly 2 arguments", name));
                }
                let lhs_ty = self.numeric_operands(name, &args[0..1], env, expected)?;
                let rhs_ty = self.infer_expr(&args[1], env, Some(Type::Int))?;
                self.require_type(&args[1], rhs_ty, Type::Int, "shift amount")?;
                Ok(lhs_ty)
            }
            "=" | "!=" => self.compare_operands(name, args, env, true),
            "<" | "<=" | ">" | ">=" => self.compare_operands(name, args, env, false),
            "and" | "or" => {
                for arg in args {
                    let ty = self.infer_expr(arg, env, Some(Type::Bool))?;
                    if !ty.is_truthy_compatible() {
                        return Err(format!(
                            "builtin '{}' expects truthy-compatible arguments, got {}",
                            name,
                            ty.name()
                        ));
                    }
                }
                Ok(Type::Bool)
            }
            "not" => {
                if args.len() != 1 {
                    return Err("not expects exactly 1 argument".to_string());
                }
                let ty = self.infer_expr(&args[0], env, Some(Type::Bool))?;
                if !ty.is_truthy_compatible() {
                    return Err(format!(
                        "not expects truthy-compatible argument, got {}",
                        ty.name()
                    ));
                }
                Ok(Type::Bool)
            }
            "strlen" => {
                if args.len() != 1 {
                    return Err("strlen expects exactly 1 argument".to_string());
                }
                let ty = self.infer_expr(&args[0], env, Some(Type::String))?;
                self.require_type(&args[0], ty, Type::String, "strlen argument")?;
                Ok(Type::Int)
            }
            "strget" => {
                if args.len() != 2 {
                    return Err("strget expects exactly 2 arguments".to_string());
                }
                let s = self.infer_expr(&args[0], env, Some(Type::String))?;
                let i = self.infer_expr(&args[1], env, Some(Type::Int))?;
                self.require_type(&args[0], s, Type::String, "strget string")?;
                self.require_type(&args[1], i, Type::Int, "strget index")?;
                Ok(Type::Int)
            }
            "strset" => {
                if args.len() != 3 {
                    return Err("strset expects exactly 3 arguments".to_string());
                }
                let s = self.infer_expr(&args[0], env, Some(Type::String))?;
                let i = self.infer_expr(&args[1], env, Some(Type::Int))?;
                let ch = self.infer_expr(&args[2], env, Some(Type::Int))?;
                self.require_type(&args[0], s, Type::String, "strset string")?;
                self.require_type(&args[1], i, Type::Int, "strset index")?;
                self.require_type(&args[2], ch, Type::Int, "strset value")?;
                Ok(Type::Int)
            }
            "array" => {
                if args.len() != 1 {
                    return Err("array expects exactly 1 size argument".to_string());
                }
                let size = self.infer_expr(&args[0], env, Some(Type::Int))?;
                self.require_type(&args[0], size, Type::Int, "array size")?;
                Ok(Type::Array)
            }
            "array-get" => {
                if args.len() != 2 {
                    return Err("array-get expects exactly 2 arguments".to_string());
                }
                let arr = self.infer_expr(&args[0], env, Some(Type::Array))?;
                let index = self.infer_expr(&args[1], env, Some(Type::Int))?;
                self.require_type(&args[0], arr, Type::Array, "array-get array")?;
                self.require_type(&args[1], index, Type::Int, "array-get index")?;
                Ok(Type::Int)
            }
            "array-set" => {
                if args.len() != 3 {
                    return Err("array-set expects exactly 3 arguments".to_string());
                }
                let arr = self.infer_expr(&args[0], env, Some(Type::Array))?;
                let index = self.infer_expr(&args[1], env, Some(Type::Int))?;
                let value = self.infer_expr(&args[2], env, Some(Type::Int))?;
                self.require_type(&args[0], arr, Type::Array, "array-set array")?;
                self.require_type(&args[1], index, Type::Int, "array-set index")?;
                self.require_type(&args[2], value, Type::Int, "array-set value")?;
                Ok(Type::Int)
            }
            "array-size" => {
                if args.len() != 1 {
                    return Err("array-size expects exactly 1 argument".to_string());
                }
                let arr = self.infer_expr(&args[0], env, Some(Type::Array))?;
                self.require_type(&args[0], arr, Type::Array, "array-size argument")?;
                Ok(Type::Int)
            }
            "vadd" | "vsub" | "vmul" | "vdiv" | "vcmp" => {
                if args.len() != 3 {
                    return Err(format!(
                        "builtin '{}' expects exactly 3 array arguments: destination, left, right",
                        name
                    ));
                }
                let dst = self.infer_expr(&args[0], env, Some(Type::Array))?;
                let left = self.infer_expr(&args[1], env, Some(Type::Array))?;
                let right = self.infer_expr(&args[2], env, Some(Type::Array))?;
                self.require_type(
                    &args[0],
                    dst,
                    Type::Array,
                    &format!("builtin '{}' destination array", name),
                )?;
                self.require_type(
                    &args[1],
                    left,
                    Type::Array,
                    &format!("builtin '{}' left array", name),
                )?;
                self.require_type(
                    &args[2],
                    right,
                    Type::Array,
                    &format!("builtin '{}' right array", name),
                )?;
                Ok(Type::Array)
            }
            other => Err(format!("unknown builtin '{}'", other)),
        }
    }

    fn numeric_operands(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &mut TypeEnv,
        expected: Option<Type>,
    ) -> Result<Type, String> {
        if args.is_empty() {
            return Err(format!("builtin '{}' expects at least 1 argument", name));
        }
        if matches!(name, "/" | "%") && args.len() != 2 {
            return Err(format!("builtin '{}' expects exactly 2 arguments", name));
        }

        let mut base = expected.filter(|ty| ty.is_numeric());
        for arg in args {
            if is_numeric_literal(arg) {
                continue;
            }
            let ty = self.infer_expr(arg, env, None)?;
            if !ty.is_numeric() {
                return Err(format!(
                    "builtin '{}' expects numeric arguments, got {}",
                    name,
                    ty.name()
                ));
            }
            match base {
                None => base = Some(ty),
                Some(existing) if existing == ty => {}
                Some(existing) => {
                    return Err(format!(
                        "type mismatch in '{}': expected operands of the same numeric type, got {} and {}",
                        name,
                        existing.name(),
                        ty.name()
                    ));
                }
            }
        }

        let base = base.unwrap_or(Type::Int);
        if base == Type::I64
            && matches!(
                name,
                "/" | "%" | "bit-and" | "bit-or" | "bit-xor" | "shl" | "shr" | "sar"
            )
        {
            return Err(format!(
                "dynamic :i64 builtin '{}' is allowed only when the expression can be folded at compile time; use :int operands for runtime evaluation",
                name
            ));
        }
        for arg in args {
            let ty = self.infer_expr(arg, env, Some(base))?;
            self.require_type(arg, ty, base, &format!("builtin '{}'", name))?;
        }
        Ok(base)
    }

    fn compare_operands(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &mut TypeEnv,
        allow_string: bool,
    ) -> Result<Type, String> {
        if args.len() != 2 {
            return Err(format!("comparison '{}' expects exactly 2 arguments", name));
        }

        let left = self.infer_expr(&args[0], env, None)?;
        let base = if is_numeric_literal(&args[0]) {
            let right_hint = self.infer_expr(&args[1], env, None)?;
            if right_hint.is_numeric() {
                right_hint
            } else {
                left
            }
        } else {
            left
        };

        if base == Type::String {
            if !allow_string {
                return Err(format!(
                    "comparison '{}' requires numeric operands; string operands are invalid",
                    name
                ));
            }
            let right = self.infer_expr(&args[1], env, Some(Type::String))?;
            self.require_type(
                &args[1],
                right,
                Type::String,
                &format!("comparison '{}'", name),
            )?;
            return Ok(Type::Bool);
        }

        if !base.is_numeric() && base != Type::Bool {
            return Err(format!(
                "comparison '{}' expects numeric/bool operands, got {}",
                name,
                base.name()
            ));
        }

        let right = self.infer_expr(&args[1], env, Some(base))?;
        self.require_type(&args[1], right, base, &format!("comparison '{}'", name))?;
        Ok(Type::Bool)
    }

    fn ensure_cast_allowed(
        &mut self,
        value: &Expr,
        env: &mut TypeEnv,
        target: Type,
    ) -> Result<(), String> {
        let source = self.infer_expr(value, env, None)?;
        let ok = match target {
            Type::Int => matches!(source, Type::Int | Type::I64 | Type::Bool | Type::Nil),
            Type::I64 => matches!(source, Type::Int | Type::I64 | Type::Bool | Type::Nil),
            Type::Bool => matches!(source, Type::Int | Type::I64 | Type::Bool | Type::Nil),
            Type::String => source == Type::String,
            Type::Array => source == Type::Array,
            Type::Nil => source == Type::Nil,
        };
        if ok {
            Ok(())
        } else {
            Err(format!(
                "cannot cast from {} to {}",
                source.name(),
                target.name()
            ))
        }
    }

    fn require_type(
        &self,
        expr: &Expr,
        actual: Type,
        expected: Type,
        context: &str,
    ) -> Result<(), String> {
        if actual == expected {
            return Ok(());
        }

        if is_numeric_literal(expr) && expected.is_numeric() && actual.is_numeric() {
            return Ok(());
        }
        Err(format!(
            "type mismatch in {context}: expected {}, got {}",
            expected.name(),
            actual.name()
        ))
    }

    fn require_named(&self, actual: Type, expected: Type, context: &str) -> Result<(), String> {
        if actual == expected {
            Ok(())
        } else {
            Err(format!(
                "type mismatch in {context}: expected {}, got {}",
                expected.name(),
                actual.name()
            ))
        }
    }
}

fn is_numeric_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Number(_) | Expr::Bool(_) | Expr::Nil)
}

fn compatible_exact_or_literal_context(actual: Type, expected: Type) -> bool {
    actual == expected
}
