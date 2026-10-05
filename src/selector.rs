// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! JMS message selectors (SQL-92 subset) with the semantics of ActiveMQ's
//! `org.apache.activemq.selector.SelectorParser` and `org.apache.activemq.filter.*`,
//! checked against activemq-client 5.18.x and 6.x: three-valued logic where `null` is
//! UNKNOWN, Java value types with ActiveMQ's conversion rules, and a message is selected
//! only when the whole selector evaluates to TRUE.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

/// A value produced while evaluating a selector. The variants mirror the Java classes
/// ActiveMQ works with, because its comparison and arithmetic rules depend on them.
#[derive(Debug, Clone, PartialEq)]
pub enum SVal {
    Null,
    Bool(bool),
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    /// A decimal literal outside the `long` range (`java.math.BigDecimal` in ActiveMQ).
    BigInt(i128),
    Float(f32),
    Double(f64),
    /// A `char` property (`java.lang.Character`): not a string, comparable only with chars.
    Char(u16),
    Str(String),
    /// A byte array, map or list property: not comparable, equal only to itself. The text
    /// identifies the value and is what string concatenation appends.
    Opaque(Arc<str>),
}

impl SVal {
    fn is_number(&self) -> bool {
        matches!(
            self,
            SVal::Byte(_)
                | SVal::Short(_)
                | SVal::Int(_)
                | SVal::Long(_)
                | SVal::BigInt(_)
                | SVal::Float(_)
                | SVal::Double(_)
        )
    }

    fn is_floating(&self) -> bool {
        matches!(self, SVal::Float(_) | SVal::Double(_))
    }

    /// `Number.intValue()`.
    fn int_value(&self) -> i32 {
        match self {
            SVal::Byte(v) => *v as i32,
            SVal::Short(v) => *v as i32,
            SVal::Int(v) => *v,
            SVal::Long(v) => *v as i32,
            SVal::BigInt(v) => *v as i32,
            SVal::Float(v) => *v as i32,
            SVal::Double(v) => *v as i32,
            _ => 0,
        }
    }

    /// `Number.longValue()`.
    fn long_value(&self) -> i64 {
        match self {
            SVal::Long(v) => *v,
            SVal::BigInt(v) => *v as i64,
            SVal::Float(v) => *v as i64,
            SVal::Double(v) => *v as i64,
            other => other.int_value() as i64,
        }
    }

    /// `Number.doubleValue()`.
    fn double_value(&self) -> f64 {
        match self {
            SVal::Long(v) => *v as f64,
            SVal::BigInt(v) => *v as f64,
            SVal::Float(v) => *v as f64,
            SVal::Double(v) => *v,
            other => other.int_value() as f64,
        }
    }
}

/// Evaluation could not complete: ActiveMQ raises an exception here (for example a number
/// added to a boolean) or the message properties cannot be decoded. The selector then does
/// not select the message, whatever operators surround the failing expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalError;

/// JMS header identifiers, recognised exactly as `org.apache.activemq.filter.PropertyExpression`
/// does in ActiveMQ 5.18.x and 6.x (names are case-sensitive; any other name, including
/// other `JMS*` names, is an application property). Values, as ActiveMQ computes them:
///
/// | Identifier                  | Type    | Value                                                                  |
/// |-----------------------------|---------|------------------------------------------------------------------------|
/// | `JMSDestination`            | String  | `originalDestination`, else `destination`, as `queue://Q`, `topic://T`, `temp-queue://...`, `temp-topic://...`; NULL if none |
/// | `JMSReplyTo`                | String  | `replyTo` in the same form; NULL if none                               |
/// | `JMSType`                   | String  | `type`; NULL if none                                                   |
/// | `JMSDeliveryMode`           | String  | `'PERSISTENT'` or `'NON_PERSISTENT'`                                   |
/// | `JMSPriority`               | int     | `priority` (0-9)                                                       |
/// | `JMSMessageID`              | String  | `MessageId.toString()` (`<producerId>:<producerSequenceId>` or the text view); NULL if none |
/// | `JMSTimestamp`              | long    | `timestamp`, milliseconds                                              |
/// | `JMSCorrelationID`          | String  | `correlationId`; NULL if none                                          |
/// | `JMSExpiration`             | long    | `expiration`, milliseconds, 0 = never                                  |
/// | `JMSRedelivered`            | boolean | `redeliveryCounter > 0`                                                |
/// | `JMSXDeliveryCount`         | int     | `redeliveryCounter + 1`                                                |
/// | `JMSXGroupID`               | String  | `groupID`; NULL if none                                                |
/// | `JMSXUserID`                | String  | `userID`, else the `JMSXUserID` application property                   |
/// | `JMSXGroupSeq`              | int     | `groupSequence` (0 when not set)                                       |
/// | `JMSXProducerTXID`          | String  | `originalTransactionId`, else `transactionId`, as `TX:<connectionId>:<n>` or `XID:[...]`; NULL if none |
/// | `JMSActiveMQBrokerInTime`   | long    | `brokerInTime`                                                         |
/// | `JMSActiveMQBrokerOutTime`  | long    | `brokerOutTime`                                                        |
/// | `JMSActiveMQBrokerPath`     | String  | `Arrays.toString(brokerPath)`: `[id1, id2]`, or the string `null` (never NULL) |
/// | `JMSXGroupFirstForConsumer` | boolean | `jmsxGroupFirstForConsumer`                                            |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Header {
    Destination,
    ReplyTo,
    Type,
    DeliveryMode,
    Priority,
    MessageId,
    Timestamp,
    CorrelationId,
    Expiration,
    Redelivered,
    DeliveryCount,
    GroupId,
    UserId,
    GroupSeq,
    ProducerTxId,
    BrokerInTime,
    BrokerOutTime,
    BrokerPath,
    GroupFirstForConsumer,
}

const HEADERS: &[(&str, Header)] = &[
    ("JMSDestination", Header::Destination),
    ("JMSReplyTo", Header::ReplyTo),
    ("JMSType", Header::Type),
    ("JMSDeliveryMode", Header::DeliveryMode),
    ("JMSPriority", Header::Priority),
    ("JMSMessageID", Header::MessageId),
    ("JMSTimestamp", Header::Timestamp),
    ("JMSCorrelationID", Header::CorrelationId),
    ("JMSExpiration", Header::Expiration),
    ("JMSRedelivered", Header::Redelivered),
    ("JMSXDeliveryCount", Header::DeliveryCount),
    ("JMSXGroupID", Header::GroupId),
    ("JMSXUserID", Header::UserId),
    ("JMSXGroupSeq", Header::GroupSeq),
    ("JMSXProducerTXID", Header::ProducerTxId),
    ("JMSActiveMQBrokerInTime", Header::BrokerInTime),
    ("JMSActiveMQBrokerOutTime", Header::BrokerOutTime),
    ("JMSActiveMQBrokerPath", Header::BrokerPath),
    ("JMSXGroupFirstForConsumer", Header::GroupFirstForConsumer),
];

impl Header {
    pub fn from_name(name: &str) -> Option<Header> {
        HEADERS.iter().find(|(n, _)| *n == name).map(|(_, h)| *h)
    }

    pub fn name(self) -> &'static str {
        HEADERS.iter().find(|(_, h)| *h == self).map(|(n, _)| *n).unwrap_or("")
    }
}

pub fn is_header(name: &str) -> bool {
    Header::from_name(name).is_some()
}

/// Gives the selector access to message headers and properties.
pub trait MessageView {
    /// Value of a JMS header identifier (see [`Header`]).
    fn header(&self, h: Header) -> Result<SVal, EvalError>;
    /// Value of an application property; `SVal::Null` when absent, `Err` when the
    /// properties cannot be decoded.
    fn property(&self, name: &str) -> Result<SVal, EvalError>;
}

/// Message of the exception sent for `XPATH` / `XQUERY` selectors.
pub const XPATH_UNSUPPORTED: &str = "XPath selectors are not supported";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorError {
    /// Zero-based character offset of the offending token.
    pub column: usize,
    pub message: String,
}

impl SelectorError {
    /// Text of the `InvalidSelectorException` sent to the client: the reason, its column
    /// and the selector, except for XPath selectors whose text is fixed.
    pub fn exception_message(&self, selector: &str) -> String {
        if self.message == XPATH_UNSUPPORTED {
            XPATH_UNSUPPORTED.to_string()
        } else {
            format!("{self} in selector: {selector}")
        }
    }
}

impl fmt::Display for SelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at column {}", self.message, self.column)
    }
}

fn error(column: usize, message: impl Into<String>) -> SelectorError {
    SelectorError {
        column,
        message: message.into(),
    }
}

// ---------------------------------------------------------------------------
// Lexer (token rules of ActiveMQ's SelectorParser.jj)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(SVal),
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

/// Whitespace skipped between tokens (the SKIP set of the grammar, not Unicode whitespace).
fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

fn lex(input: &str) -> Result<Vec<Lexed>, SelectorError> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_blank(c) {
            i += 1;
            continue;
        }
        let start = i;
        // Block comments are skipped like whitespace; `--` is not a comment.
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            loop {
                if i + 1 >= chars.len() {
                    return Err(error(start, "Unterminated comment"));
                }
                if chars[i] == '*' && chars[i + 1] == '/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }
        let tok = if c == '\'' {
            let mut s = String::new();
            i += 1;
            loop {
                if i >= chars.len() {
                    return Err(error(start, "Unterminated string literal"));
                }
                if chars[i] == '\'' {
                    if chars.get(i + 1) == Some(&'\'') {
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
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            lex_number(&chars, &mut i, start)?
        } else if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            // Identifiers are ASCII letters, digits, '_' and '$' (no '.').
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
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
                '<' => match chars.get(i) {
                    Some('>') => {
                        i += 1;
                        Tok::Ne
                    }
                    Some('=') => {
                        i += 1;
                        Tok::Le
                    }
                    _ => Tok::Lt,
                },
                '>' => {
                    if chars.get(i) == Some(&'=') {
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
                _ => return Err(error(start, format!("Unexpected character '{c}'"))),
            }
        };
        out.push(Lexed {
            tok,
            col: start,
            text: chars[start..i].iter().collect(),
        });
    }
    out.push(Lexed {
        tok: Tok::End,
        col: chars.len(),
        text: String::new(),
    });
    Ok(out)
}

/// Integer literals become `int` when they fit, else `long` (`ConstantExpression.createFrom*`).
fn integer_literal(v: i64) -> Tok {
    match i32::try_from(v) {
        Ok(i) => Tok::Num(SVal::Int(i)),
        Err(_) => Tok::Num(SVal::Long(v)),
    }
}

/// Numeric literals: decimal with optional `L`, hexadecimal `0x...` and octal `0...` (no
/// suffix), and floating point (`1.5`, `1.`, `.5`, `1e3`, `2.5E-2`, no `f`/`d` suffix).
fn lex_number(chars: &[char], i: &mut usize, start: usize) -> Result<Tok, SelectorError> {
    let digit_at = |k: usize| chars.get(k).is_some_and(|c| c.is_ascii_digit());
    if chars[*i] == '0'
        && matches!(chars.get(*i + 1), Some('x' | 'X'))
        && chars.get(*i + 2).is_some_and(|c| c.is_ascii_hexdigit())
    {
        *i += 2;
        let s = *i;
        while chars.get(*i).is_some_and(|c| c.is_ascii_hexdigit()) {
            *i += 1;
        }
        let digits: String = chars[s..*i].iter().collect();
        return i64::from_str_radix(&digits, 16)
            .map(integer_literal)
            .map_err(|_| error(start, "Hexadecimal literal out of range"));
    }
    let s = *i;
    while digit_at(*i) {
        *i += 1;
    }
    let int_end = *i;
    let mut is_float = false;
    if chars.get(*i) == Some(&'.') {
        is_float = true;
        *i += 1;
        while digit_at(*i) {
            *i += 1;
        }
    }
    if matches!(chars.get(*i), Some('e' | 'E')) {
        let mut k = *i + 1;
        if matches!(chars.get(k), Some('+' | '-')) {
            k += 1;
        }
        if digit_at(k) {
            while digit_at(k) {
                k += 1;
            }
            *i = k;
            is_float = true;
        }
    }
    if is_float {
        let text: String = chars[s..*i].iter().collect();
        return text
            .parse::<f64>()
            .map(|v| Tok::Num(SVal::Double(v)))
            .map_err(|_| error(start, "Invalid decimal literal"));
    }
    let text: String = chars[s..int_end].iter().collect();
    if text.len() > 1 && text.starts_with('0') {
        return i64::from_str_radix(&text, 8)
            .map(integer_literal)
            .map_err(|_| error(start, "Invalid octal literal"));
    }
    if matches!(chars.get(*i), Some('l' | 'L')) {
        *i += 1;
    }
    match text.parse::<i64>() {
        Ok(v) => Ok(integer_literal(v)),
        Err(_) => text
            .parse::<i128>()
            .map(|v| Tok::Num(SVal::BigInt(v)))
            .map_err(|_| error(start, "Integer literal out of range")),
    }
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Expr {
    Const(SVal),
    Header(Header),
    Prop(Box<str>),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    /// A property used where a boolean is required (`UnaryExpression.createBooleanCast`):
    /// NULL stays NULL, a boolean is itself, any other value is FALSE.
    BoolCast(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Eq(Box<Expr>, Box<Expr>),
    Cmp(Box<Expr>, Box<Expr>, CmpOp),
    Arith(Box<Expr>, Box<Expr>, ArithOp),
    Like(Box<Expr>, LikeMatcher),
    In {
        expr: Box<Expr>,
        list: InList,
        negated: bool,
    },
}

#[derive(Debug, Clone, Copy)]
enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    fn test(self, o: Ordering) -> bool {
        match self {
            CmpOp::Lt => o == Ordering::Less,
            CmpOp::Le => o != Ordering::Greater,
            CmpOp::Gt => o == Ordering::Greater,
            CmpOp::Ge => o != Ordering::Less,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
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
    fn new(items: Vec<String>) -> InList {
        if items.len() > 8 {
            InList::Set(items.into_iter().collect())
        } else {
            InList::Small(items)
        }
    }

    fn contains(&self, s: &str) -> bool {
        match self {
            InList::Small(v) => v.iter().any(|x| x == s),
            InList::Set(h) => h.contains(s),
        }
    }
}

/// LIKE pattern as a sequence of literal characters and `_` / `%` wildcards. This is the
/// generic matcher; it follows `ComparisonExpression.LikeExpression`: the escape character
/// makes a following `_`, `%` or escape character literal and is otherwise itself literal,
/// matching is case-sensitive and `_` / `%` also match line terminators.
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
                if next == '_' || next == '%' || next == c {
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
        // Consecutive '%' match the same strings as one.
        parts.dedup_by(|a, b| *a == LikePart::Any && *b == LikePart::Any);
        LikePattern { parts }
    }

    fn matches(&self, s: &str) -> bool {
        let text: Vec<char> = s.chars().collect();
        // Wildcard matching with backtracking on the last '%'.
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

/// A LIKE pattern compiled to the cheapest equivalent test.
#[derive(Debug, Clone)]
enum LikeMatcher {
    /// No wildcard: string equality.
    Exact(String),
    /// `abc%`.
    Prefix(String),
    /// `%abc`.
    Suffix(String),
    /// `%abc%`.
    Contains(String),
    /// `%`: every string.
    All,
    Generic(LikePattern),
}

impl LikeMatcher {
    fn compile(pattern: &str, escape: Option<char>) -> LikeMatcher {
        let generic = LikePattern::compile(pattern, escape);
        let parts = &generic.parts;
        let literal = |ps: &[LikePart]| -> Option<String> {
            ps.iter()
                .map(|p| match p {
                    LikePart::Lit(c) => Some(*c),
                    _ => None,
                })
                .collect()
        };
        let n = parts.len();
        let leading = n > 0 && parts[0] == LikePart::Any;
        let trailing = n > 0 && parts[n - 1] == LikePart::Any;
        let fast = if n == 1 && leading {
            Some(LikeMatcher::All)
        } else {
            match (leading, trailing) {
                (false, false) => literal(parts).map(LikeMatcher::Exact),
                (false, true) => literal(&parts[..n - 1]).map(LikeMatcher::Prefix),
                (true, false) => literal(&parts[1..]).map(LikeMatcher::Suffix),
                (true, true) => literal(&parts[1..n - 1]).map(LikeMatcher::Contains),
            }
        };
        fast.unwrap_or(LikeMatcher::Generic(generic))
    }

    fn matches(&self, s: &str) -> bool {
        match self {
            LikeMatcher::Exact(l) => s == l,
            LikeMatcher::Prefix(l) => s.starts_with(l.as_str()),
            LikeMatcher::Suffix(l) => s.ends_with(l.as_str()),
            LikeMatcher::Contains(l) => s.contains(l.as_str()),
            LikeMatcher::All => true,
            LikeMatcher::Generic(p) => p.matches(s),
        }
    }
}

// ---------------------------------------------------------------------------
// Parser (recursive descent, one function per precedence level of SelectorParser.jj)
// ---------------------------------------------------------------------------
//
//   orExpr       := andExpr ( OR andExpr )*
//   andExpr      := equalityExpr ( AND equalityExpr )*
//   equalityExpr := comparisonExpr ( '=' comparisonExpr | '<>' comparisonExpr | IS [NOT] NULL )*
//   comparisonExpr := addExpr ( ('<'|'<='|'>'|'>=') addExpr | [NOT] LIKE string [ESCAPE string]
//                     | [NOT] BETWEEN addExpr AND addExpr | [NOT] IN '(' string (',' string)* ')' )*
//   addExpr      := multExpr ( ('+'|'-') multExpr )*
//   multExpr     := unaryExpr ( ('*'|'/'|'%') unaryExpr )*
//   unaryExpr    := '+' unaryExpr | '-' unaryExpr | NOT unaryExpr | primaryExpr
//   primaryExpr  := literal | identifier | '(' orExpr ')'
//
// As in ActiveMQ, NOT binds tighter than comparisons: `NOT a = 1` is `(NOT a) = 1`.

struct Parser<'a> {
    toks: &'a [Lexed],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn col(&self) -> usize {
        self.toks[self.pos].col
    }

    fn advance(&mut self) -> &'a Lexed {
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
            error(t.col, format!("Unexpected end of selector, expected {expected}"))
        } else {
            error(t.col, format!("Unexpected token '{}'", t.text))
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> Result<(), SelectorError> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(self.error_here(what))
        }
    }

    fn string_literal(&mut self, what: &str) -> Result<String, SelectorError> {
        match &self.toks[self.pos].tok {
            Tok::Str(s) => {
                self.advance();
                Ok(s.clone())
            }
            _ => Err(self.error_here(what)),
        }
    }

    fn or_expr(&mut self) -> Result<Expr, SelectorError> {
        let col = self.col();
        let first = self.and_expr()?;
        if self.peek() != &Tok::Or {
            return Ok(first);
        }
        let mut items = vec![as_boolean(first, col)?];
        while self.eat(&Tok::Or) {
            let col = self.col();
            items.push(as_boolean(self.and_expr()?, col)?);
        }
        Ok(Expr::Or(items))
    }

    fn and_expr(&mut self) -> Result<Expr, SelectorError> {
        let col = self.col();
        let first = self.equality()?;
        if self.peek() != &Tok::And {
            return Ok(first);
        }
        let mut items = vec![as_boolean(first, col)?];
        while self.eat(&Tok::And) {
            let col = self.col();
            items.push(as_boolean(self.equality()?, col)?);
        }
        Ok(Expr::And(items))
    }

    fn equality(&mut self) -> Result<Expr, SelectorError> {
        let mut left = self.comparison()?;
        loop {
            let col = self.col();
            match self.peek() {
                Tok::Eq | Tok::Ne => {
                    let negated = self.advance().tok == Tok::Ne;
                    let right = self.comparison()?;
                    check_equal_operands(&left, &right, col)?;
                    let e = Expr::Eq(Box::new(left), Box::new(right));
                    left = if negated { Expr::Not(Box::new(e)) } else { e };
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
            let col = self.col();
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
                left = ordering(left, right, op, col)?;
                continue;
            }
            let negated = self.peek() == &Tok::Not
                && matches!(
                    self.toks.get(self.pos + 1).map(|t| &t.tok),
                    Some(Tok::Like | Tok::Between | Tok::In)
                );
            if negated {
                self.advance();
            }
            let col = self.col();
            match self.peek() {
                Tok::Like => {
                    self.advance();
                    let pattern = self.string_literal("a string literal after LIKE")?;
                    let mut escape = None;
                    if self.eat(&Tok::Escape) {
                        let col = self.col();
                        let e = self.string_literal("a string literal after ESCAPE")?;
                        let mut it = e.chars();
                        match (it.next(), it.next()) {
                            (Some(c), None) => escape = Some(c),
                            _ => return Err(error(col, "The ESCAPE string literal must be exactly one character")),
                        }
                    }
                    let e = Expr::Like(Box::new(left), LikeMatcher::compile(&pattern, escape));
                    left = if negated { Expr::Not(Box::new(e)) } else { e };
                }
                Tok::Between => {
                    self.advance();
                    let low = self.additive()?;
                    self.expect(&Tok::And, "AND")?;
                    let high = self.additive()?;
                    // ActiveMQ: `x BETWEEN a AND b` is `x >= a AND x <= b`, and
                    // `x NOT BETWEEN a AND b` is `x < a OR x > b`.
                    left = if negated {
                        Expr::Or(vec![
                            ordering(left.clone(), low, CmpOp::Lt, col)?,
                            ordering(left, high, CmpOp::Gt, col)?,
                        ])
                    } else {
                        Expr::And(vec![
                            ordering(left.clone(), low, CmpOp::Ge, col)?,
                            ordering(left, high, CmpOp::Le, col)?,
                        ])
                    };
                }
                Tok::In => {
                    self.advance();
                    if !matches!(left, Expr::Header(_) | Expr::Prop(_)) {
                        return Err(error(col, "Expected a property for IN"));
                    }
                    self.expect(&Tok::LParen, "(")?;
                    let mut items = vec![self.string_literal("a string literal in the IN list")?];
                    while self.eat(&Tok::Comma) {
                        items.push(self.string_literal("a string literal in the IN list")?);
                    }
                    self.expect(&Tok::RParen, ")")?;
                    left = Expr::In {
                        expr: Box::new(left),
                        list: InList::new(items),
                        negated,
                    };
                }
                _ => return Ok(left),
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
        let col = self.col();
        match self.peek() {
            Tok::Plus => {
                self.advance();
                self.unary()
            }
            Tok::Minus => {
                self.advance();
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Tok::Not => {
                self.advance();
                let col = self.col();
                let e = self.unary()?;
                Ok(Expr::Not(Box::new(as_boolean(e, col)?)))
            }
            Tok::XPath | Tok::XQuery => Err(error(col, XPATH_UNSUPPORTED)),
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expr, SelectorError> {
        let t = &self.toks[self.pos];
        let e = match &t.tok {
            Tok::Str(s) => Expr::Const(SVal::Str(s.clone())),
            Tok::Num(v) => Expr::Const(v.clone()),
            Tok::True => Expr::Const(SVal::Bool(true)),
            Tok::False => Expr::Const(SVal::Bool(false)),
            Tok::Null => Expr::Const(SVal::Null),
            Tok::Ident(name) => match Header::from_name(name) {
                Some(h) => Expr::Header(h),
                None => Expr::Prop(name.as_str().into()),
            },
            Tok::LParen => {
                self.advance();
                let e = self.or_expr()?;
                self.expect(&Tok::RParen, ")")?;
                return Ok(e);
            }
            _ => return Err(self.error_here("an expression")),
        };
        self.advance();
        Ok(e)
    }
}

/// Builds an ordering comparison after `ComparisonExpression.checkLessThanOperand` on both sides.
fn ordering(left: Expr, right: Expr, op: CmpOp, col: usize) -> Result<Expr, SelectorError> {
    check_ordering_operand(&left, col)?;
    check_ordering_operand(&right, col)?;
    Ok(Expr::Cmp(Box::new(left), Box::new(right), op))
}

/// Expressions that ActiveMQ types as `BooleanExpression`.
fn is_boolean_expr(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Const(SVal::Bool(_) | SVal::Null)
            | Expr::Not(_)
            | Expr::BoolCast(_)
            | Expr::And(_)
            | Expr::Or(_)
            | Expr::Eq(..)
            | Expr::Cmp(..)
            | Expr::Like(..)
            | Expr::In { .. }
    )
}

/// `SelectorParser.asBooleanExpression`: operands of AND, OR, NOT and the whole selector must
/// be boolean expressions or identifiers (which are then cast to boolean).
fn as_boolean(e: Expr, col: usize) -> Result<Expr, SelectorError> {
    if is_boolean_expr(&e) {
        Ok(e)
    } else if matches!(e, Expr::Header(_) | Expr::Prop(_)) {
        Ok(Expr::BoolCast(Box::new(e)))
    } else {
        Err(error(col, "Expression will not result in a boolean value"))
    }
}

/// `ComparisonExpression.checkEqualOperand` and `checkEqualOperandCompatability`: a NULL
/// literal cannot be an operand of `=` / `<>`, and a TRUE/FALSE literal on the left cannot
/// be compared with a literal of another type on the right.
fn check_equal_operands(left: &Expr, right: &Expr, col: usize) -> Result<(), SelectorError> {
    if matches!(left, Expr::Const(SVal::Null)) || matches!(right, Expr::Const(SVal::Null)) {
        return Err(error(col, "NULL cannot be compared; use IS NULL"));
    }
    if let (Expr::Const(SVal::Bool(_)), Expr::Const(r)) = (left, right) {
        if !matches!(r, SVal::Bool(_)) {
            return Err(error(col, "Incompatible constants cannot be compared"));
        }
    }
    Ok(())
}

/// `ComparisonExpression.checkLessThanOperand`: literals must be numbers and boolean
/// expressions cannot be ordered.
fn check_ordering_operand(e: &Expr, col: usize) -> Result<(), SelectorError> {
    match e {
        Expr::Const(v) if v.is_number() => Ok(()),
        Expr::Const(_) => Err(error(col, "Value cannot be compared with < > <= >=")),
        other if is_boolean_expr(other) => Err(error(col, "Boolean expressions cannot be compared")),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Compiled selector
// ---------------------------------------------------------------------------

/// A compiled selector.
#[derive(Debug, Clone)]
pub struct Selector {
    text: String,
    expr: Expr,
    uses_properties: bool,
}

impl Selector {
    /// Compiles a selector. Returns `Ok(None)` for an empty or blank selector.
    pub fn compile(text: &str) -> Result<Option<Selector>, SelectorError> {
        // Java's String.trim(): every character up to U+0020.
        if text.trim_matches(|c: char| c <= ' ').is_empty() {
            return Ok(None);
        }
        let toks = lex(text)?;
        let mut p = Parser { toks: &toks, pos: 0 };
        let expr = p.or_expr()?;
        if p.peek() != &Tok::End {
            return Err(p.error_here("end of selector"));
        }
        let expr = as_boolean(expr, 0)?;
        let mut uses_properties = false;
        visit(&expr, &mut |e| {
            if matches!(e, Expr::Prop(_) | Expr::Header(Header::UserId)) {
                uses_properties = true;
            }
        });
        Ok(Some(Selector {
            text: text.to_string(),
            expr,
            uses_properties,
        }))
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// True when evaluation may need application properties (not only JMS headers).
    pub fn uses_properties(&self) -> bool {
        self.uses_properties
    }

    /// Evaluates the selector: `Ok(Bool)` or `Ok(Null)` (UNKNOWN), or `Err` where ActiveMQ
    /// would raise an exception.
    pub fn evaluate(&self, view: &dyn MessageView) -> Result<SVal, EvalError> {
        eval(&self.expr, view)
    }

    /// True when the selector evaluates to TRUE.
    pub fn matches(&self, view: &dyn MessageView) -> bool {
        matches!(eval(&self.expr, view), Ok(SVal::Bool(true)))
    }
}

fn visit(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Const(_) | Expr::Header(_) | Expr::Prop(_) => {}
        Expr::Neg(a) | Expr::Not(a) | Expr::BoolCast(a) => visit(a, f),
        Expr::And(v) | Expr::Or(v) => v.iter().for_each(|x| visit(x, f)),
        Expr::Eq(a, b) | Expr::Cmp(a, b, _) | Expr::Arith(a, b, _) => {
            visit(a, f);
            visit(b, f);
        }
        Expr::Like(a, _) | Expr::In { expr: a, .. } => visit(a, f),
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

fn eval(e: &Expr, v: &dyn MessageView) -> Result<SVal, EvalError> {
    Ok(match e {
        Expr::Const(c) => c.clone(),
        Expr::Header(h) => return v.header(*h),
        Expr::Prop(name) => return v.property(name),
        Expr::Neg(a) => return negate(eval(a, v)?),
        Expr::Not(a) => match eval(a, v)? {
            SVal::Bool(b) => SVal::Bool(!b),
            SVal::Null => SVal::Null,
            _ => return Err(EvalError),
        },
        Expr::BoolCast(a) => match eval(a, v)? {
            SVal::Null => SVal::Null,
            SVal::Bool(b) => SVal::Bool(b),
            _ => SVal::Bool(false),
        },
        Expr::And(items) => {
            let mut unknown = false;
            for x in items {
                match eval(x, v)? {
                    SVal::Bool(true) => {}
                    SVal::Bool(false) => return Ok(SVal::Bool(false)),
                    SVal::Null => unknown = true,
                    _ => return Err(EvalError),
                }
            }
            if unknown {
                SVal::Null
            } else {
                SVal::Bool(true)
            }
        }
        Expr::Or(items) => {
            let mut unknown = false;
            for x in items {
                match eval(x, v)? {
                    SVal::Bool(true) => return Ok(SVal::Bool(true)),
                    SVal::Bool(false) => {}
                    SVal::Null => unknown = true,
                    _ => return Err(EvalError),
                }
            }
            if unknown {
                SVal::Null
            } else {
                SVal::Bool(false)
            }
        }
        Expr::Eq(a, b) => {
            // Both sides are always evaluated, as in `ComparisonExpression.EqualsExpression`.
            let l = eval(a, v)?;
            let r = eval(b, v)?;
            return equals(&l, &r);
        }
        Expr::Cmp(a, b, op) => {
            let l = eval(a, v)?;
            match l {
                SVal::Null => return Ok(SVal::Null),
                SVal::Opaque(_) => return Err(EvalError),
                _ => {}
            }
            let r = eval(b, v)?;
            match r {
                SVal::Null => return Ok(SVal::Null),
                SVal::Opaque(_) => return Err(EvalError),
                _ => {}
            }
            match compare(&l, &r)? {
                Some(o) => SVal::Bool(op.test(o)),
                None => SVal::Bool(false),
            }
        }
        Expr::Arith(a, b, op) => {
            let l = eval(a, v)?;
            if l == SVal::Null {
                return Ok(SVal::Null);
            }
            let r = eval(b, v)?;
            if r == SVal::Null {
                return Ok(SVal::Null);
            }
            return arith(l, r, *op);
        }
        Expr::Like(a, m) => match eval(a, v)? {
            SVal::Null => SVal::Null,
            SVal::Str(s) => SVal::Bool(m.matches(&s)),
            _ => SVal::Bool(false),
        },
        Expr::In { expr, list, negated } => match eval(expr, v)? {
            SVal::Str(s) => SVal::Bool(list.contains(&s) != *negated),
            _ => SVal::Null,
        },
    })
}

/// `EqualsExpression`: NULL = NULL is TRUE, NULL = x is UNKNOWN, x = NULL is FALSE; then
/// Java equality, then the conversions of [`compare`]; values that cannot be compared are
/// unequal.
fn equals(l: &SVal, r: &SVal) -> Result<SVal, EvalError> {
    Ok(match (l, r) {
        (SVal::Null, SVal::Null) => SVal::Bool(true),
        (SVal::Null, _) => SVal::Null,
        (_, SVal::Null) => SVal::Bool(false),
        (SVal::Opaque(a), SVal::Opaque(b)) => SVal::Bool(a == b),
        (SVal::Opaque(_), _) | (_, SVal::Opaque(_)) => SVal::Bool(false),
        _ => SVal::Bool(compare(l, r)? == Some(Ordering::Equal)),
    })
}

/// `ComparisonExpression.compare`: values of the same class use `compareTo`; a narrower
/// number on the left is widened to the class of the right operand, a `long` on the left is
/// compared with an `int` as `long`, and a `float`/`double` on the left converts an integer
/// on the right. Any other pair does not compare (`None`, FALSE), except `double` against
/// `float`, which raises an exception in ActiveMQ (`Err`).
fn compare(l: &SVal, r: &SVal) -> Result<Option<Ordering>, EvalError> {
    use SVal::*;
    Ok(Some(match (l, r) {
        (Bool(a), Bool(b)) => a.cmp(b),
        (Char(a), Char(b)) => a.cmp(b),
        (Str(a), Str(b)) => utf16_cmp(a, b),
        (BigInt(a), BigInt(b)) => a.cmp(b),
        (Byte(a), Byte(b)) => a.cmp(b),
        (Short(a), Short(b)) => a.cmp(b),
        (Int(a), Int(b)) => a.cmp(b),
        (Long(a), Long(b)) => a.cmp(b),
        (Float(a), Float(b)) => java_cmp_f32(*a, *b),
        (Double(a), Double(b)) => java_cmp_f64(*a, *b),
        (Byte(a), Short(b)) => (*a as i16).cmp(b),
        (Byte(a), Int(b)) => (*a as i32).cmp(b),
        (Byte(a), Long(b)) => (*a as i64).cmp(b),
        (Byte(a), Float(b)) => java_cmp_f32(*a as f32, *b),
        (Byte(a), Double(b)) => java_cmp_f64(*a as f64, *b),
        (Short(a), Int(b)) => (*a as i32).cmp(b),
        (Short(a), Long(b)) => (*a as i64).cmp(b),
        (Short(a), Float(b)) => java_cmp_f32(*a as f32, *b),
        (Short(a), Double(b)) => java_cmp_f64(*a as f64, *b),
        (Int(a), Long(b)) => (*a as i64).cmp(b),
        (Int(a), Float(b)) => java_cmp_f32(*a as f32, *b),
        (Int(a), Double(b)) => java_cmp_f64(*a as f64, *b),
        (Long(a), Int(b)) => a.cmp(&(*b as i64)),
        (Long(a), Float(b)) => java_cmp_f32(*a as f32, *b),
        (Long(a), Double(b)) => java_cmp_f64(*a as f64, *b),
        (Float(a), Int(b)) => java_cmp_f32(*a, *b as f32),
        (Float(a), Long(b)) => java_cmp_f32(*a, *b as f32),
        (Float(a), Double(b)) => java_cmp_f64(*a as f64, *b),
        (Double(a), Int(b)) => java_cmp_f64(*a, *b as f64),
        (Double(a), Long(b)) => java_cmp_f64(*a, *b as f64),
        (Double(_), Float(_)) => return Err(EvalError),
        _ => return Ok(None),
    }))
}

/// `String.compareTo`: UTF-16 code unit order.
fn utf16_cmp(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        a.cmp(b)
    } else {
        a.encode_utf16().cmp(b.encode_utf16())
    }
}

/// `Double.compare`: -0.0 is below 0.0 and NaN is above everything and equal to itself.
fn java_cmp_f64(a: f64, b: f64) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        let bits = |x: f64| {
            if x.is_nan() {
                0x7ff8_0000_0000_0000_i64
            } else {
                x.to_bits() as i64
            }
        };
        bits(a).cmp(&bits(b))
    }
}

/// `Float.compare`.
fn java_cmp_f32(a: f32, b: f32) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        let bits = |x: f32| {
            if x.is_nan() {
                0x7fc0_0000_i32
            } else {
                x.to_bits() as i32
            }
        };
        bits(a).cmp(&bits(b))
    }
}

/// `ArithmeticExpression`: `+` with a string on the left concatenates the Java text of the
/// right operand; otherwise both operands must be numbers. `+`, `-`, `*` give a `double` if
/// either operand is `float`/`double`, else a `long` if either is `long`, else a 32-bit `int`
/// (wrapping on overflow). `/` and `%` always compute in `double`, so division by zero gives
/// ±Infinity or NaN.
fn arith(l: SVal, r: SVal, op: ArithOp) -> Result<SVal, EvalError> {
    if op == ArithOp::Add {
        if let SVal::Str(s) = &l {
            return Ok(SVal::Str(format!("{s}{}", java_string(&r))));
        }
    }
    if !l.is_number() || !r.is_number() {
        return Err(EvalError);
    }
    Ok(match op {
        ArithOp::Div => SVal::Double(l.double_value() / r.double_value()),
        ArithOp::Mod => SVal::Double(l.double_value() % r.double_value()),
        _ if l.is_floating() || r.is_floating() => {
            let (a, b) = (l.double_value(), r.double_value());
            SVal::Double(match op {
                ArithOp::Add => a + b,
                ArithOp::Sub => a - b,
                _ => a * b,
            })
        }
        _ if matches!(l, SVal::Long(_)) || matches!(r, SVal::Long(_)) => {
            let (a, b) = (l.long_value(), r.long_value());
            SVal::Long(match op {
                ArithOp::Add => a.wrapping_add(b),
                ArithOp::Sub => a.wrapping_sub(b),
                _ => a.wrapping_mul(b),
            })
        }
        _ => {
            let (a, b) = (l.int_value(), r.int_value());
            SVal::Int(match op {
                ArithOp::Add => a.wrapping_add(b),
                ArithOp::Sub => a.wrapping_sub(b),
                _ => a.wrapping_mul(b),
            })
        }
    })
}

/// `UnaryExpression.createNegate`: NULL and non-numbers give NULL; `byte` and `short` raise
/// an exception in ActiveMQ.
fn negate(v: SVal) -> Result<SVal, EvalError> {
    Ok(match v {
        SVal::Int(i) => SVal::Int(i.wrapping_neg()),
        SVal::Long(l) => SVal::Long(l.wrapping_neg()),
        SVal::Float(f) => SVal::Float(-f),
        SVal::Double(d) => SVal::Double(-d),
        SVal::BigInt(b) if -b == i64::MIN as i128 => SVal::Long(i64::MIN),
        SVal::BigInt(b) => SVal::BigInt(-b),
        SVal::Byte(_) | SVal::Short(_) => return Err(EvalError),
        _ => SVal::Null,
    })
}

/// `String.valueOf` of a value, as appended by string concatenation.
fn java_string(v: &SVal) -> String {
    match v {
        SVal::Null => "null".to_string(),
        SVal::Bool(b) => b.to_string(),
        SVal::Byte(x) => x.to_string(),
        SVal::Short(x) => x.to_string(),
        SVal::Int(x) => x.to_string(),
        SVal::Long(x) => x.to_string(),
        SVal::BigInt(x) => x.to_string(),
        SVal::Float(x) => java_float_text(
            format!("{x:e}"),
            x.is_nan(),
            x.is_infinite(),
            *x == 0.0,
            x.is_sign_negative(),
        ),
        SVal::Double(x) => java_float_text(
            format!("{x:e}"),
            x.is_nan(),
            x.is_infinite(),
            *x == 0.0,
            x.is_sign_negative(),
        ),
        SVal::Char(c) => String::from_utf16_lossy(&[*c]),
        SVal::Str(s) => s.clone(),
        SVal::Opaque(t) => t.to_string(),
    }
}

/// `Double.toString` / `Float.toString` from the shortest round-trip digits (`{:e}` form):
/// plain notation with at least one fractional digit for 10^-3 <= |x| < 10^7, otherwise
/// `d.dddE<n>`.
fn java_float_text(sci: String, nan: bool, inf: bool, zero: bool, negative: bool) -> String {
    let sign = if negative { "-" } else { "" };
    if nan {
        return "NaN".to_string();
    }
    if inf {
        return format!("{sign}Infinity");
    }
    if zero {
        return format!("{sign}0.0");
    }
    let body = sci.trim_start_matches('-');
    let (mantissa, exp) = body.split_once('e').unwrap_or((body, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let n = digits.len() as i32;
    if (-3..7).contains(&exp) {
        let point = exp + 1;
        let text = if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point >= n {
            format!("{}{}.0", digits, "0".repeat((point - n) as usize))
        } else {
            format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
        };
        format!("{sign}{text}")
    } else {
        let frac = if n > 1 { &digits[1..] } else { "0" };
        format!("{sign}{}.{}E{}", &digits[..1], frac, exp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashMap;

    /// A message view over fixed values that counts property lookups.
    struct M {
        headers: HashMap<Header, SVal>,
        props: HashMap<&'static str, SVal>,
        broken: bool,
        lookups: Cell<usize>,
    }

    impl MessageView for M {
        fn header(&self, h: Header) -> Result<SVal, EvalError> {
            Ok(self.headers.get(&h).cloned().unwrap_or(SVal::Null))
        }
        fn property(&self, name: &str) -> Result<SVal, EvalError> {
            self.lookups.set(self.lookups.get() + 1);
            if self.broken {
                return Err(EvalError);
            }
            Ok(self.props.get(name).cloned().unwrap_or(SVal::Null))
        }
    }

    fn msg() -> M {
        let mut headers = HashMap::new();
        headers.insert(Header::CorrelationId, SVal::Str("ORD-A-100".into()));
        headers.insert(Header::Priority, SVal::Int(4));
        headers.insert(Header::DeliveryMode, SVal::Str("PERSISTENT".into()));
        let mut props = HashMap::new();
        props.insert("color", SVal::Str("red".into()));
        props.insert("size", SVal::Int(3));
        props.insert("weight", SVal::Double(2.5));
        props.insert("flag", SVal::Bool(true));
        props.insert("off", SVal::Bool(false));
        props.insert("s", SVal::Short(300));
        props.insert("b", SVal::Byte(7));
        props.insert("l", SVal::Long(10_000_000_000));
        props.insert("f", SVal::Float(1.5));
        props.insert("c", SVal::Char('x' as u16));
        props.insert("zero", SVal::Int(0));
        props.insert("str5", SVal::Str("5".into()));
        props.insert("bytes", SVal::Opaque("[B@bytes".into()));
        M {
            headers,
            props,
            broken: false,
            lookups: Cell::new(0),
        }
    }

    fn compile(s: &str) -> Selector {
        Selector::compile(s).unwrap_or_else(|e| panic!("{s}: {e}")).unwrap()
    }

    fn eval_on(m: &M, s: &str) -> Result<SVal, EvalError> {
        compile(s).evaluate(m)
    }

    /// Tri-state result: Some(true) TRUE, Some(false) FALSE, None UNKNOWN; panics on Err.
    fn tri(s: &str) -> Option<bool> {
        match eval_on(&msg(), s) {
            Ok(SVal::Bool(b)) => Some(b),
            Ok(SVal::Null) => None,
            other => panic!("{s}: {other:?}"),
        }
    }

    fn sel(s: &str) -> bool {
        compile(s).matches(&msg())
    }

    fn err_col(s: &str) -> usize {
        Selector::compile(s).expect_err(s).column
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
    fn three_valued_truth_tables() {
        let lit = |v: Option<bool>| match v {
            Some(true) => "TRUE",
            Some(false) => "FALSE",
            None => "NULL",
        };
        let vals = [Some(true), Some(false), None];
        for a in vals {
            assert_eq!(tri(&format!("NOT {}", lit(a))), a.map(|x| !x), "NOT {}", lit(a));
            for b in vals {
                let and = match (a, b) {
                    (Some(false), _) | (_, Some(false)) => Some(false),
                    (Some(true), Some(true)) => Some(true),
                    _ => None,
                };
                let or = match (a, b) {
                    (Some(true), _) | (_, Some(true)) => Some(true),
                    (Some(false), Some(false)) => Some(false),
                    _ => None,
                };
                assert_eq!(
                    tri(&format!("{} AND {}", lit(a), lit(b))),
                    and,
                    "{} AND {}",
                    lit(a),
                    lit(b)
                );
                assert_eq!(
                    tri(&format!("{} OR {}", lit(a), lit(b))),
                    or,
                    "{} OR {}",
                    lit(a),
                    lit(b)
                );
            }
        }
        // Properties stand for UNKNOWN when absent.
        assert_eq!(tri("missing = 'x'"), None);
        assert_eq!(tri("NOT (missing = 'x')"), None);
        assert_eq!(tri("missing IS NULL"), Some(true));
        assert_eq!(tri("missing IS NOT NULL"), Some(false));
        assert_eq!(tri("missing > 1 OR missing < 1"), None);
        assert_eq!(tri("missing > 1 OR size = 3"), Some(true));
        assert_eq!(tri("missing > 1 AND size = 3"), None);
        assert_eq!(tri("missing > 1 AND size = 4"), Some(false));
    }

    #[test]
    fn null_equality() {
        // Two NULL values are equal; NULL on the left is UNKNOWN; NULL on the right is FALSE.
        assert_eq!(tri("missing = other"), Some(true));
        assert_eq!(tri("missing = color"), None);
        assert_eq!(tri("color = missing"), Some(false));
        assert_eq!(tri("color <> missing"), Some(true));
        assert_eq!(tri("NULL IS NULL"), Some(true));
        // A NULL literal is not allowed with = / <>.
        assert!(Selector::compile("missing = NULL").is_err());
        assert!(Selector::compile("NULL = NULL").is_err());
    }

    #[test]
    fn not_binds_tighter_than_comparisons() {
        // `not b is null` is `(NOT b) IS NULL`: NOT of a non-boolean property is NOT FALSE.
        assert_eq!(tri("size = 3 and not color is null"), Some(false));
        assert_eq!(tri("size = 3 and not missing is null"), Some(true));
        assert_eq!(tri("NOT size"), Some(true));
        assert_eq!(tri("NOT flag"), Some(false));
        assert_eq!(tri("NOT missing"), None);
        assert_eq!(tri("NOT size = 3"), Some(false));
        assert_eq!(tri("NOT flag = FALSE"), Some(true));
        assert!(Selector::compile("NOT size > 4").is_err());
        assert!(Selector::compile("NOT color IN ('red')").is_err());
        assert!(Selector::compile("NOT -size = 3").is_err());
    }

    #[test]
    fn precedence() {
        assert_eq!(tri("size = 1 OR size = 3 AND color = 'blue'"), Some(false));
        assert_eq!(tri("size = 3 OR size = 1 AND color = 'blue'"), Some(true));
        assert_eq!(tri("(size = 1 OR size = 3) AND color = 'red'"), Some(true));
        assert_eq!(tri("1 + 2 * 3 = 7"), Some(true));
        assert_eq!(tri("(1 + 2) * 3 = 9"), Some(true));
        assert_eq!(tri("10 - 2 - 3 = 5"), Some(true));
        assert_eq!(tri("12 / 2 / 3 = 2"), Some(true));
        assert_eq!(tri("size IS NULL = FALSE"), Some(true));
        assert_eq!(tri("size > 1 = TRUE"), Some(true));
        assert_eq!(tri("-size = -3"), Some(true));
        assert_eq!(tri("size = - -3"), Some(true));
        assert_eq!(tri("size = --3"), Some(true));
        assert_eq!(tri("size BETWEEN 1 + 1 AND 2 * 3"), Some(true));
        assert!(Selector::compile("1 < 2 < 3").is_err());
    }

    #[test]
    fn type_rules() {
        // Incompatible types compare as FALSE, so NOT selects the message.
        assert_eq!(tri("str5 = 5"), Some(false));
        assert_eq!(tri("NOT (str5 = 5)"), Some(true));
        assert_eq!(tri("size <> 'a'"), Some(true));
        // Ordering of two string or boolean values works at run time.
        assert_eq!(tri("color > str5"), Some(true));
        assert_eq!(tri("flag > off"), Some(true));
        // Char properties are not strings.
        assert_eq!(tri("c = 'x'"), Some(false));
        assert_eq!(tri("c LIKE 'x'"), Some(false));
        assert_eq!(tri("c IN ('x')"), None);
        // Widening goes from the left operand only.
        assert_eq!(tri("s = 300"), Some(true));
        assert_eq!(tri("300 = s"), Some(false));
        assert_eq!(tri("b < s"), Some(true));
        assert_eq!(tri("s > b"), Some(false));
        assert_eq!(tri("f = 1.5"), Some(true));
        assert_eq!(eval_on(&msg(), "1.5 = f"), Err(EvalError));
        // LIKE on a non-string is FALSE, IN on a non-string is UNKNOWN.
        assert_eq!(tri("size LIKE '3'"), Some(false));
        assert_eq!(tri("size NOT LIKE '3'"), Some(true));
        assert_eq!(tri("size IN ('3')"), None);
        assert_eq!(tri("size NOT IN ('3')"), None);
        // Byte arrays equal only themselves and cannot be ordered.
        assert_eq!(tri("bytes = bytes"), Some(true));
        assert_eq!(tri("bytes IS NOT NULL"), Some(true));
        assert_eq!(eval_on(&msg(), "bytes > 1"), Err(EvalError));
    }

    #[test]
    fn arithmetic() {
        assert_eq!(tri("size * 2 + 1 = 7"), Some(true));
        assert_eq!(tri("weight / 0.5 = 5"), Some(true));
        // Division and modulo are computed in double.
        assert_eq!(tri("7 / 2 = 3.5"), Some(true));
        assert_eq!(tri("size % 2 = 1"), Some(true));
        assert_eq!(tri("-7 % 3 = -1"), Some(true));
        assert_eq!(tri("weight % 2 = 0.5"), Some(true));
        // Division by zero: Infinity / NaN, not UNKNOWN.
        assert_eq!(tri("size / 0 > 1000000"), Some(true));
        assert_eq!(tri("-size / zero < 0"), Some(true));
        assert_eq!(tri("size / 0 IS NULL"), Some(false));
        assert_eq!(tri("zero / 0.0 = zero / 0.0"), Some(true));
        assert_eq!(tri("zero / 0.0 > 1e308"), Some(true));
        assert!(!sel("size / 0 = 1"));
        // 32-bit int arithmetic wraps; long arithmetic when a long is involved.
        assert_eq!(tri("2147483647 + 1 < 0"), Some(true));
        assert_eq!(tri("2147483648 - 1 = 2147483647"), Some(true));
        assert_eq!(tri("size * 1000000000000 = 3000000000000"), Some(true));
        assert_eq!(tri("b + s = 307"), Some(true));
        // NULL operands give NULL; non-numbers raise.
        assert_eq!(tri("missing + 1 IS NULL"), Some(true));
        assert_eq!(tri("missing + 1 > 0"), None);
        assert_eq!(eval_on(&msg(), "flag + 1 IS NULL"), Err(EvalError));
        assert_eq!(eval_on(&msg(), "size + color = 'x'"), Err(EvalError));
        assert_eq!(eval_on(&msg(), "-b IS NULL"), Err(EvalError));
        assert_eq!(tri("-color IS NULL"), Some(true));
    }

    #[test]
    fn string_concatenation() {
        assert_eq!(tri("color + 'x' = 'redx'"), Some(true));
        assert_eq!(tri("color + size = 'red3'"), Some(true));
        assert_eq!(tri("color + flag = 'redtrue'"), Some(true));
        assert_eq!(tri("color + weight = 'red2.5'"), Some(true));
        assert_eq!(tri("color + c = 'redx'"), Some(true));
        assert_eq!(tri("color + 1 + 1 = 'red11'"), Some(true));
        // String + NULL is NULL, as in ActiveMQ.
        assert_eq!(tri("color + missing IS NULL"), Some(true));
        assert_eq!(tri("color + missing = 'rednull'"), None);
    }

    #[test]
    fn java_number_text() {
        let d = |x: f64| java_string(&SVal::Double(x));
        assert_eq!(d(2.5), "2.5");
        assert_eq!(d(1.0), "1.0");
        assert_eq!(d(100.0), "100.0");
        assert_eq!(d(0.001), "0.001");
        assert_eq!(d(0.0001), "1.0E-4");
        assert_eq!(d(1234567.0), "1234567.0");
        assert_eq!(d(12345678.0), "1.2345678E7");
        assert_eq!(d(1e20), "1.0E20");
        assert_eq!(d(-1.5e-7), "-1.5E-7");
        assert_eq!(d(1.0 / 3.0), "0.3333333333333333");
        assert_eq!(d(-0.0), "-0.0");
        assert_eq!(d(f64::INFINITY), "Infinity");
        assert_eq!(d(f64::NAN), "NaN");
        assert_eq!(java_string(&SVal::Float(1.1)), "1.1");
        assert_eq!(java_string(&SVal::Float(1.0e10)), "1.0E10");
    }

    #[test]
    fn between_and_in() {
        assert!(sel("size BETWEEN 1 AND 5"));
        assert!(!sel("size NOT BETWEEN 1 AND 5"));
        assert_eq!(tri("size BETWEEN 3 AND 3"), Some(true));
        assert_eq!(tri("size BETWEEN 5 AND 1"), Some(false));
        assert_eq!(tri("size NOT BETWEEN 5 AND 1"), Some(true));
        // BETWEEN with NULL.
        assert_eq!(tri("missing BETWEEN 1 AND 10"), None);
        assert_eq!(tri("missing NOT BETWEEN 1 AND 10"), None);
        assert_eq!(tri("size BETWEEN missing AND 10"), None);
        assert_eq!(tri("size BETWEEN 4 AND missing"), Some(false));
        assert_eq!(tri("size NOT BETWEEN missing AND 1"), Some(true));
        assert_eq!(tri("size NOT BETWEEN missing AND 10"), None);
        // NOT BETWEEN is `x < a OR x > b`: incompatible bounds give FALSE both ways.
        assert_eq!(tri("size BETWEEN color AND 5"), Some(false));
        assert_eq!(tri("size NOT BETWEEN color AND str5"), Some(false));
        // IN with NULL.
        assert_eq!(tri("missing IN ('a')"), None);
        assert_eq!(tri("missing NOT IN ('a')"), None);
        assert_eq!(tri("color IN ('red','blue')"), Some(true));
        assert_eq!(tri("color NOT IN ('red','blue')"), Some(false));
        let big: Vec<String> = (0..1000).map(|i| format!("'v{i}'")).collect();
        let s = compile(&format!("color IN ({}, 'red')", big.join(",")));
        assert!(matches!(
            &s.expr,
            Expr::In {
                list: InList::Set(_),
                ..
            }
        ));
        assert!(s.matches(&msg()));
        let s = compile(&format!("color IN ({})", big.join(",")));
        assert!(!s.matches(&msg()));
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
        // The escape character is literal before other characters.
        assert!(LikePattern::compile("r!ed", Some('!')).matches("r!ed"));
        assert!(LikePattern::compile("a!!", Some('!')).matches("a!"));
        // Wildcards match line terminators; matching is case-sensitive.
        assert!(LikePattern::compile("a_b", None).matches("a\nb"));
        assert!(!LikePattern::compile("RED", None).matches("red"));
        assert!(sel("color LIKE 're_'"));
        assert!(!sel("color LIKE 'r.d'"));
        assert_eq!(tri("missing LIKE 'a'"), None);
        assert_eq!(tri("missing NOT LIKE 'a'"), None);
        assert!(Selector::compile("color LIKE 'r%' ESCAPE ''").is_err());
        assert!(Selector::compile("color LIKE 'r%' ESCAPE 'ab'").is_err());
        assert!(Selector::compile("color LIKE size").is_err());
    }

    #[test]
    fn like_matcher_kinds() {
        let kind = |p: &str, e: Option<char>| match LikeMatcher::compile(p, e) {
            LikeMatcher::Exact(_) => "exact",
            LikeMatcher::Prefix(_) => "prefix",
            LikeMatcher::Suffix(_) => "suffix",
            LikeMatcher::Contains(_) => "contains",
            LikeMatcher::All => "all",
            LikeMatcher::Generic(_) => "generic",
        };
        assert_eq!(kind("abc", None), "exact");
        assert_eq!(kind("", None), "exact");
        assert_eq!(kind("abc%", None), "prefix");
        assert_eq!(kind("%abc", None), "suffix");
        assert_eq!(kind("%abc%", None), "contains");
        assert_eq!(kind("%", None), "all");
        assert_eq!(kind("%%", None), "all");
        assert_eq!(kind("a!%c%", Some('!')), "prefix");
        assert_eq!(kind("a_c%", None), "generic");
        assert_eq!(kind("a%c", None), "generic");
    }

    /// Reference matcher: plain recursion over the LIKE definition.
    fn reference_like(p: &[LikePart], t: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(LikePart::Any) => (0..=t.len()).any(|k| reference_like(&p[1..], &t[k..])),
            Some(LikePart::One) => !t.is_empty() && reference_like(&p[1..], &t[1..]),
            Some(LikePart::Lit(c)) => t.first() == Some(c) && reference_like(&p[1..], &t[1..]),
        }
    }

    #[test]
    fn like_matchers_agree_with_generic_matcher() {
        // xorshift64: deterministic pseudo-random patterns and inputs.
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move |n: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % n
        };
        let alphabet = ['a', 'b', '%', '_', '!', 'é'];
        for _ in 0..20_000 {
            let plen = next(7) as usize;
            let pattern: String = (0..plen)
                .map(|_| alphabet[next(alphabet.len() as u64) as usize])
                .collect();
            let escape = if next(3) == 0 { Some('!') } else { None };
            let generic = LikePattern::compile(&pattern, escape);
            let compiled = LikeMatcher::compile(&pattern, escape);
            for _ in 0..8 {
                let tlen = next(7) as usize;
                let text: String = (0..tlen)
                    .map(|_| alphabet[next(alphabet.len() as u64) as usize])
                    .collect();
                let chars: Vec<char> = text.chars().collect();
                let expected = reference_like(&generic.parts, &chars);
                assert_eq!(
                    generic.matches(&text),
                    expected,
                    "generic {pattern:?} {escape:?} on {text:?}"
                );
                assert_eq!(
                    compiled.matches(&text),
                    expected,
                    "compiled {pattern:?} {escape:?} on {text:?}"
                );
            }
        }
    }

    #[test]
    fn literals() {
        assert!(sel("size = 0x3"));
        assert!(sel("size = 0X3"));
        assert!(sel("size = 03"));
        assert!(sel("size = 3L"));
        assert!(sel("size = 3l"));
        assert!(sel("size = 3."));
        assert!(sel("weight = 25e-1"));
        assert!(sel("weight = .25e1"));
        assert!(sel("weight = 2.5E0"));
        assert!(sel("size = 3e0"));
        assert!(sel("color = 'r''ed' OR color = 'red'"));
        assert!(sel("l = 10000000000"));
        assert!(sel("l = 0x2540BE400"));
        assert!(sel("flag = True"));
        // Literal types: int when it fits, else long, else BigDecimal.
        let lit = |s: &str| match &compile(&format!("{s} IS NULL")).expr {
            Expr::Eq(a, _) => match &**a {
                Expr::Const(v) => v.clone(),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        };
        assert_eq!(lit("2147483647"), SVal::Int(i32::MAX));
        assert_eq!(lit("2147483648"), SVal::Long(2147483648));
        assert_eq!(lit("9223372036854775808"), SVal::BigInt(9223372036854775808));
        assert_eq!(lit("1e309"), SVal::Double(f64::INFINITY));
        assert_eq!(tri("-9223372036854775808 < 0"), Some(true));
        assert_eq!(tri("9223372036854775808 > 9223372036854775807"), Some(false));
        for bad in [
            "size = 3.0f",
            "size = 3D",
            "size = 0x3L",
            "size = 08",
            "size = 03L",
            "size = 0xFFFFFFFFFFFFFFFF",
            "size = 1e",
            "size = 1.5.5",
            "size = 1_000",
            "name = \"x\"",
        ] {
            assert!(Selector::compile(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn lexical_rules() {
        assert!(sel("size /* comment */ = 3"));
        assert_eq!(tri("size = 3 -- comment"), Some(false), "`--` is two minus signs");
        assert!(sel("\tsize\r\n=\u{c}3"));
        assert!(sel("size=3 AnD color='red'"));
        assert!(sel("color in('red')"));
        assert!(sel("size BETWEEN(1)AND(5)"));
        assert_eq!(tri("Color = 'red'"), None, "property names are case-sensitive");
        assert_eq!(tri("jmspriority = 4"), None, "header names are case-sensitive");
        assert_eq!(tri("$x = 1"), None);
        assert_eq!(tri("_x = 1"), None);
        assert_eq!(tri("x$y = 1"), None);
        assert!(Selector::compile("x.y = 1").is_err());
        assert!(Selector::compile("città = 1").is_err());
        assert!(Selector::compile("size = 3 /* open").is_err());
    }

    #[test]
    fn compile_time_type_checks() {
        for bad in [
            "color > 'a'",
            "'a' < color",
            "size > TRUE",
            "size > NULL",
            "flag > FALSE",
            "TRUE = 1",
            "TRUE = 'a'",
            "(TRUE) = 1",
            "TRUE BETWEEN 0 AND 1",
            "color BETWEEN 'a' AND 'z'",
            "5",
            "'x'",
            "size + 1",
            "NOT 5",
            "NOT (size + 1)",
            "size + 1 AND flag",
            "'x' IN ('x')",
            "size + 0 IN ('3')",
            "size IN (3)",
            "color IN ()",
        ] {
            assert!(Selector::compile(bad).is_err(), "{bad} should be rejected");
        }
        for good in [
            "1 = TRUE",
            "'a' = 1",
            "TRUE = TRUE",
            "5 IS NULL",
            "(size) IN ('3')",
            "size BETWEEN flag AND 5",
            "NOT NULL",
        ] {
            assert!(Selector::compile(good).is_ok(), "{good} should be accepted");
        }
        assert_eq!(tri("1 = TRUE"), Some(false));
        assert_eq!(tri("(size) IN ('3')"), None);
    }

    #[test]
    fn errors_have_positions() {
        assert_eq!(err_col("color = 'red' AN size > 2"), 14);
        let e = Selector::compile("color = 'red' AN size > 2").unwrap_err();
        assert_eq!(
            e.exception_message("color = 'red' AN size > 2"),
            "Unexpected token 'AN' at column 14 in selector: color = 'red' AN size > 2"
        );
        assert_eq!(err_col("JMSCorrelationID = = 'X'"), 19);
        assert_eq!(err_col("(a = 1"), 6);
        assert_eq!(err_col("a = 1)"), 5);
        assert_eq!(err_col("color = 'red"), 8);
        assert_eq!(err_col("a ! b"), 2);
        assert_eq!(err_col("a = 1 AND"), 9);
        assert_eq!(err_col("color > 'a'"), 6);
        assert_eq!(err_col("a = NULL"), 2);
        assert_eq!(err_col("a LIKE 'x' ESCAPE 'ab'"), 18);
        assert_eq!(err_col("a IN (1, 2)"), 6);
        assert_eq!(err_col("size = 1 AND 5"), 13);
        assert!(Selector::compile("   ").unwrap().is_none());
        assert!(Selector::compile("").unwrap().is_none());
    }

    #[test]
    fn xpath_is_rejected_with_a_fixed_message() {
        for s in [
            "XPATH '//a'",
            "xpath '//a'",
            "XQUERY '//a'",
            "color = 'x' OR XPATH '//a'",
        ] {
            let e = Selector::compile(s).unwrap_err();
            assert_eq!(e.exception_message(s), "XPath selectors are not supported", "{s}");
        }
    }

    #[test]
    fn exceptions_abort_the_whole_selector() {
        // OR and AND stop at the first decisive operand, like ActiveMQ.
        assert_eq!(tri("flag OR size + color = 'x'"), Some(true));
        assert_eq!(eval_on(&msg(), "size + color = 'x' OR flag"), Err(EvalError));
        assert_eq!(tri("NOT flag AND size + color = 'x'"), Some(false));
        // An error is not UNKNOWN: NOT does not turn it into anything.
        assert!(!sel("NOT (size + color = 'x')"));
        // Equality evaluates both sides; ordering stops at a NULL left operand.
        assert_eq!(eval_on(&msg(), "missing = size + color"), Err(EvalError));
        assert_eq!(tri("missing > size + color"), None);
    }

    #[test]
    fn undecodable_properties_make_the_selector_unknown() {
        let mut m = msg();
        m.broken = true;
        for s in [
            "color = 'red'",
            "color IS NULL",
            "NOT (color = 'red')",
            "color IS NOT NULL",
            "missing IS NULL",
        ] {
            assert_eq!(eval_on(&m, s), Err(EvalError), "{s}");
            assert!(!compile(s).matches(&m), "{s}");
        }
        // Header-only parts are still evaluated and can decide alone.
        assert!(compile("JMSPriority = 4 OR color = 'red'").matches(&m));
    }

    #[test]
    fn header_only_selectors_never_read_properties() {
        let s = compile("JMSCorrelationID LIKE 'ORD-%' AND JMSPriority BETWEEN 0 AND 9 AND JMSDeliveryMode <> 'X'");
        assert!(!s.uses_properties());
        let m = msg();
        assert!(s.matches(&m));
        assert_eq!(m.lookups.get(), 0);
        assert!(compile("color = 'x'").uses_properties());
        assert!(
            compile("JMSXUserID = 'x'").uses_properties(),
            "JMSXUserID falls back to a property"
        );
    }

    #[test]
    fn header_table() {
        for (name, h) in HEADERS {
            assert_eq!(Header::from_name(name), Some(*h));
            assert_eq!(h.name(), *name);
        }
        assert!(is_header("JMSXGroupSeq"));
        assert!(!is_header("JMSFoo"));
        assert!(!is_header("jmspriority"));
    }
}
