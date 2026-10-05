// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Deterministic payloads for compression tests and benchmarks.

#![allow(dead_code)]

use base64::Engine;

/// Small deterministic generator (64-bit LCG, high bits).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// An XML document like `XmlPayload` of the Java benchmark: 20 random fields, then a base64
/// buffer of random bytes padding the document to exactly `size` bytes.
pub fn xml_base64(size: usize, seed: u64) -> Vec<u8> {
    const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut r = Lcg(seed);
    let mut s = String::from("<message><id>1</id>");
    for i in 1..=20 {
        let value: String = match i {
            1..=4 => (0..8 + r.next() % 17).map(|_| ALNUM[(r.next() % 62) as usize] as char).collect(),
            5..=8 => (r.next() as i32).to_string(),
            9..=12 => format!("{}.{:04}", r.next() as i64 % 1_000_000, r.next() % 10_000),
            13..=16 => format!("2024-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", 1 + r.next() % 12, 1 + r.next() % 28, r.next() % 24, r.next() % 60, r.next() % 60, r.next() % 1000),
            _ => r.next().is_multiple_of(2).to_string(),
        };
        s += &format!("<field{i:02}>{value}</field{i:02}>");
    }
    s += "<payload encoding=\"base64\">";
    let tail = "</payload></message>";
    let avail = size.saturating_sub(s.len() + tail.len());
    let raw: Vec<u8> = (0..avail / 4 * 3).map(|_| r.next() as u8).collect();
    s += &base64::engine::general_purpose::STANDARD.encode(raw);
    s.extend(std::iter::repeat_n(' ', avail % 4));
    s += tail;
    s.into_bytes()
}

/// Plain English-like text of exactly `size` bytes.
pub fn plain_text(size: usize, seed: u64) -> Vec<u8> {
    const WORDS: &[&str] = &[
        "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "order", "customer", "invoice", "amount",
        "status", "shipped", "pending", "address", "street", "city", "payment", "total", "item", "quantity", "price",
        "date",
    ];
    let mut r = Lcg(seed);
    let mut s = String::with_capacity(size + 16);
    while s.len() < size {
        s += WORDS[(r.next() % WORDS.len() as u64) as usize];
        s.push(if r.next().is_multiple_of(12) { '\n' } else { ' ' });
    }
    s.truncate(size);
    s.into_bytes()
}
