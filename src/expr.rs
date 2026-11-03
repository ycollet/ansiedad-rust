use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(i64),
    Time,
    Unary(Op, Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
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
    Time,
    Op(Op),
    Open,
    Close,
    End,
}

impl Expr {
    pub fn parse(source: &str) -> Result<Self, Error> {
        if source.len() > 4096 {
            return Err(Error("expression exceeds 4096 bytes".into()));
        }
        let tokens = tokenize(source)?;
        let mut parser = Parser { tokens, pos: 0 };
        let expr = parser.expression(0, 0)?;
        if parser.peek() != &Token::End {
            return Err(Error(format!("unexpected token {:?}", parser.peek())));
        }
        Ok(expr)
    }

    pub fn eval(&self, t: i64) -> Result<i64, Error> {
        match self {
            Self::Number(n) => Ok(*n),
            Self::Time => Ok(t),
            Self::Unary(op, value) => {
                let value = value.eval(t)?;
                match op {
                    Op::Add => Ok(value),
                    Op::Sub => Ok(value.wrapping_neg()),
                    Op::Not => Ok(!value),
                    _ => unreachable!(),
                }
            }
            Self::Binary(op, left, right) => {
                let a = left.eval(t)?;
                let b = right.eval(t)?;
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
            let digits = pos;
            while pos < bytes.len() && bytes[pos].is_ascii_hexdigit() {
                pos += 1;
            }
            let value = i64::from_str_radix(&source[digits..pos], radix)
                .map_err(|_| Error(format!("invalid integer at byte {start}")))?;
            tokens.push(Token::Number(value));
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
            b't' => Token::Time,
            b'(' => Token::Open,
            b')' => Token::Close,
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

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
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
            Token::Time => Expr::Time,
            Token::Op(op @ (Op::Add | Op::Sub | Op::Not)) => {
                Expr::Unary(op, Box::new(self.expression(8, depth + 1)?))
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
                    "expected a number, t, or parenthesized expression".into(),
                ));
            }
        };
        while let Token::Op(op) = self.peek() {
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
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(source: &str, t: i64) -> i64 {
        Expr::parse(source).unwrap().eval(t).unwrap()
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
    fn errors_are_explicit_and_never_execute_code() {
        for source in ["", "t +", "(t", "t)", "0x", "0b2", "foo(t)", "t; 1", "t **"] {
            assert!(Expr::parse(source).is_err(), "{source}");
        }
        for source in ["1/0", "1%0", "t << -1", "2**-1"] {
            assert!(Expr::parse(source).unwrap().eval(0).is_err(), "{source}");
        }
        assert!(Expr::parse(&format!("{}t{}", "(".repeat(70), ")".repeat(70))).is_err());
        assert!(Expr::parse(&"t+".repeat(3000)).is_err());
    }
}
