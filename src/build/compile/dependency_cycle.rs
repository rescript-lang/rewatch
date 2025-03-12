use super::super::build_types::*;
use crate::helpers;
use std::collections::{HashMap, VecDeque};

pub fn find(modules: &Vec<(&String, &Module)>) -> Vec<String> {
    // If a cycle was found, find the shortest cycle using BFS
    find_shortest_cycle(modules)
}

fn find_shortest_cycle(modules: &Vec<(&String, &Module)>) -> Vec<String> {
    let mut shortest_cycle: Vec<String> = Vec::new();

    // Build a graph representation for easier traversal
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for (name, module) in modules {
        let deps = module.deps.iter().cloned().collect();
        graph.insert(name.to_string(), deps);
    }

    // Try BFS from each node to find the shortest cycle
    for start_node in graph.keys() {
        let start = start_node.clone();
        if let Some(cycle) = find_cycle_bfs(&start, &graph) {
            if shortest_cycle.is_empty() || cycle.len() < shortest_cycle.len() {
                shortest_cycle = cycle;
            }
        }
    }

    shortest_cycle
}

fn find_cycle_bfs(start: &String, graph: &HashMap<String, Vec<String>>) -> Option<Vec<String>> {
    // Use a BFS to find the shortest cycle
    let mut queue = VecDeque::new();
    // Store node -> (distance, parent)
    let mut visited: HashMap<String, (usize, Option<String>)> = HashMap::new();

    // Initialize with start node
    visited.insert(start.clone(), (0, None));
    queue.push_back(start.clone());

    while let Some(current) = queue.pop_front() {
        let (dist, _) = *visited.get(&current).unwrap();

        // Check all neighbors
        if let Some(neighbors) = graph.get(&current) {
            for neighbor in neighbors {
                // If we found the start node again, we have a cycle
                if neighbor == start {
                    // Reconstruct the cycle
                    let mut path = Vec::new();
                    path.push(start.clone());

                    // Backtrack from current to start using parent pointers
                    let mut curr = current.clone();
                    while curr != *start {
                        path.push(curr.clone());
                        curr = visited.get(&curr).unwrap().1.clone().unwrap();
                    }

                    return Some(path);
                }

                // If not visited, add to queue
                if !visited.contains_key(neighbor) {
                    visited.insert(neighbor.clone(), (dist + 1, Some(current.clone())));
                    queue.push_back(neighbor.clone());
                }
            }
        }
    }

    None
}

pub fn format(cycle: &[String]) -> String {
    let mut cycle = cycle.to_vec();
    cycle.reverse();
    // add the first module to the end of the cycle
    cycle.push(cycle[0].clone());

    cycle
        .iter()
        .map(|s| helpers::format_namespaced_module_name(s))
        .collect::<Vec<String>>()
        .join("\n → ")
}
