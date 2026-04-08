//! CSS selector parser and AST.
//!
//! Supports: type, class, id, attribute, :first-child, :last-child,
//! :nth-child, :nth-last-child, :only-child, :not(), :empty, :root,
//! descendant/child/sibling combinators, selector lists.

use std::fmt;

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

/// A selector list: `a, b, c` parses into `vec![Selector, Selector, Selector]`.
pub type SelectorList = Vec<Selector>;

/// A complex selector: a chain of compound selectors joined by combinators.
/// The rightmost entry has `Combinator::None`.
/// e.g. `div > p.intro` → `[(div, Child), (p.intro, None)]`
#[derive(Debug, Clone)]
pub struct Selector(pub Vec<(CompoundSelector, Combinator)>);

#[derive(Debug, Clone, PartialEq)]
pub enum Combinator {
    /// No combinator (rightmost position).
    None,
    /// ` ` (descendant)
    Descendant,
    /// `>` (child)
    Child,
    /// `+` (adjacent sibling)
    NextSibling,
    /// `~` (general sibling)
    SubsequentSibling,
}

/// A compound selector: `div.foo#bar[href]:first-child`
#[derive(Debug, Clone, Default)]
pub struct CompoundSelector {
    /// Tag name or `*` for universal. `None` means implied universal.
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attributes: Vec<AttrMatcher>,
    pub pseudo_classes: Vec<PseudoClass>,
}

/// `[name]`, `[name=value]`, `[name^=value]`, etc.
#[derive(Debug, Clone)]
pub struct AttrMatcher {
    pub name: String,
    /// `None` = existence check. `Some((op, value))` = comparison.
    pub op: Option<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum PseudoClass {
    FirstChild,
    LastChild,
    NthChild(i32, i32),     // an+b
    NthLastChild(i32, i32), // an+b
    OnlyChild,
    Not(SelectorList),
    Empty,
    Root,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a comma-separated selector list.
pub fn parse_selector_list(input: &str) -> Result<SelectorList, ParseError> {
    let mut parser = Parser::new(input);
    let mut list = vec![parser.parse_complex_selector()?];
    loop {
        parser.skip_whitespace();
        if parser.peek() == Some(',') {
            parser.advance();
            parser.skip_whitespace();
            list.push(parser.parse_complex_selector()?);
        } else {
            break;
        }
    }
    parser.skip_whitespace();
    if parser.peek().is_some() {
        return Err(ParseError(format!(
            "unexpected character at position {}: {:?}",
            parser.pos,
            parser.peek()
        )));
    }
    Ok(list)
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CSS selector parse error: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn new(input: &str) -> Self {
        Parser {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn skip_whitespace(&mut self) {
        while self.peek().map(|c| c.is_whitespace()) == Some(true) {
            self.advance();
        }
    }

    fn parse_complex_selector(&mut self) -> Result<Selector, ParseError> {
        self.skip_whitespace();
        let mut parts: Vec<(CompoundSelector, Combinator)> = Vec::new();
        let first = self.parse_compound_selector()?;
        parts.push((first, Combinator::None));

        loop {
            let had_whitespace = self.skip_ws_and_report();
            match self.peek() {
                None | Some(',') | Some(')') => break,
                Some('>') => {
                    self.advance();
                    self.skip_whitespace();
                    // Set combinator on the *previous* entry.
                    parts.last_mut().unwrap().1 = Combinator::Child;
                    let next = self.parse_compound_selector()?;
                    parts.push((next, Combinator::None));
                }
                Some('+') => {
                    self.advance();
                    self.skip_whitespace();
                    parts.last_mut().unwrap().1 = Combinator::NextSibling;
                    let next = self.parse_compound_selector()?;
                    parts.push((next, Combinator::None));
                }
                Some('~') => {
                    self.advance();
                    self.skip_whitespace();
                    parts.last_mut().unwrap().1 = Combinator::SubsequentSibling;
                    let next = self.parse_compound_selector()?;
                    parts.push((next, Combinator::None));
                }
                Some(_) if had_whitespace => {
                    // Whitespace = descendant combinator.
                    parts.last_mut().unwrap().1 = Combinator::Descendant;
                    let next = self.parse_compound_selector()?;
                    parts.push((next, Combinator::None));
                }
                Some(ch) => {
                    return Err(ParseError(format!("unexpected char {:?} at {}", ch, self.pos)));
                }
            }
        }

        Ok(Selector(parts))
    }

    fn skip_ws_and_report(&mut self) -> bool {
        let start = self.pos;
        self.skip_whitespace();
        self.pos > start
    }

    fn parse_compound_selector(&mut self) -> Result<CompoundSelector, ParseError> {
        let mut sel = CompoundSelector::default();
        let mut has_any = false;

        // Tag name or universal.
        match self.peek() {
            Some('*') => {
                self.advance();
                sel.tag = Some("*".into());
                has_any = true;
            }
            Some(c) if is_ident_start(c) => {
                let name = self.parse_ident();
                sel.tag = Some(name.to_ascii_lowercase());
                has_any = true;
            }
            _ => {}
        }

        // Additional simple selectors.
        loop {
            match self.peek() {
                Some('#') => {
                    self.advance();
                    let id = self.parse_ident();
                    sel.id = Some(id);
                    has_any = true;
                }
                Some('.') => {
                    self.advance();
                    let class = self.parse_ident();
                    sel.classes.push(class);
                    has_any = true;
                }
                Some('[') => {
                    sel.attributes.push(self.parse_attribute()?);
                    has_any = true;
                }
                Some(':') => {
                    sel.pseudo_classes.push(self.parse_pseudo()?);
                    has_any = true;
                }
                _ => break,
            }
        }

        if !has_any {
            return Err(ParseError(format!(
                "expected selector at position {}",
                self.pos
            )));
        }

        Ok(sel)
    }

    fn parse_ident(&mut self) -> String {
        let mut name = String::new();
        // Allow leading hyphen or underscore
        if self.peek().map(|c| c == '-' || c == '_') == Some(true) {
            name.push(self.advance().unwrap());
        }
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                name.push(c);
                self.advance();
            } else {
                break;
            }
        }
        name
    }

    fn parse_attribute(&mut self) -> Result<AttrMatcher, ParseError> {
        assert_eq!(self.advance(), Some('[')); // consume '['
        self.skip_whitespace();
        let name = self.parse_ident().to_ascii_lowercase();
        self.skip_whitespace();

        let op = match self.peek() {
            Some(']') => {
                self.advance();
                return Ok(AttrMatcher { name, op: None });
            }
            Some('=') => {
                self.advance();
                "=".to_string()
            }
            Some('~') => {
                self.advance();
                self.expect('=')?;
                "~=".to_string()
            }
            Some('|') => {
                self.advance();
                self.expect('=')?;
                "|=".to_string()
            }
            Some('^') => {
                self.advance();
                self.expect('=')?;
                "^=".to_string()
            }
            Some('$') => {
                self.advance();
                self.expect('=')?;
                "$=".to_string()
            }
            Some('*') => {
                self.advance();
                self.expect('=')?;
                "*=".to_string()
            }
            other => {
                return Err(ParseError(format!(
                    "unexpected {:?} in attribute selector at {}",
                    other, self.pos
                )));
            }
        };

        self.skip_whitespace();
        let value = self.parse_attr_value()?;
        self.skip_whitespace();
        self.expect(']')?;

        Ok(AttrMatcher {
            name,
            op: Some((op, value)),
        })
    }

    fn parse_attr_value(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Some('"') => self.parse_string('"'),
            Some('\'') => self.parse_string('\''),
            _ => {
                // Unquoted value.
                let mut val = String::new();
                while let Some(c) = self.peek() {
                    if c == ']' || c.is_whitespace() {
                        break;
                    }
                    val.push(c);
                    self.advance();
                }
                Ok(val)
            }
        }
    }

    fn parse_string(&mut self, quote: char) -> Result<String, ParseError> {
        self.expect(quote)?;
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('\\') => {
                    if let Some(c) = self.advance() {
                        s.push(c);
                    }
                }
                Some(c) if c == quote => return Ok(s),
                Some(c) => s.push(c),
                None => return Err(ParseError("unterminated string".into())),
            }
        }
    }

    fn parse_pseudo(&mut self) -> Result<PseudoClass, ParseError> {
        self.expect(':')?;
        let name = self.parse_ident().to_ascii_lowercase();

        match name.as_str() {
            "first-child" => Ok(PseudoClass::FirstChild),
            "last-child" => Ok(PseudoClass::LastChild),
            "only-child" => Ok(PseudoClass::OnlyChild),
            "empty" => Ok(PseudoClass::Empty),
            "root" => Ok(PseudoClass::Root),
            "nth-child" => {
                self.expect('(')?;
                let (a, b) = self.parse_nth()?;
                self.expect(')')?;
                Ok(PseudoClass::NthChild(a, b))
            }
            "nth-last-child" => {
                self.expect('(')?;
                let (a, b) = self.parse_nth()?;
                self.expect(')')?;
                Ok(PseudoClass::NthLastChild(a, b))
            }
            "not" => {
                self.expect('(')?;
                self.skip_whitespace();
                let inner = self.parse_not_inner()?;
                self.skip_whitespace();
                self.expect(')')?;
                Ok(PseudoClass::Not(inner))
            }
            _ => Err(ParseError(format!("unknown pseudo-class: {}", name))),
        }
    }

    fn parse_not_inner(&mut self) -> Result<SelectorList, ParseError> {
        // :not() can contain a selector list (Level 4) or simple selector (Level 3).
        // We support a full selector list for compatibility.
        let mut list = vec![self.parse_complex_selector()?];
        loop {
            self.skip_whitespace();
            if self.peek() == Some(',') {
                self.advance();
                self.skip_whitespace();
                list.push(self.parse_complex_selector()?);
            } else {
                break;
            }
        }
        Ok(list)
    }

    fn parse_nth(&mut self) -> Result<(i32, i32), ParseError> {
        self.skip_whitespace();

        // Handle keywords: odd, even
        if let Some(c) = self.peek() {
            if c.is_alphabetic() {
                let keyword = self.parse_ident().to_ascii_lowercase();
                self.skip_whitespace();
                return match keyword.as_str() {
                    "odd" => Ok((2, 1)),
                    "even" => Ok((2, 0)),
                    _ => Err(ParseError(format!("unknown nth keyword: {}", keyword))),
                };
            }
        }

        // Parse an+b form.
        let mut a: i32 = 0;
        let mut b: i32;

        // Try reading a number or 'n'.
        let neg = if self.peek() == Some('-') {
            self.advance();
            true
        } else if self.peek() == Some('+') {
            self.advance();
            false
        } else {
            false
        };

        if self.peek() == Some('n') {
            // Just 'n' or '-n'
            a = if neg { -1 } else { 1 };
            self.advance();
            self.skip_whitespace();

            // Optional + or - b
            b = 0;
            if self.peek() == Some('+') {
                self.advance();
                self.skip_whitespace();
                b = self.parse_int()?;
            } else if self.peek() == Some('-') {
                self.advance();
                self.skip_whitespace();
                b = -(self.parse_int()?);
            }
        } else {
            // Number — could be 'a' in 'an+b' or just 'b'.
            let num = self.parse_int()?;
            let num = if neg { -num } else { num };

            if self.peek() == Some('n') {
                a = num;
                self.advance();
                self.skip_whitespace();
                b = 0;
                if self.peek() == Some('+') {
                    self.advance();
                    self.skip_whitespace();
                    b = self.parse_int()?;
                } else if self.peek() == Some('-') {
                    self.advance();
                    self.skip_whitespace();
                    b = -(self.parse_int()?);
                }
            } else {
                b = num;
            }
        }

        self.skip_whitespace();
        Ok((a, b))
    }

    fn parse_int(&mut self) -> Result<i32, ParseError> {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        if s.is_empty() {
            return Err(ParseError(format!("expected integer at {}", self.pos)));
        }
        s.parse()
            .map_err(|_| ParseError(format!("invalid integer: {}", s)))
    }

    fn expect(&mut self, expected: char) -> Result<(), ParseError> {
        match self.advance() {
            Some(c) if c == expected => Ok(()),
            Some(c) => Err(ParseError(format!(
                "expected {:?}, got {:?} at {}",
                expected,
                c,
                self.pos - 1
            ))),
            None => Err(ParseError(format!(
                "expected {:?}, got EOF at {}",
                expected, self.pos
            ))),
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '-'
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(s: &str) -> SelectorList {
        parse_selector_list(s).unwrap_or_else(|e| panic!("parse failed for {:?}: {}", s, e))
    }

    #[test]
    fn parse_tag() {
        let list = parse_ok("div");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0[0].0.tag.as_deref(), Some("div"));
    }

    #[test]
    fn parse_class() {
        let list = parse_ok(".foo");
        assert_eq!(list[0].0[0].0.classes, vec!["foo"]);
    }

    #[test]
    fn parse_id() {
        let list = parse_ok("#bar");
        assert_eq!(list[0].0[0].0.id.as_deref(), Some("bar"));
    }

    #[test]
    fn parse_compound() {
        let list = parse_ok("div.foo#bar");
        let compound = &list[0].0[0].0;
        assert_eq!(compound.tag.as_deref(), Some("div"));
        assert_eq!(compound.classes, vec!["foo"]);
        assert_eq!(compound.id.as_deref(), Some("bar"));
    }

    #[test]
    fn parse_descendant() {
        let list = parse_ok("div p");
        assert_eq!(list[0].0.len(), 2);
        assert_eq!(list[0].0[0].1, Combinator::Descendant);
    }

    #[test]
    fn parse_child() {
        let list = parse_ok("div > p");
        assert_eq!(list[0].0[0].1, Combinator::Child);
    }

    #[test]
    fn parse_adjacent_sibling() {
        let list = parse_ok("h1 + p");
        assert_eq!(list[0].0[0].1, Combinator::NextSibling);
    }

    #[test]
    fn parse_general_sibling() {
        let list = parse_ok("h1 ~ p");
        assert_eq!(list[0].0[0].1, Combinator::SubsequentSibling);
    }

    #[test]
    fn parse_selector_list_commas() {
        let list = parse_ok("h1, h2, h3");
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn parse_attribute_exists() {
        let list = parse_ok("[href]");
        assert_eq!(list[0].0[0].0.attributes[0].name, "href");
        assert!(list[0].0[0].0.attributes[0].op.is_none());
    }

    #[test]
    fn parse_attribute_equals() {
        let list = parse_ok("[type=\"text\"]");
        let attr = &list[0].0[0].0.attributes[0];
        assert_eq!(attr.name, "type");
        let (op, val) = attr.op.as_ref().unwrap();
        assert_eq!(op, "=");
        assert_eq!(val, "text");
    }

    #[test]
    fn parse_attribute_starts_with() {
        let list = parse_ok("[class^=\"intro\"]");
        let attr = &list[0].0[0].0.attributes[0];
        let (op, _) = attr.op.as_ref().unwrap();
        assert_eq!(op, "^=");
    }

    #[test]
    fn parse_nth_child_number() {
        let list = parse_ok(":nth-child(3)");
        match &list[0].0[0].0.pseudo_classes[0] {
            PseudoClass::NthChild(a, b) => {
                assert_eq!(*a, 0);
                assert_eq!(*b, 3);
            }
            _ => panic!("expected NthChild"),
        }
    }

    #[test]
    fn parse_nth_child_odd() {
        let list = parse_ok(":nth-child(odd)");
        match &list[0].0[0].0.pseudo_classes[0] {
            PseudoClass::NthChild(a, b) => {
                assert_eq!(*a, 2);
                assert_eq!(*b, 1);
            }
            _ => panic!("expected NthChild"),
        }
    }

    #[test]
    fn parse_nth_child_expression() {
        let list = parse_ok(":nth-child(2n+1)");
        match &list[0].0[0].0.pseudo_classes[0] {
            PseudoClass::NthChild(a, b) => {
                assert_eq!(*a, 2);
                assert_eq!(*b, 1);
            }
            _ => panic!("expected NthChild"),
        }
    }

    #[test]
    fn parse_not() {
        let list = parse_ok(":not(.hidden)");
        match &list[0].0[0].0.pseudo_classes[0] {
            PseudoClass::Not(inner) => {
                assert_eq!(inner.len(), 1);
                assert_eq!(inner[0].0[0].0.classes, vec!["hidden"]);
            }
            _ => panic!("expected Not"),
        }
    }

    #[test]
    fn parse_complex_real_world() {
        // Real-world selector from a website
        let list = parse_ok("div.container > ul.nav-list > li:first-child > a[href^=\"/\"]");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0.len(), 4);
    }

    #[test]
    fn parse_multiple_classes() {
        let list = parse_ok(".foo.bar.baz");
        let compound = &list[0].0[0].0;
        assert_eq!(compound.classes, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn parse_universal() {
        let list = parse_ok("*");
        assert_eq!(list[0].0[0].0.tag.as_deref(), Some("*"));
    }

    #[test]
    fn error_on_invalid() {
        assert!(parse_selector_list(">>>").is_err());
    }
}
