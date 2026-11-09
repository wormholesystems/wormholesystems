//! The chain laid out as a tree, the way the website's automatic placement draws it: a port of
//! `frontend/src/lib/map/tree.ts`, `computeTreeLayout` and `compareForTree`. Change both
//! together, or the card and the map put the systems in different places.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};

/// What the layout reads off the chain.
pub(crate) struct Input<'a> {
    pub node_ids: Vec<i64>,
    pub edges: Vec<(i64, i64)>,
    /// Pinned systems, which become the roots the branches grow out of.
    pub root_ids: Vec<i64>,
    /// A root like the pinned ones, and the one the column starts with.
    pub home_id: Option<i64>,
    /// Orders siblings, and the separate trees, down the cross axis.
    pub compare: &'a dyn Fn(i64, i64) -> Ordering,
}

/// A node in the spanning forest, plus the scratch fields the walks mutate. Links are
/// indices into the arena.
#[derive(Default)]
struct Node {
    id: i64,
    depth: usize,
    parent: Option<usize>,
    children: Vec<usize>,
    /// 1-based position among siblings.
    sibling_index: usize,
    prelim: f64,
    modifier: f64,
    change: f64,
    shift: f64,
    thread: Option<usize>,
    ancestor: Option<usize>,
    cross: f64,
}

struct Walk {
    nodes: Vec<Node>,
    sibling_gap: f64,
}

impl Walk {
    fn next_left(&self, v: usize) -> Option<usize> {
        self.nodes[v]
            .children
            .first()
            .copied()
            .or(self.nodes[v].thread)
    }

    fn next_right(&self, v: usize) -> Option<usize> {
        self.nodes[v]
            .children
            .last()
            .copied()
            .or(self.nodes[v].thread)
    }

    fn left_sibling(&self, v: usize) -> Option<usize> {
        let node = &self.nodes[v];
        let parent = node.parent?;
        (node.sibling_index > 1).then(|| self.nodes[parent].children[node.sibling_index - 2])
    }

    fn move_subtree(&mut self, left: usize, right: usize, distance: f64) {
        let subtrees =
            count(self.nodes[right].sibling_index) - count(self.nodes[left].sibling_index);
        self.nodes[right].change -= distance / subtrees;
        self.nodes[right].shift += distance;
        self.nodes[left].change += distance / subtrees;
        self.nodes[right].prelim += distance;
        self.nodes[right].modifier += distance;
    }

    fn ancestor_of(&self, v: usize) -> usize {
        self.nodes[v].ancestor.unwrap_or(v)
    }

    fn ancestor_for(&self, inner: usize, v: usize, fallback: usize) -> usize {
        let a = self.ancestor_of(inner);
        if self.nodes[a].parent == self.nodes[v].parent {
            a
        } else {
            fallback
        }
    }

    fn execute_shifts(&mut self, v: usize) {
        let mut shift = 0.0;
        let mut change = 0.0;
        for &child in self.nodes[v].children.clone().iter().rev() {
            let c = &mut self.nodes[child];
            c.prelim += shift;
            c.modifier += shift;
            change += c.change;
            shift += c.shift + change;
        }
    }

    fn apportion(&mut self, v: usize, mut default_ancestor: usize) -> usize {
        let Some(left_sibling) = self.left_sibling(v) else {
            return default_ancestor;
        };
        let parent = self.nodes[v].parent.unwrap_or(v);
        let mut inner_right = v;
        let mut outer_right = v;
        let mut inner_left = left_sibling;
        let mut outer_left = self.nodes[parent].children[0];
        let mut s_inner_right = self.nodes[inner_right].modifier;
        let mut s_outer_right = self.nodes[outer_right].modifier;
        let mut s_inner_left = self.nodes[inner_left].modifier;
        let mut s_outer_left = self.nodes[outer_left].modifier;

        while let (Some(il), Some(ir)) = (self.next_right(inner_left), self.next_left(inner_right))
        {
            inner_left = il;
            inner_right = ir;
            outer_left = self.next_left(outer_left).unwrap_or(outer_left);
            outer_right = self.next_right(outer_right).unwrap_or(outer_right);
            self.nodes[outer_right].ancestor = Some(v);
            let shift = self.nodes[inner_left].prelim + s_inner_left
                - (self.nodes[inner_right].prelim + s_inner_right)
                + self.sibling_gap;
            if shift > 0.0 {
                let from = self.ancestor_for(inner_left, v, default_ancestor);
                self.move_subtree(from, v, shift);
                s_inner_right += shift;
                s_outer_right += shift;
            }
            s_inner_left += self.nodes[inner_left].modifier;
            s_inner_right += self.nodes[inner_right].modifier;
            s_outer_left += self.nodes[outer_left].modifier;
            s_outer_right += self.nodes[outer_right].modifier;
        }
        if self.next_right(inner_left).is_some() && self.next_right(outer_right).is_none() {
            self.nodes[outer_right].thread = self.next_right(inner_left);
            self.nodes[outer_right].modifier += s_inner_left - s_outer_right;
        } else if self.next_left(inner_right).is_some() && self.next_left(outer_left).is_none() {
            self.nodes[outer_left].thread = self.next_left(inner_right);
            self.nodes[outer_left].modifier += s_inner_right - s_outer_left;
            default_ancestor = v;
        }
        default_ancestor
    }

    fn first_walk(&mut self, v: usize) {
        let children = self.nodes[v].children.clone();
        if children.is_empty() {
            self.nodes[v].prelim = self
                .left_sibling(v)
                .map_or(0.0, |l| self.nodes[l].prelim + self.sibling_gap);
            return;
        }
        let mut default_ancestor = children[0];
        for &child in &children {
            self.first_walk(child);
            default_ancestor = self.apportion(child, default_ancestor);
        }
        self.execute_shifts(v);
        let first = self.nodes[children[0]].prelim;
        let last = self.nodes[children[children.len() - 1]].prelim;
        let midpoint = f64::midpoint(first, last);
        if let Some(l) = self.left_sibling(v) {
            self.nodes[v].prelim = self.nodes[l].prelim + self.sibling_gap;
            self.nodes[v].modifier = self.nodes[v].prelim - midpoint;
        } else {
            self.nodes[v].prelim = midpoint;
        }
    }

    fn second_walk(&mut self, v: usize, mod_sum: f64) {
        self.nodes[v].cross = self.nodes[v].prelim + mod_sum;
        let next = mod_sum + self.nodes[v].modifier;
        for child in self.nodes[v].children.clone() {
            self.second_walk(child, next);
        }
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "sibling counts and depths are a handful"
)]
fn count(i: usize) -> f64 {
    i as f64
}

/// JavaScript's `Math.round`: halves go up, also below zero.
fn round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// The spanning forest: each node's depth and children, siblings in order, and the roots in
/// order.
struct Forest {
    depth_of: HashMap<i64, usize>,
    children_of: HashMap<i64, Vec<i64>>,
    roots: Vec<i64>,
}

/// The breadth-first walk carving the forest out of the graph.
struct Search<'a> {
    adjacency: &'a HashMap<i64, Vec<i64>>,
    depth_of: HashMap<i64, usize>,
    children_of: HashMap<i64, Vec<i64>>,
    visited: HashSet<i64>,
    roots: Vec<i64>,
    queue: VecDeque<i64>,
}

impl Search<'_> {
    fn seed(&mut self, root: i64) -> bool {
        if !self.visited.insert(root) {
            return false;
        }
        self.roots.push(root);
        self.depth_of.insert(root, 0);
        self.queue.push_back(root);
        true
    }

    fn drain(&mut self) {
        while let Some(current) = self.queue.pop_front() {
            for &neighbour in self.adjacency.get(&current).into_iter().flatten() {
                if !self.visited.insert(neighbour) {
                    continue;
                }
                let depth = self.depth_of.get(&current).copied().unwrap_or(0) + 1;
                self.depth_of.insert(neighbour, depth);
                self.children_of.entry(current).or_default().push(neighbour);
                self.queue.push_back(neighbour);
            }
        }
    }
}

/// Rooted at home and the pinned systems, everything else attaching to whichever root reaches
/// it first; whatever they cannot reach a tree of its own, densest first.
fn forest(input: &Input<'_>) -> Forest {
    // The undirected graph of systems, neighbours in the order the edges name them.
    let mut adjacency: HashMap<i64, Vec<i64>> =
        input.node_ids.iter().map(|&id| (id, Vec::new())).collect();
    for &(from, to) in &input.edges {
        if from == to || !adjacency.contains_key(&from) || !adjacency.contains_key(&to) {
            continue;
        }
        adjacency.entry(from).or_default().push(to);
        adjacency.entry(to).or_default().push(from);
    }
    for neighbours in adjacency.values_mut() {
        let mut seen = HashSet::new();
        neighbours.retain(|n| seen.insert(*n));
    }

    let mut search = Search {
        adjacency: &adjacency,
        depth_of: HashMap::new(),
        children_of: input.node_ids.iter().map(|&id| (id, Vec::new())).collect(),
        visited: HashSet::new(),
        roots: Vec::new(),
        queue: VecDeque::new(),
    };
    let home = input.home_id.filter(|id| adjacency.contains_key(id));
    let mut candidates: Vec<i64> = home.into_iter().collect();
    candidates.extend(
        input
            .root_ids
            .iter()
            .copied()
            .filter(|id| adjacency.contains_key(id) && Some(*id) != home),
    );
    // All roots seeded before the walk, so each system attaches to the nearest one.
    let chosen: HashSet<i64> = candidates.into_iter().filter(|&r| search.seed(r)).collect();
    search.drain();

    let mut stranded: Vec<i64> = input
        .node_ids
        .iter()
        .copied()
        .filter(|id| !search.visited.contains(id))
        .collect();
    stranded.sort_by(|a, b| adjacency[b].len().cmp(&adjacency[a].len()).then(a.cmp(b)));
    for root in stranded {
        if search.seed(root) {
            search.drain();
        }
    }

    let Search {
        depth_of,
        mut children_of,
        mut roots,
        ..
    } = search;
    for children in children_of.values_mut() {
        children.sort_by(|a, b| (input.compare)(*a, *b));
    }
    // Home, then pinned, then unreached; the comparator only orders within each rank.
    let rank = |id: i64| {
        if Some(id) == home {
            0
        } else if chosen.contains(&id) {
            1
        } else {
            2
        }
    };
    roots.sort_by(|a, b| rank(*a).cmp(&rank(*b)).then((input.compare)(*a, *b)));
    Forest {
        depth_of,
        children_of,
        roots,
    }
}

/// A left-to-right spanning forest rooted at the pinned systems, each node's top-left in world
/// units, snapped to the grid.
pub(crate) fn layout(input: &Input<'_>, grid_size: f64) -> HashMap<i64, (f64, f64)> {
    let snap = |v: f64| round(v / grid_size) * grid_size;
    let cross_step = grid_size / 2.0;
    let snap_cross = |v: f64| round(v / cross_step) * cross_step;
    let level_gap = snap(320.0);
    let sibling_gap = grid_size.max(snap(60.0));
    let margin_x = snap(20.0);
    let margin_y = snap(20.0);

    let Forest {
        depth_of,
        children_of,
        roots,
    } = forest(input);

    // The forest as an arena the walks can chew on, the super-root last.
    let index: HashMap<i64, usize> = input
        .node_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();
    let mut nodes: Vec<Node> = input
        .node_ids
        .iter()
        .map(|&id| Node {
            id,
            depth: depth_of.get(&id).copied().unwrap_or(0),
            sibling_index: 1,
            ..Node::default()
        })
        .collect();
    for (&id, child_ids) in &children_of {
        let Some(&parent) = index.get(&id) else {
            continue;
        };
        let children: Vec<usize> = child_ids
            .iter()
            .filter_map(|c| index.get(c).copied())
            .collect();
        for (i, &child) in children.iter().enumerate() {
            nodes[child].parent = Some(parent);
            nodes[child].sibling_index = i + 1;
        }
        nodes[parent].children = children;
    }
    // A virtual super-root packs the trees like any other siblings, so a shallow tree rises
    // into a deeper neighbour's empty rows. Never drawn.
    let super_root = nodes.len();
    let root_children: Vec<usize> = roots.iter().filter_map(|r| index.get(r).copied()).collect();
    for (i, &child) in root_children.iter().enumerate() {
        nodes[child].parent = Some(super_root);
        nodes[child].sibling_index = i + 1;
    }
    nodes.push(Node {
        id: -1,
        children: root_children,
        sibling_index: 1,
        ..Node::default()
    });

    let mut walk = Walk { nodes, sibling_gap };
    walk.first_walk(super_root);
    walk.second_walk(super_root, 0.0);
    walk.nodes.pop();

    let min_cross = walk
        .nodes
        .iter()
        .map(|n| n.cross)
        .fold(f64::INFINITY, f64::min);
    let min_cross = if min_cross.is_finite() {
        min_cross
    } else {
        0.0
    };
    walk.nodes
        .iter()
        .map(|n| {
            (
                n.id,
                (
                    snap(margin_x + count(n.depth) * level_gap),
                    snap_cross(margin_y + n.cross - min_cross),
                ),
            )
        })
        .collect()
}

/// Where a class sits among siblings: high, low and null sec, Pochven, then wormhole space by
/// class, anything unknown last. `classMeta(...).sortWeight` on the website.
pub(crate) fn class_rank(class: Option<i64>, security: Option<f64>) -> f64 {
    if class.is_none() && security.is_none() {
        return 99.0;
    }
    let class =
        class.unwrap_or_else(|| crate::classes::class_from_security(security.unwrap_or(0.0)));
    match class {
        1..=6 | 12..=23 => count(usize::try_from(class).unwrap_or(0)) + 10.0,
        7 => 0.0,
        8 => 1.0,
        9 => 2.0,
        25 => 3.0,
        _ => 99.0,
    }
}

/// `localeCompare` closely enough for system names and aliases: case and accents only break
/// ties, punctuation sorts before digits and digits before letters.
pub(crate) fn locale_cmp(a: &str, b: &str) -> Ordering {
    let key = |c: char| {
        let group = if c.is_alphabetic() {
            2
        } else {
            u8::from(c.is_numeric())
        };
        (group, c.to_lowercase().next().unwrap_or(c))
    };
    let primary = a.chars().map(key).cmp(b.chars().map(key));
    // Lower case before upper, as the browser's collation has it.
    primary.then_with(|| {
        a.chars()
            .map(char::is_uppercase)
            .cmp(b.chars().map(char::is_uppercase))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by_id(a: i64, b: i64) -> Ordering {
        a.cmp(&b)
    }

    /// The website's own test shape: a root with two children, one of which has two of its
    /// own, comes out one level per column and the parent level with its children.
    #[test]
    fn a_parent_sits_between_its_children() {
        let input = Input {
            node_ids: vec![1, 2, 3, 4, 5],
            edges: vec![(1, 2), (1, 3), (2, 4), (2, 5)],
            root_ids: Vec::new(),
            home_id: Some(1),
            compare: &by_id,
        };
        let at = layout(&input, 20.0);
        assert_eq!(at[&1].0, 20.0);
        assert_eq!(at[&2].0, 340.0);
        assert_eq!(at[&4].0, 660.0);
        // 4 and 5 a sibling gap apart, 2 halfway between them.
        assert_eq!(at[&5].1 - at[&4].1, 60.0);
        assert_eq!(at[&2].1, f64::midpoint(at[&4].1, at[&5].1));
        assert!(at[&3].1 > at[&2].1);
        assert_eq!(at[&4].1, 20.0);
    }

    #[test]
    fn an_island_gets_a_tree_of_its_own() {
        let input = Input {
            node_ids: vec![1, 2, 3, 4],
            edges: vec![(1, 2), (3, 4)],
            root_ids: Vec::new(),
            home_id: Some(1),
            compare: &by_id,
        };
        let at = layout(&input, 20.0);
        assert_eq!(at[&3].0, 20.0);
        assert!(at[&3].1 > at[&1].1);
    }

    #[test]
    fn names_compare_like_the_browser() {
        assert_eq!(locale_cmp("amarr", "Jita"), Ordering::Less);
        assert_eq!(locale_cmp("J123", "Jita"), Ordering::Less);
        assert_eq!(locale_cmp("a", "A"), Ordering::Less);
        assert_eq!(locale_cmp("5ZXX-K", "Amarr"), Ordering::Less);
    }
}
