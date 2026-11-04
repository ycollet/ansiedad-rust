use std::{collections::BTreeMap, fmt, sync::Arc};

#[derive(Debug)]
pub enum Constant {
    Integer(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub fn parse_integer(source: &str) -> Result<i64, Error> {
    let (sign, unsigned) = match source.strip_prefix('-') {
        Some(value) => ("-", value),
        None => ("", source.strip_prefix('+').unwrap_or(source)),
    };
    let (radix, digits) = match unsigned.get(..2) {
        Some("0x" | "0X") => (16, &unsigned[2..]),
        Some("0b" | "0B") => (2, &unsigned[2..]),
        Some("0o" | "0O") => (8, &unsigned[2..]),
        _ => (10, unsigned),
    };
    if digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || digits
            .chars()
            .any(|character| character != '_' && !character.is_digit(radix))
    {
        return Err(Error(format!("invalid integer {source:?}")));
    }
    i64::from_str_radix(&format!("{sign}{}", digits.replace('_', "")), radix)
        .map_err(|_| Error(format!("invalid integer {source:?}")))
}

pub fn parse_string(source: &str, pos: &mut usize) -> Result<String, Error> {
    let quote = match source.as_bytes().get(*pos) {
        Some(b'\'') => '\'',
        Some(b'"') => '"',
        _ => return Err(Error("expected a quoted string".into())),
    };
    *pos += 1;
    let mut text = String::new();
    loop {
        let character = source[*pos..]
            .chars()
            .next()
            .ok_or_else(|| Error("unterminated string literal".into()))?;
        *pos += character.len_utf8();
        match character {
            character if character == quote => return Ok(text),
            '\n' | '\r' => return Err(Error("unterminated string literal".into())),
            '\\' => {
                let escaped = source[*pos..]
                    .chars()
                    .next()
                    .ok_or_else(|| Error("unterminated string escape".into()))?;
                *pos += escaped.len_utf8();
                text.push(match escaped {
                    '\\' | '\'' | '"' => escaped,
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => return Err(Error(format!("unsupported string escape \\{escaped}"))),
                });
            }
            _ => text.push(character),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(i64),
    Time,
    X,
    Y,
    Frame,
    Text(Arc<[char]>),
    CharacterAt(Box<Expr>, Box<Expr>),
    Ord(Box<Expr>),
    Length(Box<Expr>),
    Unary(Op, Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Context {
    pub t: i64,
    pub x: i64,
    pub y: i64,
    pub frame: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Shl,
    Shr,
    And,
    Or,
    Xor,
    Not,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Number(i64),
    Identifier(String),
    Text(String),
    Op(Op),
    Open,
    Close,
    OpenIndex,
    CloseIndex,
    End,
}

impl Expr {
    pub fn parse(source: &str, constants: &BTreeMap<String, Constant>) -> Result<Self, Error> {
        Self::parse_internal(source, constants, false)
    }

    pub fn parse_canvas(
        source: &str,
        constants: &BTreeMap<String, Constant>,
    ) -> Result<Self, Error> {
        Self::parse_internal(source, constants, true)
    }

    fn parse_internal(
        source: &str,
        constants: &BTreeMap<String, Constant>,
        canvas: bool,
    ) -> Result<Self, Error> {
        if source.len() > 4096 {
            return Err(Error("expression exceeds 4096 bytes".into()));
        }
        let tokens = tokenize(source)?;
        let mut parser = Parser {
            tokens,
            pos: 0,
            constants,
            canvas,
        };
        let expr = parser.expression(0, 0)?;
        if parser.peek() != &Token::End {
            return Err(Error(format!("unexpected token {:?}", parser.peek())));
        }
        Ok(expr)
    }

    pub fn eval(&self, t: i64) -> Result<i64, Error> {
        self.eval_context(Context {
            t,
            ..Context::default()
        })
    }

    pub fn eval_context(&self, context: Context) -> Result<i64, Error> {
        match self {
            Self::Number(n) => Ok(*n),
            Self::Time => Ok(context.t),
            Self::X => Ok(context.x),
            Self::Y => Ok(context.y),
            Self::Frame => Ok(context.frame),
            Self::Text(_) | Self::CharacterAt(_, _) => Err(Error(
                "expected an integer; use ord(...) to convert a character".into(),
            )),
            Self::Ord(value) => Ok(i64::from(u32::from(value.eval_character_context(context)?))),
            Self::Length(value) => match value.as_ref() {
                Self::Text(text) => Ok(text.len() as i64),
                _ => {
                    value.eval_character_context(context)?;
                    Ok(1)
                }
            },
            Self::Unary(op, value) => {
                let value = value.eval_context(context)?;
                match op {
                    Op::Add => Ok(value),
                    Op::Sub => Ok(value.wrapping_neg()),
                    Op::Not => Ok(!value),
                    _ => unreachable!(),
                }
            }
            Self::Binary(op, left, right) => {
                let a = left.eval_context(context)?;
                let b = right.eval_context(context)?;
                Ok(match op {
                    Op::Add => a.wrapping_add(b),
                    Op::Sub => a.wrapping_sub(b),
                    Op::Mul => a.wrapping_mul(b),
                    Op::Div | Op::Mod => {
                        if b == 0 {
                            return Err(Error("division or modulo by zero".into()));
                        }
                        let q = a.wrapping_div(b);
                        let r = a.wrapping_rem(b);
                        let adjust = r != 0 && (r < 0) != (b < 0);
                        if *op == Op::Div {
                            q.wrapping_sub(i64::from(adjust))
                        } else if adjust {
                            r.wrapping_add(b)
                        } else {
                            r
                        }
                    }
                    Op::Pow => {
                        let exponent = u32::try_from(b).map_err(|_| {
                            Error("exponent must be between 0 and 4294967295".into())
                        })?;
                        a.wrapping_pow(exponent)
                    }
                    Op::Shl | Op::Shr => {
                        if b < 0 {
                            return Err(Error("negative shift count".into()));
                        }
                        if b >= 64 {
                            if *op == Op::Shr && a < 0 { -1 } else { 0 }
                        } else if *op == Op::Shl {
                            a.wrapping_shl(b as u32)
                        } else {
                            a >> b
                        }
                    }
                    Op::And => a & b,
                    Op::Or => a | b,
                    Op::Xor => a ^ b,
                    Op::Eq => i64::from(a == b),
                    Op::Ne => i64::from(a != b),
                    Op::Lt => i64::from(a < b),
                    Op::Le => i64::from(a <= b),
                    Op::Gt => i64::from(a > b),
                    Op::Ge => i64::from(a >= b),
                    Op::Not => unreachable!(),
                })
            }
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text(_) | Self::CharacterAt(_, _))
    }

    pub fn eval_character(&self, t: i64) -> Result<char, Error> {
        self.eval_character_context(Context {
            t,
            ..Context::default()
        })
    }

    pub fn eval_character_context(&self, context: Context) -> Result<char, Error> {
        match self {
            Self::Text(text) if text.len() == 1 => Ok(text[0]),
            Self::Text(_) => Err(Error(
                "expected exactly one character; index the string".into(),
            )),
            Self::CharacterAt(source, index) => match source.as_ref() {
                Self::Text(text) => {
                    let index = index.eval_context(context)?;
                    let len = text.len() as i64;
                    let index = if index < 0 { len + index } else { index };
                    usize::try_from(index)
                        .ok()
                        .and_then(|index| text.get(index))
                        .copied()
                        .ok_or_else(|| Error("string index out of range".into()))
                }
                _ => {
                    let character = source.eval_character_context(context)?;
                    if matches!(index.eval_context(context)?, 0 | -1) {
                        Ok(character)
                    } else {
                        Err(Error("string index out of range".into()))
                    }
                }
            },
            _ => Err(Error("expected a string or character".into())),
        }
    }
}

fn tokenize(source: &str) -> Result<Vec<Token>, Error> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if byte.is_ascii_whitespace() {
            pos += 1;
            continue;
        }
        if byte.is_ascii_digit() {
            let start = pos;
            let radix = if byte == b'0' && pos + 1 < bytes.len() {
                match bytes[pos + 1] {
                    b'x' | b'X' => 16,
                    b'b' | b'B' => 2,
                    b'o' | b'O' => 8,
                    _ => 10,
                }
            } else {
                10
            };
            if radix != 10 {
                pos += 2;
            }
            while pos < bytes.len() && (bytes[pos].is_ascii_hexdigit() || bytes[pos] == b'_') {
                pos += 1;
            }
            let value = parse_integer(&source[start..pos])
                .map_err(|e| Error(format!("{e} at byte {start}")))?;
            tokens.push(Token::Number(value));
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = pos;
            pos += 1;
            while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
                pos += 1;
            }
            tokens.push(Token::Identifier(source[start..pos].into()));
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            tokens.push(Token::Text(parse_string(source, &mut pos)?));
            continue;
        }
        let pair = bytes.get(pos..pos + 2);
        let paired_op = match pair {
            Some(b"**") => Some(Op::Pow),
            Some(b"//") => Some(Op::Div),
            Some(b"<<") => Some(Op::Shl),
            Some(b">>") => Some(Op::Shr),
            Some(b"==") => Some(Op::Eq),
            Some(b"!=") => Some(Op::Ne),
            Some(b"<=") => Some(Op::Le),
            Some(b">=") => Some(Op::Ge),
            _ => None,
        };
        if let Some(op) = paired_op {
            tokens.push(Token::Op(op));
            pos += 2;
            continue;
        }
        tokens.push(match byte {
            b'(' => Token::Open,
            b')' => Token::Close,
            b'[' => Token::OpenIndex,
            b']' => Token::CloseIndex,
            b'+' => Token::Op(Op::Add),
            b'-' => Token::Op(Op::Sub),
            b'*' => Token::Op(Op::Mul),
            b'/' => Token::Op(Op::Div),
            b'%' => Token::Op(Op::Mod),
            b'&' => Token::Op(Op::And),
            b'|' => Token::Op(Op::Or),
            b'^' => Token::Op(Op::Xor),
            b'~' => Token::Op(Op::Not),
            b'<' => Token::Op(Op::Lt),
            b'>' => Token::Op(Op::Gt),
            _ => return Err(Error(format!("unexpected character at byte {pos}"))),
        });
        pos += 1;
    }
    tokens.push(Token::End);
    Ok(tokens)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    constants: &'a BTreeMap<String, Constant>,
    canvas: bool,
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn expression(&mut self, min: u8, depth: usize) -> Result<Expr, Error> {
        if depth > 64 {
            return Err(Error("expression nesting exceeds 64 levels".into()));
        }
        let token = self.peek().clone();
        self.pos += 1;
        let mut left = match token {
            Token::Number(n) => Expr::Number(n),
            Token::Identifier(name) if name == "t" => Expr::Time,
            Token::Identifier(name) if self.canvas && name == "x" => Expr::X,
            Token::Identifier(name) if self.canvas && name == "y" => Expr::Y,
            Token::Identifier(name) if self.canvas && name == "frame" => Expr::Frame,
            Token::Text(text) => Expr::Text(text.chars().collect()),
            Token::Identifier(name) if matches!(name.as_str(), "ord" | "len") => {
                self.function(&name, depth + 1)?
            }
            Token::Identifier(name) => match self.constants.get(&name) {
                Some(Constant::Integer(value)) => Expr::Number(*value),
                Some(Constant::Text(text)) => Expr::Text(text.chars().collect()),
                None => return Err(Error(format!("unknown constant {name}"))),
            },
            Token::Op(op @ (Op::Add | Op::Sub | Op::Not)) => {
                let value = self.expression(8, depth + 1)?;
                if value.is_text() {
                    return Err(Error(
                        "unary operators require an integer; use ord(...)".into(),
                    ));
                }
                Expr::Unary(op, Box::new(value))
            }
            Token::Open => {
                let inner = self.expression(0, depth + 1)?;
                if self.peek() != &Token::Close {
                    return Err(Error("missing closing parenthesis".into()));
                }
                self.pos += 1;
                inner
            }
            _ => {
                return Err(Error(
                    "expected a number, string, name, or parenthesized expression".into(),
                ));
            }
        };
        loop {
            if self.peek() == &Token::OpenIndex {
                if !left.is_text() {
                    return Err(Error("indexing requires a string".into()));
                }
                self.pos += 1;
                let index = self.expression(0, depth + 1)?;
                if index.is_text() {
                    return Err(Error("string index must be an integer".into()));
                }
                if self.peek() != &Token::CloseIndex {
                    return Err(Error("missing closing ']' in string index".into()));
                }
                self.pos += 1;
                left = Expr::CharacterAt(Box::new(left), Box::new(index));
                continue;
            }
            let Token::Op(op) = self.peek() else {
                break;
            };
            let op = *op;
            let precedence = match op {
                Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => 1,
                Op::Or => 2,
                Op::Xor => 3,
                Op::And => 4,
                Op::Shl | Op::Shr => 5,
                Op::Add | Op::Sub => 6,
                Op::Mul | Op::Div | Op::Mod => 7,
                Op::Pow => 9,
                Op::Not => break,
            };
            if precedence < min {
                break;
            }
            self.pos += 1;
            let next_min = if op == Op::Pow {
                precedence
            } else {
                precedence + 1
            };
            let right = self.expression(next_min, depth + 1)?;
            if left.is_text() || right.is_text() {
                return Err(Error(
                    "binary operators require integers; use ord(...)".into(),
                ));
            }
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn function(&mut self, name: &str, depth: usize) -> Result<Expr, Error> {
        if self.peek() != &Token::Open {
            return Err(Error(format!("expected '(' after {name}")));
        }
        self.pos += 1;
        let value = self.expression(0, depth)?;
        if !value.is_text() {
            return Err(Error(format!("{name} requires a string or character")));
        }
        if name == "ord"
            && let Expr::Text(text) = &value
            && text.len() != 1
        {
            return Err(Error(
                "ord requires exactly one character; index the string".into(),
            ));
        }
        if self.peek() != &Token::Close {
            return Err(Error(format!(
                "missing closing parenthesis after {name} argument"
            )));
        }
        self.pos += 1;
        Ok(if name == "ord" {
            Expr::Ord(Box::new(value))
        } else {
            Expr::Length(Box::new(value))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Expr, Error> {
        Expr::parse(source, &BTreeMap::new())
    }

    fn eval(source: &str, t: i64) -> i64 {
        parse(source).unwrap().eval(t).unwrap()
    }

    #[test]
    fn upstream_expressions_and_python_precedence() {
        for t in [0_i64, 1, 63, 1024, 65535] {
            assert_eq!(eval("t<<5 ^ t>>1", t), (t << 5) ^ (t >> 1));
            assert_eq!(eval("(t*t>>17)%16+240", t), (((t * t) >> 17) % 16) + 240);
            assert_eq!(eval("144+(t^(t>>8))%4", t), 144 + (t ^ (t >> 8)) % 4);
            assert_eq!(eval("t >> 0xE", t), t >> 14);
            assert_eq!(eval("144+t % 0b110", t), 144 + t % 6);
        }
        assert_eq!(eval("-2**2", 0), -4);
        assert_eq!(eval("2**3**2", 0), 512);
        assert_eq!(eval("t & 1 == 0", 2), 1);
        assert_eq!(eval("~0", 0), -1);
    }

    #[test]
    fn signed_arithmetic_and_overflow_are_defined() {
        assert_eq!(eval("-7 % 3", 0), 2);
        assert_eq!(eval("7 % -3", 0), -2);
        assert_eq!(eval("-7 // 3", 0), -3);
        assert_eq!(eval("7 / -3", 0), -3);
        assert_eq!(eval("9223372036854775807 + 1", 0), i64::MIN);
        assert_eq!(eval("t / -1", i64::MIN), i64::MIN);
        assert_eq!(eval("t % -1", i64::MIN), 0);
        assert_eq!(eval("1 << 64", 0), 0);
        assert_eq!(eval("-1 >> 1000", 0), -1);
    }

    #[test]
    fn ord_supports_literals_unicode_and_python_string_indexing() {
        assert_eq!(eval("ord('A') - 32", 0), 33);
        assert_eq!(eval(r#"ord("\n")"#, 0), 10);
        assert_eq!(eval(r#"ord('\'')"#, 0), 39);
        assert_eq!(eval("ord('Aé😀'[t])", 1), 233);
        assert_eq!(eval("ord('Aé😀'[t])", -1), 0x1f600);
        assert_eq!(eval("ord('Aé😀'[t])", -3), 65);
        assert_eq!(eval("ord('ab'[ord('0') % 2])", 0), 97);
        assert_eq!(eval("ord('a'[0][0])", 0), 97);
        for t in [-4, 3] {
            assert!(
                parse("ord('Aé😀'[t])")
                    .unwrap()
                    .eval(t)
                    .unwrap_err()
                    .0
                    .contains("string index out of range")
            );
        }
        assert!(parse("ord(''[0])").unwrap().eval(0).is_err());
    }

    #[test]
    fn rejects_invalid_ord_arguments_and_string_syntax() {
        for source in [
            "ord",
            "ord()",
            "ord(65)",
            "ord('')",
            "ord('ab')",
            "ord('a'",
            "ord('a'[0)",
            "ord('a'[])",
            "ord('a'[0], 1)",
            "ord('a)",
            r"ord('\q')",
            r"ord('a\",
            "'a' + 1",
            "unknown",
            "time",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
        let nested = format!("{}0{}", "ord('a'[".repeat(70), "])".repeat(70));
        assert!(parse(&nested).is_err());
    }

    #[test]
    fn standalone_string_indexing_returns_a_character_not_an_integer() {
        let expression = parse("('Aé😀')[t]").unwrap();
        for (t, expected) in [(0, 'A'), (1, 'é'), (-1, '😀')] {
            assert_eq!(expression.eval_character(t).unwrap(), expected);
            assert!(expression.eval(t).is_err());
        }
        assert_eq!(
            parse("'abc'[len('abc')-1]")
                .unwrap()
                .eval_character(0)
                .unwrap(),
            'c'
        );
        assert_eq!(
            parse("'abc'[1][-1]").unwrap().eval_character(0).unwrap(),
            'b'
        );
        for source in ["'abc'[3]", "'abc'[-4]", "''[0]", "'abc'[0][1]"] {
            assert!(
                parse(source)
                    .unwrap()
                    .eval_character(0)
                    .unwrap_err()
                    .0
                    .contains("string index out of range"),
                "{source}"
            );
        }
    }

    #[test]
    fn len_counts_unicode_characters_in_strings_and_indexed_characters() {
        assert_eq!(eval("len('')", 0), 0);
        assert_eq!(eval("len('TEST')", 0), 4);
        assert_eq!(eval("len('Aé😀')", 0), 3);
        assert_eq!(eval(r"len('a\nb')", 0), 3);
        assert_eq!(eval("2*len(('abc'))+1", 0), 7);
        assert_eq!(eval("len('abc'[t])", 1), 1);
        assert!(parse("len('abc'[t])").unwrap().eval(3).is_err());
        for source in [
            "len",
            "len()",
            "len(1)",
            "len(t)",
            "len('abc', 1)",
            "len('abc'",
            "len(ord('A'))",
            "ord(len('A'))",
            "'abc'['A']",
            "1[0]",
            "'A'[0] + 1",
            "-'A'[0]",
            "len('A')[0]",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
        assert!(parse(&format!("{}'a'{}", "len(".repeat(70), ")".repeat(70))).is_err());
    }

    #[test]
    fn errors_are_explicit_and_never_execute_code() {
        for source in ["", "t +", "(t", "t)", "0x", "0b2", "foo(t)", "t; 1", "t **"] {
            assert!(parse(source).is_err(), "{source}");
        }
        for source in ["1/0", "1%0", "t << -1", "2**-1"] {
            assert!(parse(source).unwrap().eval(0).is_err(), "{source}");
        }
        assert!(parse(&format!("{}t{}", "(".repeat(70), ")".repeat(70))).is_err());
        assert!(parse(&"t+".repeat(3000)).is_err());
    }

    #[test]
    fn canvas_context_propagates_through_strings_and_arithmetic_without_reserving_console_names() {
        let expression =
            Expr::parse_canvas("ord('AB'[x])+len('ab'[y])+frame+t", &BTreeMap::new()).unwrap();
        let context = Context {
            t: 5,
            x: 1,
            y: 0,
            frame: 3,
        };
        assert_eq!(expression.eval_context(context).unwrap(), 75);
        let constants = BTreeMap::from([
            ("x".into(), Constant::Integer(20)),
            ("y".into(), Constant::Integer(2)),
            ("frame".into(), Constant::Integer(3)),
        ]);
        assert_eq!(
            Expr::parse("x+y+frame", &constants)
                .unwrap()
                .eval(0)
                .unwrap(),
            25
        );
        assert!(Expr::parse("x", &BTreeMap::new()).is_err());
    }
}
