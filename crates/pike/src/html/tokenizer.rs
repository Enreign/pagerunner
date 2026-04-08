//! HTML5 tokenizer — state machine per WHATWG spec (simplified).
//!
//! Emits tokens consumed by the tree builder. Handles:
//! - Start/end tags with attributes
//! - Text content (character tokens coalesced)
//! - Comments
//! - DOCTYPE
//! - Self-closing tags
//! - Void elements
//! - Unquoted/single-quoted/double-quoted attribute values
//! - Tag soup recovery (missing quotes, missing close tags)

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Doctype {
        name: String,
        force_quirks: bool,
    },
    StartTag {
        name: String,
        attributes: Vec<(String, String)>,
        self_closing: bool,
    },
    EndTag {
        name: String,
    },
    Character(String),
    Comment(String),
    Eof,
}

// ---------------------------------------------------------------------------
// Tokenizer states
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentEndDash,
    CommentEnd,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    RawText,     // for <script>, <style> content
    RawTextEndTagOpen,
    RawTextEndTagName,
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

pub struct Tokenizer {
    input: Vec<char>,
    pos: usize,
    state: State,
    /// Accumulated tokens ready to emit.
    tokens: Vec<Token>,

    // Current tag being built.
    tag_name: String,
    tag_is_end: bool,
    tag_self_closing: bool,
    tag_attrs: Vec<(String, String)>,
    current_attr_name: String,
    current_attr_value: String,

    // Comment / doctype buffer.
    buffer: String,

    // For raw text elements (script, style).
    raw_text_tag: String,
    raw_text_buf: String,
    raw_text_end_tag_buf: String,
}

impl Tokenizer {
    pub fn new(input: &str) -> Self {
        Tokenizer {
            input: input.chars().collect(),
            pos: 0,
            state: State::Data,
            tokens: Vec::new(),
            tag_name: String::new(),
            tag_is_end: false,
            tag_self_closing: false,
            tag_attrs: Vec::new(),
            current_attr_name: String::new(),
            current_attr_value: String::new(),
            buffer: String::new(),
            raw_text_tag: String::new(),
            raw_text_buf: String::new(),
            raw_text_end_tag_buf: String::new(),
        }
    }

    /// Tokenize the entire input and return all tokens.
    pub fn tokenize(mut self) -> Vec<Token> {
        while self.pos <= self.input.len() {
            self.step();
            if self.tokens.last() == Some(&Token::Eof) {
                break;
            }
        }
        self.tokens
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn emit(&mut self, token: Token) {
        self.tokens.push(token);
    }

    fn emit_current_tag(&mut self) {
        // Flush current attribute if any.
        self.flush_attr();

        if self.tag_is_end {
            self.emit(Token::EndTag {
                name: self.tag_name.clone(),
            });
        } else {
            self.emit(Token::StartTag {
                name: self.tag_name.clone(),
                attributes: self.tag_attrs.clone(),
                self_closing: self.tag_self_closing,
            });
        }
        self.tag_name.clear();
        self.tag_is_end = false;
        self.tag_self_closing = false;
        self.tag_attrs.clear();
    }

    fn flush_attr(&mut self) {
        if !self.current_attr_name.is_empty() {
            self.tag_attrs.push((
                self.current_attr_name.drain(..).collect(),
                self.current_attr_value.drain(..).collect(),
            ));
        }
        self.current_attr_name.clear();
        self.current_attr_value.clear();
    }

    fn step(&mut self) {
        match self.state {
            State::Data => self.state_data(),
            State::TagOpen => self.state_tag_open(),
            State::EndTagOpen => self.state_end_tag_open(),
            State::TagName => self.state_tag_name(),
            State::BeforeAttributeName => self.state_before_attribute_name(),
            State::AttributeName => self.state_attribute_name(),
            State::AfterAttributeName => self.state_after_attribute_name(),
            State::BeforeAttributeValue => self.state_before_attribute_value(),
            State::AttributeValueDoubleQuoted => self.state_attribute_value_double_quoted(),
            State::AttributeValueSingleQuoted => self.state_attribute_value_single_quoted(),
            State::AttributeValueUnquoted => self.state_attribute_value_unquoted(),
            State::AfterAttributeValueQuoted => self.state_after_attribute_value_quoted(),
            State::SelfClosingStartTag => self.state_self_closing_start_tag(),
            State::BogusComment => self.state_bogus_comment(),
            State::MarkupDeclarationOpen => self.state_markup_declaration_open(),
            State::CommentStart => self.state_comment_start(),
            State::CommentStartDash => self.state_comment_start_dash(),
            State::Comment => self.state_comment(),
            State::CommentEndDash => self.state_comment_end_dash(),
            State::CommentEnd => self.state_comment_end(),
            State::Doctype => self.state_doctype(),
            State::BeforeDoctypeName => self.state_before_doctype_name(),
            State::DoctypeName => self.state_doctype_name(),
            State::AfterDoctypeName => self.state_after_doctype_name(),
            State::RawText => self.state_raw_text(),
            State::RawTextEndTagOpen => self.state_raw_text_end_tag_open(),
            State::RawTextEndTagName => self.state_raw_text_end_tag_name(),
        }
    }

    // -- State handlers -----------------------------------------------------

    fn state_data(&mut self) {
        match self.advance() {
            Some('<') => {
                self.state = State::TagOpen;
            }
            Some('&') => {
                // Simplified: just emit the ampersand for now.
                // Full entity decoding would go here.
                let decoded = self.consume_char_reference();
                self.emit(Token::Character(decoded));
            }
            Some(c) => {
                // Coalesce consecutive characters.
                let mut text = String::new();
                text.push(c);
                while let Some(&next) = self.input.get(self.pos) {
                    if next == '<' || next == '&' {
                        break;
                    }
                    text.push(next);
                    self.pos += 1;
                }
                self.emit(Token::Character(text));
            }
            None => {
                self.emit(Token::Eof);
            }
        }
    }

    fn state_tag_open(&mut self) {
        match self.peek() {
            Some('!') => {
                self.advance();
                self.state = State::MarkupDeclarationOpen;
            }
            Some('/') => {
                self.advance();
                self.state = State::EndTagOpen;
            }
            Some('?') => {
                // Processing instruction — treat as bogus comment.
                self.advance();
                self.buffer.clear();
                self.state = State::BogusComment;
            }
            Some(c) if c.is_ascii_alphabetic() => {
                self.tag_name.clear();
                self.tag_is_end = false;
                self.tag_self_closing = false;
                self.tag_attrs.clear();
                self.state = State::TagName;
            }
            _ => {
                // Not a tag — emit '<' as character.
                self.emit(Token::Character("<".into()));
                self.state = State::Data;
            }
        }
    }

    fn state_end_tag_open(&mut self) {
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => {
                self.tag_name.clear();
                self.tag_is_end = true;
                self.tag_self_closing = false;
                self.tag_attrs.clear();
                self.state = State::TagName;
            }
            Some('>') => {
                // </> — ignore.
                self.advance();
                self.state = State::Data;
            }
            _ => {
                // Bogus: emit '</' and reconsume.
                self.emit(Token::Character("</".into()));
                self.state = State::Data;
            }
        }
    }

    fn state_tag_name(&mut self) {
        match self.advance() {
            Some(c) if c.is_whitespace() => {
                self.state = State::BeforeAttributeName;
            }
            Some('/') => {
                self.state = State::SelfClosingStartTag;
            }
            Some('>') => {
                self.tag_name = self.tag_name.to_ascii_lowercase();
                let tag = self.tag_name.clone();
                self.emit_current_tag();
                // Switch to raw text mode for script/style.
                if !self.tag_is_end && is_raw_text_element(&tag) {
                    self.raw_text_tag = tag;
                    self.raw_text_buf.clear();
                    self.state = State::RawText;
                } else {
                    self.state = State::Data;
                }
            }
            Some(c) => {
                self.tag_name.push(c.to_ascii_lowercase());
            }
            None => {
                self.emit(Token::Eof);
            }
        }
    }

    fn state_before_attribute_name(&mut self) {
        match self.peek() {
            Some(c) if c.is_whitespace() => {
                self.advance();
            }
            Some('/') | Some('>') | None => {
                self.state = State::AfterAttributeName;
            }
            Some('=') => {
                // Attribute name starting with '=' — unusual but handle it.
                self.current_attr_name.clear();
                self.current_attr_value.clear();
                self.current_attr_name.push('=');
                self.advance();
                self.state = State::AttributeName;
            }
            _ => {
                self.flush_attr();
                self.current_attr_name.clear();
                self.current_attr_value.clear();
                self.state = State::AttributeName;
            }
        }
    }

    fn state_attribute_name(&mut self) {
        match self.peek() {
            Some(c) if c.is_whitespace() => {
                self.advance();
                self.current_attr_name = self.current_attr_name.to_ascii_lowercase();
                self.state = State::AfterAttributeName;
            }
            Some('/') | Some('>') | None => {
                self.current_attr_name = self.current_attr_name.to_ascii_lowercase();
                self.state = State::AfterAttributeName;
            }
            Some('=') => {
                self.advance();
                self.current_attr_name = self.current_attr_name.to_ascii_lowercase();
                self.state = State::BeforeAttributeValue;
            }
            Some(c) => {
                self.advance();
                self.current_attr_name.push(c);
            }
        }
    }

    fn state_after_attribute_name(&mut self) {
        match self.peek() {
            Some(c) if c.is_whitespace() => {
                self.advance();
            }
            Some('/') => {
                self.advance();
                self.state = State::SelfClosingStartTag;
            }
            Some('=') => {
                self.advance();
                self.state = State::BeforeAttributeValue;
            }
            Some('>') => {
                self.advance();
                self.tag_name = self.tag_name.to_ascii_lowercase();
                let tag = self.tag_name.clone();
                self.emit_current_tag();
                if is_raw_text_element(&tag) {
                    self.raw_text_tag = tag;
                    self.raw_text_buf.clear();
                    self.state = State::RawText;
                } else {
                    self.state = State::Data;
                }
            }
            None => {
                self.emit(Token::Eof);
            }
            _ => {
                self.flush_attr();
                self.current_attr_name.clear();
                self.current_attr_value.clear();
                self.state = State::AttributeName;
            }
        }
    }

    fn state_before_attribute_value(&mut self) {
        match self.peek() {
            Some(c) if c.is_whitespace() => {
                self.advance();
            }
            Some('"') => {
                self.advance();
                self.state = State::AttributeValueDoubleQuoted;
            }
            Some('\'') => {
                self.advance();
                self.state = State::AttributeValueSingleQuoted;
            }
            Some('>') => {
                // Missing value — emit tag.
                self.advance();
                self.tag_name = self.tag_name.to_ascii_lowercase();
                self.emit_current_tag();
                self.state = State::Data;
            }
            _ => {
                self.state = State::AttributeValueUnquoted;
            }
        }
    }

    fn state_attribute_value_double_quoted(&mut self) {
        match self.advance() {
            Some('"') => {
                self.state = State::AfterAttributeValueQuoted;
            }
            Some('&') => {
                let decoded = self.consume_char_reference();
                self.current_attr_value.push_str(&decoded);
            }
            Some(c) => {
                self.current_attr_value.push(c);
            }
            None => {
                self.emit(Token::Eof);
            }
        }
    }

    fn state_attribute_value_single_quoted(&mut self) {
        match self.advance() {
            Some('\'') => {
                self.state = State::AfterAttributeValueQuoted;
            }
            Some('&') => {
                let decoded = self.consume_char_reference();
                self.current_attr_value.push_str(&decoded);
            }
            Some(c) => {
                self.current_attr_value.push(c);
            }
            None => {
                self.emit(Token::Eof);
            }
        }
    }

    fn state_attribute_value_unquoted(&mut self) {
        match self.peek() {
            Some(c) if c.is_whitespace() => {
                self.advance();
                self.state = State::BeforeAttributeName;
            }
            Some('&') => {
                self.advance();
                let decoded = self.consume_char_reference();
                self.current_attr_value.push_str(&decoded);
            }
            Some('>') => {
                self.advance();
                self.tag_name = self.tag_name.to_ascii_lowercase();
                let tag = self.tag_name.clone();
                self.emit_current_tag();
                if is_raw_text_element(&tag) {
                    self.raw_text_tag = tag;
                    self.raw_text_buf.clear();
                    self.state = State::RawText;
                } else {
                    self.state = State::Data;
                }
            }
            Some(c) => {
                self.advance();
                self.current_attr_value.push(c);
            }
            None => {
                self.emit(Token::Eof);
            }
        }
    }

    fn state_after_attribute_value_quoted(&mut self) {
        match self.peek() {
            Some(c) if c.is_whitespace() => {
                self.advance();
                self.state = State::BeforeAttributeName;
            }
            Some('/') => {
                self.advance();
                self.state = State::SelfClosingStartTag;
            }
            Some('>') => {
                self.advance();
                self.tag_name = self.tag_name.to_ascii_lowercase();
                let tag = self.tag_name.clone();
                self.emit_current_tag();
                if is_raw_text_element(&tag) {
                    self.raw_text_tag = tag;
                    self.raw_text_buf.clear();
                    self.state = State::RawText;
                } else {
                    self.state = State::Data;
                }
            }
            _ => {
                // Missing whitespace — reconsume in before attribute name.
                self.state = State::BeforeAttributeName;
            }
        }
    }

    fn state_self_closing_start_tag(&mut self) {
        match self.peek() {
            Some('>') => {
                self.advance();
                self.tag_self_closing = true;
                self.tag_name = self.tag_name.to_ascii_lowercase();
                self.emit_current_tag();
                self.state = State::Data;
            }
            _ => {
                // Treat '/' as part of a weird attribute.
                self.state = State::BeforeAttributeName;
            }
        }
    }

    fn state_markup_declaration_open(&mut self) {
        // After '<!'
        if self.starts_with("--") {
            self.pos += 2;
            self.buffer.clear();
            self.state = State::CommentStart;
        } else if self.starts_with_ci("doctype") {
            self.pos += 7;
            self.state = State::Doctype;
        } else if self.starts_with("[CDATA[") {
            // CDATA — treat as bogus comment for HTML.
            self.pos += 7;
            self.buffer.clear();
            self.state = State::BogusComment;
        } else {
            self.buffer.clear();
            self.state = State::BogusComment;
        }
    }

    fn state_comment_start(&mut self) {
        match self.peek() {
            Some('-') => {
                self.advance();
                self.state = State::CommentStartDash;
            }
            Some('>') => {
                self.advance();
                self.emit(Token::Comment(self.buffer.clone()));
                self.state = State::Data;
            }
            _ => {
                self.state = State::Comment;
            }
        }
    }

    fn state_comment_start_dash(&mut self) {
        match self.peek() {
            Some('-') => {
                self.advance();
                self.state = State::CommentEnd;
            }
            Some('>') => {
                self.advance();
                self.emit(Token::Comment(self.buffer.clone()));
                self.state = State::Data;
            }
            _ => {
                self.buffer.push('-');
                self.state = State::Comment;
            }
        }
    }

    fn state_comment(&mut self) {
        match self.advance() {
            Some('-') => {
                self.state = State::CommentEndDash;
            }
            Some(c) => {
                self.buffer.push(c);
            }
            None => {
                self.emit(Token::Comment(self.buffer.clone()));
                self.emit(Token::Eof);
            }
        }
    }

    fn state_comment_end_dash(&mut self) {
        match self.peek() {
            Some('-') => {
                self.advance();
                self.state = State::CommentEnd;
            }
            _ => {
                self.buffer.push('-');
                self.state = State::Comment;
            }
        }
    }

    fn state_comment_end(&mut self) {
        match self.peek() {
            Some('>') => {
                self.advance();
                self.emit(Token::Comment(self.buffer.clone()));
                self.buffer.clear();
                self.state = State::Data;
            }
            Some('-') => {
                self.advance();
                self.buffer.push('-');
            }
            _ => {
                self.buffer.push_str("--");
                self.state = State::Comment;
            }
        }
    }

    fn state_bogus_comment(&mut self) {
        match self.advance() {
            Some('>') => {
                self.emit(Token::Comment(self.buffer.clone()));
                self.buffer.clear();
                self.state = State::Data;
            }
            Some(c) => {
                self.buffer.push(c);
            }
            None => {
                self.emit(Token::Comment(self.buffer.clone()));
                self.emit(Token::Eof);
            }
        }
    }

    fn state_doctype(&mut self) {
        match self.peek() {
            Some(c) if c.is_whitespace() => {
                self.advance();
                self.state = State::BeforeDoctypeName;
            }
            Some('>') => {
                self.state = State::BeforeDoctypeName;
            }
            _ => {
                self.state = State::BeforeDoctypeName;
            }
        }
    }

    fn state_before_doctype_name(&mut self) {
        match self.peek() {
            Some(c) if c.is_whitespace() => {
                self.advance();
            }
            Some('>') => {
                self.advance();
                self.emit(Token::Doctype {
                    name: String::new(),
                    force_quirks: true,
                });
                self.state = State::Data;
            }
            Some(_) => {
                self.buffer.clear();
                self.state = State::DoctypeName;
            }
            None => {
                self.emit(Token::Doctype {
                    name: String::new(),
                    force_quirks: true,
                });
                self.emit(Token::Eof);
            }
        }
    }

    fn state_doctype_name(&mut self) {
        match self.advance() {
            Some(c) if c.is_whitespace() => {
                self.state = State::AfterDoctypeName;
            }
            Some('>') => {
                self.emit(Token::Doctype {
                    name: self.buffer.to_ascii_lowercase(),
                    force_quirks: false,
                });
                self.buffer.clear();
                self.state = State::Data;
            }
            Some(c) => {
                self.buffer.push(c);
            }
            None => {
                self.emit(Token::Doctype {
                    name: self.buffer.to_ascii_lowercase(),
                    force_quirks: true,
                });
                self.emit(Token::Eof);
            }
        }
    }

    fn state_after_doctype_name(&mut self) {
        // Skip to '>' (simplified — ignores PUBLIC/SYSTEM identifiers).
        match self.advance() {
            Some('>') => {
                self.emit(Token::Doctype {
                    name: self.buffer.to_ascii_lowercase(),
                    force_quirks: false,
                });
                self.buffer.clear();
                self.state = State::Data;
            }
            Some(_) => { /* skip */ }
            None => {
                self.emit(Token::Doctype {
                    name: self.buffer.to_ascii_lowercase(),
                    force_quirks: true,
                });
                self.emit(Token::Eof);
            }
        }
    }

    // -- Raw text states (for <script>, <style>) ----------------------------

    fn state_raw_text(&mut self) {
        match self.advance() {
            Some('<') => {
                self.state = State::RawTextEndTagOpen;
            }
            Some(c) => {
                self.raw_text_buf.push(c);
            }
            None => {
                if !self.raw_text_buf.is_empty() {
                    let text = self.raw_text_buf.clone();
                    self.emit(Token::Character(text));
                }
                self.emit(Token::Eof);
            }
        }
    }

    fn state_raw_text_end_tag_open(&mut self) {
        match self.peek() {
            Some('/') => {
                self.advance();
                self.raw_text_end_tag_buf.clear();
                self.state = State::RawTextEndTagName;
            }
            _ => {
                self.raw_text_buf.push('<');
                self.state = State::RawText;
            }
        }
    }

    fn state_raw_text_end_tag_name(&mut self) {
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => {
                self.advance();
                self.raw_text_end_tag_buf.push(c.to_ascii_lowercase());
            }
            Some('>') if self.raw_text_end_tag_buf == self.raw_text_tag => {
                self.advance();
                // Emit accumulated raw text.
                if !self.raw_text_buf.is_empty() {
                    let text = self.raw_text_buf.clone();
                    self.emit(Token::Character(text));
                    self.raw_text_buf.clear();
                }
                // Emit the end tag.
                self.emit(Token::EndTag {
                    name: self.raw_text_tag.clone(),
                });
                self.raw_text_tag.clear();
                self.state = State::Data;
            }
            Some(c) if c.is_whitespace() && self.raw_text_end_tag_buf == self.raw_text_tag => {
                self.advance();
                // End of raw text — emit text and end tag.
                if !self.raw_text_buf.is_empty() {
                    let text = self.raw_text_buf.clone();
                    self.emit(Token::Character(text));
                    self.raw_text_buf.clear();
                }
                self.tag_name = self.raw_text_tag.clone();
                self.tag_is_end = true;
                self.tag_self_closing = false;
                self.tag_attrs.clear();
                self.state = State::BeforeAttributeName;
            }
            _ => {
                // Not a matching end tag — put it all back as text.
                self.raw_text_buf.push('<');
                self.raw_text_buf.push('/');
                self.raw_text_buf.push_str(&self.raw_text_end_tag_buf);
                self.raw_text_end_tag_buf.clear();
                self.state = State::RawText;
            }
        }
    }

    // -- Helpers ------------------------------------------------------------

    fn starts_with(&self, s: &str) -> bool {
        let remaining: String = self.input[self.pos..].iter().collect();
        remaining.starts_with(s)
    }

    fn starts_with_ci(&self, s: &str) -> bool {
        let remaining: String = self.input[self.pos..].iter().collect();
        remaining
            .get(..s.len())
            .map(|r| r.eq_ignore_ascii_case(s))
            .unwrap_or(false)
    }

    /// Simplified character reference consumer.
    /// Handles: &amp; &lt; &gt; &quot; &apos; &#NNN; &#xHHH;
    fn consume_char_reference(&mut self) -> String {
        let start = self.pos;

        if self.peek() == Some('#') {
            self.advance();
            let hex = self.peek() == Some('x') || self.peek() == Some('X');
            if hex {
                self.advance();
            }
            let mut digits = String::new();
            while let Some(c) = self.peek() {
                if c == ';' {
                    self.advance();
                    break;
                }
                if hex && c.is_ascii_hexdigit() || !hex && c.is_ascii_digit() {
                    digits.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
            let base = if hex { 16 } else { 10 };
            if let Ok(cp) = u32::from_str_radix(&digits, base) {
                if let Some(ch) = char::from_u32(cp) {
                    return ch.to_string();
                }
            }
            // Failed to decode — return raw.
            self.pos = start;
            return "&".into();
        }

        // Named references (common subset).
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c == ';' {
                self.advance();
                break;
            }
            if c.is_ascii_alphabetic() {
                name.push(c);
                self.advance();
            } else {
                break;
            }
        }

        match name.as_str() {
            "amp" => "&".into(),
            "lt" => "<".into(),
            "gt" => ">".into(),
            "quot" => "\"".into(),
            "apos" => "'".into(),
            "nbsp" => "\u{00A0}".into(),
            "copy" => "\u{00A9}".into(),
            "reg" => "\u{00AE}".into(),
            "trade" => "\u{2122}".into(),
            "mdash" => "\u{2014}".into(),
            "ndash" => "\u{2013}".into(),
            "laquo" => "\u{00AB}".into(),
            "raquo" => "\u{00BB}".into(),
            "hellip" => "\u{2026}".into(),
            "bull" => "\u{2022}".into(),
            "rarr" => "\u{2192}".into(),
            "larr" => "\u{2190}".into(),
            _ => {
                // Unknown entity — return raw text.
                self.pos = start;
                "&".into()
            }
        }
    }
}

fn is_raw_text_element(tag: &str) -> bool {
    matches!(tag, "script" | "style" | "textarea" | "title")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(input: &str) -> Vec<Token> {
        Tokenizer::new(input).tokenize()
    }

    #[test]
    fn simple_text() {
        let tokens = tokenize("hello world");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], Token::Character("hello world".into()));
        assert_eq!(tokens[1], Token::Eof);
    }

    #[test]
    fn simple_element() {
        let tokens = tokenize("<div>hello</div>");
        assert!(matches!(&tokens[0], Token::StartTag { name, .. } if name == "div"));
        assert_eq!(tokens[1], Token::Character("hello".into()));
        assert!(matches!(&tokens[2], Token::EndTag { name } if name == "div"));
    }

    #[test]
    fn self_closing_tag() {
        let tokens = tokenize("<br/>");
        assert!(matches!(
            &tokens[0],
            Token::StartTag {
                name,
                self_closing: true,
                ..
            } if name == "br"
        ));
    }

    #[test]
    fn attributes() {
        let tokens = tokenize("<a href=\"/foo\" class='bar' disabled>");
        match &tokens[0] {
            Token::StartTag { attributes, .. } => {
                assert_eq!(attributes.len(), 3);
                assert_eq!(attributes[0], ("href".into(), "/foo".into()));
                assert_eq!(attributes[1], ("class".into(), "bar".into()));
                assert_eq!(attributes[2], ("disabled".into(), "".into()));
            }
            _ => panic!("expected StartTag"),
        }
    }

    #[test]
    fn unquoted_attribute() {
        let tokens = tokenize("<input type=text>");
        match &tokens[0] {
            Token::StartTag { attributes, .. } => {
                assert_eq!(attributes[0], ("type".into(), "text".into()));
            }
            _ => panic!("expected StartTag"),
        }
    }

    #[test]
    fn comment() {
        let tokens = tokenize("<!-- hello -->");
        assert_eq!(tokens[0], Token::Comment(" hello ".into()));
    }

    #[test]
    fn doctype() {
        let tokens = tokenize("<!DOCTYPE html>");
        assert!(matches!(
            &tokens[0],
            Token::Doctype {
                name,
                force_quirks: false
            } if name == "html"
        ));
    }

    #[test]
    fn case_insensitive_tags() {
        let tokens = tokenize("<DIV>text</DIV>");
        assert!(matches!(&tokens[0], Token::StartTag { name, .. } if name == "div"));
        assert!(matches!(&tokens[2], Token::EndTag { name } if name == "div"));
    }

    #[test]
    fn entity_decoding() {
        let tokens = tokenize("&amp; &lt; &gt; &#65; &#x41;");
        // Should decode to "& < > A A"
        let text: String = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Character(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert!(text.contains('&'));
        assert!(text.contains('<'));
        assert!(text.contains('>'));
        assert!(text.contains('A'));
    }

    #[test]
    fn script_raw_text() {
        let tokens = tokenize("<script>var x = '<div>';</script>");
        assert!(matches!(&tokens[0], Token::StartTag { name, .. } if name == "script"));
        assert_eq!(tokens[1], Token::Character("var x = '<div>';".into()));
        assert!(matches!(&tokens[2], Token::EndTag { name } if name == "script"));
    }

    #[test]
    fn style_raw_text() {
        let tokens = tokenize("<style>.foo { color: red; }</style>");
        assert!(matches!(&tokens[0], Token::StartTag { name, .. } if name == "style"));
        assert_eq!(
            tokens[1],
            Token::Character(".foo { color: red; }".into())
        );
    }

    #[test]
    fn nested_elements() {
        let tokens = tokenize("<div><p>hello</p><p>world</p></div>");
        let tag_names: Vec<&str> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::StartTag { name, .. } => Some(name.as_str()),
                Token::EndTag { name } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(tag_names, vec!["div", "p", "p", "p", "p", "div"]);
    }

    #[test]
    fn multiple_attributes() {
        let tokens = tokenize("<div id=\"main\" class=\"container\" data-role=\"page\">");
        match &tokens[0] {
            Token::StartTag {
                name, attributes, ..
            } => {
                assert_eq!(name, "div");
                assert_eq!(attributes.len(), 3);
                assert_eq!(attributes[0], ("id".into(), "main".into()));
                assert_eq!(attributes[1], ("class".into(), "container".into()));
                assert_eq!(attributes[2], ("data-role".into(), "page".into()));
            }
            _ => panic!("expected StartTag"),
        }
    }

    #[test]
    fn tag_soup_recovery() {
        // Unclosed tags, missing quotes — should not panic.
        let tokens = tokenize("<div><p>unclosed<span>also unclosed");
        assert!(!tokens.is_empty());
        assert!(tokens.last() == Some(&Token::Eof));
    }

    #[test]
    fn empty_input() {
        let tokens = tokenize("");
        assert_eq!(tokens, vec![Token::Eof]);
    }

    #[test]
    fn real_world_html_fragment() {
        let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Test</title>
</head>
<body>
    <div id="app" class="container">
        <h1>Hello World</h1>
        <p>This is a <strong>test</strong> page.</p>
        <img src="test.png" alt="Test image" />
        <a href="https://example.com">Link</a>
    </div>
    <script>console.log('hello');</script>
</body>
</html>"#;
        let tokens = tokenize(html);
        // Should parse without errors and end with Eof.
        assert!(tokens.last() == Some(&Token::Eof));
        // Count start tags.
        let start_tags: Vec<&str> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::StartTag { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(start_tags.contains(&"html"));
        assert!(start_tags.contains(&"head"));
        assert!(start_tags.contains(&"body"));
        assert!(start_tags.contains(&"div"));
        assert!(start_tags.contains(&"h1"));
        assert!(start_tags.contains(&"p"));
        assert!(start_tags.contains(&"strong"));
        assert!(start_tags.contains(&"img"));
        assert!(start_tags.contains(&"a"));
        assert!(start_tags.contains(&"script"));
    }
}
