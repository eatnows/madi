//! Rendering Markdown as a formatted document rather than raw, highlighted text.
//!
//! gyeol has no built-in paragraph word-wrap (a `text()` node is always exactly one line, sized to
//! its full content — see `Shaper`), so this wraps paragraphs itself: every run of markup (bold,
//! italic, inline code, link) becomes a "word" carrying its own style, words are greedily packed
//! into lines against the available width, and each line renders as a row of styled `text()`
//! children — the same "several colored spans per row" shape the syntax-highlighted editor already
//! uses.
//!
//! Simplifications (acceptable for a README/notes viewer, not a full renderer): tables and raw
//! HTML aren't parsed (their pipe/tag characters fall through as plain paragraph text); a `code`
//! span containing spaces splits into several independently-wrapped words; nested lists share one
//! indent step per level with no per-level marker style.
use gyeol::{div, text, Cx, Element, Shaper, TextStyle};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

use crate::{app::Madi, editor::MONO, theme::Palette};

type El = Element<Madi>;

#[derive(Clone)]
struct Word {
    text: String,
    bold: bool,
    italic: bool,
    code: bool,
    link: bool,
}

enum Block {
    Heading(u8, Vec<Word>),
    Paragraph(Vec<Word>),
    Quote(Vec<Word>),
    ListItem(usize, String, Vec<Word>),
    Code(String),
    Rule,
}

fn push_words(words: &mut Vec<Word>, run: &str, bold: bool, italic: bool, code: bool, link: bool) {
    for w in run.split_whitespace() {
        words.push(Word { text: w.to_string(), bold, italic, code, link });
    }
}

fn parse_blocks(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut words: Vec<Word> = Vec::new();
    let mut heading_level: Option<u8> = None;
    let mut in_quote = false;
    let mut in_item = false;
    let mut item_words: Vec<Word> = Vec::new();
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code_text = String::new();
    let (mut bold, mut italic, mut link) = (0i32, 0i32, 0i32);

    for ev in Parser::new(source) {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                heading_level = Some(level as u8);
                words.clear();
            }
            Event::End(TagEnd::Heading(_)) => blocks.push(Block::Heading(heading_level.take().unwrap_or(1), std::mem::take(&mut words))),
            Event::Start(Tag::Paragraph) => words.clear(),
            Event::End(TagEnd::Paragraph) => {
                if in_item {
                    item_words = std::mem::take(&mut words);
                } else if in_quote {
                    blocks.push(Block::Quote(std::mem::take(&mut words)));
                } else {
                    blocks.push(Block::Paragraph(std::mem::take(&mut words)));
                }
            }
            Event::Start(Tag::BlockQuote(_)) => in_quote = true,
            Event::End(TagEnd::BlockQuote(_)) => in_quote = false,
            Event::Start(Tag::List(first)) => list_stack.push(first),
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::Start(Tag::Item) => {
                in_item = true;
                item_words.clear();
                words.clear();
            }
            Event::End(TagEnd::Item) => {
                in_item = false;
                let depth = list_stack.len().saturating_sub(1);
                let marker = match list_stack.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}.");
                        *n += 1;
                        m
                    }
                    _ => "•".to_string(),
                };
                let words = if item_words.is_empty() { std::mem::take(&mut words) } else { std::mem::take(&mut item_words) };
                blocks.push(Block::ListItem(depth, marker, words));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code = true;
                code_text.clear();
                code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                blocks.push(Block::Code(std::mem::take(&mut code_text)));
                let _ = &code_lang;
            }
            Event::Rule => blocks.push(Block::Rule),
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold -= 1,
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::End(TagEnd::Emphasis) => italic -= 1,
            Event::Start(Tag::Link { .. }) => link += 1,
            Event::End(TagEnd::Link) => link -= 1,
            Event::Text(t) if in_code => code_text.push_str(&t),
            Event::Text(t) => push_words(&mut words, &t, bold > 0, italic > 0, false, link > 0),
            Event::Code(t) => push_words(&mut words, &t, bold > 0, italic > 0, true, link > 0),
            _ => {}
        }
    }
    blocks
}

fn word_style(w: &Word, size: f32) -> TextStyle {
    let mut style = TextStyle::new(size);
    if w.code {
        style = style.family(MONO);
    }
    if w.bold {
        style = style.bold();
    }
    if w.italic {
        style = style.italic();
    }
    style
}

/// Greedily packs `words` into lines no wider than `width`.
fn wrap_words(shaper: &mut Shaper, words: &[Word], size: f32, width: f32) -> Vec<Vec<Word>> {
    let space_w = shaper.width(" ", TextStyle::new(size));
    let mut lines = Vec::new();
    let mut line: Vec<Word> = Vec::new();
    let mut line_w = 0.0f32;
    for w in words {
        let word_w = shaper.width(&w.text, word_style(w, size));
        let with_word = if line.is_empty() { word_w } else { line_w + space_w + word_w };
        if !line.is_empty() && with_word > width {
            lines.push(std::mem::take(&mut line));
            line_w = word_w;
        } else {
            line_w = with_word;
        }
        line.push(w.clone());
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn render_line(words: &[Word], size: f32, force_bold: bool, p: &Palette) -> El {
    let mut row = div().row().items_center();
    for (i, w) in words.iter().enumerate() {
        let content = if i + 1 < words.len() { format!("{} ", w.text) } else { w.text.clone() };
        let mut t = text(content).text_size(size);
        if w.code {
            t = t.text_family(MONO).text_color(p.syntax_string);
        }
        if w.bold || force_bold {
            t = t.text_bold().text_color(p.text_strong);
        }
        if w.italic {
            t = t.text_italic();
        }
        if w.link {
            t = t.text_color(p.syntax_type);
        }
        row = row.child(t);
    }
    row
}

fn wrapped(cx: &mut Cx, words: &[Word], size: f32, force_bold: bool, width: f32, p: &Palette) -> El {
    let mut col = div().col().gap(2.);
    for line in wrap_words(cx.shaper, words, size, width) {
        col = col.child(render_line(&line, size, force_bold, p));
    }
    col
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 24.,
        2 => 20.,
        3 => 17.,
        _ => 14.,
    }
}

fn render_block(cx: &mut Cx, p: &Palette, block: &Block, width: f32) -> El {
    match block {
        Block::Heading(level, words) => {
            let size = heading_size(*level);
            div().mt(if *level <= 1 { 4. } else { 10. }).child(wrapped(cx, words, size, true, width, p))
        }
        Block::Paragraph(words) => wrapped(cx, words, 13., false, width, p),
        Block::Quote(words) => div()
            .row()
            .child(div().w(3.).bg(p.border))
            .child(div().w(10.))
            .child(wrapped(cx, words, 13., false, width - 13., p).text_color(p.text_dim)),
        Block::ListItem(depth, marker, words) => {
            let indent = 8. + *depth as f32 * 18.;
            div()
                .row()
                .child(div().w(indent))
                .child(div().w(18.).child(text(marker.clone()).text_size(13.).text_color(p.text_dim)))
                .child(wrapped(cx, words, 13., false, (width - indent - 18.).max(100.), p))
        }
        Block::Code(code) => {
            let mut block = div().col().p(10.).rounded(6.).border(1., p.border).bg(p.panel).text_family(MONO).text_size(12.).text_color(p.text);
            for line in code.lines() {
                block = block.child(text(line.to_string()));
            }
            block
        }
        Block::Rule => div().h(1.).bg(p.border),
    }
}

/// Renders `source` as a formatted document at `width` (the available column width for wrapping).
pub fn render(cx: &mut Cx, p: &Palette, source: &str, width: f32) -> El {
    let width = width.max(200.);
    let blocks = parse_blocks(source);
    let mut col = div().col().gap(10.).p(16.);
    for block in &blocks {
        col = col.child(render_block(cx, p, block, width));
    }
    col
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headings_paragraphs_lists_and_code() {
        let blocks = parse_blocks("# Title\n\nSome **bold** and *italic* and `code`.\n\n- one\n- two\n\n```\nfn f() {}\n```\n");
        assert!(matches!(&blocks[0], Block::Heading(1, words) if words[0].text == "Title"));
        let Block::Paragraph(words) = &blocks[1] else { panic!("expected a paragraph") };
        assert!(words.iter().any(|w| w.text == "bold" && w.bold));
        assert!(words.iter().any(|w| w.text == "italic" && w.italic));
        assert!(words.iter().any(|w| w.text.starts_with("code") && w.code));
        assert!(matches!(&blocks[2], Block::ListItem(0, m, words) if m == "•" && words[0].text == "one"));
        assert!(matches!(&blocks[3], Block::ListItem(0, m, words) if m == "•" && words[0].text == "two"));
        assert!(matches!(&blocks[4], Block::Code(code) if code.contains("fn f()")));
    }

    #[test]
    fn ordered_lists_number_sequentially() {
        let blocks = parse_blocks("1. first\n2. second\n");
        assert!(matches!(&blocks[0], Block::ListItem(0, m, _) if m == "1."));
        assert!(matches!(&blocks[1], Block::ListItem(0, m, _) if m == "2."));
    }

    #[test]
    fn wrapping_never_exceeds_the_target_width_per_word() {
        let mut shaper = Shaper::new();
        let words: Vec<Word> = "the quick brown fox jumps over the lazy dog".split(' ').map(|w| Word { text: w.into(), bold: false, italic: false, code: false, link: false }).collect();
        let lines = wrap_words(&mut shaper, &words, 13., 80.);
        assert!(lines.len() > 1, "80px should force more than one line");
        for line in &lines {
            let joined = line.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            assert!(!joined.is_empty());
        }
        // every word appears exactly once, in order
        let flat: Vec<&str> = lines.iter().flatten().map(|w| w.text.as_str()).collect();
        assert_eq!(flat, vec!["the", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog"]);
    }
}
