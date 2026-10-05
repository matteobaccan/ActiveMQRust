// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! XML detection and formatted view of message bodies for the admin console.
//!
//! The pull parser never reads DTDs, resolves entities or fetches anything: references are
//! shown as written, so external entities and entity-expansion bombs cannot do harm. Input,
//! output and nesting are bounded.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::fmt;

use super::pages::esc;

/// Largest body text that is formatted.
pub const MAX_INPUT: usize = 1024 * 1024;
/// Largest formatted output (plain text); longer output is truncated with a notice.
pub const MAX_OUTPUT: usize = 256 * 1024;
/// Deepest element nesting that is formatted.
pub const MAX_DEPTH: usize = 256;

const INDENT: &str = "  ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlError {
    /// The text does not start like an XML document.
    NotXml,
    TooLarge,
    TooDeep,
    Malformed {
        line: usize,
        column: usize,
        message: String,
    },
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XmlError::NotXml => f.write_str("Not XML"),
            XmlError::TooLarge => f.write_str("Too large to format (limit 1 MB)"),
            XmlError::TooDeep => write!(f, "Nesting deeper than {MAX_DEPTH} levels: not formatted"),
            XmlError::Malformed { line, column, message } => {
                write!(f, "Not well-formed XML: line {line}, column {column}: {message}")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Formatted {
    /// HTML-escaped, with `x-*` colouring classes.
    pub html: String,
    /// The same text without markup (for the JSON API).
    pub plain: String,
    /// Output stopped at `MAX_OUTPUT`.
    pub truncated: bool,
}

/// True when the text starts like an XML document (after a byte-order mark and whitespace).
pub fn looks_like_xml(text: &str) -> bool {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("<?xml") || t.starts_with("<!--") || t.starts_with("<!DOCTYPE") {
        return true;
    }
    let mut c = t.chars();
    c.next() == Some('<') && c.next().is_some_and(|ch| ch.is_alphabetic() || ch == '_')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    None,
    Tag,
    Attr,
    Val,
    Com,
    CData,
    Pi,
}

impl Class {
    fn css(self) -> &'static str {
        match self {
            Class::None => "",
            Class::Tag => "x-tag",
            Class::Attr => "x-attr",
            Class::Val => "x-val",
            Class::Com => "x-com",
            Class::CData => "x-cdata",
            Class::Pi => "x-pi",
        }
    }
}

type Segments = Vec<(Class, String)>;

enum Tok {
    Start {
        name: String,
        attrs: Vec<(String, String)>,
        empty: bool,
    },
    End(String),
    /// Text as written, references included.
    Text(String),
    /// Comment, CDATA, processing instruction, declaration or DOCTYPE, as written.
    Other(Class, String),
}

fn utf8(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn start_tok(e: &BytesStart, empty: bool) -> Result<Tok, String> {
    let mut attrs = Vec::new();
    for a in e.attributes().with_checks(true) {
        let a = a.map_err(|e| e.to_string())?;
        attrs.push((utf8(a.key.as_ref()), utf8(&a.value)));
    }
    Ok(Tok::Start {
        name: utf8(e.name().as_ref()),
        attrs,
        empty,
    })
}

fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let mut cut = offset;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let before = &text[..cut];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

/// Parses the whole text into tokens, checking well-formedness and the depth limit.
fn tokenize(text: &str) -> Result<Vec<Tok>, XmlError> {
    let mut r = Reader::from_str(text);
    {
        let c = r.config_mut();
        c.check_end_names = true;
        c.check_comments = true;
        c.expand_empty_elements = false;
        c.trim_text(false);
    }
    let malformed = |offset: u64, message: String| {
        let (line, column) = line_col(text, offset as usize);
        XmlError::Malformed { line, column, message }
    };
    let mut toks: Vec<Tok> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut root_seen = false;
    loop {
        let at = r.buffer_position();
        let ev = match r.read_event() {
            Ok(ev) => ev,
            Err(e) => return Err(malformed(r.error_position(), e.to_string())),
        };
        let top_level = stack.is_empty();
        match ev {
            Event::Start(e) | Event::Empty(e) if top_level && root_seen => {
                let _ = e;
                return Err(malformed(at, "more than one root element".into()));
            }
            Event::Start(e) => {
                let tok = start_tok(&e, false).map_err(|m| malformed(at, m))?;
                if let Tok::Start { name, .. } = &tok {
                    stack.push(name.clone());
                }
                if stack.len() > MAX_DEPTH {
                    return Err(XmlError::TooDeep);
                }
                root_seen = true;
                toks.push(tok);
            }
            Event::Empty(e) => {
                root_seen = true;
                toks.push(start_tok(&e, true).map_err(|m| malformed(at, m))?);
            }
            Event::End(e) => {
                stack.pop();
                toks.push(Tok::End(utf8(e.name().as_ref())));
            }
            Event::Text(t) => {
                let s = utf8(&t);
                if top_level && !s.trim().is_empty() {
                    return Err(malformed(at, "text outside the root element".into()));
                }
                match toks.last_mut() {
                    Some(Tok::Text(prev)) => prev.push_str(&s),
                    _ => toks.push(Tok::Text(s)),
                }
            }
            Event::GeneralRef(g) => {
                if top_level {
                    return Err(malformed(at, "reference outside the root element".into()));
                }
                let s = format!("&{};", utf8(&g));
                match toks.last_mut() {
                    Some(Tok::Text(prev)) => prev.push_str(&s),
                    _ => toks.push(Tok::Text(s)),
                }
            }
            Event::CData(c) => {
                if top_level {
                    return Err(malformed(at, "CDATA outside the root element".into()));
                }
                toks.push(Tok::Other(Class::CData, format!("<![CDATA[{}]]>", utf8(&c))));
            }
            Event::Comment(c) => toks.push(Tok::Other(Class::Com, format!("<!--{}-->", utf8(&c)))),
            Event::Decl(d) => toks.push(Tok::Other(Class::Pi, format!("<?{}?>", utf8(&d)))),
            Event::PI(p) => toks.push(Tok::Other(Class::Pi, format!("<?{}?>", utf8(&p)))),
            Event::DocType(d) => toks.push(Tok::Other(Class::Pi, format!("<!DOCTYPE {}>", utf8(&d).trim()))),
            Event::Eof => break,
        }
    }
    if let Some(open) = stack.last() {
        return Err(malformed(text.len() as u64, format!("element <{open}> is not closed")));
    }
    if !root_seen {
        return Err(malformed(text.len() as u64, "no root element".into()));
    }
    Ok(toks)
}

fn start_segments(name: &str, attrs: &[(String, String)], empty: bool, out: &mut Segments) {
    out.push((Class::Tag, format!("<{name}")));
    for (k, v) in attrs {
        out.push((Class::None, " ".into()));
        out.push((Class::Attr, k.clone()));
        out.push((Class::None, "=".into()));
        let q = if v.contains('"') { '\'' } else { '"' };
        out.push((Class::Val, format!("{q}{v}{q}")));
    }
    out.push((Class::Tag, if empty { "/>".into() } else { ">".into() }));
}

fn tok_segments(t: &Tok, out: &mut Segments) {
    match t {
        Tok::Start { name, attrs, empty } => start_segments(name, attrs, *empty, out),
        Tok::End(name) => out.push((Class::Tag, format!("</{name}>"))),
        Tok::Text(s) => out.push((Class::None, s.clone())),
        Tok::Other(c, s) => out.push((*c, s.clone())),
    }
}

/// For each start tag: the index of its end tag, and whether its content mixes text and elements.
fn structure(toks: &[Tok]) -> (Vec<usize>, Vec<bool>) {
    let mut end = vec![0; toks.len()];
    let mut mixed = vec![false; toks.len()];
    // (start index, has text, has element)
    let mut stack: Vec<(usize, bool, bool)> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Start { empty, .. } => {
                if let Some(top) = stack.last_mut() {
                    top.2 = true;
                }
                if *empty {
                    end[i] = i;
                } else {
                    stack.push((i, false, false));
                }
            }
            Tok::End(_) => {
                if let Some((s, text, elem)) = stack.pop() {
                    end[s] = i;
                    mixed[s] = text && elem;
                }
            }
            Tok::Text(s) if !s.trim().is_empty() => {
                if let Some(top) = stack.last_mut() {
                    top.1 = true;
                }
            }
            _ => {}
        }
    }
    (end, mixed)
}

/// Formats an XML text: one element per line, two spaces per level.
pub fn format(text: &str) -> Result<Formatted, XmlError> {
    if text.len() > MAX_INPUT {
        return Err(XmlError::TooLarge);
    }
    if !looks_like_xml(text) {
        return Err(XmlError::NotXml);
    }
    let body = text.trim_start_matches('\u{feff}');
    let toks = tokenize(body)?;
    let (end, mixed) = structure(&toks);
    let mut lines: Vec<(usize, Segments)> = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < toks.len() {
        let mut segs = Segments::new();
        match &toks[i] {
            Tok::Start { empty: false, .. } => {
                let e = end[i];
                let only_text = e == i + 2 && matches!(toks[i + 1], Tok::Text(_));
                if mixed[i] || e == i + 1 || only_text {
                    for t in &toks[i..=e] {
                        tok_segments(t, &mut segs);
                    }
                    lines.push((depth, segs));
                    i = e + 1;
                    continue;
                }
                tok_segments(&toks[i], &mut segs);
                lines.push((depth, segs));
                depth += 1;
            }
            Tok::End(_) => {
                depth = depth.saturating_sub(1);
                tok_segments(&toks[i], &mut segs);
                lines.push((depth, segs));
            }
            Tok::Text(s) => {
                if !s.trim().is_empty() {
                    segs.push((Class::None, s.trim().to_string()));
                    lines.push((depth, segs));
                }
            }
            t => {
                tok_segments(t, &mut segs);
                lines.push((depth, segs));
            }
        }
        i += 1;
    }
    let mut html = String::new();
    let mut plain = String::new();
    let mut truncated = false;
    for (n, (depth, segs)) in lines.iter().enumerate() {
        let line_len: usize = depth * INDENT.len() + segs.iter().map(|s| s.1.len()).sum::<usize>() + 1;
        if plain.len() + line_len > MAX_OUTPUT {
            truncated = true;
            break;
        }
        if n > 0 {
            html.push('\n');
            plain.push('\n');
        }
        let pad = INDENT.repeat(*depth);
        html.push_str(&pad);
        plain.push_str(&pad);
        for (c, s) in segs {
            plain.push_str(s);
            if *c == Class::None {
                html.push_str(&esc(s));
            } else {
                html.push_str(&format!("<span class=\"{}\">{}</span>", c.css(), esc(s)));
            }
        }
    }
    Ok(Formatted { html, plain, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> String {
        format(s).unwrap().plain
    }

    #[test]
    fn detection() {
        assert!(looks_like_xml("<?xml version=\"1.0\"?><a/>"));
        assert!(looks_like_xml("\u{feff}  \n<order/>"));
        assert!(looks_like_xml("<!-- c --><a/>"));
        assert!(looks_like_xml("<_x/>"));
        assert!(!looks_like_xml("hello <world>"));
        assert!(!looks_like_xml("< a>"));
        assert!(!looks_like_xml("{\"json\":1}"));
        assert_eq!(format("hello <world>").unwrap_err(), XmlError::NotXml);
    }

    #[test]
    fn indentation() {
        assert_eq!(
            plain("<order id=\"7\"><item qty=\"2\">A</item><item qty=\"1\">B</item></order>"),
            "<order id=\"7\">\n  <item qty=\"2\">A</item>\n  <item qty=\"1\">B</item>\n</order>"
        );
        assert_eq!(
            plain("<?xml version=\"1.0\"?>\n<a>\n   <b><c/></b>\n  <d></d></a>"),
            "<?xml version=\"1.0\"?>\n<a>\n  <b>\n    <c/>\n  </b>\n  <d></d>\n</a>"
        );
    }

    #[test]
    fn text_only_and_mixed_content() {
        assert_eq!(plain("<a><b>x y</b></a>"), "<a>\n  <b>x y</b>\n</a>");
        assert_eq!(
            plain("<p>Hello <b>big</b>  world</p>"),
            "<p>Hello <b>big</b>  world</p>"
        );
    }

    #[test]
    fn comments_cdata_pi_and_references_preserved() {
        let out = plain("<a><!-- note --><![CDATA[<b>&amp;</b>]]><?pi data?><t>caf&#233; &amp; &e;</t></a>");
        assert_eq!(
            out,
            "<a>\n  <!-- note -->\n  <![CDATA[<b>&amp;</b>]]>\n  <?pi data?>\n  <t>caf&#233; &amp; &e;</t>\n</a>"
        );
    }

    #[test]
    fn attribute_order_and_values_kept() {
        assert_eq!(
            plain("<a z=\"1\" b='x\"y' m=\"&lt;\"/>"),
            "<a z=\"1\" b='x\"y' m=\"&lt;\"/>"
        );
    }

    #[test]
    fn colouring_classes() {
        let f = format("<a b=\"c\"/>").unwrap();
        assert!(f.html.contains("<span class=\"x-tag\">&lt;a</span>"), "{}", f.html);
        assert!(f.html.contains("<span class=\"x-attr\">b</span>"));
        assert!(f.html.contains("<span class=\"x-val\">&quot;c&quot;</span>"));
        let f = format("<a><!--c--><![CDATA[d]]><?p q?></a>").unwrap();
        for c in ["x-com", "x-cdata", "x-pi"] {
            assert!(f.html.contains(c), "{c}");
        }
        assert!(!f.html.contains("<a>"));
    }

    #[test]
    fn malformed_with_position() {
        match format("<order>\n  <item></order>") {
            Err(XmlError::Malformed { line, column, .. }) => assert_eq!((line, column), (2, 9)),
            other => panic!("{other:?}"),
        }
        assert!(matches!(format("<a></a><b/>"), Err(XmlError::Malformed { .. })));
        assert!(matches!(format("<a>"), Err(XmlError::Malformed { .. })));
        assert!(matches!(format("<a></a>text"), Err(XmlError::Malformed { .. })));
        assert!(matches!(
            format("<a x=\"1\" x=\"2\"/>"),
            Err(XmlError::Malformed { .. })
        ));
        let msg = format("<order><item></order>").unwrap_err().to_string();
        assert!(msg.starts_with("Not well-formed XML: line 1, column"), "{msg}");
    }

    #[test]
    fn external_entity_not_resolved() {
        let out = plain("<!DOCTYPE x [<!ENTITY e SYSTEM \"file:///C:/Windows/win.ini\">]><x>&e;</x>");
        assert!(out.contains("<x>&e;</x>"), "{out}");
        assert!(out.starts_with("<!DOCTYPE x [<!ENTITY e SYSTEM"), "{out}");
    }

    #[test]
    fn billion_laughs_not_expanded() {
        let mut doc = String::from("<?xml version=\"1.0\"?>\n<!DOCTYPE lolz [\n <!ENTITY lol \"lol\">\n");
        doc.push_str(" <!ENTITY lol1 \"&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;\">\n");
        for i in 2..=9 {
            let p = i - 1;
            doc.push_str(&format!(
                " <!ENTITY lol{i} \"&lol{p};&lol{p};&lol{p};&lol{p};&lol{p};&lol{p};&lol{p};&lol{p};&lol{p};&lol{p};\">\n"
            ));
        }
        doc.push_str("]>\n<lolz>&lol9;</lolz>");
        let out = plain(&doc);
        assert!(out.ends_with("<lolz>&lol9;</lolz>"), "{out}");
        assert!(out.len() < 2 * doc.len());
    }

    #[test]
    fn limits() {
        let big = format!("<a>{}</a>", "x".repeat(MAX_INPUT));
        assert_eq!(format(&big).unwrap_err(), XmlError::TooLarge);
        let deep = format!("{}{}", "<a>".repeat(MAX_DEPTH + 1), "</a>".repeat(MAX_DEPTH + 1));
        assert_eq!(format(&deep).unwrap_err(), XmlError::TooDeep);
        let ok = format!("{}{}", "<a>".repeat(MAX_DEPTH), "</a>".repeat(MAX_DEPTH));
        assert!(format(&ok).is_ok());
        // Many short elements: the output passes 256 KB before the input reaches 1 MB.
        let wide = format!("<r>{}</r>", "<i>1</i>".repeat(100_000));
        let f = format(&wide).unwrap();
        assert!(f.truncated);
        assert!(f.plain.len() <= MAX_OUTPUT);
    }
}
