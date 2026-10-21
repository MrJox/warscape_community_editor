//! A typed element tree: what tile.ted, the river splines and the tile database entry are made of in every game that uses TEd's family of
//! files. A node is a class (`BUILDING_INSTANCE`, `LayerInstance`, `children`, `point` ...), its properties in the source's order, and its
//! child nodes. Wrappers (`children`, `points`, `modules`) are nodes without properties.
//!
//! **Trivia.** A node can carry what only the format it was read from needs to give the same bytes back: the original text of a value
//! that format would write differently (`verbatim`), and pieces of layout that differ from the document's style (`layout`). Setting a
//! property drops its verbatim text, so an edited value is always written from the value. Other formats ignore trivia.

use crate::sym::Sym;
use crate::value::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub name: Sym,
    pub value: Value,
}

/// Source-format details of one node (see the module docs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trivia {
    /// (property, original text) for values whose text the source format would not reproduce from the value.
    pub verbatim: Vec<(Sym, Box<str>)>,

    /// (format-defined position code, bytes) for layout that differs from the document's style.
    pub layout: Vec<(u32, Box<[u8]>)>,
}

impl Trivia {
    pub fn is_empty(&self) -> bool {
        self.verbatim.is_empty() && self.layout.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub class: Sym,
    props: Vec<Prop>,
    pub children: Vec<Node>,
    trivia: Option<Box<Trivia>>,
}

impl Node {
    pub fn new(class: impl Into<Sym>) -> Self {
        Self { class: class.into(), props: vec![], children: vec![], trivia: None }
    }

    /// A node with these properties, in this order.
    pub fn with_props(class: impl Into<Sym>, props: Vec<Prop>) -> Self {
        Self { class: class.into(), props, children: vec![], trivia: None }
    }

    pub fn props(&self) -> &[Prop] {
        &self.props
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.props.iter().find(|p| p.name == name).map(|p| &p.value)
    }

    /// Sets a property (in place, or appended when the node does not have it) and drops its verbatim text.
    pub fn set(&mut self, name: impl Into<Sym>, value: Value) {
        let name = name.into();
        self.drop_verbatim(name);
        match self.props.iter_mut().find(|p| p.name == name) {
            Some(p) => p.value = value,
            None => self.props.push(Prop { name, value }),
        }
    }

    /// Removes a property; `None` when the node did not have it.
    pub fn remove(&mut self, name: &str) -> Option<Value> {
        let i = self.props.iter().position(|p| p.name == name)?;
        let p = self.props.remove(i);
        self.drop_verbatim(p.name);
        Some(p.value)
    }

    /// For readers: appends a property as read, with its original text when the format would not write it back the same way.
    pub fn push_read(&mut self, name: Sym, value: Value, verbatim: Option<Box<str>>) {
        if let Some(text) = verbatim {
            self.trivia_mut().verbatim.push((name, text));
        }

        self.props.push(Prop { name, value });
    }

    /// The text the source format had for this property, when its value would not give it back.
    pub fn verbatim(&self, name: Sym) -> Option<&str> {
        self.trivia.as_ref()?.verbatim.iter().find(|(n, _)| *n == name).map(|(_, t)| &**t)
    }

    pub fn trivia(&self) -> Option<&Trivia> {
        self.trivia.as_deref()
    }

    pub fn trivia_mut(&mut self) -> &mut Trivia {
        self.trivia.get_or_insert_with(Default::default)
    }

    /// Drops every trivia of this node (not of its children).
    pub fn clear_trivia(&mut self) {
        self.trivia = None;
    }

    fn drop_verbatim(&mut self, name: Sym) {
        if let Some(t) = &mut self.trivia {
            t.verbatim.retain(|(n, _)| *n != name);
            if t.is_empty() {
                self.trivia = None;
            }
        }
    }

    /// The first child of this class.
    pub fn child(&self, class: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.class == class)
    }

    pub fn child_mut(&mut self, class: &str) -> Option<&mut Node> {
        self.children.iter_mut().find(|c| c.class == class)
    }

    /// The nodes inside the wrapper child `wrapper` (TEd: `children`, `points`); empty when there is none.
    pub fn wrapped(&self, wrapper: &str) -> &[Node] {
        self.child(wrapper).map_or(&[], |w| &w.children)
    }

    /// The wrapper child `wrapper`'s nodes, the wrapper added (last) when there is none.
    pub fn wrapped_mut(&mut self, wrapper: &str) -> &mut Vec<Node> {
        let i = match self.children.iter().position(|c| c.class == wrapper) {
            Some(i) => i,
            None => {
                self.children.push(Node::new(wrapper));
                self.children.len() - 1
            }
        };

        &mut self.children[i].children
    }

    /// Calls `f` for this node and every node below it, depth first, in order.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Node)) {
        f(self);
        for c in &self.children {
            c.walk(f);
        }
    }

    /// The number of nodes in this tree.
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(Node::count).sum::<usize>()
    }

    /// The tree without trivia: two trees that mean the same compare equal this way.
    pub fn without_trivia(&self) -> Node {
        Node { class: self.class, props: self.props.clone(), children: self.children.iter().map(Node::without_trivia).collect(), trivia: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_a_value_drops_its_verbatim_text() {
        let mut n = Node::new("SCENE_GRAPH");
        let flags = Sym::new("flags");
        n.push_read(flags, Value::Flags(vec![Sym::new("OIF_VISIBLE")]), Some("OIF_VISIBLE".into()));
        n.push_read(Sym::new("name"), Value::Str("root".into()), None);
        assert_eq!(n.verbatim(flags), Some("OIF_VISIBLE"));
        n.set("flags", Value::Flags(vec![]));
        assert_eq!(n.verbatim(flags), None);
        assert!(n.trivia().is_none());
        assert_eq!(n.props().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["flags", "name"]);
    }

    #[test]
    fn wrappers() {
        let mut layer = Node::new("LayerInstance");
        assert!(layer.wrapped("children").is_empty());
        layer.wrapped_mut("children").push(Node::new("BUILDING_INSTANCE"));
        assert_eq!(layer.wrapped("children").len(), 1);
        assert_eq!(layer.count(), 3);
    }
}
