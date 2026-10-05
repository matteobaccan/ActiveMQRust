// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! JMS message selectors (SQL-92 subset) with ActiveMQ evaluation semantics
//! (`org.apache.activemq.filter.*`): three-valued logic where `null` is UNKNOWN,
//! and a message is selected only when the expression evaluates to TRUE.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;

/// A value produced while evaluating a selector.
#[derive(Debug, Clone, PartialEq)]
pub enum SVal {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// A non-null value that compares unequal to everything (e.g. destinations, byte arrays).
    Opaque,
}

/// Gives the selector access to message headers and properties.
pub trait MessageView {
    /// Value of a JMS header identifier (`JMSPriority`, ...); `None` if `name` is not a header.
    fn header(&self, name: &str) -> Option<SVal>;
    /// Value of an application property; `SVal::Null` when absent.
    fn property(&self, name: &str) -> SVal;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorError {
    pub column: usize,
    pub message: String,
}

impl fmt::Display for SelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at column {}", self.message, self.column)
    }
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Int(i64),
    Float(f64),
    True,
    False,
    Null,
    And,
    Or,
    Not,
    Like,
    Escape,
    Between,
    In,
    Is,
    XPath,
    XQuery,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
    Comma,
    End,
}

struct Lexed {
    tok: Tok,
    col: usize,
    text: String,
}

fn lex(input: &str) -> Result<Vec<Lexed>, SelectorError> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let tok = if c == '\'' {
            let mut s = String::new();
            i += 1;
            loop {
                if i >= chars.len() {
                    return Err(SelectorError { column: start, message: "Unterminated string literal".into() });
                }
                if chars[i] == '\'' {
                    if i + 1 < chars.len() && chars[i + 1] == '\'' {
                        s.push('\'');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                s.push(chars[i]);
                i += 1;
            }
            Tok::Str(s)
        } else if c.is_ascii_digit() || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
            lex_number(&chars, &mut i, start)?
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$' || chars[i] == '.') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.to_ascii_uppercase().as_str() {
                "AND" => Tok::And,
                "OR" => Tok::Or,
                "NOT" => Tok::Not,
                "LIKE" => Tok::Like,
                "ESCAPE" => Tok::Escape,
                "BETWEEN" => Tok::Between,
                "IN" => Tok::In,
                "IS" => Tok::Is,
                "TRUE" => Tok::True,
                "FALSE" => Tok::False,
                "NULL" => Tok::Null,
                "XPATH" => Tok::XPath,
                "XQUERY" => Tok::XQuery,
                _ => Tok::Ident(word),
            }
        } else {
            i += 1;
            match c {
                '=' => Tok::Eq,
                '<' => {
                    if i < chars.len() && chars[i] == '>' {
                        i += 1;
                        Tok::Ne
                    } else if i < chars.len() && chars[i] == '=' {
                        i += 1;
                        Tok::Le
                    } else {
                        Tok::Lt
                    }
                }
                '>' => {
                    if i < chars.len() && chars[i] == '=' {
                        i += 1;
                        Tok::Ge
                    } else {
                        Tok::Gt
                    }
                }
                '+' => Tok::Plus,
                '-' => Tok::Minus,
                '*' => Tok::Star,
                '/' => Tok::Slash,
                '%' => Tok::Percent,
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                ',' => Tok::Comma,
                _ => {
                    return Err(SelectorError { column: start, message: format!("Unexpected character '{c}'") })
                }
            }
        };
        out.push(Lexed { tok, col: start, text: chars[start..i].iter().collect() });
    }
    out.push(Lexed { tok: Tok::End, col: chars.len(), text: String::new() });
    Ok(out)
}

fn lex_number(chars: &[char], i: &mut usize, start: usize) -> Result<Tok, SelectorError> {
    let bad = |msg: &str| SelectorError { column: start, message: msg.to_string() };
    // Hexadecimal.
    if chars[*i] == '0' && *i + 1 < chars.len() && (chars[*i + 1] == 'x' || chars[*i + 1] == 'X') {
        *i += 2;
        let s = *i;
        while *i < chars.len() && chars[*i].is_ascii_hexdigit() {
            *i += 1;
        }
        let digits: String = chars[s..*i].iter().collect();
        if *i < chars.len() && (chars[*i] == 'l' || chars[*i] == 'L') {
            *i += 1;
        }
        return u64::from_str_radix(&digits, 16)
            .map(|v| Tok::Int(v as i64))
            .map_err(|_| bad("Invalid hexadecimal literal"));
    }
    let s = *i;
    while *i < chars.len() && chars[*i].is_ascii_digit() {
        *i += 1;
    }
    let mut is_float = false;
    if *i < chars.len() && chars[*i] == '.' {
        is_float = true;
        *i += 1;
        while *i < chars.len() && chars[*i].is_ascii_digit() {
            *i += 1;
        }
    }
    if *i < chars.len() && (chars[*i] == 'e' || chars[*i] == 'E') {
        let save = *i;
        *i += 1;
        if *i < chars.len() && (chars[*i] == '+' || chars[*i] == '-') {
            *i += 1;
        }
        if *i < chars.len() && chars[*i].is_ascii_digit() {
            is_float = true;
            while *i < chars.len() && chars[*i].is_ascii_digit() {
                *i += 1;
            }
        } else {
            *i = save;
        }
    }
    let text: String = chars[s..*i].iter().collect();
    if *i < chars.len() && matches!(chars[*i], 'f' | 'F' | 'd' | 'D') {
        *i += 1;
        is_float = true;
    }
    if is_float {
        return text.parse::<f64>().map(Tok::Float).map_err(|_| bad("Invalid decimal literal"));
    }
    if *i < chars.len() && (chars[*i] == 'l' || chars[*i] == 'L') {
        *i += 1;
    }
    // Leading zero means octal, as in Java and ActiveMQ.
    let value = if text.len() > 1 && text.starts_with('0') {
        i64::from_str_radix(&text[1..], 8).map_err(|_| bad("Invalid octal literal"))?
    } else {
        text.parse::<i64>().map_err(|_| bad("Integer literal out of range"))?
    };
    Ok(Tok::Int(value))
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Expr {
    Const(SVal),
    Ident(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Eq(Box<Expr>, Box<Expr>),
    Cmp(Box<Expr>, Box<Expr>, CmpOp),
    Arith(Box<Expr>, Box<Expr>, ArithOp),
    Like { expr: Box<Expr>, pattern: LikePattern, negated: bool },
    In { expr: Box<Expr>, list: InList, negated: bool },
}

#[derive(Debug, Clone, Copy)]
enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy)]
enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone)]
enum InList {
    Small(Vec<String>),
    Set(HashSet<String>),
}

impl InList {
    fn contains(&self, s: &str) -> bool {
        match self {
            InList::Small(v) => v.iter().any(|x| x == s),
            InList::Set(h) => h.contains(s),
        }
    }
}

/// LIKE pattern compiled to a sequence of literal, single and multi wildcards.
#[derive(Debug, Clone)]
struct LikePattern {
    parts: Vec<LikePart>,
}

#[derive(Debug, Clone, PartialEq)]
enum LikePart {
    Lit(char),
    One,
    Any,
}

impl LikePattern {
    fn compile(pattern: &str, escape: Option<char>) -> LikePattern {
        let chars: Vec<char> = pattern.chars().collect();
        let mut parts = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if escape == Some(c) && i + 1 < chars.len() {
                let next = chars[i + 1];
                if next == '_' || next == '%' || Some(next) == escape {
                    parts.push(LikePart::Lit(next));
                    i += 2;
                    continue;
                }
            }
            parts.push(match c {
                '%' => LikePart::Any,
                '_' => LikePart::One,
                other => LikePart::Lit(other),
            });
            i += 1;
        }
        // Collapse consecutive '%'.
        parts.dedup_by(|a, b| *a == LikePart::Any && *b == LikePart::Any);
        LikePattern { parts }
    }

    fn matches(&self, s: &str) -> bool {
        let text: Vec<char> = s.chars().collect();
        // Classic wildcard matching with backtracking on the last '%'.
        let (mut t, mut p) = (0usize, 0usize);
        let mut star: Option<usize> = None;
        let mut mark = 0usize;
        while t < text.len() {
            if p < self.parts.len() {
                match &self.parts[p] {
                    LikePart::Lit(c) if *c == text[t] => {
                        t += 1;
                        p += 1;
                        continue;
                    }
                    LikePart::One => {
                        t += 1;
                        p += 1;
                        continue;
                    }
                    LikePart::Any => {
                        star = Some(p);
                        mark = t;
                        p += 1;
                        continue;
                    }
                    _ => {}
                }
            }
            match star {
                Some(sp) => {
                    p = sp + 1;
                    mark += 1;
                    t = mark;
                }
                None => return false,
            }
        }
        while p < self.parts.len() && self.parts[p] == LikePart::Any {
            p += 1;
        }
        p == self.parts.len()
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    toks: &'a [Lexed],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn advance(&mut self) -> &Lexed {
        let t = &self.toks[self.pos];
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t {
            self.advance();
            true
        } else {
            false
        }
    }

    fn error_here(&self, expected: &str) -> SelectorError {
        let t = &self.toks[self.pos];
        if t.tok == Tok::End {
            SelectorError { column: t.col, message: format!("Unexpected end of selector, expected {expected}") }
        } else {
            SelectorError { column: t.col, message: format!("Unexpected token '{}'", t.text) }
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> Result<(), SelectorError> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(self.error_here(what))
        }
    }

    fn or_expr(&mut self) -> Result<Expr, SelectorError> {
        let mut items = vec![self.and_expr()?];
        while self.eat(&Tok::Or) {
            items.push(self.and_expr()?);
        }
        Ok(if items.len() == 1 { items.pop().unwrap() } else { Expr::Or(items) })
    }

    fn and_expr(&mut self) -> Result<Expr, SelectorError> {
        let mut items = vec![self.equality()?];
        while self.eat(&Tok::And) {
            items.push(self.equality()?);
        }
        Ok(if items.len() == 1 { items.pop().unwrap() } else { Expr::And(items) })
    }

    fn equality(&mut self) -> Result<Expr, SelectorError> {
        let mut left = self.comparison()?;
        loop {
            match self.peek() {
                Tok::Eq => {
                    let col = self.toks[self.pos].col;
                    self.advance();
                    let right = self.comparison()?;
                    check_equal_operands(&left, &right, col)?;
                    left = Expr::Eq(Box::new(left), Box::new(right));
                }
                Tok::Ne => {
                    let col = self.toks[self.pos].col;
                    self.advance();
                    let right = self.comparison()?;
                    check_equal_operands(&left, &right, col)?;
                    left = Expr::Not(Box::new(Expr::Eq(Box::new(left), Box::new(right))));
                }
                Tok::Is => {
                    self.advance();
                    let negated = self.eat(&Tok::Not);
                    self.expect(&Tok::Null, "NULL")?;
                    let e = Expr::Eq(Box::new(left), Box::new(Expr::Const(SVal::Null)));
                    left = if negated { Expr::Not(Box::new(e)) } else { e };
                }
                _ => return Ok(left),
            }
        }
    }

    fn comparison(&mut self) -> Result<Expr, SelectorError> {
        let mut left = self.additive()?;
        loop {
            let col = self.toks[self.pos].col;
            let op = match self.peek() {
                Tok::Lt => Some(CmpOp::Lt),
                Tok::Le => Some(CmpOp::Le),
                Tok::Gt => Some(CmpOp::Gt),
                Tok::Ge => Some(CmpOp::Ge),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let right = self.additive()?;
                check_ordering_operand(&left, col)?;
                check_ordering_operand(&right, col)?;
                left = Expr::Cmp(Box::new(left), Box::new(right), op);
                continue;
            }
            let negated = if self.peek() == &Tok::Not
                && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Like | Tok::Between | Tok::In))
            {
                self.advance();
                true
            } else {
                false
            };
            match self.peek() {
                Tok::Like => {
                    self.advance();
                    let pattern = match &self.advance().tok {
                        Tok::Str(s) => s.clone(),
                        _ => {
                            self.pos -= 1;
                            return Err(self.error_here("a string literal after LIKE"));
                        }
                    };
                    let mut escape = None;
                    if self.eat(&Tok::Escape) {
                        let col = self.toks[self.pos].col;
                        match &self.advance().tok {
                            Tok::Str(s) if s.chars().count() == 1 => escape = s.chars().next(),
                            _ => {
                                return Err(SelectorError {
                                    column: col,
                                    message: "The ESCAPE string literal must be exactly one character".into(),
                                })
                            }
                        }
                    }
                    left = Expr::Like {
                        expr: Box::new(left),
                        pattern: LikePattern::compile(&pattern, escape),
                        negated,
                    };
                }
                Tok::Between => {
                    self.advance();
                    let low = self.additive()?;
                    self.expect(&Tok::And, "AND")?;
                    let high = self.additive()?;
                    let e = Expr::And(vec![
                        Expr::Cmp(Box::new(left.clone()), Box::new(low), CmpOp::Ge),
                        Expr::Cmp(Box::new(left), Box::new(high), CmpOp::Le),
                    ]);
                    left = if negated { Expr::Not(Box::new(e)) } else { e };
                }
                Tok::In => {
                    let col = self.toks[self.pos].col;
                    self.advance();
                    if !matches!(left, Expr::Ident(_)) {
                        return Err(SelectorError { column: col, message: "Expected a property for IN".into() });
                    }
                    self.expect(&Tok::LParen, "(")?;
                    let mut items = Vec::new();
                    loop {
                        match &self.advance().tok {
                            Tok::Str(s) => items.push(s.clone()),
                            _ => {
                                self.pos -= 1;
                                return Err(self.error_here("a string literal in the IN list"));
                            }
                        }
                        if self.eat(&Tok::Comma) {
                            continue;
                        }
                        self.expect(&Tok::RParen, ")")?;
                        break;
                    }
                    let list = if items.len() > 8 {
                        InList::Set(items.into_iter().collect())
                    } else {
                        InList::Small(items)
                    };
                    left = Expr::In { expr: Box::new(left), list, negated };
                }
                _ => {
                    if negated {
                        self.pos -= 1;
                    }
                    return Ok(left);
                }
            }
        }
    }

    fn additive(&mut self) -> Result<Expr, SelectorError> {
        let mut left = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => ArithOp::Add,
                Tok::Minus => ArithOp::Sub,
                _ => return Ok(left),
            };
            self.advance();
            let right = self.multiplicative()?;
            left = Expr::Arith(Box::new(left), Box::new(right), op);
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, SelectorError> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => ArithOp::Mul,
                Tok::Slash => ArithOp::Div,
                Tok::Percent => ArithOp::Mod,
                _ => return Ok(left),
            };
            self.advance();
            let right = self.unary()?;
            left = Expr::Arith(Box::new(left), Box::new(right), op);
        }
    }

    fn unary(&mut self) -> Result<Expr, SelectorError> {
        match self.peek() {
            Tok::Plus => {
                self.advance();
                self.unary()
            }
            Tok::Minus => {
                self.advance();
                Ok(match self.unary()? {
                    Expr::Const(SVal::Int(v)) => Expr::Const(SVal::Int(v.wrapping_neg())),
                    Expr::Const(SVal::Float(v)) => Expr::Const(SVal::Float(-v)),
                    e => Expr::Neg(Box::new(e)),
                })
            }
            Tok::Not => {
                self.advance();
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expr, SelectorError> {
        let t = self.advance();
        let col = t.col;
        Ok(match &t.tok {
            Tok::Str(s) => Expr::Const(SVal::Str(s.clone())),
            Tok::Int(v) => Expr::Const(SVal::Int(*v)),
            Tok::Float(v) => Expr::Const(SVal::Float(*v)),
            Tok::True => Expr::Const(SVal::Bool(true)),
            Tok::False => Expr::Const(SVal::Bool(false)),
            Tok::Null => Expr::Const(SVal::Null),
            Tok::Ident(name) => Expr::Ident(name.clone()),
            Tok::LParen => {
                let e = self.or_expr()?;
                self.expect(&Tok::RParen, ")")?;
                e
            }
            Tok::XPath | Tok::XQuery => {
                return Err(SelectorError { column: col, message: "XPath selectors are not supported".into() })
            }
            _ => {
                self.pos -= 1;
                if self.toks[self.pos].tok == Tok::End {
                    return Err(self.error_here("an expression"));
                }
                return Err(self.error_here("an expression"));
            }
        })
    }
}

fn is_boolean_expr(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Const(SVal::Bool(_)) | Expr::Not(_) | Expr::And(_) | Expr::Or(_) | Expr::Eq(..) | Expr::Cmp(..)
            | Expr::Like { .. } | Expr::In { .. }
    )
}

/// `ComparisonExpression.checkEqualOperand(Compatability)`: NULL literals and
/// boolean-vs-non-boolean constants are rejected at compile time.
fn check_equal_operands(left: &Expr, right: &Expr, col: usize) -> Result<(), SelectorError> {
    for e in [left, right] {
        if matches!(e, Expr::Const(SVal::Null)) {
            return Err(SelectorError { column: col, message: "NULL cannot be compared; use IS NULL".into() });
        }
    }
    if let (Expr::Const(l), Expr::Const(r)) = (left, right) {
        if matches!(l, SVal::Bool(_)) != matches!(r, SVal::Bool(_)) {
            return Err(SelectorError { column: col, message: "Incompatible constants cannot be compared".into() });
        }
    }
    Ok(())
}

/// `ComparisonExpression.checkLessThanOperand`: only numbers may be ordered.
fn check_ordering_operand(e: &Expr, col: usize) -> Result<(), SelectorError> {
    match e {
        Expr::Const(SVal::Int(_)) | Expr::Const(SVal::Float(_)) => Ok(()),
        Expr::Const(_) => Err(SelectorError { column: col, message: "Value cannot be compared with < > <= >=".into() }),
        other if is_boolean_expr(other) => {
            Err(SelectorError { column: col, message: "Boolean expressions cannot be compared".into() })
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// A compiled selector.
#[derive(Debug, Clone)]
pub struct Selector {
    text: String,
    expr: Expr,
    uses_properties: bool,
}

impl Selector {
    /// Compiles a selector. Returns `Ok(None)` for an empty selector.
    pub fn compile(text: &str) -> Result<Option<Selector>, SelectorError> {
        if text.trim().is_empty() {
            return Ok(None);
        }
        let toks = lex(text)?;
        let mut p = Parser { toks: &toks, pos: 0 };
        let expr = p.or_expr()?;
        if p.peek() != &Tok::End {
            return Err(p.error_here("end of selector"));
        }
        let mut uses_properties = false;
        visit_idents(&expr, &mut |name| {
            if !is_header(name) {
                uses_properties = true;
            }
        });
        Ok(Some(Selector { text: text.to_string(), expr, uses_properties }))
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// True when evaluation needs application properties (not only JMS headers).
    pub fn uses_properties(&self) -> bool {
        self.uses_properties
    }

    pub fn matches(&self, view: &dyn MessageView) -> bool {
        eval(&self.expr, view) == SVal::Bool(true)
    }
}

const HEADERS: &[&str] = &[
    "JMSDestination",
    "JMSReplyTo",
    "JMSType",
    "JMSDeliveryMode",
    "JMSPriority",
    "JMSMessageID",
    "JMSTimestamp",
    "JMSCorrelationID",
    "JMSExpiration",
    "JMSRedelivered",
    "JMSXDeliveryCount",
    "JMSXGroupID",
    "JMSXUserID",
    "JMSXGroupSeq",
    "JMSXProducerTXID",
    "JMSActiveMQBrokerInTime",
    "JMSActiveMQBrokerOutTime",
    "JMSActiveMQBrokerPath",
    "JMSXGroupFirstForConsumer",
];

pub fn is_header(name: &str) -> bool {
    HEADERS.contains(&name)
}

fn visit_idents(e: &Expr, f: &mut dyn FnMut(&str)) {
    match e {
        Expr::Ident(n) => f(n),
        Expr::Const(_) => {}
        Expr::Neg(a) | Expr::Not(a) => visit_idents(a, f),
        Expr::And(v) | Expr::Or(v) => v.iter().for_each(|x| visit_idents(x, f)),
        Expr::Eq(a, b) | Expr::Cmp(a, b, _) | Expr::Arith(a, b, _) => {
            visit_idents(a, f);
            visit_idents(b, f);
        }
        Expr::Like { expr, .. } | Expr::In { expr, .. } => visit_idents(expr, f),
    }
}

fn eval(e: &Expr, v: &dyn MessageView) -> SVal {
    match e {
        Expr::Const(c) => c.clone(),
        Expr::Ident(name) => v.header(name).unwrap_or_else(|| v.property(name)),
        Expr::Neg(a) => match eval(a, v) {
            SVal::Int(i) => SVal::Int(i.wrapping_neg()),
            SVal::Float(f) => SVal::Float(-f),
            _ => SVal::Null,
        },
        Expr::Not(a) => match eval(a, v) {
            SVal::Bool(b) => SVal::Bool(!b),
            SVal::Null => SVal::Null,
            _ => SVal::Bool(false),
        },
        Expr::And(items) => {
            let mut some_null = false;
            for x in items {
                match eval(x, v) {
                    SVal::Bool(true) => {}
                    SVal::Null => some_null = true,
                    _ => return SVal::Bool(false),
                }
            }
            if some_null {
                SVal::Null
            } else {
                SVal::Bool(true)
            }
        }
        Expr::Or(items) => {
            let mut some_null = false;
            for x in items {
                match eval(x, v) {
                    SVal::Bool(true) => return SVal::Bool(true),
                    SVal::Null => some_null = true,
                    _ => {}
                }
            }
            if some_null {
                SVal::Null
            } else {
                SVal::Bool(false)
            }
        }
        Expr::Eq(a, b) => {
            let l = eval(a, v);
            let r = eval(b, v);
            match (&l, &r) {
                (SVal::Null, SVal::Null) => SVal::Bool(true),
                (SVal::Null, _) => SVal::Null,
                (_, SVal::Null) => SVal::Bool(false),
                _ => match compare(&l, &r) {
                    Some(o) => SVal::Bool(o == Ordering::Equal),
                    None => SVal::Bool(false),
                },
            }
        }
        Expr::Cmp(a, b, op) => {
            let l = eval(a, v);
            if l == SVal::Null {
                return SVal::Null;
            }
            let r = eval(b, v);
            if r == SVal::Null {
                return SVal::Null;
            }
            match compare(&l, &r) {
                Some(o) => SVal::Bool(match op {
                    CmpOp::Lt => o == Ordering::Less,
                    CmpOp::Le => o != Ordering::Greater,
                    CmpOp::Gt => o == Ordering::Greater,
                    CmpOp::Ge => o != Ordering::Less,
                }),
                None => SVal::Bool(false),
            }
        }
        Expr::Arith(a, b, op) => arith(eval(a, v), eval(b, v), *op),
        Expr::Like { expr, pattern, negated } => match eval(expr, v) {
            SVal::Null => SVal::Null,
            SVal::Str(s) => SVal::Bool(pattern.matches(&s) != *negated),
            _ => SVal::Bool(*negated),
        },
        Expr::In { expr, list, negated } => match eval(expr, v) {
            SVal::Str(s) => SVal::Bool(list.contains(&s) != *negated),
            _ => SVal::Null,
        },
    }
}

/// Compares like `ComparisonExpression.compare`: numbers are promoted, same-type
/// strings and booleans compare naturally, anything else does not compare.
fn compare(l: &SVal, r: &SVal) -> Option<Ordering> {
    match (l, r) {
        (SVal::Int(a), SVal::Int(b)) => Some(a.cmp(b)),
        (SVal::Int(a), SVal::Float(b)) => (*a as f64).partial_cmp(b),
        (SVal::Float(a), SVal::Int(b)) => a.partial_cmp(&(*b as f64)),
        (SVal::Float(a), SVal::Float(b)) => a.partial_cmp(b),
        (SVal::Str(a), SVal::Str(b)) => Some(a.cmp(b)),
        (SVal::Bool(a), SVal::Bool(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

fn arith(l: SVal, r: SVal, op: ArithOp) -> SVal {
    if let (ArithOp::Add, SVal::Str(s)) = (op, &l) {
        // ActiveMQ concatenates when the left operand is a string.
        let rhs = match &r {
            SVal::Str(x) => x.clone(),
            SVal::Int(i) => i.to_string(),
            SVal::Float(f) => f.to_string(),
            SVal::Bool(b) => b.to_string(),
            SVal::Null => "null".to_string(),
            SVal::Opaque => return SVal::Null,
        };
        return SVal::Str(format!("{s}{rhs}"));
    }
    match (l, r) {
        (SVal::Int(a), SVal::Int(b)) => match op {
            ArithOp::Add => SVal::Int(a.wrapping_add(b)),
            ArithOp::Sub => SVal::Int(a.wrapping_sub(b)),
            ArithOp::Mul => SVal::Int(a.wrapping_mul(b)),
            ArithOp::Div => {
                if b == 0 {
                    SVal::Null
                } else {
                    SVal::Int(a.wrapping_div(b))
                }
            }
            ArithOp::Mod => {
                if b == 0 {
                    SVal::Null
                } else {
                    SVal::Int(a.wrapping_rem(b))
                }
            }
        },
        (a, b) => {
            let (Some(x), Some(y)) = (as_f64(&a), as_f64(&b)) else {
                return SVal::Null;
            };
            SVal::Float(match op {
                ArithOp::Add => x + y,
                ArithOp::Sub => x - y,
                ArithOp::Mul => x * y,
                ArithOp::Div => x / y,
                ArithOp::Mod => x % y,
            })
        }
    }
}

fn as_f64(v: &SVal) -> Option<f64> {
    match v {
        SVal::Int(i) => Some(*i as f64),
        SVal::Float(f) => Some(*f),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct M {
        headers: HashMap<&'static str, SVal>,
        props: HashMap<&'static str, SVal>,
    }

    impl MessageView for M {
        fn header(&self, name: &str) -> Option<SVal> {
            if is_header(name) {
                Some(self.headers.get(name).cloned().unwrap_or(SVal::Null))
            } else {
                None
            }
        }
        fn property(&self, name: &str) -> SVal {
            self.props.get(name).cloned().unwrap_or(SVal::Null)
        }
    }

    fn msg() -> M {
        let mut headers = HashMap::new();
        headers.insert("JMSCorrelationID", SVal::Str("ORD-A-100".into()));
        headers.insert("JMSPriority", SVal::Int(4));
        headers.insert("JMSDeliveryMode", SVal::Str("PERSISTENT".into()));
        let mut props = HashMap::new();
        props.insert("color", SVal::Str("red".into()));
        props.insert("size", SVal::Int(3));
        props.insert("weight", SVal::Float(2.5));
        props.insert("flag", SVal::Bool(true));
        M { headers, props }
    }

    fn sel(s: &str) -> bool {
        Selector::compile(s).unwrap().unwrap().matches(&msg())
    }

    #[test]
    fn basic_comparisons() {
        assert!(sel("color = 'red'"));
        assert!(!sel("color = 'blue'"));
        assert!(sel("color <> 'blue'"));
        assert!(sel("size > 2 AND size < 4"));
        assert!(sel("size >= 3 and weight <= 2.5"));
        assert!(sel("size = 3.0"));
        assert!(sel("flag = TRUE"));
        assert!(sel("flag"));
        assert!(sel("JMSPriority = 4"));
        assert!(sel("JMSDeliveryMode = 'PERSISTENT'"));
    }

    #[test]
    fn correlation_id_selectors() {
        assert!(sel("JMSCorrelationID IN ('ORD-A-100','ORD-C')"));
        assert!(!sel("JMSCorrelationID IN ('ORD-B')"));
        assert!(sel("JMSCorrelationID LIKE 'ORD-A-%'"));
        assert!(!sel("JMSCorrelationID LIKE 'ORD-B-%'"));
        assert!(sel("JMSCorrelationID NOT LIKE 'ORD-B-%'"));
    }

    #[test]
    fn three_valued_logic() {
        assert!(!sel("missing = 'x'"));
        assert!(!sel("NOT (missing = 'x')"));
        assert!(sel("missing IS NULL"));
        assert!(!sel("missing IS NOT NULL"));
        assert!(sel("color IS NOT NULL"));
        assert!(!sel("missing > 1 OR missing < 1"));
        assert!(sel("missing > 1 OR size = 3"));
        assert!(!sel("missing > 1 AND size = 3"));
        assert!(!sel("missing IN ('a')"));
        assert!(!sel("missing NOT IN ('a')"));
    }

    #[test]
    fn activemq_type_mismatch_is_false() {
        // Mismatched types compare as FALSE (not UNKNOWN), so NOT selects the message.
        assert!(!sel("size = 'three'"));
        assert!(sel("NOT (size = 'three')"));
    }

    #[test]
    fn between_and_arithmetic() {
        assert!(sel("size BETWEEN 1 AND 5"));
        assert!(!sel("size NOT BETWEEN 1 AND 5"));
        assert!(sel("size * 2 + 1 = 7"));
        assert!(sel("-size = -3"));
        assert!(sel("weight / 0.5 = 5"));
        assert!(!sel("size / 0 = 1"));
    }

    #[test]
    fn like_and_escape() {
        let p = LikePattern::compile("a_c%", None);
        assert!(p.matches("abcdef"));
        assert!(!p.matches("ac"));
        let p = LikePattern::compile("100!%", Some('!'));
        assert!(p.matches("100%"));
        assert!(!p.matches("1000"));
        assert!(LikePattern::compile("%", None).matches(""));
        assert!(LikePattern::compile("%b%", None).matches("abc"));
    }

    #[test]
    fn literals() {
        assert!(sel("size = 0x3"));
        assert!(sel("size = 03"));
        assert!(sel("size = 3L"));
        assert!(sel("weight = 25e-1"));
        assert!(sel("color = 'r''ed' OR color = 'red'"));
    }

    #[test]
    fn errors_have_positions() {
        let e = Selector::compile("color = 'red' AN size > 2").unwrap_err();
        assert_eq!(e.column, 14);
        assert!(Selector::compile("JMSCorrelationID = = 'X'").is_err());
        assert!(Selector::compile("color > 'a'").is_err());
        assert!(Selector::compile("a = NULL").is_err());
        assert!(Selector::compile("XPATH '//a'").is_err());
        assert!(Selector::compile("a IN (1, 2)").is_err());
        assert!(Selector::compile("(a = 1").is_err());
        assert!(Selector::compile("   ").unwrap().is_none());
    }

    #[test]
    fn header_only_detection() {
        assert!(!Selector::compile("JMSCorrelationID = 'x'").unwrap().unwrap().uses_properties());
        assert!(Selector::compile("color = 'x'").unwrap().unwrap().uses_properties());
    }
}
