//! Real syntax-tree-based highlighting and symbol extraction, via tree-sitter grammars bundled
//! directly into Madi at build time (the same approach Zed uses for its core languages) — no
//! plugin download, no WASM runtime. Adding a language means adding its `tree-sitter-*` crate as a
//! dependency and one match arm in [`grammar_for_extension`].
use std::ops::Range;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

/// A definition found in a file (e.g. a function), for a "go to symbol" picker.
#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: usize,
}

struct Grammar {
    language: tree_sitter::Language,
    highlights: &'static str,
    tags: &'static str,
}

fn grammar_for_extension(ext: &str) -> Option<Grammar> {
    match ext.to_ascii_lowercase().as_str() {
        "rs" => Some(Grammar {
            language: tree_sitter_rust::LANGUAGE.into(),
            highlights: tree_sitter_rust::HIGHLIGHTS_QUERY,
            tags: tree_sitter_rust::TAGS_QUERY,
        }),
        _ => None,
    }
}

pub struct Language {
    grammar: Grammar,
    highlight_query: Query,
    tags_query: Query,
}

/// Maps a tree-sitter highlight capture name (e.g. `"function.method"`, `"punctuation.bracket"`)
/// to one of the few colors the editor actually has; anything else renders in the plain text color.
fn bucket(name: &str) -> Option<&'static str> {
    if name.starts_with("comment") {
        Some("comment")
    } else if name.starts_with("string") {
        Some("string")
    } else if name.starts_with("number") || name.starts_with("constant") {
        Some("number")
    } else if name.starts_with("keyword") {
        Some("keyword")
    } else if name.starts_with("type") {
        Some("type")
    } else {
        None
    }
}

/// Byte offset of the start of each line (row `i` starts at `line_starts[i]`); its length is the
/// line count, matching how `madi_text::Buffer` splits on `'\n'` (a trailing newline counts as one
/// more, empty, line).
fn line_start_offsets(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn row_of(line_starts: &[usize], byte_offset: usize) -> usize {
    match line_starts.binary_search(&byte_offset) {
        Ok(row) => row,
        Err(row) => row.saturating_sub(1),
    }
}

fn line_end(line_starts: &[usize], text_len: usize, row: usize) -> usize {
    if row + 1 < line_starts.len() { line_starts[row + 1] - 1 } else { text_len }
}

/// Splits a global byte range into the (row, line-relative range) pieces it covers — usually one,
/// but a multi-line comment or string covers several.
fn split_by_line(line_starts: &[usize], text_len: usize, range: Range<usize>) -> Vec<(usize, Range<usize>)> {
    if range.start >= range.end {
        return Vec::new();
    }
    let start_row = row_of(line_starts, range.start);
    let end_row = row_of(line_starts, range.end - 1);
    (start_row..=end_row)
        .filter_map(|row| {
            let (row_start, row_end) = (line_starts[row], line_end(line_starts, text_len, row));
            let (lo, hi) = (range.start.max(row_start), range.end.min(row_end));
            (lo < hi).then(|| (row, (lo - row_start)..(hi - row_start)))
        })
        .collect()
}

impl Language {
    /// The bundled grammar for `ext` (without the dot, e.g. `"rs"`), if Madi ships one.
    pub fn for_extension(ext: &str) -> Option<Self> {
        let grammar = grammar_for_extension(ext)?;
        let highlight_query = Query::new(&grammar.language, grammar.highlights).ok()?;
        let tags_query = Query::new(&grammar.language, grammar.tags).ok()?;
        Some(Self { grammar, highlight_query, tags_query })
    }

    fn parse(&self, text: &str) -> Option<tree_sitter::Tree> {
        let mut parser = Parser::new();
        parser.set_language(&self.grammar.language).ok()?;
        parser.parse(text, None)
    }

    /// Highlight spans per line: `result[row]` covers byte ranges *within that line*, each with a
    /// bucket name; any byte range not covered renders in the plain text color. Parses the whole
    /// document (not incrementally) — callers should cache the result per document revision rather
    /// than call this once per frame.
    pub fn highlight_lines(&self, text: &str) -> Vec<Vec<(Range<usize>, &'static str)>> {
        let line_starts = line_start_offsets(text);
        let mut spans: Vec<Vec<(Range<usize>, &'static str)>> = vec![Vec::new(); line_starts.len()];
        let Some(tree) = self.parse(text) else { return spans };

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.highlight_query, tree.root_node(), text.as_bytes());
        while let Some(m) = matches.next() {
            for cap in m.captures {
                let Some(name) = bucket(self.highlight_query.capture_names()[cap.index as usize]) else { continue };
                for (row, range) in split_by_line(&line_starts, text.len(), cap.node.byte_range()) {
                    spans[row].push((range, name));
                }
            }
        }
        for row_spans in &mut spans {
            row_spans.sort_by_key(|(range, _)| range.start);
        }
        spans
    }

    /// Every symbol definition in the file (the grammar's `definition.*` tag captures, e.g. a
    /// function or type), with its 0-based line number.
    pub fn symbols(&self, text: &str) -> Vec<Symbol> {
        let Some(tree) = self.parse(text) else { return Vec::new() };
        let line_starts = line_start_offsets(text);
        let names = self.tags_query.capture_names();
        let mut out = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.tags_query, tree.root_node(), text.as_bytes());
        while let Some(m) = matches.next() {
            let Some(kind_cap) = m.captures.iter().find(|c| names[c.index as usize].starts_with("definition")) else { continue };
            let Some(name_cap) = m.captures.iter().find(|c| names[c.index as usize] == "name") else { continue };
            let kind = names[kind_cap.index as usize].trim_start_matches("definition.").to_string();
            let name = text[name_cap.node.byte_range()].to_string();
            out.push(Symbol { name, kind, line: row_of(&line_starts, name_cap.node.start_byte()) });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "fn main() {\n    let x = \"hi\"; // note\n    helper();\n}\n\nfn helper() {}\n";

    #[test]
    fn unsupported_extensions_return_none() {
        assert!(Language::for_extension("made-up-lang").is_none());
    }

    #[test]
    fn highlights_keywords_strings_and_comments_per_line() {
        let lang = Language::for_extension("rs").unwrap();
        let lines = lang.highlight_lines(CODE);
        assert_eq!(lines.len(), CODE.matches('\n').count() + 1);

        let line0 = CODE.lines().next().unwrap();
        assert!(lines[0].iter().any(|(r, b)| &line0[r.clone()] == "fn" && *b == "keyword"), "{:?}", lines[0]);

        let line1 = &CODE.lines().nth(1).unwrap();
        let string_span = lines[1].iter().find(|(_, b)| *b == "string").expect("a string span on line 1");
        assert_eq!(&line1[string_span.0.clone()], "\"hi\"");
        let comment_span = lines[1].iter().find(|(_, b)| *b == "comment").expect("a comment span on line 1");
        assert_eq!(&line1[comment_span.0.clone()], "// note");
    }

    #[test]
    fn finds_function_definitions_with_their_line_number() {
        let lang = Language::for_extension("rs").unwrap();
        let symbols = lang.symbols(CODE);
        assert_eq!(
            symbols,
            vec![
                Symbol { name: "main".into(), kind: "function".into(), line: 0 },
                Symbol { name: "helper".into(), kind: "function".into(), line: 5 },
            ]
        );
    }

    #[test]
    fn a_block_comment_spanning_lines_is_highlighted_on_every_line_it_touches() {
        let lang = Language::for_extension("rs").unwrap();
        let code = "/* line one\nline two */\nfn f() {}\n";
        let lines = lang.highlight_lines(code);
        assert!(lines[0].iter().any(|(_, b)| *b == "comment"));
        assert!(lines[1].iter().any(|(_, b)| *b == "comment"));
    }
}
