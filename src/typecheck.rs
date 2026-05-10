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

        let mut last_ty = Type::Nil;
        for expr in &defun.body {
            last_ty = self.expr_type(expr, &mut env)?;
        }

        self.require_assignable(
            last_ty,
            sig.ret,
            &format!("return type of '{}'", defun.name),
        )?;
        Ok(sig.ret)
    }

    fn check_top_level(&mut self, program: &Program) -> Result<(), String> {
        let mut env = TypeEnv::new();
        for (name, ty) in &self.globals {
            env.insert(name.clone(), *ty)?;
        }

        for form in &program.forms {
            if let TopForm::Expr(expr) = form {
                self.expr_type(expr, &mut env)?;
            }
        }
        Ok(())
    }

    fn expr_type(&mut self, expr: &Expr, env: &mut TypeEnv) -> Result<Type, String> {
        match expr {
            Expr::Number(value) => Ok(if i32::try_from(*value).is_ok() {
                Type::Int
            } else {
                Type::I64
            }),
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
                let cond_ty = self.expr_type(cond, env)?;
                self.require_truthy(cond_ty, "if condition")?;
                let then_ty = self.expr_type(then_branch, env)?;
                let else_ty = self.expr_type(else_branch, env)?;
                merge_branch_types(then_ty, else_ty)
            }
            Expr::Begin(items) => {
                let mut last = Type::Nil;
                for item in items {
                    last = self.expr_type(item, env)?;
                }
                Ok(last)
            }
            Expr::Let { bindings, body } => {
                env.push_scope();
                for binding in bindings {
                    self.check_binding(binding, env)?;
                }
                let mut last = Type::Nil;
                for expr in body {
                    last = self.expr_type(expr, env)?;
                }
                env.pop_scope();
                Ok(last)
            }
            Expr::Loop {
                cond,
                body,
                finally,
            } => {
                let cond_ty = self.expr_type(cond, env)?;
                self.require_truthy(cond_ty, "loop condition")?;
                for expr in body {
                    self.expr_type(expr, env)?;
                }
                self.expr_type(finally, env)
            }
            Expr::Print(value) => self.expr_type(value, env),
            Expr::PrintStr(value) => {
                let ty = self.expr_type(value, env)?;
                self.require_assignable(ty, Type::String, "print-str argument")?;
                Ok(Type::String)
            }
            Expr::ReadChar => Ok(Type::Int),
            Expr::ReadLine => Ok(Type::String),
            Expr::ReadInputData | Expr::HandlerDone => Ok(Type::Int),
            Expr::Halt => Ok(Type::Nil),
            Expr::Call { callee, args } => self.call_type(callee, args, env),
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

        if let Some(existing) = env.lookup(name).or_else(|| self.globals.get(name).copied()) {
            if existing != declared {
                return Err(format!(
                    "type mismatch for '{}': existing type is {}, but setq declares {}",
                    name,
                    existing.name(),
                    declared.name()
                ));
            }
        }

        let actual = self.expr_type(value, env)?;
        self.require_assignable(actual, declared, "setq")?;

        if env.lookup(name).is_none() && !self.globals.contains_key(name) {
            self.globals.insert(name.to_string(), declared);
        }
        env.assign_or_check(name, declared)?;
        Ok(declared)
    }

    fn check_binding(&mut self, binding: &Binding, env: &mut TypeEnv) -> Result<Type, String> {
        let declared = Type::from_ann(binding.type_ann);
        let actual = self.expr_type(&binding.value, env)?;
        self.require_assignable(actual, declared, "let binding")?;
        env.insert(binding.name.clone(), declared)?;
        Ok(declared)
    }

    fn call_type(
        &mut self,
        callee: &Callee,
        args: &[Expr],
        env: &mut TypeEnv,
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
                    let actual = self.expr_type(arg, env)?;
                    self.require_assignable(
                        actual,
                        *expected_ty,
                        &format!("argument {} of function '{}'", index + 1, name),
                    )?;
                }

                Ok(sig.ret)
            }
            Callee::Builtin(name) => self.builtin_type(name, args, env),
        }
    }

    fn builtin_type(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &mut TypeEnv,
    ) -> Result<Type, String> {
        match name {
            "+" | "-" | "*" | "/" | "%" | "bit-and" | "bit-or" | "bit-xor" => {
                self.numeric_operands(name, args, env)
            }
            "shl" | "shr" | "sar" => {
                if args.len() != 2 {
                    return Err(format!("builtin '{}' expects exactly 2 arguments", name));
                }
                let lhs_ty = self.numeric_operands(name, &args[0..1], env)?;
                let rhs_ty = self.expr_type(&args[1], env)?;
                self.require_assignable(rhs_ty, Type::Int, "shift amount")?;
                Ok(lhs_ty)
            }
            "=" | "!=" => self.compare_operands(name, args, env, true),
            "<" | "<=" | ">" | ">=" => self.compare_operands(name, args, env, false),
            "and" | "or" => {
                for arg in args {
                    let ty = self.expr_type(arg, env)?;
                    self.require_truthy(ty, &format!("builtin '{}' argument", name))?;
                }
                Ok(Type::Bool)
            }
            "not" => {
                if args.len() != 1 {
                    return Err("not expects exactly 1 argument".to_string());
                }
                let ty = self.expr_type(&args[0], env)?;
                self.require_truthy(ty, "not argument")?;
                Ok(Type::Bool)
            }
            "array" => {
                if args.len() != 1 {
                    return Err("array expects exactly 1 size argument".to_string());
                }
                let size = self.expr_type(&args[0], env)?;
                self.require_assignable(size, Type::Int, "array size")?;
                Ok(Type::Array)
            }
            "array-get" => {
                if args.len() != 2 {
                    return Err("array-get expects exactly 2 arguments".to_string());
                }
                let arr = self.expr_type(&args[0], env)?;
                let index = self.expr_type(&args[1], env)?;
                self.require_assignable(arr, Type::Array, "array-get array")?;
                self.require_assignable(index, Type::Int, "array-get index")?;
                Ok(Type::Int)
            }
            "array-set" => {
                if args.len() != 3 {
                    return Err("array-set expects exactly 3 arguments".to_string());
                }
                let arr = self.expr_type(&args[0], env)?;
                let index = self.expr_type(&args[1], env)?;
                let value = self.expr_type(&args[2], env)?;
                self.require_assignable(arr, Type::Array, "array-set array")?;
                self.require_assignable(index, Type::Int, "array-set index")?;
                self.require_assignable(value, Type::Int, "array-set value")?;
                Ok(Type::Int)
            }
            "array-size" => {
                if args.len() != 1 {
                    return Err("array-size expects exactly 1 argument".to_string());
                }
                let arr = self.expr_type(&args[0], env)?;
                self.require_assignable(arr, Type::Array, "array-size argument")?;
                Ok(Type::Int)
            }
            "vadd" | "vsub" | "vmul" | "vdiv" | "vcmp" => {
                if args.len() != 3 {
                    return Err(format!(
                        "builtin '{}' expects exactly 3 array arguments: destination, left, right",
                        name
                    ));
                }
                let dst = self.expr_type(&args[0], env)?;
                let left = self.expr_type(&args[1], env)?;
                let right = self.expr_type(&args[2], env)?;
                self.require_assignable(
                    dst,
                    Type::Array,
                    &format!("builtin '{}' destination array", name),
                )?;
                self.require_assignable(
                    left,
                    Type::Array,
                    &format!("builtin '{}' left array", name),
                )?;
                self.require_assignable(
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
    ) -> Result<Type, String> {
        if args.is_empty() {
            return Err(format!("builtin '{}' expects at least 1 argument", name));
        }
        if matches!(name, "/" | "%") && args.len() != 2 {
            return Err(format!("builtin '{}' expects exactly 2 arguments", name));
        }

        let mut result = Type::Int;
        for arg in args {
            let ty = self.expr_type(arg, env)?;
            if !ty.is_numeric() {
                return Err(format!(
                    "builtin '{}' expects numeric arguments, got {}",
                    name,
                    ty.name()
                ));
            }
            if ty == Type::I64 {
                result = Type::I64;
            }
        }

        if result == Type::I64
            && matches!(
                name,
                "/" | "%" | "bit-and" | "bit-or" | "bit-xor" | "shl" | "shr" | "sar"
            )
        {
            return Err(format!(
                "dynamic :i64 builtin '{}' is not supported; use :int operands for runtime evaluation",
                name
            ));
        }

        Ok(result)
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

        let left = self.expr_type(&args[0], env)?;
        let right = self.expr_type(&args[1], env)?;

        if left == Type::String || right == Type::String {
            if allow_string && left == Type::String && right == Type::String {
                return Ok(Type::Bool);
            }
            return Err(format!(
                "comparison '{}' requires numeric operands; string operands are invalid",
                name
            ));
        }

        if left.is_numeric() && right.is_numeric() {
            return Ok(Type::Bool);
        }
        if left == Type::Bool && right == Type::Bool {
            return Ok(Type::Bool);
        }

        Err(format!(
            "comparison '{}' expects compatible operands, got {} and {}",
            name,
            left.name(),
            right.name()
        ))
    }

    fn ensure_cast_allowed(
        &mut self,
        value: &Expr,
        env: &mut TypeEnv,
        target: Type,
    ) -> Result<(), String> {
        let source = self.expr_type(value, env)?;
        let ok = match target {
            Type::Int | Type::I64 => {
                matches!(source, Type::Int | Type::I64 | Type::Bool | Type::Nil)
            }
            Type::Bool | Type::String | Type::Array | Type::Nil => false,
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

    fn require_assignable(
        &self,
        actual: Type,
        expected: Type,
        context: &str,
    ) -> Result<(), String> {
        if is_assignable(actual, expected) {
            Ok(())
        } else {
            Err(format!(
                "type mismatch in {context}: expected {}, got {}",
                expected.name(),
                actual.name()
            ))
        }
    }

    fn require_truthy(&self, ty: Type, context: &str) -> Result<(), String> {
        if ty.is_truthy_compatible() {
            Ok(())
        } else {
            Err(format!(
                "{context} must be truthy-compatible, got {}",
                ty.name()
            ))
        }
    }
}

fn is_assignable(actual: Type, expected: Type) -> bool {
    actual == expected || (actual == Type::Int && expected == Type::I64)
}

fn merge_branch_types(left: Type, right: Type) -> Result<Type, String> {
    if left == right {
        Ok(left)
    } else if left.is_numeric() && right.is_numeric() {
        Ok(if left == Type::I64 || right == Type::I64 {
            Type::I64
        } else {
            Type::Int
        })
    } else {
        Err(format!(
            "type mismatch in if branches: then has {}, else has {}",
            left.name(),
            right.name()
        ))
    }
}
