//! Arena-allocated DOM tree.
//!
//! Every node lives in a single `Vec<Node>` and is referenced by [`NodeId`].
//! The document root is always at index 0.

pub mod query;

use std::fmt;

// ---------------------------------------------------------------------------
// NodeId
// ---------------------------------------------------------------------------

/// Lightweight handle into the DOM arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

impl NodeId {
    /// The document node is always slot 0.
    pub const DOCUMENT: NodeId = NodeId(0);
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}

// ---------------------------------------------------------------------------
// Node kinds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct ElementData {
    /// Lowercase tag name (e.g. "div", "input").
    pub tag_name: String,
    pub attributes: Vec<Attribute>,
}

impl ElementData {
    /// Get attribute value by name (case-insensitive lookup).
    pub fn get_attr(&self, name: &str) -> Option<&str> {
        let lower = name.to_ascii_lowercase();
        self.attributes
            .iter()
            .find(|a| a.name == lower)
            .map(|a| a.value.as_str())
    }

    /// Check if element has a given class.
    pub fn has_class(&self, class: &str) -> bool {
        self.get_attr("class")
            .map(|c| c.split_whitespace().any(|tok| tok == class))
            .unwrap_or(false)
    }

    /// Return the id attribute, if any.
    pub fn id(&self) -> Option<&str> {
        self.get_attr("id")
    }

    /// Return all class tokens.
    pub fn classes(&self) -> Vec<&str> {
        self.get_attr("class")
            .map(|c| c.split_whitespace().collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    /// The root document node.
    Document,
    /// `<!DOCTYPE ...>`
    Doctype { name: String },
    /// An element (tag).
    Element(ElementData),
    /// A run of text.
    Text(String),
    /// `<!-- ... -->`
    Comment(String),
}

// ---------------------------------------------------------------------------
// Node
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

// ---------------------------------------------------------------------------
// Dom (the arena)
// ---------------------------------------------------------------------------

/// The entire DOM tree.
#[derive(Debug, Clone)]
pub struct Dom {
    nodes: Vec<Node>,
}

impl Dom {
    /// Create a new DOM with an empty Document root at index 0.
    pub fn new() -> Self {
        let root = Node {
            id: NodeId::DOCUMENT,
            kind: NodeKind::Document,
            parent: None,
            children: Vec::new(),
        };
        Dom { nodes: vec![root] }
    }

    /// Total number of nodes (including the document root).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Borrow a node by id.
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    /// Mutably borrow a node by id.
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0]
    }

    /// Allocate a new node and return its id. The node is **not** attached to
    /// any parent yet — call [`append_child`] to wire it into the tree.
    pub fn create_node(&mut self, kind: NodeKind) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node {
            id,
            kind,
            parent: None,
            children: Vec::new(),
        });
        id
    }

    /// Append `child` as the last child of `parent`.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.nodes[child.0].parent = Some(parent);
        self.nodes[parent.0].children.push(child);
    }

    /// Insert `child` before `reference` in `parent`'s child list.
    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, reference: NodeId) {
        self.nodes[child.0].parent = Some(parent);
        let children = &mut self.nodes[parent.0].children;
        if let Some(pos) = children.iter().position(|&c| c == reference) {
            children.insert(pos, child);
        } else {
            children.push(child);
        }
    }

    /// Remove `child` from its parent's child list (does not deallocate).
    pub fn detach(&mut self, child: NodeId) {
        if let Some(parent_id) = self.nodes[child.0].parent {
            self.nodes[parent_id.0]
                .children
                .retain(|&c| c != child);
            self.nodes[child.0].parent = None;
        }
    }

    /// Convenience: create an element node.
    pub fn create_element(&mut self, tag: &str, attrs: Vec<Attribute>) -> NodeId {
        self.create_node(NodeKind::Element(ElementData {
            tag_name: tag.to_ascii_lowercase(),
            attributes: attrs,
        }))
    }

    /// Convenience: create a text node.
    pub fn create_text(&mut self, text: &str) -> NodeId {
        self.create_node(NodeKind::Text(text.to_string()))
    }

    /// Convenience: create a comment node.
    pub fn create_comment(&mut self, text: &str) -> NodeId {
        self.create_node(NodeKind::Comment(text.to_string()))
    }

    // -- Accessors ----------------------------------------------------------

    /// If the node is an element, return its data.
    pub fn element(&self, id: NodeId) -> Option<&ElementData> {
        match &self.node(id).kind {
            NodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    /// If the node is a text node, return its content.
    pub fn text(&self, id: NodeId) -> Option<&str> {
        match &self.node(id).kind {
            NodeKind::Text(t) => Some(t),
            _ => None,
        }
    }

    /// Return the tag name if the node is an element.
    pub fn tag_name(&self, id: NodeId) -> Option<&str> {
        self.element(id).map(|e| e.tag_name.as_str())
    }

    /// Recursively collect all text content under a node.
    pub fn inner_text(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        let node = self.node(id);
        match &node.kind {
            NodeKind::Text(t) => out.push_str(t),
            NodeKind::Element(e) => {
                // Skip script/style content
                if e.tag_name == "script" || e.tag_name == "style" {
                    return;
                }
                // Block-level elements get newlines
                if is_block_element(&e.tag_name) && !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                for &child in &node.children {
                    self.collect_text(child, out);
                }
                if is_block_element(&e.tag_name) && !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            _ => {
                for &child in &node.children {
                    self.collect_text(child, out);
                }
            }
        }
    }

    /// Serialize the subtree rooted at `id` to an HTML string.
    pub fn outer_html(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.serialize_node(id, &mut out);
        out
    }

    fn serialize_node(&self, id: NodeId, out: &mut String) {
        let node = self.node(id);
        match &node.kind {
            NodeKind::Document => {
                for &child in &node.children {
                    self.serialize_node(child, out);
                }
            }
            NodeKind::Doctype { name } => {
                out.push_str("<!DOCTYPE ");
                out.push_str(name);
                out.push('>');
            }
            NodeKind::Element(e) => {
                out.push('<');
                out.push_str(&e.tag_name);
                for attr in &e.attributes {
                    out.push(' ');
                    out.push_str(&attr.name);
                    out.push_str("=\"");
                    out.push_str(&html_escape_attr(&attr.value));
                    out.push('"');
                }
                if is_void_element(&e.tag_name) {
                    out.push_str(" />");
                    return;
                }
                out.push('>');
                for &child in &node.children {
                    self.serialize_node(child, out);
                }
                out.push_str("</");
                out.push_str(&e.tag_name);
                out.push('>');
            }
            NodeKind::Text(t) => out.push_str(t),
            NodeKind::Comment(c) => {
                out.push_str("<!--");
                out.push_str(c);
                out.push_str("-->");
            }
        }
    }

    /// DFS iterator over all node ids starting from `root`.
    pub fn descendants(&self, root: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        self.dfs_collect(root, &mut result);
        result
    }

    fn dfs_collect(&self, id: NodeId, out: &mut Vec<NodeId>) {
        out.push(id);
        let children: Vec<NodeId> = self.node(id).children.clone();
        for child in children {
            self.dfs_collect(child, out);
        }
    }

    /// Find the <html> element if it exists.
    pub fn html_element(&self) -> Option<NodeId> {
        self.node(NodeId::DOCUMENT)
            .children
            .iter()
            .find(|&&id| self.tag_name(id) == Some("html"))
            .copied()
    }

    /// Find the <body> element if it exists.
    pub fn body_element(&self) -> Option<NodeId> {
        self.html_element().and_then(|html| {
            self.node(html)
                .children
                .iter()
                .find(|&&id| self.tag_name(id) == Some("body"))
                .copied()
        })
    }

    /// Find the <head> element if it exists.
    pub fn head_element(&self) -> Option<NodeId> {
        self.html_element().and_then(|html| {
            self.node(html)
                .children
                .iter()
                .find(|&&id| self.tag_name(id) == Some("head"))
                .copied()
        })
    }

    /// Get the previous sibling of a node.
    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.node(id).parent?;
        let siblings = &self.node(parent).children;
        let pos = siblings.iter().position(|&c| c == id)?;
        if pos > 0 {
            Some(siblings[pos - 1])
        } else {
            None
        }
    }

    /// Get the next sibling of a node.
    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.node(id).parent?;
        let siblings = &self.node(parent).children;
        let pos = siblings.iter().position(|&c| c == id)?;
        siblings.get(pos + 1).copied()
    }

    /// Count element children of a node (ignoring text/comment nodes).
    pub fn child_element_count(&self, id: NodeId) -> usize {
        self.node(id)
            .children
            .iter()
            .filter(|&&c| matches!(self.node(c).kind, NodeKind::Element(_)))
            .count()
    }

    /// Get nth child element (0-indexed).
    pub fn nth_child_element(&self, parent: NodeId, n: usize) -> Option<NodeId> {
        self.node(parent)
            .children
            .iter()
            .filter(|&&c| matches!(self.node(c).kind, NodeKind::Element(_)))
            .nth(n)
            .copied()
    }

    /// Index of this node among its parent's element children (0-indexed).
    pub fn element_index(&self, id: NodeId) -> Option<usize> {
        let parent = self.node(id).parent?;
        self.node(parent)
            .children
            .iter()
            .filter(|&&c| matches!(self.node(c).kind, NodeKind::Element(_)))
            .position(|&c| c == id)
    }
}

impl Default for Dom {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// HTML void elements that cannot have children.
pub fn is_void_element(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Block-level elements for text extraction line breaks.
fn is_block_element(tag: &str) -> bool {
    matches!(
        tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "details"
            | "dialog"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hgroup"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "ul"
            | "br"
            | "tr"
            | "td"
            | "th"
    )
}

fn html_escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_simple_tree() {
        let mut dom = Dom::new();
        let html = dom.create_element("html", vec![]);
        dom.append_child(NodeId::DOCUMENT, html);

        let body = dom.create_element("body", vec![]);
        dom.append_child(html, body);

        let p = dom.create_element("p", vec![]);
        dom.append_child(body, p);

        let text = dom.create_text("Hello, world!");
        dom.append_child(p, text);

        assert_eq!(dom.len(), 5); // document + html + body + p + text
        assert_eq!(dom.tag_name(html), Some("html"));
        assert_eq!(dom.tag_name(body), Some("body"));
        assert_eq!(dom.inner_text(body), "Hello, world!\n");
    }

    #[test]
    fn element_data_helpers() {
        let e = ElementData {
            tag_name: "div".into(),
            attributes: vec![
                Attribute {
                    name: "class".into(),
                    value: "foo bar baz".into(),
                },
                Attribute {
                    name: "id".into(),
                    value: "main".into(),
                },
            ],
        };
        assert!(e.has_class("foo"));
        assert!(e.has_class("bar"));
        assert!(!e.has_class("qux"));
        assert_eq!(e.id(), Some("main"));
        assert_eq!(e.classes(), vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn outer_html_roundtrip() {
        let mut dom = Dom::new();
        let html = dom.create_element("html", vec![]);
        dom.append_child(NodeId::DOCUMENT, html);

        let body = dom.create_element("body", vec![]);
        dom.append_child(html, body);

        let div = dom.create_element(
            "div",
            vec![Attribute {
                name: "class".into(),
                value: "test".into(),
            }],
        );
        dom.append_child(body, div);

        let text = dom.create_text("hello");
        dom.append_child(div, text);

        let result = dom.outer_html(NodeId::DOCUMENT);
        assert_eq!(
            result,
            "<html><body><div class=\"test\">hello</div></body></html>"
        );
    }

    #[test]
    fn void_elements_self_close() {
        let mut dom = Dom::new();
        let br = dom.create_element("br", vec![]);
        dom.append_child(NodeId::DOCUMENT, br);
        assert_eq!(dom.outer_html(NodeId::DOCUMENT), "<br />");
    }

    #[test]
    fn detach_node() {
        let mut dom = Dom::new();
        let html = dom.create_element("html", vec![]);
        dom.append_child(NodeId::DOCUMENT, html);

        let a = dom.create_element("div", vec![]);
        let b = dom.create_element("span", vec![]);
        dom.append_child(html, a);
        dom.append_child(html, b);
        assert_eq!(dom.node(html).children.len(), 2);

        dom.detach(a);
        assert_eq!(dom.node(html).children.len(), 1);
        assert_eq!(dom.node(html).children[0], b);
    }

    #[test]
    fn inner_text_skips_script_style() {
        let mut dom = Dom::new();
        let body = dom.create_element("body", vec![]);
        dom.append_child(NodeId::DOCUMENT, body);

        let p = dom.create_element("p", vec![]);
        dom.append_child(body, p);
        let t1 = dom.create_text("visible");
        dom.append_child(p, t1);

        let script = dom.create_element("script", vec![]);
        dom.append_child(body, script);
        let t2 = dom.create_text("hidden js");
        dom.append_child(script, t2);

        let style = dom.create_element("style", vec![]);
        dom.append_child(body, style);
        let t3 = dom.create_text("hidden css");
        dom.append_child(style, t3);

        let text = dom.inner_text(body);
        assert!(text.contains("visible"));
        assert!(!text.contains("hidden js"));
        assert!(!text.contains("hidden css"));
    }

    #[test]
    fn sibling_navigation() {
        let mut dom = Dom::new();
        let parent = dom.create_element("div", vec![]);
        dom.append_child(NodeId::DOCUMENT, parent);

        let a = dom.create_element("span", vec![]);
        let b = dom.create_element("span", vec![]);
        let c = dom.create_element("span", vec![]);
        dom.append_child(parent, a);
        dom.append_child(parent, b);
        dom.append_child(parent, c);

        assert_eq!(dom.prev_sibling(a), None);
        assert_eq!(dom.next_sibling(a), Some(b));
        assert_eq!(dom.prev_sibling(b), Some(a));
        assert_eq!(dom.next_sibling(b), Some(c));
        assert_eq!(dom.next_sibling(c), None);
    }

    #[test]
    fn element_index_counting() {
        let mut dom = Dom::new();
        let parent = dom.create_element("ul", vec![]);
        dom.append_child(NodeId::DOCUMENT, parent);

        let li1 = dom.create_element("li", vec![]);
        let text = dom.create_text("spacer");
        let li2 = dom.create_element("li", vec![]);
        dom.append_child(parent, li1);
        dom.append_child(parent, text); // text node — not counted
        dom.append_child(parent, li2);

        assert_eq!(dom.element_index(li1), Some(0));
        assert_eq!(dom.element_index(li2), Some(1));
        assert_eq!(dom.child_element_count(parent), 2);
    }
}
