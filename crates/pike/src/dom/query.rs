//! querySelector / querySelectorAll — CSS selector matching on the DOM.

use crate::css::selector::{
    parse_selector_list, AttrMatcher, Combinator, CompoundSelector, PseudoClass, Selector,
};
use crate::dom::{Dom, ElementData, NodeId, NodeKind};

impl Dom {
    /// CSS querySelector — returns the first matching element, DFS order.
    pub fn query_selector(&self, root: NodeId, selector: &str) -> Option<NodeId> {
        let selectors = parse_selector_list(selector).ok()?;
        let descendants = self.descendants(root);
        for &id in &descendants {
            if !matches!(self.node(id).kind, NodeKind::Element(_)) {
                continue;
            }
            if selectors.iter().any(|sel| self.matches(id, sel)) {
                return Some(id);
            }
        }
        None
    }

    /// CSS querySelectorAll — returns all matching elements, DFS order.
    pub fn query_selector_all(&self, root: NodeId, selector: &str) -> Vec<NodeId> {
        let selectors = match parse_selector_list(selector) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let descendants = self.descendants(root);
        descendants
            .into_iter()
            .filter(|&id| {
                matches!(self.node(id).kind, NodeKind::Element(_))
                    && selectors.iter().any(|sel| self.matches(id, sel))
            })
            .collect()
    }

    /// Test whether an element matches a complete selector (complex selector).
    fn matches(&self, id: NodeId, selector: &Selector) -> bool {
        // A Selector is a list of (CompoundSelector, Combinator) pairs.
        // We match right-to-left: the rightmost compound must match `id`,
        // then we walk up/sideways according to combinators.
        if selector.0.is_empty() {
            return false;
        }

        let mut current = id;
        let mut idx = selector.0.len() - 1;

        // The rightmost compound must match the element itself.
        if !self.matches_compound(current, &selector.0[idx].0) {
            return false;
        }

        // Walk left through the selector, checking combinators.
        // The combinator connecting compound[i] to compound[i+1] is stored
        // at selector.0[i].1 (on the left compound).
        while idx > 0 {
            idx -= 1;
            let combinator = &selector.0[idx].1;
            let compound = &selector.0[idx].0;

            match combinator {
                Combinator::Descendant => {
                    // Any ancestor must match.
                    let mut found = false;
                    let mut ancestor = self.node(current).parent;
                    while let Some(anc_id) = ancestor {
                        if matches!(self.node(anc_id).kind, NodeKind::Element(_))
                            && self.matches_compound(anc_id, compound)
                        {
                            current = anc_id;
                            found = true;
                            break;
                        }
                        ancestor = self.node(anc_id).parent;
                    }
                    if !found {
                        return false;
                    }
                }
                Combinator::Child => {
                    // Direct parent must match.
                    let parent = match self.node(current).parent {
                        Some(p) => p,
                        None => return false,
                    };
                    if !matches!(self.node(parent).kind, NodeKind::Element(_))
                        || !self.matches_compound(parent, compound)
                    {
                        return false;
                    }
                    current = parent;
                }
                Combinator::NextSibling => {
                    // Previous sibling element must match.
                    let prev = match self.prev_element_sibling(current) {
                        Some(p) => p,
                        None => return false,
                    };
                    if !self.matches_compound(prev, compound) {
                        return false;
                    }
                    current = prev;
                }
                Combinator::SubsequentSibling => {
                    // Any preceding sibling element must match.
                    let mut found = false;
                    let mut sib = self.prev_element_sibling(current);
                    while let Some(sib_id) = sib {
                        if self.matches_compound(sib_id, compound) {
                            current = sib_id;
                            found = true;
                            break;
                        }
                        sib = self.prev_element_sibling(sib_id);
                    }
                    if !found {
                        return false;
                    }
                }
                Combinator::None => {}
            }
        }

        true
    }

    /// Match a single compound selector against an element.
    fn matches_compound(&self, id: NodeId, compound: &CompoundSelector) -> bool {
        let elem = match self.element(id) {
            Some(e) => e,
            None => return false,
        };

        // Universal selector matches everything.
        // Type selector.
        if let Some(ref tag) = compound.tag {
            if tag != "*" && tag != &elem.tag_name {
                return false;
            }
        }

        // ID selector.
        if let Some(ref sel_id) = compound.id {
            if elem.id() != Some(sel_id.as_str()) {
                return false;
            }
        }

        // Class selectors.
        for class in &compound.classes {
            if !elem.has_class(class) {
                return false;
            }
        }

        // Attribute selectors.
        for attr_sel in &compound.attributes {
            if !self.matches_attr(elem, attr_sel) {
                return false;
            }
        }

        // Pseudo-classes.
        for pseudo in &compound.pseudo_classes {
            if !self.matches_pseudo(id, pseudo) {
                return false;
            }
        }

        true
    }

    fn matches_attr(&self, elem: &ElementData, sel: &AttrMatcher) -> bool {
        let val = elem.get_attr(&sel.name);
        match &sel.op {
            None => val.is_some(), // [attr] — existence check
            Some((op, expected)) => {
                let val = match val {
                    Some(v) => v,
                    None => return false,
                };
                match op.as_str() {
                    "=" => val == expected,
                    "~=" => val.split_whitespace().any(|w| w == expected),
                    "|=" => val == expected || val.starts_with(&format!("{}-", expected)),
                    "^=" => val.starts_with(expected.as_str()),
                    "$=" => val.ends_with(expected.as_str()),
                    "*=" => val.contains(expected.as_str()),
                    _ => false,
                }
            }
        }
    }

    fn matches_pseudo(&self, id: NodeId, pseudo: &PseudoClass) -> bool {
        match pseudo {
            PseudoClass::FirstChild => self.element_index(id) == Some(0),
            PseudoClass::LastChild => {
                if let Some(parent) = self.node(id).parent {
                    let count = self.child_element_count(parent);
                    self.element_index(id) == Some(count.saturating_sub(1))
                } else {
                    false
                }
            }
            PseudoClass::NthChild(a, b) => {
                if let Some(index) = self.element_index(id) {
                    let n1 = (index + 1) as i32; // 1-indexed
                    nth_matches(*a, *b, n1)
                } else {
                    false
                }
            }
            PseudoClass::NthLastChild(a, b) => {
                if let (Some(parent), Some(index)) = (self.node(id).parent, self.element_index(id))
                {
                    let count = self.child_element_count(parent);
                    let from_end = (count - index) as i32; // 1-indexed from end
                    nth_matches(*a, *b, from_end)
                } else {
                    false
                }
            }
            PseudoClass::OnlyChild => {
                if let Some(parent) = self.node(id).parent {
                    self.child_element_count(parent) == 1
                } else {
                    false
                }
            }
            PseudoClass::Not(inner_selectors) => {
                !inner_selectors.iter().any(|sel| self.matches(id, sel))
            }
            PseudoClass::Empty => {
                self.node(id).children.iter().all(|&c| {
                    matches!(self.node(c).kind, NodeKind::Comment(_))
                })
            }
            PseudoClass::Root => {
                // The root element is the <html> element.
                self.node(id).parent == Some(NodeId::DOCUMENT)
                    && self.tag_name(id) == Some("html")
            }
        }
    }

    /// Get previous sibling that is an element (skip text/comment nodes).
    fn prev_element_sibling(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.node(id).parent?;
        let siblings = &self.node(parent).children;
        let pos = siblings.iter().position(|&c| c == id)?;
        siblings[..pos]
            .iter()
            .rev()
            .find(|&&c| matches!(self.node(c).kind, NodeKind::Element(_)))
            .copied()
    }
}

/// Check if `n` satisfies `an + b` for some non-negative integer value of n.
fn nth_matches(a: i32, b: i32, n: i32) -> bool {
    if a == 0 {
        return n == b;
    }
    let diff = n - b;
    if a > 0 {
        diff >= 0 && diff % a == 0
    } else {
        diff <= 0 && diff % a == 0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::Attribute;

    fn build_test_dom() -> Dom {
        // <html>
        //   <body>
        //     <div id="main" class="container">
        //       <h1>Title</h1>
        //       <p class="intro first">Hello</p>
        //       <p class="body">World</p>
        //       <ul>
        //         <li data-index="0">A</li>
        //         <li data-index="1">B</li>
        //         <li data-index="2">C</li>
        //       </ul>
        //     </div>
        //     <div id="sidebar" class="container">
        //       <span class="empty"></span>
        //     </div>
        //   </body>
        // </html>
        let mut dom = Dom::new();

        let html = dom.create_element("html", vec![]);
        dom.append_child(NodeId::DOCUMENT, html);

        let body = dom.create_element("body", vec![]);
        dom.append_child(html, body);

        let main_div = dom.create_element(
            "div",
            vec![
                Attribute { name: "id".into(), value: "main".into() },
                Attribute { name: "class".into(), value: "container".into() },
            ],
        );
        dom.append_child(body, main_div);

        let h1 = dom.create_element("h1", vec![]);
        dom.append_child(main_div, h1);
        let h1_text = dom.create_text("Title");
        dom.append_child(h1, h1_text);

        let p1 = dom.create_element(
            "p",
            vec![Attribute { name: "class".into(), value: "intro first".into() }],
        );
        dom.append_child(main_div, p1);
        let p1_text = dom.create_text("Hello");
        dom.append_child(p1, p1_text);

        let p2 = dom.create_element(
            "p",
            vec![Attribute { name: "class".into(), value: "body".into() }],
        );
        dom.append_child(main_div, p2);
        let p2_text = dom.create_text("World");
        dom.append_child(p2, p2_text);

        let ul = dom.create_element("ul", vec![]);
        dom.append_child(main_div, ul);

        for i in 0..3 {
            let li = dom.create_element(
                "li",
                vec![Attribute {
                    name: "data-index".into(),
                    value: i.to_string(),
                }],
            );
            dom.append_child(ul, li);
            let text = dom.create_text(&["A", "B", "C"][i as usize]);
            dom.append_child(li, text);
        }

        let sidebar = dom.create_element(
            "div",
            vec![
                Attribute { name: "id".into(), value: "sidebar".into() },
                Attribute { name: "class".into(), value: "container".into() },
            ],
        );
        dom.append_child(body, sidebar);

        let empty_span = dom.create_element(
            "span",
            vec![Attribute { name: "class".into(), value: "empty".into() }],
        );
        dom.append_child(sidebar, empty_span);

        dom
    }

    #[test]
    fn query_by_tag() {
        let dom = build_test_dom();
        let results = dom.query_selector_all(NodeId::DOCUMENT, "li");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn query_by_id() {
        let dom = build_test_dom();
        let result = dom.query_selector(NodeId::DOCUMENT, "#main");
        assert!(result.is_some());
        assert_eq!(dom.element(result.unwrap()).unwrap().id(), Some("main"));
    }

    #[test]
    fn query_by_class() {
        let dom = build_test_dom();
        let results = dom.query_selector_all(NodeId::DOCUMENT, ".container");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_compound_selector() {
        let dom = build_test_dom();
        let result = dom.query_selector(NodeId::DOCUMENT, "p.intro");
        assert!(result.is_some());
        assert_eq!(dom.inner_text(result.unwrap()).trim(), "Hello");
    }

    #[test]
    fn query_descendant_combinator() {
        let dom = build_test_dom();
        let results = dom.query_selector_all(NodeId::DOCUMENT, "#main li");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn query_child_combinator() {
        let dom = build_test_dom();
        let results = dom.query_selector_all(NodeId::DOCUMENT, "#main > p");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_attribute_selector() {
        let dom = build_test_dom();
        let result = dom.query_selector(NodeId::DOCUMENT, "[data-index=\"1\"]");
        assert!(result.is_some());
        assert_eq!(dom.inner_text(result.unwrap()).trim(), "B");
    }

    #[test]
    fn query_attribute_starts_with() {
        let dom = build_test_dom();
        let results = dom.query_selector_all(NodeId::DOCUMENT, "[class^=\"intro\"]");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn query_first_child() {
        let dom = build_test_dom();
        let result = dom.query_selector(NodeId::DOCUMENT, "li:first-child");
        assert!(result.is_some());
        assert_eq!(dom.inner_text(result.unwrap()).trim(), "A");
    }

    #[test]
    fn query_last_child() {
        let dom = build_test_dom();
        let result = dom.query_selector(NodeId::DOCUMENT, "li:last-child");
        assert!(result.is_some());
        assert_eq!(dom.inner_text(result.unwrap()).trim(), "C");
    }

    #[test]
    fn query_nth_child() {
        let dom = build_test_dom();
        let result = dom.query_selector(NodeId::DOCUMENT, "li:nth-child(2)");
        assert!(result.is_some());
        assert_eq!(dom.inner_text(result.unwrap()).trim(), "B");
    }

    #[test]
    fn query_not_pseudo() {
        let dom = build_test_dom();
        let results = dom.query_selector_all(NodeId::DOCUMENT, "p:not(.body)");
        assert_eq!(results.len(), 1);
        assert_eq!(dom.inner_text(results[0]).trim(), "Hello");
    }

    #[test]
    fn query_universal() {
        let dom = build_test_dom();
        // * matches all elements
        let results = dom.query_selector_all(NodeId::DOCUMENT, "#sidebar > *");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn query_selector_list() {
        let dom = build_test_dom();
        let results = dom.query_selector_all(NodeId::DOCUMENT, "h1, p");
        assert_eq!(results.len(), 3); // 1 h1 + 2 p
    }

    #[test]
    fn query_adjacent_sibling() {
        let dom = build_test_dom();
        // h1 + p should match the first p (directly after h1)
        let result = dom.query_selector(NodeId::DOCUMENT, "h1 + p");
        assert!(result.is_some());
        assert_eq!(dom.inner_text(result.unwrap()).trim(), "Hello");
    }

    #[test]
    fn query_general_sibling() {
        let dom = build_test_dom();
        // h1 ~ p should match all p siblings after h1
        let results = dom.query_selector_all(NodeId::DOCUMENT, "h1 ~ p");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_empty() {
        let dom = build_test_dom();
        let result = dom.query_selector(NodeId::DOCUMENT, ":empty");
        assert!(result.is_some());
        assert_eq!(
            dom.element(result.unwrap()).unwrap().tag_name.as_str(),
            "span"
        );
    }
}
