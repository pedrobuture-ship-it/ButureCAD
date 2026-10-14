//! A* over the visibility graph of obstacle corners.

use crate::obstacle::ObstacleSet;
use buturecad_geometry::{BoundingBox, Point};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Upper bound on graph nodes (corners). Beyond this the search gives up instead of freezing
/// the UI; the caller reports it.
pub const MAX_NODES: usize = 2500;

#[derive(Debug, PartialEq, Eq)]
pub enum Search {
    Found(Vec<Point>),
    NoPath,
    TooComplex,
}

struct Open {
    f: f64,
    node: usize,
}
impl PartialEq for Open {
    fn eq(&self, o: &Self) -> bool {
        self.f.total_cmp(&o.f) == Ordering::Equal && self.node == o.node
    }
}
impl Eq for Open {}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Open {
    // Min-heap on f; node index breaks ties so the result is deterministic.
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f).then(o.node.cmp(&self.node))
    }
}

/// Shortest obstacle-free polyline from `from` to `to` through corners of obstacles inside
/// `corridor`. The caller guarantees `from` and `to` are not inside an obstacle.
pub fn shortest(set: &ObstacleSet, from: Point, to: Point, corridor: &BoundingBox) -> Search {
    let mut nodes: Vec<Point> = vec![from, to];
    for i in set.in_area(corridor) {
        for v in &set.obstacles()[i].hull {
            // A corner buried in another obstacle is not a place a track can go.
            if set.containing(*v).is_none() {
                nodes.push(*v);
            }
        }
        if nodes.len() > MAX_NODES {
            return Search::TooComplex;
        }
    }
    nodes.sort_unstable_by_key(|p| (p.x, p.y));
    nodes.dedup();
    let (src, dst) = match (
        nodes.binary_search_by_key(&(from.x, from.y), |p| (p.x, p.y)),
        nodes.binary_search_by_key(&(to.x, to.y), |p| (p.x, p.y)),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return Search::NoPath,
    };
    let dist = |a: usize, b: usize| nodes[a].distance(nodes[b]);
    let mut g = vec![f64::INFINITY; nodes.len()];
    let mut prev: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut done = vec![false; nodes.len()];
    let mut heap = BinaryHeap::new();
    g[src] = 0.0;
    heap.push(Open {
        f: dist(src, dst),
        node: src,
    });
    while let Some(Open { node, .. }) = heap.pop() {
        if done[node] {
            continue;
        }
        done[node] = true;
        if node == dst {
            let mut path = vec![nodes[dst]];
            let mut cur = dst;
            while let Some(p) = prev[cur] {
                path.push(nodes[p]);
                cur = p;
            }
            path.reverse();
            return Search::Found(path);
        }
        for next in 0..nodes.len() {
            if done[next] {
                continue;
            }
            let cost = g[node] + dist(node, next);
            if cost >= g[next] {
                continue;
            }
            if set.blocking(nodes[node], nodes[next]).is_some() {
                continue;
            }
            g[next] = cost;
            prev[next] = Some(node);
            heap.push(Open {
                f: cost + dist(next, dst),
                node: next,
            });
        }
    }
    Search::NoPath
}
