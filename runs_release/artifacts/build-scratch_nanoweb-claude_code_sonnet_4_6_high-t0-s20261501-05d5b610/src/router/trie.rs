use std::collections::HashMap;

/// A segment in a route pattern.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Segment {
    Literal(String),
    Param(String),
    Wildcard(String),
}

impl Segment {
    pub fn from_str(s: &str) -> Self {
        if let Some(name) = s.strip_prefix(':') {
            Self::Param(name.to_string())
        } else if let Some(name) = s.strip_prefix('*') {
            Self::Wildcard(name.to_string())
        } else {
            Self::Literal(s.to_string())
        }
    }
}

#[derive(Debug)]
pub struct TrieNode<V> {
    pub value: Option<V>,
    pub literal_children: HashMap<String, TrieNode<V>>,
    pub param_child: Option<(String, Box<TrieNode<V>>)>,
    pub wildcard: Option<(String, V)>,
}

impl<V> TrieNode<V> {
    pub fn new() -> Self {
        Self {
            value: None,
            literal_children: HashMap::new(),
            param_child: None,
            wildcard: None,
        }
    }
}

impl<V> Default for TrieNode<V> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Trie<V> {
    root: TrieNode<V>,
}

impl<V> Trie<V> {
    pub fn new() -> Self {
        Self {
            root: TrieNode::new(),
        }
    }

    pub fn insert(&mut self, pattern: &str, value: V) {
        let segments = parse_pattern(pattern);
        insert_node(&mut self.root, &segments, value);
    }

    /// Returns (value, params) for the best matching route.
    /// Allocation: only the params HashMap is allocated per lookup.
    pub fn lookup<'a>(&'a self, path: &str) -> Option<(&'a V, HashMap<String, String>)> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut params = HashMap::new();
        match lookup_node(&self.root, &parts, &mut params) {
            Some(v) => Some((v, params)),
            None => None,
        }
    }
}

impl<V: Default> Trie<V> {
    /// Get or insert a mutable reference to the value at the given pattern.
    pub fn get_or_insert(&mut self, pattern: &str) -> &mut V {
        let segments = parse_pattern(pattern);
        get_or_insert_node(&mut self.root, &segments)
    }
}

fn get_or_insert_node<'a, V: Default>(
    node: &'a mut TrieNode<V>,
    segments: &[Segment],
) -> &'a mut V {
    if segments.is_empty() {
        if node.value.is_none() {
            node.value = Some(V::default());
        }
        return node.value.as_mut().unwrap();
    }

    match &segments[0] {
        Segment::Literal(lit) => {
            let lit = lit.clone();
            let child = node
                .literal_children
                .entry(lit)
                .or_insert_with(TrieNode::new);
            get_or_insert_node(child, &segments[1..])
        }
        Segment::Param(name) => {
            let name = name.clone();
            if node.param_child.is_none() {
                node.param_child = Some((name.clone(), Box::new(TrieNode::new())));
            }
            let (_, child) = node.param_child.as_mut().unwrap();
            get_or_insert_node(child, &segments[1..])
        }
        Segment::Wildcard(name) => {
            // Wildcard: store default value directly in wildcard slot
            let name = name.clone();
            if node.wildcard.is_none() {
                node.wildcard = Some((name, V::default()));
            }
            &mut node.wildcard.as_mut().unwrap().1
        }
    }
}

impl<V> Default for Trie<V> {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_pattern(pattern: &str) -> Vec<Segment> {
    pattern
        .split('/')
        .filter(|s| !s.is_empty())
        .map(Segment::from_str)
        .collect()
}

fn insert_node<V>(node: &mut TrieNode<V>, segments: &[Segment], value: V) {
    if segments.is_empty() {
        node.value = Some(value);
        return;
    }

    match &segments[0] {
        Segment::Literal(lit) => {
            let child = node
                .literal_children
                .entry(lit.clone())
                .or_insert_with(TrieNode::new);
            insert_node(child, &segments[1..], value);
        }
        Segment::Param(name) => {
            if node.param_child.is_none() {
                node.param_child = Some((name.clone(), Box::new(TrieNode::new())));
            }
            if let Some((_, child)) = &mut node.param_child {
                insert_node(child, &segments[1..], value);
            }
        }
        Segment::Wildcard(name) => {
            // Wildcard must be last segment; store directly
            node.wildcard = Some((name.clone(), value));
        }
    }
}

fn lookup_node<'a, V>(
    node: &'a TrieNode<V>,
    parts: &[&str],
    params: &mut HashMap<String, String>,
) -> Option<&'a V> {
    if parts.is_empty() {
        if let Some(v) = &node.value {
            return Some(v);
        }
        // Allow wildcard to match empty remainder too
        if let Some((name, v)) = &node.wildcard {
            params.insert(name.clone(), String::new());
            return Some(v);
        }
        return None;
    }

    let head = parts[0];
    let tail = &parts[1..];

    // 1. Try literal match first (highest priority)
    if let Some(child) = node.literal_children.get(head) {
        let result = lookup_node(child, tail, params);
        if result.is_some() {
            return result;
        }
    }

    // 2. Try parameter match
    if let Some((name, child)) = &node.param_child {
        let saved = params.get(name).cloned();
        params.insert(name.clone(), head.to_string());
        let result = lookup_node(child, tail, params);
        if result.is_some() {
            return result;
        }
        // Backtrack
        match saved {
            Some(v) => {
                params.insert(name.clone(), v);
            }
            None => {
                params.remove(name);
            }
        }
    }

    // 3. Try wildcard (matches remaining path)
    if let Some((name, v)) = &node.wildcard {
        let wildcard_value = parts.join("/");
        params.insert(name.clone(), wildcard_value);
        return Some(v);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_route() {
        let mut trie = Trie::new();
        trie.insert("/users", "users-handler");
        trie.insert("/posts", "posts-handler");

        let (v, p) = trie.lookup("/users").unwrap();
        assert_eq!(*v, "users-handler");
        assert!(p.is_empty());

        assert!(trie.lookup("/missing").is_none());
    }

    #[test]
    fn test_param_route() {
        let mut trie = Trie::new();
        trie.insert("/users/:id", "user-handler");

        let (v, p) = trie.lookup("/users/42").unwrap();
        assert_eq!(*v, "user-handler");
        assert_eq!(p.get("id").unwrap(), "42");
    }

    #[test]
    fn test_wildcard_route() {
        let mut trie = Trie::new();
        trie.insert("/static/*path", "static-handler");

        let (v, p) = trie.lookup("/static/css/main.css").unwrap();
        assert_eq!(*v, "static-handler");
        assert_eq!(p.get("path").unwrap(), "css/main.css");
    }

    #[test]
    fn test_literal_priority_over_param() {
        let mut trie = Trie::new();
        trie.insert("/users/me", "me-handler");
        trie.insert("/users/:id", "id-handler");

        let (v, _) = trie.lookup("/users/me").unwrap();
        assert_eq!(*v, "me-handler");

        let (v, p) = trie.lookup("/users/123").unwrap();
        assert_eq!(*v, "id-handler");
        assert_eq!(p.get("id").unwrap(), "123");
    }

    #[test]
    fn test_nested_params() {
        let mut trie = Trie::new();
        trie.insert("/orgs/:org/repos/:repo", "repo-handler");

        let (v, p) = trie.lookup("/orgs/acme/repos/myrepo").unwrap();
        assert_eq!(*v, "repo-handler");
        assert_eq!(p.get("org").unwrap(), "acme");
        assert_eq!(p.get("repo").unwrap(), "myrepo");
    }

    #[test]
    fn test_root_path() {
        let mut trie = Trie::new();
        trie.insert("/", "root-handler");
        let (v, _) = trie.lookup("/").unwrap();
        assert_eq!(*v, "root-handler");
    }

    #[test]
    fn test_no_partial_match() {
        let mut trie = Trie::new();
        trie.insert("/users", "handler");
        // /users/extra should NOT match /users
        assert!(trie.lookup("/users/extra").is_none());
    }
}
