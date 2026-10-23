//! Syntax highlighting for diff lines, in the colors React gets from Prism:
//! `prism-vs` in light mode and `prism-vsc-dark-plus` in dark mode.
//!
//! React highlights each diff line on its own (`HighlightedLine`), so this
//! does too: a line is parsed from a fresh state, and its spans are cached
//! by language and text because a diff redraws on every hover.

use std::{
  cell::RefCell,
  collections::HashMap,
  hash::{BuildHasher, RandomState},
  ops::Range,
  rc::Rc,
  sync::LazyLock,
};

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};
use tauri_plugin_gpui::gpui::{
  rgb, FontStyle, FontWeight, HighlightStyle, Hsla, SharedString, StyledText,
};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// The cache holds at most this many lines, which a long review session
/// reaches without holding much memory.
const CACHE_MAX: usize = 20_000;

/// What Prism would call a span, reduced to what the two themes color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Token {
  Comment,
  String,
  Number,
  Boolean,
  Constant,
  Keyword,
  /// `if`, `return`, `import`: purple in vsc-dark-plus for JavaScript.
  ControlFlow,
  Function,
  ClassName,
  Property,
  Tag,
  AttrName,
  Regex,
  Escape,
  /// A Markdown heading's text: Prism's `title important`.
  Title,
  /// Markdown inline code, which Prism aliases to `keyword`.
  Code,
  Bold,
  Italic,
  /// Text an enclosing scope would color, which Prism leaves plain (a
  /// heading's `#`).
  Plain,
}

pub(super) type Spans = Rc<Vec<(Range<usize>, Token)>>;

/// A highlighted line: its text, which [`styled`] draws without copying
/// it again, and its spans.
#[derive(Clone)]
struct Line {
  text: SharedString,
  spans: Spans,
}

/// Lines by language and a hash of their text, so a hit allocates nothing.
/// Lines go into `current`; once it holds half of [`CACHE_MAX`] it becomes
/// `previous`, and a line found there moves back. A full cache so drops
/// only lines not drawn for a while, never the ones on screen.
#[derive(Default)]
struct Cache {
  current: HashMap<(&'static str, u64), Line>,
  previous: HashMap<(&'static str, u64), Line>,
  hasher: RandomState,
}

impl Cache {
  fn get(&mut self, key: (&'static str, u64), text: &str) -> Option<Line> {
    if let Some(line) = self.current.get(&key) {
      // Another line with the same hash is a miss, and is replaced.
      return (line.text.as_ref() == text).then(|| line.clone());
    }
    let line = self.previous.remove(&key)?;
    if line.text.as_ref() != text {
      return None;
    }
    self.insert(key, line.clone());
    Some(line)
  }

  fn insert(&mut self, key: (&'static str, u64), line: Line) {
    if self.current.len() >= CACHE_MAX / 2 {
      self.previous = std::mem::take(&mut self.current);
    }
    self.current.insert(key, line);
  }
}

thread_local! {
  static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
  /// What each scope maps to, by whether the file is JavaScript: a scope's
  /// name is built and matched once.
  static TOKENS: RefCell<HashMap<(Scope, bool), Option<Token>>> = RefCell::new(HashMap::new());
}

/// The language React's `getLanguageFromPath` picks for a file, as the
/// syntect token that finds its grammar; `None` leaves the file plain.
pub(super) fn language(path: &str) -> Option<&'static str> {
  let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
  Some(match ext.as_str() {
    "bash" | "sh" | "zsh" => "sh",
    "c" | "h" => "c",
    "cc" | "cpp" | "hpp" => "cpp",
    "css" => "css",
    "erl" | "hrl" => "erl",
    "ex" | "exs" => "ex",
    "go" => "go",
    "htm" | "html" => "html",
    "java" => "java",
    "js" | "mjs" => "js",
    "json" => "json",
    "jsx" => "js",
    "md" => "md",
    "py" => "py",
    "rb" => "rb",
    "rs" => "rs",
    "scss" => "scss",
    "sql" => "sql",
    "svg" | "xml" => "xml",
    "ts" => "ts",
    "tsx" => "tsx",
    "yaml" | "yml" => "yaml",
    _ => return None,
  })
}

fn syntax(language: &str) -> Option<&'static SyntaxReference> {
  SYNTAXES
    .find_syntax_by_extension(language)
    .or_else(|| SYNTAXES.find_syntax_by_token(language))
}

/// Token spans of one line, byte ranges into `line`.
pub(super) fn spans(language: &'static str, line: &str) -> Spans {
  highlighted(language, line).spans
}

fn highlighted(language: &'static str, line: &str) -> Line {
  let key = CACHE.with(|c| {
    let mut c = c.borrow_mut();
    let key = (language, c.hasher.hash_one(line));
    c.get(key, line).ok_or(key)
  });
  let key = match key {
    Ok(hit) => return hit,
    Err(key) => key,
  };
  let hit = Line {
    text: SharedString::from(line.to_string()),
    spans: Rc::new(
      syntax(language)
        .map(|s| parse(language, s, line))
        .unwrap_or_default(),
    ),
  };
  CACHE.with(|c| c.borrow_mut().insert(key, hit.clone()));
  hit
}

fn parse(language: &str, syntax: &SyntaxReference, line: &str) -> Vec<(Range<usize>, Token)> {
  let mut state = ParseState::new(syntax);
  let mut scopes = ScopeStack::new();
  let mut out: Vec<(Range<usize>, Token)> = Vec::new();
  let text = format!("{line}\n");
  let Ok(ops) = state.parse_line(&text, &SYNTAXES) else {
    return out;
  };
  let js = matches!(language, "js" | "ts" | "tsx");
  let mut push = |range: Range<usize>, scopes: &ScopeStack| {
    let range = range.start.min(line.len())..range.end.min(line.len());
    if range.is_empty() {
      return;
    }
    let Some(token) = scopes.scopes.iter().rev().find_map(|s| token(*s, js)) else {
      return;
    };
    match out.last_mut() {
      Some((last, t)) if last.end == range.start && *t == token => last.end = range.end,
      _ => out.push((range, token)),
    }
  };
  let mut cursor = 0;
  for (index, op) in ops {
    push(cursor..index, &scopes);
    let _ = scopes.apply(&op);
    cursor = index;
  }
  push(cursor..text.len(), &scopes);
  out
}

/// [`token_of`], remembered.
fn token(scope: Scope, js: bool) -> Option<Token> {
  TOKENS.with(|t| {
    *t.borrow_mut()
      .entry((scope, js))
      .or_insert_with(|| token_of(scope, js))
  })
}

/// Maps a TextMate scope onto the Prism token that colors the same thing.
fn token_of(scope: Scope, js: bool) -> Option<Token> {
  let scope = scope.build_string();
  let is = |prefix: &str| {
    scope == prefix
      || scope
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('.'))
  };
  Some(if is("punctuation.definition.heading") {
    Token::Plain
  } else if is("comment") || is("punctuation.definition.comment") {
    Token::Comment
  } else if is("constant.character.escape") {
    Token::Escape
  } else if is("string.regexp") {
    Token::Regex
  } else if is("string") || is("punctuation.definition.string") {
    Token::String
  } else if is("constant.numeric") {
    Token::Number
  } else if is("constant.language.boolean") {
    Token::Boolean
  } else if is("constant.language") {
    // `null`, `undefined`, `None`: Prism lexes these as keywords.
    Token::Keyword
  } else if is("constant.other") || is("entity.name.constant") || is("support.constant") {
    Token::Constant
  } else if is("markup.heading") || is("entity.name.section") {
    Token::Title
  } else if is("markup.raw.inline") {
    Token::Code
  } else if is("markup.bold") {
    Token::Bold
  } else if is("markup.italic") {
    Token::Italic
  } else if is("keyword.operator") || is("punctuation") {
    return None;
  } else if js && (is("keyword.control") || is("keyword.import") || is("keyword.export")) {
    Token::ControlFlow
  } else if is("keyword") || is("storage") {
    Token::Keyword
  } else if is("entity.name.function") || is("support.function") || is("variable.function") {
    Token::Function
  } else if is("entity.name.type")
    || is("entity.name.class")
    || is("entity.name.struct")
    || is("entity.name.enum")
    || is("entity.name.trait")
    || is("entity.other.inherited-class")
    || is("support.class")
    || is("support.type") && !is("support.type.property-name")
  {
    Token::ClassName
  } else if is("support.type.property-name") || is("entity.name.tag.yaml") {
    Token::Property
  } else if is("entity.name.tag") {
    Token::Tag
  } else if is("entity.other.attribute-name") {
    Token::AttrName
  } else {
    return None;
  })
}

/// The token's color in the Prism theme for `dark`; `None` keeps the text
/// color, as Prism does for punctuation and operators.
pub(super) fn color(token: Token, dark: bool) -> Option<Hsla> {
  let hex = if dark {
    match token {
      Token::Comment => 0x6a9955,
      Token::String => 0xce9178,
      Token::Number => 0xb5cea8,
      Token::Boolean | Token::Keyword | Token::Tag => 0x569cd6,
      Token::ControlFlow => 0xc586c0,
      Token::Function => 0xdcdcaa,
      Token::ClassName => 0x4ec9b0,
      Token::Constant | Token::Property | Token::AttrName => 0x9cdcfe,
      Token::Regex => 0xd16969,
      Token::Escape => 0xd7ba7d,
      Token::Title | Token::Code => 0x569cd6,
      Token::Bold | Token::Italic | Token::Plain => return None,
    }
  } else {
    match token {
      Token::Comment => 0x008000,
      Token::String | Token::Escape => 0xa31515,
      Token::Number | Token::Boolean | Token::Constant => 0x36acaa,
      Token::Keyword | Token::ControlFlow => 0x0000ff,
      Token::Function => return None,
      Token::ClassName => 0x2b91af,
      Token::Tag => 0x800000,
      Token::Property | Token::AttrName | Token::Regex => 0xff0000,
      Token::Title => 0xee9900,
      Token::Code => 0x0000ff,
      Token::Bold | Token::Italic | Token::Plain => return None,
    }
  };
  Some(rgb(hex).into())
}

/// `line` drawn with its highlights; plain when the file has no language.
pub(super) fn styled(line: &str, language: Option<&'static str>, dark: bool) -> StyledText {
  let (text, styles) = text_and_styles(line, language, dark);
  StyledText::new(text).with_highlights(styles)
}

/// [`styled`] with `marks` painted behind the text (search matches).
pub(super) fn styled_marked(
  line: &str,
  language: Option<&'static str>,
  dark: bool,
  marks: &[(Range<usize>, Hsla)],
) -> StyledText {
  let (text, styles) = text_and_styles(line, language, dark);
  StyledText::new(text).with_highlights(overlay(styles, marks))
}

/// `line` as drawn, from the cache when it has a language, and its styles.
fn text_and_styles(
  line: &str,
  language: Option<&'static str>,
  dark: bool,
) -> (SharedString, Vec<(Range<usize>, HighlightStyle)>) {
  let Some(language) = language else {
    return (SharedString::from(line.to_string()), Vec::new());
  };
  let Line { text, spans } = highlighted(language, line);
  let styles = spans
    .iter()
    .filter_map(|(range, token)| {
      let italic = *token == Token::Italic || *token == Token::Comment && !dark;
      // Only prism-vs makes these bold.
      let bold = matches!(token, Token::Title | Token::Bold) && !dark;
      let color = color(*token, dark);
      (color.is_some() || italic || bold).then(|| {
        (
          range.clone(),
          HighlightStyle {
            color,
            font_style: italic.then_some(FontStyle::Italic),
            font_weight: bold.then_some(FontWeight::BOLD),
            ..Default::default()
          },
        )
      })
    })
    .collect();
  (text, styles)
}

/// Splits sorted, disjoint `styles` at the `marks`' edges so each piece
/// carries both: `StyledText` takes no overlapping ranges.
fn overlay(
  styles: Vec<(Range<usize>, HighlightStyle)>,
  marks: &[(Range<usize>, Hsla)],
) -> Vec<(Range<usize>, HighlightStyle)> {
  if marks.is_empty() {
    return styles;
  }
  let mut edges: Vec<usize> = styles
    .iter()
    .flat_map(|(r, _)| [r.start, r.end])
    .chain(marks.iter().flat_map(|(r, _)| [r.start, r.end]))
    .collect();
  edges.sort_unstable();
  edges.dedup();
  let mut out: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
  for pair in edges.windows(2) {
    let piece = pair[0]..pair[1];
    let token = styles
      .iter()
      .find(|(r, _)| r.start <= piece.start && piece.end <= r.end)
      .map(|(_, s)| *s);
    let mark = marks
      .iter()
      .find(|(r, _)| r.start <= piece.start && piece.end <= r.end)
      .map(|(_, c)| *c);
    if token.is_none() && mark.is_none() {
      continue;
    }
    let mut style = token.unwrap_or_default();
    style.background_color = mark;
    out.push((piece, style));
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  fn tokens<'a>(language: &'static str, line: &'a str) -> Vec<(&'a str, Token)> {
    spans(language, line)
      .iter()
      .map(|(r, t)| (&line[r.clone()], *t))
      .collect()
  }

  #[test]
  fn marks_split_the_tokens_they_cross() {
    let bold = HighlightStyle {
      font_weight: Some(FontWeight::BOLD),
      ..Default::default()
    };
    let red: Hsla = rgb(0xff0000).into();
    let out = overlay(vec![(0..4, bold)], &[(2..6, red)]);
    let ranges: Vec<_> = out.iter().map(|(r, _)| r.clone()).collect();
    assert_eq!(ranges, vec![0..2, 2..4, 4..6]);
    assert_eq!(out[0].1.background_color, None);
    assert_eq!(out[1].1.background_color, Some(red));
    assert_eq!(out[1].1.font_weight, Some(FontWeight::BOLD));
    assert_eq!(out[2].1.font_weight, None);
  }

  #[test]
  fn a_full_cache_keeps_the_lines_read_since() {
    let mut cache = Cache::default();
    let line = |text: &str| Line {
      text: text.to_string().into(),
      spans: Rc::default(),
    };
    cache.insert(("rs", 0), line("kept"));
    cache.insert(("rs", 1), line("dropped"));
    for i in 2..2 * CACHE_MAX as u64 {
      cache.insert(("rs", i), line("other"));
      if i % 1000 == 0 {
        assert!(cache.get(("rs", 0), "kept").is_some());
      }
    }
    assert!(cache.get(("rs", 0), "kept").is_some());
    assert!(cache.get(("rs", 1), "dropped").is_none());
    assert!(cache.current.len() + cache.previous.len() <= CACHE_MAX);
    // Another text with the same hash is not taken for the line.
    assert!(cache.get(("rs", 0), "collides").is_none());
  }

  #[test]
  fn picks_the_language_react_does() {
    assert_eq!(language("src/a.tsx"), Some("tsx"));
    assert_eq!(language("x.YML"), Some("yaml"));
    assert_eq!(language("Makefile"), None);
    assert_eq!(language("notes.txt"), None);
  }

  #[test]
  fn every_language_has_a_grammar() {
    for ext in [
      "a.sh", "a.c", "a.cpp", "a.css", "a.erl", "a.ex", "a.go", "a.html", "a.java", "a.js",
      "a.json", "a.jsx", "a.md", "a.py", "a.rb", "a.rs", "a.scss", "a.sql", "a.xml", "a.ts",
      "a.tsx", "a.yaml",
    ] {
      let lang = language(ext).unwrap();
      assert!(syntax(lang).is_some(), "{ext} has no grammar");
    }
  }

  #[test]
  fn highlights_rust_keywords_strings_and_comments() {
    let t = tokens("rs", "let s = \"hi\"; // note");
    assert!(t.contains(&("let", Token::Keyword)), "{t:?}");
    assert!(t.contains(&("\"hi\"", Token::String)), "{t:?}");
    assert!(t.contains(&("// note", Token::Comment)), "{t:?}");
  }

  #[test]
  fn javascript_control_flow_is_its_own_token() {
    let t = tokens("ts", "return 42;");
    assert!(t.contains(&("return", Token::ControlFlow)), "{t:?}");
    assert!(t.contains(&("42", Token::Number)), "{t:?}");
  }

  #[test]
  fn markdown_headings_and_inline_code() {
    let t = tokens("md", "# Logout");
    assert!(t.contains(&(" Logout", Token::Title)), "{t:?}");
    assert!(
      !t.iter().any(|(s, k)| s.contains('#') && *k == Token::Title),
      "{t:?}"
    );
    let t = tokens("md", "Call `logout()` now");
    assert!(t.contains(&("`logout()`", Token::Code)), "{t:?}");
  }

  #[test]
  fn colors_follow_the_prism_themes() {
    assert_eq!(color(Token::Keyword, false), Some(rgb(0x0000ff).into()));
    assert_eq!(color(Token::Keyword, true), Some(rgb(0x569cd6).into()));
    assert_eq!(color(Token::Function, false), None);
  }
}
