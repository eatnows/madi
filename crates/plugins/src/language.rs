//! A language definition: regex-based highlight rules and symbol patterns, in the spirit of a
//! VS Code TextMate grammar rather than a real parser (no tree-sitter/WASM runtime). Rules are
//! plain data, tried in order against each line; the `regex` crate is used specifically because it
//! guarantees linear-time matching, so a pathological pattern can't hang on a long line the way a
//! backtracking engine could.
use std::ops::Range;

use regex::Regex;
use serde::Deserialize;

/// Lines longer than this render in the plain text color without tokenizing — the same kind of
/// safeguard VS Code's `editor.maxTokenizationLineLength` is, against wasting cycles on a single
/// enormous (often generated/minified) line.
pub const MAX_LINE_LEN: usize = 4000;

#[derive(Deserialize)]
struct RawRule {
    scope: String,
    pattern: String,
}

#[derive(Deserialize)]
struct RawSymbolRule {
    kind: String,
    /// Must have exactly one capture group: the symbol's name.
    pattern: String,
}

#[derive(Deserialize)]
struct RawLanguage {
    #[serde(default)]
    highlight: Vec<RawRule>,
    #[serde(default)]
    symbols: Vec<RawSymbolRule>,
}

struct Rule {
    scope: String,
    regex: Regex,
}

struct SymbolRule {
    kind: String,
    regex: Regex,
}

pub struct Language {
    highlight: Vec<Rule>,
    symbols: Vec<SymbolRule>,
}

/// A definition found on one line (e.g. a function or type), for a "go to symbol" picker.
#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: usize,
}

impl Language {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let raw: RawLanguage = serde_json::from_slice(bytes).map_err(|e| format!("invalid language rules: {e}"))?;
        let highlight = raw
            .highlight
            .into_iter()
            .map(|r| {
                let regex = anchored(&r.pattern).map_err(|e| format!("bad highlight pattern for {}: {e}", r.scope))?;
                Ok(Rule { scope: r.scope, regex })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let symbols = raw
            .symbols
            .into_iter()
            .map(|r| {
                let regex = Regex::new(&r.pattern).map_err(|e| format!("bad symbol pattern for {}: {e}", r.kind))?;
                if regex.captures_len() < 2 {
                    return Err(format!("symbol pattern for {} needs a capture group for the name", r.kind));
                }
                Ok(SymbolRule { kind: r.kind, regex })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self { highlight, symbols })
    }

    /// Splits `line` into runs, in order, covering every byte: `Some(scope)` for a highlighted
    /// span, `None` for a run with no matching rule (rendered in the plain text color).
    pub fn tokenize<'s>(&'s self, line: &str) -> Vec<(Range<usize>, Option<&'s str>)> {
        let mut spans = Vec::new();
        if self.highlight.is_empty() || line.len() > MAX_LINE_LEN {
            if !line.is_empty() {
                spans.push((0..line.len(), None));
            }
            return spans;
        }
        let mut i = 0;
        let mut plain_start = 0;
        while i < line.len() {
            let hit = self.highlight.iter().find_map(|rule| {
                let m = rule.regex.find(&line[i..])?;
                (m.start() == 0 && !m.as_str().is_empty()).then_some((rule.scope.as_str(), m.end()))
            });
            match hit {
                Some((scope, len)) => {
                    if plain_start < i {
                        spans.push((plain_start..i, None));
                    }
                    spans.push((i..i + len, Some(scope)));
                    i += len;
                    plain_start = i;
                }
                None => i += line[i..].chars().next().map_or(1, char::len_utf8),
            }
        }
        if plain_start < line.len() {
            spans.push((plain_start..line.len(), None));
        }
        spans
    }

    /// Every symbol definition found across `lines`, in order.
    pub fn symbols<'a>(&self, lines: impl Iterator<Item = &'a str>) -> Vec<Symbol> {
        let mut found = Vec::new();
        for (row, line) in lines.enumerate() {
            for rule in &self.symbols {
                for caps in rule.regex.captures_iter(line) {
                    if let Some(name) = caps.get(1) {
                        found.push(Symbol { name: name.as_str().to_string(), kind: rule.kind.clone(), line: row });
                    }
                }
            }
        }
        found
    }
}

fn anchored(pattern: &str) -> Result<Regex, regex::Error> {
    Regex::new(&format!("^(?:{pattern})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang() -> Language {
        Language::parse(
            br#"{
                "highlight": [
                    {"scope": "comment", "pattern": "//.*"},
                    {"scope": "string", "pattern": "\"([^\"\\\\]|\\\\.)*\""},
                    {"scope": "number", "pattern": "\\b\\d+\\b"},
                    {"scope": "keyword", "pattern": "\\b(fn|let)\\b"}
                ],
                "symbols": [
                    {"kind": "function", "pattern": "fn\\s+([A-Za-z_][A-Za-z0-9_]*)"}
                ]
            }"#,
        )
        .unwrap()
    }

    fn rendered<'a, 'b>(l: &'b Language, line: &'a str) -> Vec<(&'a str, Option<&'b str>)> {
        l.tokenize(line).into_iter().map(|(r, scope)| (&line[r], scope)).collect()
    }

    #[test]
    fn tokenizes_keywords_strings_numbers_and_comments_leaving_gaps_plain() {
        let l = lang();
        assert_eq!(
            rendered(&l, r#"let x = "hi" + 1; // note"#),
            vec![
                ("let", Some("keyword")),
                (" x = ", None),
                (r#""hi""#, Some("string")),
                (" + ", None),
                ("1", Some("number")),
                ("; ", None),
                ("// note", Some("comment")),
            ]
        );
    }

    #[test]
    fn an_unmatched_line_is_a_single_plain_run() {
        assert_eq!(rendered(&lang(), "plain text"), vec![("plain text", None)]);
        assert_eq!(lang().tokenize(""), Vec::new());
    }

    #[test]
    fn a_line_past_the_length_cap_is_skipped_untokenized() {
        let long = "x".repeat(MAX_LINE_LEN + 1);
        let l = lang();
        let spans = l.tokenize(&long);
        assert_eq!(spans, vec![(0..long.len(), None)]);
    }

    #[test]
    fn symbols_are_collected_with_their_line_number() {
        let l = lang();
        let text = "struct Foo;\nfn bar() {}\nfn baz() {}";
        let found = l.symbols(text.lines());
        assert_eq!(
            found,
            vec![
                Symbol { name: "bar".into(), kind: "function".into(), line: 1 },
                Symbol { name: "baz".into(), kind: "function".into(), line: 2 },
            ]
        );
    }

    #[test]
    fn a_symbol_pattern_without_a_capture_group_is_rejected() {
        let err = match Language::parse(br#"{"symbols":[{"kind":"function","pattern":"fn [a-z]+"}]}"#) { Err(e) => e, Ok(_) => panic!("expected an error") };
        assert!(err.contains("capture group"), "{err}");
    }
}
