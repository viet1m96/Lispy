const BUILTIN_NAMES: &[&str] = &[
    "+",
    "-",
    "*",
    "/",
    "%",
    "=",
    "!=",
    "<",
    "<=",
    ">",
    ">=",
    "and",
    "or",
    "not",
    "bit-and",
    "bit-or",
    "bit-xor",
    "shl",
    "shr",
    "sar",
    "array",
    "array-get",
    "array-set",
    "array-size",
    "vadd",
    "vsub",
    "vmul",
    "vdiv",
    "vcmp",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub forms: Vec<TopForm>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopForm {
    Defun(Defun),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defun {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: TypeName,
    pub body: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub type_ann: TypeName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeName {
    Int,
    I64,
    Bool,
    String,
    Array,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Number(i64),
    Cast {
        target_type: TypeName,
        value: Box<Expr>,
    },
    String(String),
    Bool(bool),
    Nil,
    Ident(String),
    Setq {
        name: String,
        type_ann: TypeName,
        value: Box<Expr>,
    },
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
    Begin(Vec<Expr>),
    Let {
        bindings: Vec<Binding>,
        body: Vec<Expr>,
    },
    Loop {
        cond: Box<Expr>,
        body: Vec<Expr>,
        finally: Box<Expr>,
    },
    Print(Box<Expr>),
    PrintStr(Box<Expr>),
    ReadChar,
    ReadLine,
    ReadInputData,
    HandlerDone,
    Halt,
    Call {
        callee: Callee,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    pub type_ann: TypeName,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callee {
    Ident(String),
    Builtin(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    LParen,
    RParen,
    Number(i64),
    String(String),
    Symbol(String),
}

pub fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '(' => {
                tokens.push(Token {
                    kind: TokenKind::LParen,
                    offset: i,
                });
                i += 1;
            }
            ')' => {
                tokens.push(Token {
                    kind: TokenKind::RParen,
                    offset: i,
                });
                i += 1;
            }
            ' ' | '\t' | '\r' | '\n' => {
                i += 1;
            }
            ';' => {
                i += 1;
                while i < bytes.len() && (bytes[i] as char) != '\n' {
                    i += 1;
                }
            }
            '"' => {
                let start = i;
                i += 1;
                let mut text = String::new();
                let mut terminated = false;
                while i < bytes.len() {
                    let ch = bytes[i] as char;
                    if ch == '"' {
                        i += 1;
                        terminated = true;
                        break;
                    }
                    if ch == '\n' {
                        return Err(format!("unterminated string starting at byte {start}"));
                    }
                    if ch == '\\' {
                        i += 1;
                        if i >= bytes.len() {
                            return Err(format!(
                                "unterminated escape in string starting at byte {start}"
                            ));
                        }
                        let escaped = bytes[i] as char;
                        let value = match escaped {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            '\\' => '\\',
                            '"' => '"',
                            other => other,
                        };
                        text.push(value);
                        i += 1;
                        continue;
                    }
                    text.push(ch);
                    i += 1;
                }
                if !terminated {
                    return Err(format!("unterminated string starting at byte {start}"));
                }
                tokens.push(Token {
                    kind: TokenKind::String(text),
                    offset: start,
                });
            }
            '-' if i + 1 < bytes.len() && (bytes[i + 1] as char).is_ascii_digit() => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
                let text = &source[start..i];
                let value = text
                    .parse::<i64>()
                    .map_err(|_| format!("invalid number at byte {start}: {text}"))?;
                tokens.push(Token {
                    kind: TokenKind::Number(value),
                    offset: start,
                });
            }
            ch if ch.is_ascii_digit() => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
                let text = &source[start..i];
                let value = text
                    .parse::<i64>()
                    .map_err(|_| format!("invalid number at byte {start}: {text}"))?;
                tokens.push(Token {
                    kind: TokenKind::Number(value),
                    offset: start,
                });
            }
            _ => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    let ch = bytes[i] as char;
                    if ch.is_whitespace() || ch == '(' || ch == ')' || ch == ';' {
                        break;
                    }
                    i += 1;
                }
                let symbol = source[start..i].to_string();
                tokens.push(Token {
                    kind: TokenKind::Symbol(symbol),
                    offset: start,
                });
            }
        }
    }

    Ok(tokens)
}

pub fn parse_program(source: &str) -> Result<Program, String> {
    let tokens = tokenize(source)?;
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0 }
    }

    fn parse_program(&mut self) -> Result<Program, String> {
        let mut forms = Vec::new();
        while !self.is_eof() {
            forms.push(self.parse_top_form()?);
        }
        Ok(Program { forms })
    }

    fn parse_top_form(&mut self) -> Result<TopForm, String> {
        if self.peek_is_lparen() && self.peek_nth_symbol(1) == Some("defun") {
            Ok(TopForm::Defun(self.parse_defun()?))
        } else {
            Ok(TopForm::Expr(self.parse_expr()?))
        }
    }

    fn parse_defun(&mut self) -> Result<Defun, String> {
        self.expect_lparen()?;
        self.expect_symbol_text("defun")?;
        let name = self.expect_identifier()?;
        self.expect_lparen()?;
        let mut params = Vec::new();
        while !self.peek_is_rparen() {
            params.push(self.parse_param()?);
        }
        self.expect_rparen()?;
        let return_type = self.expect_type_name()?;
        let body = self.parse_body_until_rparen()?;
        self.expect_rparen()?;
        Ok(Defun {
            name,
            params,
            return_type,
            body,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        let token = self
            .peek()
            .ok_or_else(|| "unexpected end of input while parsing expression".to_string())?;
        match &token.kind {
            TokenKind::Number(value) => {
                let value = *value;
                self.index += 1;
                Ok(Expr::Number(value))
            }
            TokenKind::String(text) => {
                let text = text.clone();
                self.index += 1;
                Ok(Expr::String(text))
            }
            TokenKind::Symbol(symbol) => {
                let symbol = symbol.clone();
                self.index += 1;
                match symbol.as_str() {
                    "t" => Ok(Expr::Bool(true)),
                    "nil" => Ok(Expr::Nil),
                    _ => Ok(Expr::Ident(symbol)),
                }
            }
            TokenKind::LParen => self.parse_list_expr(),
            TokenKind::RParen => Err(format!("unexpected ')' at byte {}", token.offset)),
        }
    }

    fn parse_list_expr(&mut self) -> Result<Expr, String> {
        self.expect_lparen()?;
        let head = self.expect_symbol_any()?;

        let expr = match head.as_str() {
            "setq" => {
                let name = self.expect_identifier()?;
                let type_ann = self.expect_type_name()?;
                let value = self.parse_expr()?;
                self.expect_rparen()?;
                Expr::Setq {
                    name,
                    type_ann,
                    value: Box::new(value),
                }
            }
            "if" => {
                let cond = self.parse_expr()?;
                let then_branch = self.parse_expr()?;
                let else_branch = self.parse_expr()?;
                self.expect_rparen()?;
                Expr::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(else_branch),
                }
            }
            "begin" => {
                let body = self.parse_body_until_rparen()?;
                self.expect_rparen()?;
                Expr::Begin(body)
            }
            "let" => {
                self.expect_lparen()?;
                let mut bindings = Vec::new();
                while !self.peek_is_rparen() {
                    self.expect_lparen()?;
                    let name = self.expect_identifier()?;
                    let type_ann = self.expect_type_name()?;
                    let value = self.parse_expr()?;
                    self.expect_rparen()?;
                    bindings.push(Binding {
                        name,
                        type_ann,
                        value,
                    });
                }
                self.expect_rparen()?;
                let body = self.parse_body_until_rparen()?;
                self.expect_rparen()?;
                Expr::Let { bindings, body }
            }
            "loop" => {
                self.expect_symbol_text("while")?;
                let cond = self.parse_expr()?;
                self.expect_symbol_text("do")?;
                let mut body = Vec::new();
                while !self.peek_is_symbol("finally") {
                    if self.peek_is_rparen() {
                        return Err("loop form is missing 'finally' clause".to_string());
                    }
                    body.push(self.parse_expr()?);
                }
                if body.is_empty() {
                    return Err("loop body must contain at least one expression".to_string());
                }
                self.expect_symbol_text("finally")?;
                let finally = self.parse_expr()?;
                self.expect_rparen()?;
                Expr::Loop {
                    cond: Box::new(cond),
                    body,
                    finally: Box::new(finally),
                }
            }
            "print" => {
                let value = self.parse_expr()?;
                self.expect_rparen()?;
                Expr::Print(Box::new(value))
            }
            "print-str" => {
                let value = self.parse_expr()?;
                self.expect_rparen()?;
                Expr::PrintStr(Box::new(value))
            }
            "read-char" => {
                self.expect_rparen()?;
                Expr::ReadChar
            }
            "read-line" => {
                self.expect_rparen()?;
                Expr::ReadLine
            }
            "read-input-data" => {
                self.expect_rparen()?;
                Expr::ReadInputData
            }
            "handler-done" => {
                self.expect_rparen()?;
                Expr::HandlerDone
            }
            "halt" => {
                self.expect_rparen()?;
                Expr::Halt
            }
            "as-int" => {
                let value = self.parse_expr()?;
                self.expect_rparen()?;
                Expr::Cast {
                    target_type: TypeName::Int,
                    value: Box::new(value),
                }
            }
            "as-i64" => {
                let value = self.parse_expr()?;
                self.expect_rparen()?;
                Expr::Cast {
                    target_type: TypeName::I64,
                    value: Box::new(value),
                }
            }
            "defun" => {
                return Err("defun is only allowed as a top-level form".to_string());
            }
            other => {
                let callee = if is_builtin_name(other) {
                    Callee::Builtin(other.to_string())
                } else {
                    Callee::Ident(other.to_string())
                };
                let mut args = Vec::new();
                while !self.peek_is_rparen() {
                    args.push(self.parse_expr()?);
                }
                self.expect_rparen()?;
                Expr::Call { callee, args }
            }
        };

        Ok(expr)
    }

    fn parse_body_until_rparen(&mut self) -> Result<Vec<Expr>, String> {
        let mut body = Vec::new();
        while !self.peek_is_rparen() {
            if self.is_eof() {
                return Err("unexpected end of input while parsing body".to_string());
            }
            body.push(self.parse_expr()?);
        }
        if body.is_empty() {
            return Err("body must contain at least one expression".to_string());
        }
        Ok(body)
    }

    fn is_eof(&self) -> bool {
        self.index >= self.tokens.len()
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.index)
    }

    fn peek_nth(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.index + n)
    }

    fn peek_is_lparen(&self) -> bool {
        matches!(self.peek().map(|t| &t.kind), Some(TokenKind::LParen))
    }

    fn peek_is_rparen(&self) -> bool {
        matches!(self.peek().map(|t| &t.kind), Some(TokenKind::RParen))
    }

    fn peek_is_symbol(&self, text: &str) -> bool {
        matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Symbol(sym)) if sym == text)
    }

    fn peek_nth_symbol(&self, n: usize) -> Option<&str> {
        match self.peek_nth(n).map(|t| &t.kind) {
            Some(TokenKind::Symbol(text)) => Some(text.as_str()),
            _ => None,
        }
    }

    fn expect_lparen(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::LParen,
                ..
            }) => {
                self.index += 1;
                Ok(())
            }
            Some(token) => Err(format!(
                "expected '(', found {} at byte {}",
                token_name(&token.kind),
                token.offset
            )),
            None => Err("expected '(', found end of input".to_string()),
        }
    }

    fn expect_rparen(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::RParen,
                ..
            }) => {
                self.index += 1;
                Ok(())
            }
            Some(token) => Err(format!(
                "expected ')', found {} at byte {}",
                token_name(&token.kind),
                token.offset
            )),
            None => Err("expected ')', found end of input".to_string()),
        }
    }

    fn expect_symbol_text(&mut self, expected: &str) -> Result<(), String> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::Symbol(text),
                ..
            }) if text == expected => {
                self.index += 1;
                Ok(())
            }
            Some(token) => Err(format!(
                "expected symbol '{expected}', found {} at byte {}",
                token_name(&token.kind),
                token.offset
            )),
            None => Err(format!("expected symbol '{expected}', found end of input")),
        }
    }

    fn expect_symbol_any(&mut self) -> Result<String, String> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::Symbol(text),
                ..
            }) => {
                let text = text.clone();
                self.index += 1;
                Ok(text)
            }
            Some(token) => Err(format!(
                "expected symbol, found {} at byte {}",
                token_name(&token.kind),
                token.offset
            )),
            None => Err("expected symbol, found end of input".to_string()),
        }
    }

    fn expect_identifier(&mut self) -> Result<String, String> {
        let name = self.expect_symbol_any()?;
        if is_valid_identifier(&name) {
            Ok(name)
        } else {
            Err(format!("expected identifier, found '{name}'"))
        }
    }

    fn parse_param(&mut self) -> Result<Param, String> {
        self.expect_lparen()?;
        let name = self.expect_identifier()?;
        let type_ann = self.expect_type_name()?;
        self.expect_rparen()?;
        Ok(Param { name, type_ann })
    }

    fn expect_type_name(&mut self) -> Result<TypeName, String> {
        let token = self.expect_symbol_any()?;
        parse_type_name(&token).ok_or_else(|| {
            format!(
                "expected type annotation (:int, :i64, :bool, :string, or :array), found '{token}'"
            )
        })
    }
}

fn parse_type_name(name: &str) -> Option<TypeName> {
    match name {
        ":int" => Some(TypeName::Int),
        ":i64" => Some(TypeName::I64),
        ":bool" => Some(TypeName::Bool),
        ":string" => Some(TypeName::String),
        ":array" => Some(TypeName::Array),
        _ => None,
    }
}

fn is_builtin_name(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name)
}

fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(head) = chars.next() else {
        return false;
    };
    if !head.is_ascii_alphabetic() && head != '_' {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '?')
}

fn token_name(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::LParen => "'('",
        TokenKind::RParen => "')'",
        TokenKind::Number(_) => "number",
        TokenKind::String(_) => "string",
        TokenKind::Symbol(_) => "symbol",
    }
}
