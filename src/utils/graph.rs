use std::collections::HashSet;

type Cycle<T> = Vec<T>;

#[derive(Debug)]
pub struct Graph<T>
where
    T: Eq + std::hash::Hash + Copy,
{
    adjacency: indexmap::IndexMap<T, Vec<T>>,
}

impl<T> Graph<T>
where
    T: Eq + std::hash::Hash + Copy,
{
    pub fn new() -> Self {
        Self {
            adjacency: indexmap::IndexMap::new(),
        }
    }

    pub fn add_vertex(&mut self, vertex: T) {
        self.adjacency.entry(vertex).or_default();
    }

    pub fn add_edge(&mut self, from: T, to: T) {
        self.adjacency.entry(from).or_default().push(to);
        self.adjacency.entry(to).or_default();
    }

    /// Performs a **locally stable topological sort** of the graph.
    ///
    /// Note:
    /// - The order of vertices in the returned `Vec` preserves the insertion order
    ///   of independent nodes wherever possible.
    /// - To maintain this stability, edges must be added **from the dependent node to the dependency**
    ///   (i.e., opposite to the conventional direction used in standard topological sort algorithms).
    ///
    /// Returns:
    /// - `Ok(sorted)` if topological sort is successful.
    /// - `Err(cycle)` if a cycle is detected in the graph.
    ///
    /// # Example
    /// ```rust,ignore
    /// use mepris::graph::Graph;
    /// let mut graph = Graph::new();
    /// graph.add_vertex(1);
    /// graph.add_vertex(2);
    /// graph.add_vertex(3);
    /// graph.add_edge(2, 3); // 2 depends on 3
    /// let sorted = graph.stable_toposort().unwrap(); // 1 3 2
    /// ```
    pub fn stable_toposort(&self) -> Result<Vec<T>, Cycle<T>> {
        let mut visited = HashSet::new();
        let mut path = Vec::new();
        let mut result = Vec::new();

        for &node in self.adjacency.keys() {
            if !visited.contains(&node) {
                Self::visit(node, &self.adjacency, &mut visited, &mut path, &mut result)?;
            }
        }

        Ok(result)
    }

    fn visit(
        node: T,
        adjacency: &indexmap::IndexMap<T, Vec<T>>,
        visited: &mut HashSet<T>,
        path: &mut Vec<T>,
        result: &mut Vec<T>,
    ) -> Result<(), Vec<T>> {
        if visited.contains(&node) {
            return Ok(());
        }
        if path.contains(&node) {
            path.push(node);
            return Err(path.clone());
        }

        path.push(node);

        if let Some(neighbors) = adjacency.get(&node) {
            for &neighbor in neighbors {
                Self::visit(neighbor, adjacency, visited, path, result)?;
            }
        }

        path.pop();
        visited.insert(node);
        result.push(node);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toposort_simple_dependency() {
        let mut g = Graph::new();
        g.add_vertex(1);
        g.add_vertex(2);
        g.add_vertex(3);

        g.add_edge(2, 3); // 2 depends on 3

        let sorted = g.stable_toposort().unwrap();
        assert_eq!(sorted, vec![1, 3, 2]);
    }

    #[test]
    fn toposort_multiple_dependencies() {
        let mut g = Graph::new();

        g.add_vertex(1);
        g.add_vertex(2);
        g.add_vertex(3);
        g.add_vertex(4);

        g.add_edge(3, 4); // 3 depends on 4
        g.add_edge(2, 3); // 2 depends on 3

        let sorted = g.stable_toposort().unwrap();

        assert_eq!(sorted, vec![1, 4, 3, 2]);
    }

    #[test]
    fn preserves_insertion_order_for_independent_nodes() {
        let mut g = Graph::new();

        g.add_vertex(1);
        g.add_vertex(2);
        g.add_vertex(3);

        let sorted = g.stable_toposort().unwrap();

        assert_eq!(sorted, vec![1, 2, 3]);
    }

    #[test]
    fn detects_cycle() {
        let mut g = Graph::new();

        g.add_vertex(1);
        g.add_vertex(2);
        g.add_vertex(3);

        g.add_edge(1, 2);
        g.add_edge(2, 3);
        g.add_edge(3, 1); // cycle

        let result = g.stable_toposort();

        assert!(result.is_err());

        let cycle = result.unwrap_err();
        assert!(cycle.len() >= 2);
    }

    #[test]
    fn empty_graph() {
        let g: Graph<i32> = Graph::new();

        let sorted = g.stable_toposort().unwrap();

        assert!(sorted.is_empty());
    }

    #[test]
    fn single_node() {
        let mut g = Graph::new();

        g.add_vertex(42);

        let sorted = g.stable_toposort().unwrap();

        assert_eq!(sorted, vec![42]);
    }
}
