//! Private DAG validation and iterative dependent traversal.

use super::{validate_spec, GraphError, GraphLimits, NodeId, NodeSpec};
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) struct Dag {
    pub(super) nodes: Vec<NodeSpec>,
    pub(super) indices: HashMap<NodeId, usize>,
    pub(super) dependencies: Vec<Vec<usize>>,
    pub(super) dependents: Vec<Vec<usize>>,
}
impl Dag {
    pub(super) fn empty() -> Self {
        Self {
            nodes: vec![],
            indices: HashMap::new(),
            dependencies: vec![],
            dependents: vec![],
        }
    }
    pub(super) fn validate(nodes: Vec<NodeSpec>, limits: GraphLimits) -> Result<Self, GraphError> {
        if nodes.len() > limits.max_nodes {
            return Err(GraphError::Limit("nodes"));
        }
        let mut indices = HashMap::with_capacity(nodes.len());
        let mut edge_count = 0usize;
        for (i, node) in nodes.iter().enumerate() {
            validate_spec(node)?;
            edge_count = edge_count
                .checked_add(node.dependencies.len())
                .ok_or(GraphError::Limit("edges"))?;
            if edge_count > limits.max_edges {
                return Err(GraphError::Limit("edges"));
            }
            if indices.insert(node.id.clone(), i).is_some() {
                return Err(GraphError::DuplicateNode(node.id.clone()));
            }
        }
        let mut dependencies = vec![vec![]; nodes.len()];
        let mut dependents = vec![vec![]; nodes.len()];
        let mut indegree = vec![0usize; nodes.len()];
        for (i, node) in nodes.iter().enumerate() {
            let mut seen = HashSet::with_capacity(node.dependencies.len());
            for dep in &node.dependencies {
                let &d = indices
                    .get(dep)
                    .ok_or_else(|| GraphError::UnknownNode(dep.clone()))?;
                if !seen.insert(d) {
                    return Err(GraphError::DuplicateDependency(node.id.clone()));
                }
                dependencies[i].push(d);
                dependents[d].push(i);
                if dependents[d].len() > limits.max_fanout {
                    return Err(GraphError::Limit("fanout"));
                }
            }
            indegree[i] = dependencies[i].len();
        }
        let mut ready: VecDeque<_> = indegree
            .iter()
            .enumerate()
            .filter_map(|(i, &n)| (n == 0).then_some(i))
            .collect();
        let mut depth = vec![1usize; nodes.len()];
        let mut visited = 0;
        while let Some(i) = ready.pop_front() {
            visited += 1;
            if depth[i] > limits.max_depth {
                return Err(GraphError::Limit("depth"));
            }
            for &d in &dependents[i] {
                depth[d] = depth[d].max(depth[i] + 1);
                indegree[d] -= 1;
                if indegree[d] == 0 {
                    ready.push_back(d);
                }
            }
        }
        if visited != nodes.len() {
            return Err(GraphError::Cycle);
        }
        Ok(Self {
            nodes,
            indices,
            dependencies,
            dependents,
        })
    }
    /// The roots with every node connected to them through dependencies or dependents.
    pub(super) fn component(&self, roots: &[usize]) -> Vec<bool> {
        let mut seen = vec![false; self.nodes.len()];
        let mut work = Vec::with_capacity(roots.len());
        for &root in roots {
            if !seen[root] {
                seen[root] = true;
                work.push(root);
            }
        }
        while let Some(i) = work.pop() {
            for &d in self.dependencies[i].iter().chain(&self.dependents[i]) {
                if !seen[d] {
                    seen[d] = true;
                    work.push(d);
                }
            }
        }
        seen
    }
    pub(super) fn affected(&self, roots: &[usize]) -> Vec<usize> {
        let mut seen = vec![false; self.nodes.len()];
        let mut work = Vec::with_capacity(self.nodes.len());
        for &root in roots {
            if !seen[root] {
                seen[root] = true;
                work.push(root);
            }
        }
        let mut cursor = 0;
        while cursor < work.len() {
            let i = work[cursor];
            cursor += 1;
            for &d in &self.dependents[i] {
                if !seen[d] {
                    seen[d] = true;
                    work.push(d);
                }
            }
        }
        work
    }
}
