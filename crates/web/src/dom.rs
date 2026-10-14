//! The document tree: nodes in one array, each knowing its parent and
//! children by index.

use alloc::string::String;
use alloc::vec::Vec;

pub type NodeId = usize;

/// The document node is always the first.
pub const DOCUMENT: NodeId = 0;

#[derive(Clone, Debug)]
pub enum NodeData {
    Document,
    Element(Element),
    Text(String),
    Comment,
}

#[derive(Clone, Debug)]
pub struct Element {
    /// The tag's name, in lower case.
    pub name: String,
    /// The attributes, names in lower case, in order.
    pub attrs: Vec<(String, String)>,
    /// What the form control holds now (its value, or checked), kept here
    /// so that typing in a field survives a new layout.
    pub value: Option<String>,
    pub checked: bool,
}

impl Element {
    pub fn new(name: &str, attrs: Vec<(String, String)>) -> Self {
        let checked = attrs.iter().any(|(n, _)| n == "checked");
        Self { name: String::from(name), attrs, value: None, checked }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    pub fn id(&self) -> Option<&str> {
        self.attr("id").filter(|s| !s.is_empty())
    }

    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.attr("class").unwrap_or("").split_ascii_whitespace()
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.classes().any(|c| c == class)
    }

    /// The value a form control holds: what was typed, else its `value`.
    pub fn current_value(&self) -> &str {
        self.value.as_deref().or_else(|| self.attr("value")).unwrap_or("")
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub data: NodeData,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub nodes: Vec<Node>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Self { nodes: alloc::vec![Node { parent: None, children: Vec::new(), data: NodeData::Document }] }
    }

    /// Adds `data` as the last child of `parent`.
    pub fn append(&mut self, parent: NodeId, data: NodeData) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node { parent: Some(parent), children: Vec::new(), data });
        self.nodes[parent].children.push(id);
        id
    }

    /// Adds text at the end of `parent`, joined to its last text node.
    pub fn append_text(&mut self, parent: NodeId, text: &str) {
        if let Some(&last) = self.nodes[parent].children.last()
            && let NodeData::Text(t) = &mut self.nodes[last].data
        {
            t.push_str(text);
            return;
        }
        self.append(parent, NodeData::Text(String::from(text)));
    }

    /// Moves `node` to the end of `parent`'s children.
    pub fn reparent(&mut self, node: NodeId, parent: NodeId) {
        if let Some(old) = self.nodes[node].parent {
            self.nodes[old].children.retain(|&c| c != node);
        }
        self.nodes[node].parent = Some(parent);
        self.nodes[parent].children.push(node);
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.nodes[id].data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn element_mut(&mut self, id: NodeId) -> Option<&mut Element> {
        match &mut self.nodes[id].data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    /// Whether `id` is an element named `name`.
    pub fn is(&self, id: NodeId, name: &str) -> bool {
        self.element(id).is_some_and(|e| e.name == name)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].parent
    }

    /// The nearest ancestor (or `id` itself) named `name`.
    pub fn closest(&self, mut id: NodeId, name: &str) -> Option<NodeId> {
        loop {
            if self.is(id, name) {
                return Some(id);
            }
            id = self.nodes[id].parent?;
        }
    }

    /// The element children of `id`.
    pub fn element_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes[id].children.iter().copied().filter(|&c| self.element(c).is_some())
    }

    /// The element before `id` among its parent's children.
    pub fn previous_element(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.nodes[id].parent?;
        let siblings = &self.nodes[parent].children;
        let at = siblings.iter().position(|&c| c == id)?;
        siblings[..at].iter().rev().copied().find(|&c| self.element(c).is_some())
    }

    /// The element after `id` among its parent's children.
    pub fn next_element(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.nodes[id].parent?;
        let siblings = &self.nodes[parent].children;
        let at = siblings.iter().position(|&c| c == id)?;
        siblings[at + 1..].iter().copied().find(|&c| self.element(c).is_some())
    }

    /// Every node under `id`, in document order (`id` first).
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = alloc::vec![id];
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.nodes[n].children.iter().rev());
        }
        out
    }

    /// The first element named `name`, in document order.
    pub fn find(&self, name: &str) -> Option<NodeId> {
        self.descendants(DOCUMENT).into_iter().find(|&n| self.is(n, name))
    }

    /// The element whose `id` (or, for an anchor, `name`) is `id`.
    pub fn by_id(&self, id: &str) -> Option<NodeId> {
        let all = self.descendants(DOCUMENT);
        all.iter().copied().find(|&n| self.element(n).and_then(|e| e.id()) == Some(id)).or_else(|| {
            all.into_iter().find(|&n| self.element(n).is_some_and(|e| e.name == "a" && e.attr("name") == Some(id)))
        })
    }

    /// The text under `id`, joined.
    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        for n in self.descendants(id) {
            if let NodeData::Text(t) = &self.nodes[n].data {
                out.push_str(t);
            }
        }
        out
    }

    /// The page's title, spaces collapsed.
    pub fn title(&self) -> String {
        let Some(t) = self.find("title") else { return String::new() };
        collapse(&self.text_content(t))
    }
}

/// `text` with runs of white space made one space, trimmed.
pub fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_ascii_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}
