//! HTML tree builder — converts a token stream into a DOM tree.
//!
//! Implements a simplified version of the WHATWG tree construction algorithm:
//! - Implicit element creation (html, head, body)
//! - Auto-closing for void elements
//! - Scope-based tag closing (p closes p, li closes li, etc.)
//! - Foster parenting fallback
//! - Script/style content preserved as text nodes

use crate::dom::{is_void_element, Attribute, Dom, NodeId, NodeKind};
use crate::html::tokenizer::{Token, Tokenizer};

/// Parse an HTML string into a DOM tree.
pub fn parse(html: &str) -> Dom {
    let tokens = Tokenizer::new(html).tokenize();
    let mut builder = TreeBuilder::new();
    for token in tokens {
        builder.process(token);
    }
    builder.dom
}

struct TreeBuilder {
    dom: Dom,
    /// Stack of open element node ids.
    open_elements: Vec<NodeId>,
    /// Whether we've seen <head> yet.
    head_inserted: bool,
    /// Whether we've seen <body> yet.
    body_inserted: bool,
    /// The current insertion point.
    insertion_mode: InsertionMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum InsertionMode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
    AfterBody,
    AfterAfterBody,
    Text, // for script/style raw content
}

impl TreeBuilder {
    fn new() -> Self {
        TreeBuilder {
            dom: Dom::new(),
            open_elements: Vec::new(),
            head_inserted: false,
            body_inserted: false,
            insertion_mode: InsertionMode::Initial,
        }
    }

    fn current_node(&self) -> NodeId {
        self.open_elements
            .last()
            .copied()
            .unwrap_or(NodeId::DOCUMENT)
    }

    fn current_tag(&self) -> Option<&str> {
        self.dom.tag_name(self.current_node())
    }

    fn process(&mut self, token: Token) {
        match self.insertion_mode {
            InsertionMode::Initial => self.process_initial(token),
            InsertionMode::BeforeHtml => self.process_before_html(token),
            InsertionMode::BeforeHead => self.process_before_head(token),
            InsertionMode::InHead => self.process_in_head(token),
            InsertionMode::AfterHead => self.process_after_head(token),
            InsertionMode::InBody => self.process_in_body(token),
            InsertionMode::AfterBody => self.process_after_body(token),
            InsertionMode::AfterAfterBody => self.process_after_after_body(token),
            InsertionMode::Text => self.process_text(token),
        }
    }

    // -- Initial mode -------------------------------------------------------

    fn process_initial(&mut self, token: Token) {
        match token {
            Token::Doctype { name, .. } => {
                let doctype = self.dom.create_node(NodeKind::Doctype { name });
                self.dom.append_child(NodeId::DOCUMENT, doctype);
                self.insertion_mode = InsertionMode::BeforeHtml;
            }
            Token::Comment(text) => {
                let comment = self.dom.create_comment(&text);
                self.dom.append_child(NodeId::DOCUMENT, comment);
            }
            Token::Character(ref s) if s.trim().is_empty() => {
                // Ignore whitespace before doctype.
            }
            _ => {
                // No doctype — switch to BeforeHtml and reprocess.
                self.insertion_mode = InsertionMode::BeforeHtml;
                self.process(token);
            }
        }
    }

    // -- BeforeHtml mode ----------------------------------------------------

    fn process_before_html(&mut self, token: Token) {
        match token {
            Token::StartTag { ref name, .. } if name == "html" => {
                let html = self.create_element_from_token(&token);
                self.dom.append_child(NodeId::DOCUMENT, html);
                self.open_elements.push(html);
                self.insertion_mode = InsertionMode::BeforeHead;
            }
            Token::Comment(text) => {
                let comment = self.dom.create_comment(&text);
                self.dom.append_child(NodeId::DOCUMENT, comment);
            }
            Token::Character(ref s) if s.trim().is_empty() => {}
            Token::Eof => {}
            _ => {
                // Implied <html>.
                let html = self.dom.create_element("html", vec![]);
                self.dom.append_child(NodeId::DOCUMENT, html);
                self.open_elements.push(html);
                self.insertion_mode = InsertionMode::BeforeHead;
                self.process(token);
            }
        }
    }

    // -- BeforeHead mode ----------------------------------------------------

    fn process_before_head(&mut self, token: Token) {
        match token {
            Token::StartTag { ref name, .. } if name == "head" => {
                let head = self.create_element_from_token(&token);
                self.dom.append_child(self.current_node(), head);
                self.open_elements.push(head);
                self.head_inserted = true;
                self.insertion_mode = InsertionMode::InHead;
            }
            Token::Character(ref s) if s.trim().is_empty() => {}
            Token::Comment(text) => {
                let comment = self.dom.create_comment(&text);
                self.dom.append_child(self.current_node(), comment);
            }
            Token::Eof => {}
            _ => {
                // Implied <head>.
                let head = self.dom.create_element("head", vec![]);
                self.dom.append_child(self.current_node(), head);
                self.open_elements.push(head);
                self.head_inserted = true;
                self.insertion_mode = InsertionMode::InHead;
                self.process(token);
            }
        }
    }

    // -- InHead mode --------------------------------------------------------

    fn process_in_head(&mut self, token: Token) {
        match token {
            Token::Character(ref s) if s.trim().is_empty() => {
                let text = self.dom.create_text(s);
                self.dom.append_child(self.current_node(), text);
            }
            Token::Comment(text) => {
                let comment = self.dom.create_comment(&text);
                self.dom.append_child(self.current_node(), comment);
            }
            Token::StartTag { ref name, .. }
                if matches!(
                    name.as_str(),
                    "base" | "basefont" | "bgsound" | "link" | "meta"
                ) =>
            {
                let el = self.create_element_from_token(&token);
                self.dom.append_child(self.current_node(), el);
                // Void — don't push to stack.
            }
            Token::StartTag { ref name, .. } if name == "title" => {
                let el = self.create_element_from_token(&token);
                self.dom.append_child(self.current_node(), el);
                self.open_elements.push(el);
                self.insertion_mode = InsertionMode::Text;
            }
            Token::StartTag { ref name, .. } if name == "style" || name == "script" => {
                let el = self.create_element_from_token(&token);
                self.dom.append_child(self.current_node(), el);
                self.open_elements.push(el);
                self.insertion_mode = InsertionMode::Text;
            }
            Token::EndTag { ref name } if name == "head" => {
                self.open_elements.pop();
                self.insertion_mode = InsertionMode::AfterHead;
            }
            Token::StartTag { ref name, .. } if name == "head" => {
                // Ignore duplicate <head>.
            }
            _ => {
                // Implied </head>.
                self.open_elements.pop();
                self.insertion_mode = InsertionMode::AfterHead;
                self.process(token);
            }
        }
    }

    // -- Text mode (for title, script, style content) -----------------------

    fn process_text(&mut self, token: Token) {
        match token {
            Token::Character(s) => {
                let text = self.dom.create_text(&s);
                self.dom.append_child(self.current_node(), text);
            }
            Token::EndTag { .. } => {
                self.open_elements.pop();
                // Return to previous mode.
                if self.body_inserted {
                    self.insertion_mode = InsertionMode::InBody;
                } else if self.head_inserted {
                    self.insertion_mode = InsertionMode::InHead;
                } else {
                    self.insertion_mode = InsertionMode::InBody;
                }
            }
            Token::Eof => {
                self.open_elements.pop();
            }
            _ => {}
        }
    }

    // -- AfterHead mode -----------------------------------------------------

    fn process_after_head(&mut self, token: Token) {
        match token {
            Token::Character(ref s) if s.trim().is_empty() => {
                let text = self.dom.create_text(s);
                self.dom.append_child(self.current_node(), text);
            }
            Token::Comment(text) => {
                let comment = self.dom.create_comment(&text);
                self.dom.append_child(self.current_node(), comment);
            }
            Token::StartTag { ref name, .. } if name == "body" => {
                let body = self.create_element_from_token(&token);
                self.dom.append_child(self.current_node(), body);
                self.open_elements.push(body);
                self.body_inserted = true;
                self.insertion_mode = InsertionMode::InBody;
            }
            Token::Eof => {}
            _ => {
                // Implied <body>.
                let body = self.dom.create_element("body", vec![]);
                self.dom.append_child(self.current_node(), body);
                self.open_elements.push(body);
                self.body_inserted = true;
                self.insertion_mode = InsertionMode::InBody;
                self.process(token);
            }
        }
    }

    // -- InBody mode --------------------------------------------------------

    fn process_in_body(&mut self, token: Token) {
        match token {
            Token::Character(s) => {
                // Merge with existing text node if the last child is text.
                let current = self.current_node();
                let last_child = self.dom.node(current).children.last().copied();
                if let Some(last_id) = last_child {
                    if let NodeKind::Text(ref existing) = self.dom.node(last_id).kind {
                        let merged = format!("{}{}", existing, s);
                        self.dom.node_mut(last_id).kind = NodeKind::Text(merged);
                        return;
                    }
                }
                let text = self.dom.create_text(&s);
                self.dom.append_child(current, text);
            }
            Token::Comment(text) => {
                let comment = self.dom.create_comment(&text);
                self.dom.append_child(self.current_node(), comment);
            }
            Token::StartTag {
                ref name,
                ref attributes,
                self_closing,
            } => {
                let tag = name.clone();
                let attrs = attributes.clone();

                // Auto-close certain elements before opening new ones.
                self.auto_close_before_open(&tag);

                if is_void_element(&tag) || self_closing {
                    let el = self.dom.create_element(
                        &tag,
                        attrs
                            .into_iter()
                            .map(|(n, v)| Attribute { name: n, value: v })
                            .collect(),
                    );
                    self.dom.append_child(self.current_node(), el);
                } else if tag == "script" || tag == "style" {
                    let el = self.dom.create_element(
                        &tag,
                        attrs
                            .into_iter()
                            .map(|(n, v)| Attribute { name: n, value: v })
                            .collect(),
                    );
                    self.dom.append_child(self.current_node(), el);
                    self.open_elements.push(el);
                    self.insertion_mode = InsertionMode::Text;
                } else {
                    let el = self.dom.create_element(
                        &tag,
                        attrs
                            .into_iter()
                            .map(|(n, v)| Attribute { name: n, value: v })
                            .collect(),
                    );
                    self.dom.append_child(self.current_node(), el);
                    self.open_elements.push(el);
                }
            }
            Token::EndTag { name } => {
                if name == "body" {
                    self.insertion_mode = InsertionMode::AfterBody;
                    // Pop everything up to and including body.
                    while let Some(id) = self.open_elements.last() {
                        if self.dom.tag_name(*id) == Some("body") {
                            self.open_elements.pop();
                            break;
                        }
                        self.open_elements.pop();
                    }
                } else if name == "html" {
                    self.insertion_mode = InsertionMode::AfterBody;
                    // Pop to html.
                    while let Some(id) = self.open_elements.last() {
                        let is_html = self.dom.tag_name(*id) == Some("html");
                        self.open_elements.pop();
                        if is_html {
                            break;
                        }
                    }
                } else {
                    // Pop elements until we find a matching open tag.
                    self.close_tag(&name);
                }
            }
            Token::Eof => {
                // Close all open elements.
                self.open_elements.clear();
            }
            Token::Doctype { .. } => {
                // Ignore doctype in body.
            }
        }
    }

    // -- AfterBody / AfterAfterBody -----------------------------------------

    fn process_after_body(&mut self, token: Token) {
        match token {
            Token::Character(ref s) if s.trim().is_empty() => {}
            Token::Comment(text) => {
                let comment = self.dom.create_comment(&text);
                // Append to html element.
                if let Some(html) = self.dom.html_element() {
                    self.dom.append_child(html, comment);
                }
            }
            Token::EndTag { ref name } if name == "html" => {
                self.insertion_mode = InsertionMode::AfterAfterBody;
            }
            Token::Eof => {}
            _ => {
                // Reprocess in body.
                self.insertion_mode = InsertionMode::InBody;
                // Re-push body if we can find it.
                if let Some(body) = self.dom.body_element() {
                    self.open_elements.push(body);
                }
                self.process(token);
            }
        }
    }

    fn process_after_after_body(&mut self, token: Token) {
        match token {
            Token::Character(ref s) if s.trim().is_empty() => {}
            Token::Comment(text) => {
                let comment = self.dom.create_comment(&text);
                self.dom.append_child(NodeId::DOCUMENT, comment);
            }
            Token::Eof => {}
            _ => {
                // Reprocess in body.
                self.insertion_mode = InsertionMode::InBody;
                if let Some(body) = self.dom.body_element() {
                    self.open_elements.push(body);
                }
                self.process(token);
            }
        }
    }

    // -- Helpers ------------------------------------------------------------

    fn create_element_from_token(&mut self, token: &Token) -> NodeId {
        match token {
            Token::StartTag {
                name, attributes, ..
            } => self.dom.create_element(
                name,
                attributes
                    .iter()
                    .map(|(n, v)| Attribute {
                        name: n.clone(),
                        value: v.clone(),
                    })
                    .collect(),
            ),
            _ => unreachable!("create_element_from_token called with non-StartTag"),
        }
    }

    /// Auto-close certain elements when specific new elements are opened.
    fn auto_close_before_open(&mut self, new_tag: &str) {
        // <p> closes an open <p>.
        if new_tag == "p" || is_heading(new_tag) {
            if self.current_tag() == Some("p") {
                self.open_elements.pop();
            }
        }

        // <li> closes an open <li>.
        if new_tag == "li" {
            if self.current_tag() == Some("li") {
                self.open_elements.pop();
            }
        }

        // <dd>/<dt> close open <dd>/<dt>.
        if new_tag == "dd" || new_tag == "dt" {
            let cur = self.current_tag().map(|s| s.to_string());
            if cur.as_deref() == Some("dd") || cur.as_deref() == Some("dt") {
                self.open_elements.pop();
            }
        }

        // <option> closes open <option>.
        if new_tag == "option" {
            if self.current_tag() == Some("option") {
                self.open_elements.pop();
            }
        }

        // <tr> closes open <tr>.
        if new_tag == "tr" {
            if self.current_tag() == Some("tr") {
                self.open_elements.pop();
            }
        }

        // <td>/<th> close open <td>/<th>.
        if new_tag == "td" || new_tag == "th" {
            let cur = self.current_tag().map(|s| s.to_string());
            if cur.as_deref() == Some("td") || cur.as_deref() == Some("th") {
                self.open_elements.pop();
            }
        }
    }

    /// Close elements by popping the stack until we find a matching tag.
    fn close_tag(&mut self, tag: &str) {
        // Search the stack from top to bottom for a matching tag.
        let pos = self
            .open_elements
            .iter()
            .rposition(|&id| self.dom.tag_name(id) == Some(tag));

        if let Some(pos) = pos {
            // Pop everything from the stack down to and including the match.
            self.open_elements.truncate(pos);
        }
        // If not found, ignore the end tag (tag soup recovery).
    }
}

fn is_heading(tag: &str) -> bool {
    matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_document() {
        let dom = parse("<!DOCTYPE html><html><head><title>Test</title></head><body><p>Hello</p></body></html>");
        assert!(dom.html_element().is_some());
        assert!(dom.head_element().is_some());
        assert!(dom.body_element().is_some());

        let body = dom.body_element().unwrap();
        let text = dom.inner_text(body);
        assert!(text.contains("Hello"));
    }

    #[test]
    fn parse_implied_elements() {
        // Missing html, head, body tags — should be auto-created.
        let dom = parse("<p>Hello</p>");
        assert!(dom.html_element().is_some());
        assert!(dom.head_element().is_some());
        assert!(dom.body_element().is_some());

        let body = dom.body_element().unwrap();
        let text = dom.inner_text(body);
        assert!(text.contains("Hello"));
    }

    #[test]
    fn parse_void_elements() {
        let dom = parse("<p>Before<br>After</p>");
        let body = dom.body_element().unwrap();

        // p should contain: text("Before"), br, text("After")
        let p = dom.node(body).children[0];
        let children = &dom.node(p).children;
        assert_eq!(children.len(), 3);
        assert!(matches!(dom.node(children[0]).kind, NodeKind::Text(_)));
        assert_eq!(dom.tag_name(children[1]), Some("br"));
        assert!(matches!(dom.node(children[2]).kind, NodeKind::Text(_)));
    }

    #[test]
    fn parse_auto_close_p() {
        // Two consecutive <p> tags — the first should auto-close.
        let dom = parse("<p>First<p>Second");
        let body = dom.body_element().unwrap();
        let children = &dom.node(body).children;
        // Should have two <p> elements as siblings, not nested.
        assert_eq!(children.len(), 2);
        assert_eq!(dom.tag_name(children[0]), Some("p"));
        assert_eq!(dom.tag_name(children[1]), Some("p"));
    }

    #[test]
    fn parse_auto_close_li() {
        let dom = parse("<ul><li>A<li>B<li>C</ul>");
        let body = dom.body_element().unwrap();
        let ul = dom.node(body).children[0];
        let lis: Vec<NodeId> = dom
            .node(ul)
            .children
            .iter()
            .filter(|&&id| dom.tag_name(id) == Some("li"))
            .copied()
            .collect();
        assert_eq!(lis.len(), 3);
    }

    #[test]
    fn parse_script_content() {
        let dom = parse("<body><script>var x = 1 < 2;</script></body>");
        let body = dom.body_element().unwrap();
        let script = dom.node(body).children[0];
        assert_eq!(dom.tag_name(script), Some("script"));

        let _text = dom.inner_text(script);
        // inner_text skips script content, so check the raw child.
        let script_child = dom.node(script).children[0];
        assert!(matches!(&dom.node(script_child).kind, NodeKind::Text(t) if t.contains("var x")));
    }

    #[test]
    fn parse_nested_divs() {
        let dom = parse("<div id=\"outer\"><div id=\"inner\">Content</div></div>");
        let body = dom.body_element().unwrap();
        let outer = dom.node(body).children[0];
        assert_eq!(dom.element(outer).unwrap().id(), Some("outer"));

        let inner = dom.node(outer).children[0];
        assert_eq!(dom.element(inner).unwrap().id(), Some("inner"));

        assert_eq!(dom.inner_text(inner).trim(), "Content");
    }

    #[test]
    fn parse_attributes_preserved() {
        let dom =
            parse("<a href=\"/test\" class=\"nav-link\" data-id=\"42\">Click</a>");
        let body = dom.body_element().unwrap();
        let a = dom.node(body).children[0];
        let elem = dom.element(a).unwrap();
        assert_eq!(elem.get_attr("href"), Some("/test"));
        assert_eq!(elem.get_attr("class"), Some("nav-link"));
        assert_eq!(elem.get_attr("data-id"), Some("42"));
    }

    #[test]
    fn parse_real_world_page() {
        let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Test Page</title>
    <link rel="stylesheet" href="/style.css">
    <style>body { margin: 0; }</style>
</head>
<body>
    <header>
        <nav>
            <a href="/" class="logo">Home</a>
            <ul>
                <li><a href="/about">About</a>
                <li><a href="/contact">Contact</a>
            </ul>
        </nav>
    </header>
    <main>
        <h1>Welcome</h1>
        <p>This is a <strong>test</strong> page with <em>various</em> elements.</p>
        <div class="card" data-testid="main-card">
            <img src="image.jpg" alt="Test">
            <p>Card content</p>
        </div>
    </main>
    <footer>
        <p>&copy; 2024 Test</p>
    </footer>
    <script>
        document.addEventListener('DOMContentLoaded', function() {
            console.log('ready');
        });
    </script>
</body>
</html>"#;

        let dom = parse(html);
        assert!(dom.html_element().is_some());

        let body = dom.body_element().unwrap();
        let text = dom.inner_text(body);
        assert!(text.contains("Welcome"));
        assert!(text.contains("test"));
        assert!(text.contains("Card content"));

        // Test querySelector integration.
        let h1 = dom.query_selector(NodeId::DOCUMENT, "h1");
        assert!(h1.is_some());
        assert_eq!(dom.inner_text(h1.unwrap()).trim(), "Welcome");

        let card = dom.query_selector(NodeId::DOCUMENT, "[data-testid=\"main-card\"]");
        assert!(card.is_some());

        let nav_links = dom.query_selector_all(NodeId::DOCUMENT, "nav a");
        assert!(nav_links.len() >= 3); // logo + about + contact

        let list_items = dom.query_selector_all(NodeId::DOCUMENT, "ul > li");
        assert_eq!(list_items.len(), 2);
    }

    #[test]
    fn parse_text_coalescing() {
        let dom = parse("<p>Hello world</p>");
        let body = dom.body_element().unwrap();
        let p = dom.node(body).children[0];
        // Should be a single text node, not fragmented.
        assert_eq!(dom.node(p).children.len(), 1);
        assert!(matches!(
            &dom.node(dom.node(p).children[0]).kind,
            NodeKind::Text(t) if t == "Hello world"
        ));
    }

    #[test]
    fn parse_comment_preserved() {
        let dom = parse("<body><!-- TODO: fix this --><p>Content</p></body>");
        let body = dom.body_element().unwrap();
        let first_child = dom.node(body).children[0];
        assert!(matches!(
            &dom.node(first_child).kind,
            NodeKind::Comment(c) if c.contains("TODO")
        ));
    }

    #[test]
    fn outer_html_after_parse() {
        let dom = parse("<div><p>Hello</p></div>");
        let body = dom.body_element().unwrap();
        let html = dom.outer_html(body);
        assert!(html.contains("<div><p>Hello</p></div>"));
    }
}
