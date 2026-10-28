//! Maxima documentation index built from the manual's Info files.

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct DocEntry {
    pub name: String,
    /// "Function", "Option variable", ...
    pub kind: String,
    /// Signature lines, e.g. `integrate (<expr>, <x>, <a>, <b>)`.
    pub signatures: Vec<String>,
    pub body: String,
    /// Manual section the entry was found in, e.g. "Functions and Variables for Integration".
    pub category: String,
}

impl DocEntry {
    /// Body text with hard-wrapped prose paragraphs joined; examples and indented blocks are kept as-is.
    pub fn reflowed(&self) -> Vec<(bool, String)> {
        let mut out = Vec::new();
        for para in self.body.split("\n\n") {
            let lines: Vec<&str> = para.lines().collect();
            if lines.is_empty() {
                continue;
            }
            let code = lines.iter().any(|l| l.starts_with(' ') || l.starts_with("(%") || l.starts_with("-->") || l.contains("(%i") || l.contains("(%o"));
            if code {
                out.push((true, para.to_string()));
            } else {
                out.push((false, lines.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ")));
            }
        }
        out
    }

    /// Argument names of the first signature (`<expr>` -> `expr`).
    pub fn args(&self) -> Vec<String> {
        let Some(sig) = self.signatures.first() else { return vec![] };
        let (Some(a), Some(b)) = (sig.find('('), sig.rfind(')')) else { return vec![] };
        if b <= a + 1 {
            return vec![];
        }
        sig[a + 1..b]
            .split(',')
            .map(|s| s.trim().trim_matches(|c| c == '<' || c == '>' || c == '[' || c == ']').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn is_function(&self) -> bool {
        self.kind.to_lowercase().contains("function")
    }
}

#[derive(Clone, Debug, Default)]
pub struct HelpIndex {
    pub entries: Vec<DocEntry>,
    by_name: HashMap<String, Vec<usize>>,
}

impl HelpIndex {
    pub fn from_info(text: &str) -> HelpIndex {
        let mut entries: Vec<DocEntry> = Vec::new();
        let mut node = String::new();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        let mut after_sep = false;
        while i < lines.len() {
            let line = lines[i];
            if line.starts_with('\u{1f}') {
                after_sep = true;
                i += 1;
                continue;
            }
            if after_sep {
                after_sep = false;
                if let Some(rest) = line.strip_prefix("File:") {
                    if let Some(p) = rest.find("Node:") {
                        let n = &rest[p + 5..];
                        node = n.split(',').next().unwrap_or("").trim().to_string();
                    }
                }
                i += 1;
                continue;
            }
            if let Some(head) = line.strip_prefix(" -- ") {
                if let Some((kind, sig)) = head.split_once(':') {
                    let mut signatures = vec![sig.trim().to_string()];
                    let name = sig.trim().split(|c: char| c.is_whitespace() || c == '(').next().unwrap_or("").to_string();
                    i += 1;
                    while i < lines.len() {
                        let l = lines[i];
                        if let Some(h) = l.strip_prefix(" -- ") {
                            if let Some((_, s)) = h.split_once(':') {
                                signatures.push(s.trim().to_string());
                            }
                        } else if l.starts_with("          ") && !l.trim().is_empty() {
                            let bare = signatures.last().map_or(false, |s| !s.contains('(') && !s.contains(' '));
                            let starts_new = l.trim().starts_with(name.as_str()) && l.trim()[name.len()..].trim_start().starts_with('(');
                            if bare {
                                // Newer manuals list the signatures on indented lines below the heading.
                                signatures.pop();
                                signatures.push(l.trim().to_string());
                            } else if starts_new {
                                signatures.push(l.trim().to_string());
                            } else if let Some(last) = signatures.last_mut() {
                                last.push(' ');
                                last.push_str(l.trim());
                            }
                        } else {
                            break;
                        }
                        i += 1;
                    }
                    let mut body = String::new();
                    while i < lines.len() {
                        let l = lines[i];
                        if l.starts_with(" -- ") || l.starts_with('\u{1f}') {
                            break;
                        }
                        if !l.is_empty() && !l.starts_with(' ') {
                            break;
                        }
                        body.push_str(l.strip_prefix("     ").unwrap_or(l.trim_start()));
                        body.push('\n');
                        i += 1;
                    }
                    if !name.is_empty() {
                        entries.push(DocEntry {
                            name,
                            kind: kind.trim().to_string(),
                            signatures,
                            body: body.trim().to_string(),
                            category: node.clone(),
                        });
                    }
                    continue;
                }
            }
            i += 1;
        }
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, e) in entries.iter().enumerate() {
            by_name.entry(e.name.clone()).or_default().push(idx);
        }
        HelpIndex { entries, by_name }
    }

    /// Loads the Maxima manual from the usual system locations.
    pub fn load_system() -> HelpIndex {
        let dirs = ["/usr/share/info", "/usr/local/share/info", "/opt/homebrew/share/info", "/usr/share/maxima/info"];
        for d in dirs {
            let mut text = String::new();
            let base = std::path::Path::new(d);
            if let Ok(rd) = std::fs::read_dir(base) {
                let mut files: Vec<_> = rd
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .map_or(false, |n| n.starts_with("maxima.info") && !n.ends_with(".gz") && n != "maxima.info")
                    })
                    .collect();
                files.sort_by_key(|p| natural_key(p));
                for f in files {
                    if let Ok(b) = std::fs::read(&f) {
                        text.push_str(&String::from_utf8_lossy(&b));
                    }
                }
                if text.is_empty() {
                    // Unsplit manual.
                    if let Ok(b) = std::fs::read(base.join("maxima.info")) {
                        text.push_str(&String::from_utf8_lossy(&b));
                    }
                }
            }
            if !text.is_empty() {
                let idx = HelpIndex::from_info(&text);
                if !idx.entries.is_empty() {
                    return idx;
                }
            }
        }
        HelpIndex::default()
    }

    pub fn lookup(&self, name: &str) -> Vec<&DocEntry> {
        self.by_name.get(name).map(|v| v.iter().map(|i| &self.entries[*i]).collect()).unwrap_or_default()
    }

    pub fn first(&self, name: &str) -> Option<&DocEntry> {
        self.lookup(name).into_iter().next()
    }

    /// Entries whose name starts with, or else contains, the query. Prefix matches first.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&DocEntry> {
        let q = query.to_lowercase();
        if q.is_empty() {
            return vec![];
        }
        let mut pre: Vec<&DocEntry> = Vec::new();
        let mut sub: Vec<&DocEntry> = Vec::new();
        for e in &self.entries {
            let n = e.name.to_lowercase();
            if n.starts_with(&q) {
                pre.push(e);
            } else if n.contains(&q) {
                sub.push(e);
            }
        }
        pre.sort_by_key(|e| e.name.len());
        pre.dedup_by(|a, b| a.name == b.name);
        sub.dedup_by(|a, b| a.name == b.name);
        pre.into_iter().chain(sub).take(limit).collect()
    }

    /// Function-like entries grouped by the manual section they appear in.
    pub fn categories(&self) -> Vec<(String, Vec<&DocEntry>)> {
        let mut order: Vec<String> = Vec::new();
        let mut map: HashMap<String, Vec<&DocEntry>> = HashMap::new();
        for e in &self.entries {
            if !e.is_function() || e.name.is_empty() || !e.name.chars().next().unwrap().is_alphabetic() {
                continue;
            }
            if !map.contains_key(&e.category) {
                order.push(e.category.clone());
            }
            map.entry(e.category.clone()).or_default().push(e);
        }
        order
            .into_iter()
            .map(|c| {
                let mut v = map.remove(&c).unwrap_or_default();
                v.dedup_by(|a, b| a.name == b.name);
                (c, v)
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn natural_key(p: &std::path::Path) -> u32 {
    p.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.rsplit('-').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// Returns the identifier under or immediately before `cursor` (a char index) in `text`.
pub fn word_at(text: &str, cursor: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let is_w = |c: char| c.is_alphanumeric() || c == '_' || c == '%';
    let cursor = cursor.min(chars.len());
    let mut s = cursor;
    while s > 0 && is_w(chars[s - 1]) {
        s -= 1;
    }
    let mut e = cursor;
    while e < chars.len() && is_w(chars[e]) {
        e += 1;
    }
    if s == e {
        None
    } else {
        Some(chars[s..e].iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{1f}\nFile: maxima.info,  Node: Functions and Variables for Integration,  Next: x\n\n21.2 Functions\n=====\n\n -- Function: integrate (<expr>, <x>)\n -- Function: integrate (<expr>, <x>, <a>, <b>)\n     Attempts to symbolically compute the integral.\n\n     More text here.\n\n -- Option variable: integrate_use_rootsof\n     Default value: `false'\n\n\u{1f}\nFile: maxima.info,  Node: Other\n\n -- Function: foo ()\n     Foo does stuff.\n";

    #[test]
    fn parses_entries() {
        let idx = HelpIndex::from_info(SAMPLE);
        let e = idx.first("integrate").unwrap();
        assert_eq!(e.signatures.len(), 2);
        assert_eq!(e.args(), vec!["expr", "x"]);
        assert_eq!(e.category, "Functions and Variables for Integration");
        assert!(e.body.contains("More text here"));
        assert_eq!(idx.first("integrate_use_rootsof").unwrap().kind, "Option variable");
        assert_eq!(idx.first("foo").unwrap().category, "Other");
        assert_eq!(idx.search("integr", 10).len(), 2);
    }

    #[test]
    fn word_lookup() {
        assert_eq!(word_at("x+integrate(a)", 8).as_deref(), Some("integrate"));
        assert_eq!(word_at("x+integrate(a)", 11).as_deref(), Some("integrate"));
        assert_eq!(word_at("x + ", 2), None);
    }

    #[test]
    fn real_manual_if_present() {
        let idx = HelpIndex::load_system();
        if idx.is_empty() {
            return;
        }
        assert!(idx.first("integrate").is_some());
        assert!(idx.first("solve").is_some());
        assert!(idx.categories().len() > 20);
    }
}

#[cfg(test)]
mod real_tests {
    use super::*;

    #[test]
    fn integrate_signatures_are_clean() {
        let idx = HelpIndex::load_system();
        if idx.is_empty() {
            return;
        }
        let e = idx.first("integrate").unwrap();
        assert_eq!(e.signatures, vec!["integrate (<expr>, <x>)", "integrate (<expr>, <x>, <a>, <b>)"], "{:?}", e.signatures);
        assert!(!e.body.is_empty());
    }
}
