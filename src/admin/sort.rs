// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Server-side table sorting: column definitions, typed comparisons and sortable headers.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt::Write;
use std::net::SocketAddr;

use super::pages::{esc, Ctx};

/// Case-insensitive natural order: runs of digits compare as numbers (`Q2` before `Q10`), an
/// empty value comes first, and values equal apart from case fall back to a case-sensitive order.
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a, b);
    loop {
        let (Some(c), Some(d)) = (x.chars().next(), y.chars().next()) else {
            return x.len().cmp(&y.len()).then_with(|| a.cmp(b));
        };
        let ord = if c.is_ascii_digit() && d.is_ascii_digit() {
            let n = x.find(|c: char| !c.is_ascii_digit()).unwrap_or(x.len());
            let m = y.find(|c: char| !c.is_ascii_digit()).unwrap_or(y.len());
            let (p, q) = (x[..n].trim_start_matches('0'), y[..m].trim_start_matches('0'));
            (x, y) = (&x[n..], &y[m..]);
            p.len().cmp(&q.len()).then_with(|| p.cmp(q))
        } else {
            (x, y) = (&x[c.len_utf8()..], &y[d.len_utf8()..]);
            c.to_lowercase().cmp(d.to_lowercase())
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
}

/// Socket addresses: IP numerically (IPv4 before IPv6), then port; unparsable ones last, as text.
pub fn address(a: &str, b: &str) -> Ordering {
    match (a.parse::<SocketAddr>(), b.parse::<SocketAddr>()) {
        (Ok(x), Ok(y)) => (x.ip(), x.port()).cmp(&(y.ip(), y.port())),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => natural(a, b),
    }
}

/// How a column reads its sort key from a row.
pub enum Key<T> {
    Text(fn(&T) -> Cow<'_, str>),
    Number(fn(&T) -> u64),
    /// Milliseconds since the epoch.
    Time(fn(&T) -> i64),
    Address(fn(&T) -> Cow<'_, str>),
}

impl<T> Key<T> {
    fn cmp(&self, a: &T, b: &T) -> Ordering {
        match self {
            Key::Text(f) => natural(&f(a), &f(b)),
            Key::Number(f) => f(a).cmp(&f(b)),
            Key::Time(f) => f(a).cmp(&f(b)),
            Key::Address(f) => address(&f(a), &f(b)),
        }
    }
}

pub struct Column<T> {
    pub key: &'static str,
    pub label: &'static str,
    pub sort: Key<T>,
}

impl<T> Column<T> {
    pub const fn text(key: &'static str, label: &'static str, f: fn(&T) -> Cow<'_, str>) -> Self {
        Column {
            key,
            label,
            sort: Key::Text(f),
        }
    }

    pub const fn number(key: &'static str, label: &'static str, f: fn(&T) -> u64) -> Self {
        Column {
            key,
            label,
            sort: Key::Number(f),
        }
    }

    pub const fn time(key: &'static str, label: &'static str, f: fn(&T) -> i64) -> Self {
        Column {
            key,
            label,
            sort: Key::Time(f),
        }
    }

    pub const fn address(key: &'static str, label: &'static str, f: fn(&T) -> Cow<'_, str>) -> Self {
        Column {
            key,
            label,
            sort: Key::Address(f),
        }
    }
}

/// A sortable table: its columns, the default column and the prefix of its query parameters
/// (`<prefix>sort`, `<prefix>order`), so several tables on one page sort independently.
pub struct Table<T: 'static> {
    pub prefix: &'static str,
    pub default: &'static str,
    pub columns: &'static [Column<T>],
}

impl<T> Table<T> {
    /// Normalized column and direction: an unknown column falls back to the default, an unknown
    /// order to ascending.
    pub fn params(&self, sort: Option<&str>, order: Option<&str>) -> (&'static str, bool) {
        let col = self.columns.iter().map(|c| c.key).find(|&c| Some(c) == sort);
        (col.unwrap_or(self.default), order == Some("desc"))
    }

    /// Sorts `rows` stably; equal values are ordered by the first column ascending.
    pub fn sort(&self, rows: &mut [T], sort: Option<&str>, order: Option<&str>) -> (&'static str, bool) {
        let (key, desc) = self.params(sort, order);
        let col = self.columns.iter().find(|c| c.key == key).unwrap_or(&self.columns[0]);
        let first = &self.columns[0];
        rows.sort_by(|a, b| {
            let ord = col.sort.cmp(a, b);
            let ord = if desc { ord.reverse() } else { ord };
            ord.then_with(|| first.sort.cmp(a, b))
        });
        (key, desc)
    }

    /// Sorts `rows` by the page's parameters and returns the header cells: links that sort by
    /// each column, with an arrow and `aria-sort` on the sorted one.
    pub fn sort_page(&self, rows: &mut [T], ctx: &Ctx) -> String {
        let (sort_param, order_param) = (format!("{}sort", self.prefix), format!("{}order", self.prefix));
        let (current, desc) = self.sort(rows, ctx.get(&sort_param), ctx.get(&order_param));
        let mut out = String::new();
        for col in self.columns {
            let num = if matches!(col.sort, Key::Number(_)) {
                " class=\"num\""
            } else {
                ""
            };
            let (next, aria, arrow) = match (col.key == current, desc) {
                (true, true) => (
                    "asc",
                    " aria-sort=\"descending\"",
                    " <span aria-hidden=\"true\">&#9660;</span>",
                ),
                (true, false) => (
                    "desc",
                    " aria-sort=\"ascending\"",
                    " <span aria-hidden=\"true\">&#9650;</span>",
                ),
                (false, _) => ("asc", "", ""),
            };
            let href = ctx.with_kept(&[(&sort_param, Some(col.key)), (&order_param, Some(next))]);
            let _ = write!(
                out,
                "<th scope=\"col\"{num}{aria}><a href=\"{}\">{}{arrow}</a></th>",
                esc(&href),
                col.label
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::*;

    #[test]
    fn natural_order() {
        assert_eq!(natural("", ""), Equal);
        assert_eq!(natural("", "a"), Less);
        assert_eq!(natural("a", ""), Greater);
        assert_eq!(natural("Q1", "q2"), Less);
        assert_eq!(natural("q2", "Q10"), Less);
        assert_eq!(natural("ID:host-1:2", "ID:host-1:10"), Less);
        assert_eq!(natural("10", "9"), Greater);
        assert_eq!(natural("same", "same"), Equal);
        let mut v = vec!["Q10", "q2", "Q1", "", "a"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, vec!["", "a", "Q1", "q2", "Q10"]);
    }

    #[test]
    fn natural_order_is_total() {
        // Equal apart from case or leading zeros: still a stable, antisymmetric order.
        assert_eq!(natural("abc", "ABC"), natural("ABC", "abc").reverse());
        assert_ne!(natural("abc", "ABC"), Equal);
        assert_eq!(natural("a07", "a7"), natural("a7", "a07").reverse());
    }

    #[test]
    fn address_order() {
        assert_eq!(address("10.0.0.9:5001", "10.0.0.9:6000"), Less);
        assert_eq!(address("10.0.0.9:6000", "10.0.0.10:5000"), Less);
        assert_eq!(address("10.0.0.9:5001", "10.0.0.9:5001"), Equal);
        assert_eq!(address("127.0.0.1:80", "[::1]:80"), Less);
        assert_eq!(address("127.0.0.1:80", "unparsable"), Less);
        assert_eq!(address("pipe-2", "pipe-10"), Less);
    }

    static TABLE: Table<(&str, u64)> = Table {
        prefix: "x",
        default: "name",
        columns: &[
            Column::text("name", "Name", |r| r.0.into()),
            Column::number("count", "Count", |r| r.1),
        ],
    };

    #[test]
    fn params_fall_back() {
        assert_eq!(TABLE.params(None, None), ("name", false));
        assert_eq!(TABLE.params(Some("count"), Some("desc")), ("count", true));
        assert_eq!(TABLE.params(Some("count"), Some("bogus")), ("count", false));
        assert_eq!(TABLE.params(Some("bogus"), Some("desc")), ("name", true));
    }

    #[test]
    fn ties_by_first_column_ascending() {
        let mut rows = vec![("B", 0), ("A", 0), ("C", 5)];
        TABLE.sort(&mut rows, Some("count"), Some("desc"));
        assert_eq!(rows, vec![("C", 5), ("A", 0), ("B", 0)]);
    }
}
