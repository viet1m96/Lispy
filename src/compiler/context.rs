use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueKind {
    Int,
    I64,
    String,
    Array,
    Unknown,
}

impl ValueKind {
    pub(super) fn from_type_name<T: std::borrow::Borrow<TypeName>>(ty: T) -> Self {
        match *ty.borrow() {
            TypeName::Int => Self::Int,
            TypeName::I64 => Self::I64,
            TypeName::Bool => Self::Int,
            TypeName::String => Self::String,
            TypeName::Array => Self::Array,
        }
    }

    pub(super) fn width_words(self) -> usize {
        match self {
            Self::I64 => 2,
            Self::Int | Self::String | Self::Array | Self::Unknown => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct FunctionSig {
    pub(super) label: String,
    pub(super) param_kinds: Vec<ValueKind>,
    pub(super) param_offsets: Vec<i32>,
    pub(super) param_word_count: usize,
    pub(super) return_kind: ValueKind,
}

impl FunctionSig {
    pub(super) fn param_count(&self) -> usize {
        self.param_kinds.len()
    }
}

#[derive(Debug, Clone)]
pub(super) enum VarLoc {
    Global(String),
    Frame(i32),
}

#[derive(Debug, Clone)]
pub(super) struct VarInfo {
    pub(super) loc: VarLoc,
    pub(super) kind: ValueKind,
}

#[derive(Debug, Clone)]
pub(super) struct Env {
    scopes: Vec<BTreeMap<String, VarInfo>>,
    pub(super) next_local_slot: usize,
}

impl Env {
    pub(super) fn top() -> Self {
        Self {
            scopes: vec![BTreeMap::new()],
            next_local_slot: 0,
        }
    }

    pub(super) fn function(params: &[Param], sig: &FunctionSig) -> Self {
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

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub(super) fn insert_current(&mut self, name: String, info: VarInfo) {
        self.scopes
            .last_mut()
            .expect("scope stack is never empty")
            .insert(name, info);
    }

    pub(super) fn lookup(&self, name: &str) -> Option<VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.clone());
            }
        }
        None
    }

    pub(super) fn alloc_frame_slots(&mut self, width_words: usize) -> i32 {
        let offset = (self.next_local_slot as i32) * 4;
        self.next_local_slot += width_words.max(1);
        offset
    }
}

#[derive(Clone, Copy)]
pub(super) enum CompareKind {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

pub(super) fn arg_reg(index: usize) -> Result<Reg, String> {
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
