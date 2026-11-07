// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries ADMesh code: deps_src/admesh/connect.cpp of OrcaSlicer v2.4.2 (HashEdge,
// HashTableEdges, stl_check_facets_exact, stl_check_facets_nearby, stl_remove_unconnected_facets,
// stl_fill_holes, stl_add_facet), translated on 2026-11-07. Copyright (C) 1995, 1996 Anthony D.
// Martin and (C) 2013, 2014 the ADMesh contributors, with changes by the PrusaSlicer and OrcaSlicer
// authors, under the GNU General Public License, version 2 or later (GPL-2.0-or-later;
// licenses/GPL-2.0.txt), taken here under version 3 (licenses/GPL-3.0.txt) as section 13 of the
// AGPL-3.0 allows.

//! The facet connectivity: which facets share an edge.
//!
//! Each edge is a key of six 32-bit words, its two vertices in a fixed order (the lower first;
//! `which_edge` gains 3 when the facet runs the edge the other way), in a chained hash table of a
//! prime size about six times the facet count. Inserting an edge whose key a chained edge of
//! another facet has makes the two facets neighbours and takes that edge out of the chain;
//! otherwise the edge joins the end of its chain. So each edge pairs with the first unpaired one
//! of the same key, in the order the facets are visited, as the C++ pairs them.
//!
//! - The exact check keys an edge by the bits of its two `float` vertices (-0 made +0), after
//!   removing every facet with two equal vertices (the last facet moved into its place).
//! - The nearby check keys the edges still without a neighbour by the grid cells (`tolerance`
//!   wide, from the bounding box's minimum) of their vertices, and moves the vertices of each
//!   pair it joins onto each other, walking the fan of facets around each moved vertex.
//! - `stl_remove_unconnected_facets` collapses the facets the nearby check made degenerate and
//!   removes those left with no neighbour.
//!
//! The C++ table is an array of `M` chain heads, allocated whole; here the heads are allocated on
//! the first insertion, as a table that receives none (no facets left) is never read. With no
//! facets the C++ allocates its largest size, 1610612741 heads (`parity/HAZARDS.md`, orca-mesh).

use orca_eigen::Vector;

use super::{StlFacet, StlFile, StlNeighbors, StlVertex, max3};
use crate::log::{Severity, log};

/// `HashEdge`: an edge's key and the facet edge it belongs to.
#[derive(Clone, Copy, Debug)]
struct HashEdge {
    /// `key[6]`: the vertices' bits, the lower vertex first.
    key: [u32; 6],
    /// `facet_number`.
    facet_number: i32,
    /// `which_edge`: the edge's index in its facet, plus 3 when stored backwards.
    which_edge: i32,
}

impl HashEdge {
    fn new(facet_number: i32, which_edge: i32) -> Self {
        HashEdge {
            key: [0; 6],
            facet_number,
            which_edge,
        }
    }

    /// `hash(M)`: `((k0 / 11 + k1 / 7 + k2 / 3) ^ (k3 / 11 + k4 / 7 + k5 / 3)) % M` in `uint32_t`.
    fn hash(&self, m: u32) -> usize {
        let k = &self.key;
        let a = (k[0] / 11).wrapping_add(k[1] / 7).wrapping_add(k[2] / 3);
        let b = (k[3] / 11).wrapping_add(k[4] / 7).wrapping_add(k[5] / 3);
        ((a ^ b) % m) as usize
    }

    /// `load_exact(stl, a, b)`: updates `shortest_edge`, then keys the edge by the bits of its
    /// vertices.
    fn load_exact(&mut self, stl: &mut StlFile, a: &StlVertex, b: &StlVertex) {
        let diff = (*a - *b).cwise_abs();
        let max_diff = max3(diff);
        stl.stats.shortest_edge = orca_stl::min(max_diff, stl.stats.shortest_edge);

        let (a, b) = if vertex_lower(a, b) {
            (a, b)
        } else {
            self.which_edge += 3;
            (b, a)
        };
        for i in 0..3 {
            self.key[i] = a[i].to_bits();
            self.key[3 + i] = b[i].to_bits();
        }
        // Negative zeros made positive, so that equal edges compare equal byte for byte.
        for k in &mut self.key {
            if *k == 0x8000_0000 {
                *k = 0;
            }
        }
    }

    /// `load_nearby(stl, a, b, tolerance)`: keys the edge by the grid cells of its vertices;
    /// false when both fall in one cell.
    fn load_nearby(&mut self, stl: &StlFile, a: &StlVertex, b: &StlVertex, tolerance: f32) -> bool {
        let cell = |v: &StlVertex| -> Vector<i32, 3> { ((*v - stl.stats.min) / tolerance).cast() };
        let vertex1 = cell(a);
        let vertex2 = cell(b);
        if vertex1 == vertex2 {
            return false;
        }
        let lower = if vertex1[0] != vertex2[0] {
            vertex1[0] < vertex2[0]
        } else if vertex1[1] != vertex2[1] {
            vertex1[1] < vertex2[1]
        } else {
            vertex1[2] < vertex2[2]
        };
        let (first, second) = if lower {
            (vertex1, vertex2)
        } else {
            self.which_edge += 3;
            (vertex2, vertex1)
        };
        for i in 0..3 {
            self.key[i] = first[i] as u32;
            self.key[3 + i] = second[i] as u32;
        }
        true
    }
}

/// `vertex_lower(a, b)`: lexicographic `<` of the coordinates.
fn vertex_lower(a: &StlVertex, b: &StlVertex) -> bool {
    if a[0] != b[0] {
        a[0] < b[0]
    } else if a[1] != b[1] {
        a[1] < b[1]
    } else {
        a[2] < b[2]
    }
}

/// `edges_equal`: edges of different facets with equal keys.
fn edges_equal(a: &HashEdge, b: &HashEdge) -> bool {
    a.facet_number != b.facet_number && a.key == b.key
}

/// The chains' end (`tail`).
const TAIL: u32 = u32::MAX;

/// The primes of `hash_size_from_nr_faces`.
const PRIMES: [u32; 15] = [
    98317, 196613, 393241, 786433, 1572869, 3145739, 6291469, 12582917, 25165843, 50331653,
    100663319, 201326611, 402653189, 805306457, 1610612741,
];

/// `hash_size_from_nr_faces`: the first prime above `nr_faces * 6 - 1` (in `size_t`, so no facets
/// gives the largest), else the largest.
fn hash_size_from_nr_faces(nr_faces: usize) -> u32 {
    let wanted = (nr_faces as u64).wrapping_mul(6).wrapping_sub(1);
    PRIMES
        .iter()
        .copied()
        .find(|&p| wanted < u64::from(p))
        .unwrap_or(PRIMES[PRIMES.len() - 1])
}

/// What an insertion does with a pair it finds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Match {
    /// `record_neighbors`.
    Exact,
    /// `match_neighbors_nearby`.
    Nearby,
}

/// `HashTableEdges`: the chains, as indices into a pool of edges.
struct HashTableEdges {
    m: u32,
    heads: Vec<u32>,
    pool: Vec<(HashEdge, u32)>,
}

impl HashTableEdges {
    fn new(number_of_faces: usize) -> Self {
        HashTableEdges {
            m: hash_size_from_nr_faces(number_of_faces),
            heads: Vec::new(),
            pool: Vec::new(),
        }
    }

    fn push(&mut self, edge: HashEdge, next: u32) -> u32 {
        let id = u32::try_from(self.pool.len()).expect("edge pool past u32");
        self.pool.push((edge, next));
        id
    }

    /// `insert_edge(stl, edge, match_neighbors)`.
    fn insert_edge(&mut self, stl: &mut StlFile, edge: &HashEdge, how: Match) {
        if self.heads.is_empty() {
            self.heads = vec![TAIL; self.m as usize];
        }
        let chain_number = edge.hash(self.m);
        let link = self.heads[chain_number];
        if link == TAIL {
            let new_edge = self.push(*edge, TAIL);
            self.heads[chain_number] = new_edge;
        } else if edges_equal(edge, &self.pool[link as usize].0) {
            let other = self.pool[link as usize].0;
            match_neighbors(stl, edge, &other, how);
            self.heads[chain_number] = self.pool[link as usize].1;
        } else {
            let mut link = link;
            loop {
                let next = self.pool[link as usize].1;
                if next == TAIL {
                    let new_edge = self.push(*edge, TAIL);
                    self.pool[link as usize].1 = new_edge;
                    break;
                }
                if edges_equal(edge, &self.pool[next as usize].0) {
                    let other = self.pool[next as usize].0;
                    match_neighbors(stl, edge, &other, how);
                    self.pool[link as usize].1 = self.pool[next as usize].1;
                    break;
                }
                link = next;
            }
        }
    }
}

fn match_neighbors(stl: &mut StlFile, a: &HashEdge, b: &HashEdge, how: Match) {
    match how {
        Match::Exact => record_neighbors(stl, a, b),
        Match::Nearby => match_neighbors_nearby(stl, a, b),
    }
}

/// `++ connected_facets_N_edge` for the facet's neighbour count.
fn count_connects(stl: &mut StlFile, facet: i32) {
    match stl.neighbors_start[facet as usize].num_neighbors() {
        1 => stl.stats.connected_facets_1_edge += 1,
        2 => stl.stats.connected_facets_2_edge += 1,
        3 => stl.stats.connected_facets_3_edge += 1,
        _ => {}
    }
}

/// `record_neighbors(stl, edge_a, edge_b)`: makes the two facets neighbours over the edge.
fn record_neighbors(stl: &mut StlFile, edge_a: &HashEdge, edge_b: &HashEdge) {
    let (fa, ea) = (
        edge_a.facet_number as usize,
        (edge_a.which_edge % 3) as usize,
    );
    let (fb, eb) = (
        edge_b.facet_number as usize,
        (edge_b.which_edge % 3) as usize,
    );
    stl.neighbors_start[fa].neighbor[ea] = edge_b.facet_number;
    stl.neighbors_start[fa].which_vertex_not[ea] = ((edge_b.which_edge + 2) % 3) as i8;
    stl.neighbors_start[fb].neighbor[eb] = edge_a.facet_number;
    stl.neighbors_start[fb].which_vertex_not[eb] = ((edge_a.which_edge + 2) % 3) as i8;

    if (edge_a.which_edge < 3 && edge_b.which_edge < 3)
        || (edge_a.which_edge > 2 && edge_b.which_edge > 2)
    {
        stl.neighbors_start[fa].which_vertex_not[ea] += 3;
        stl.neighbors_start[fb].which_vertex_not[eb] += 3;
    }

    stl.stats.connected_edges += 2;
    count_connects(stl, edge_a.facet_number);
    count_connects(stl, edge_b.facet_number);
}

/// `change_vertices` of `match_neighbors_nearby`: moves the vertex onto `new_vertex` in each facet
/// of its fan.
fn change_vertices(stl: &mut StlFile, mut facet_num: i32, mut vnot: i32, new_vertex: StlVertex) {
    let first_facet = facet_num;
    let mut direction = false;
    loop {
        let pivot_vertex;
        let next_edge;
        if vnot > 2 {
            if direction {
                pivot_vertex = (vnot + 1) % 3;
                next_edge = vnot % 3;
            } else {
                pivot_vertex = (vnot + 2) % 3;
                next_edge = pivot_vertex;
            }
            direction = !direction;
        } else if direction {
            pivot_vertex = (vnot + 2) % 3;
            next_edge = pivot_vertex;
        } else {
            pivot_vertex = (vnot + 1) % 3;
            next_edge = vnot;
        }
        stl.facet_start[facet_num as usize].vertex[pivot_vertex as usize] = new_vertex;
        let neighbors = stl.neighbors_start[facet_num as usize];
        vnot = i32::from(neighbors.which_vertex_not[next_edge as usize]);
        facet_num = neighbors.neighbor[next_edge as usize];
        if facet_num == -1 {
            break;
        }
        if facet_num == first_facet {
            log(
                Severity::Info,
                "Back to the first facet changing vertices: probably a mobius part. Try using a \
                 smaller tolerance or don't do a nearby check.",
            );
            return;
        }
    }
}

/// `match_neighbors_nearby(stl, edge_a, edge_b)`: records the pair, then moves the vertices that
/// differ.
fn match_neighbors_nearby(stl: &mut StlFile, edge_a: &HashEdge, edge_b: &HashEdge) {
    record_neighbors(stl, edge_a, edge_b);

    let (fa, fb) = (edge_a.facet_number, edge_b.facet_number);
    let mut facet1 = -1;
    let mut facet2 = -1;
    let mut vertex1 = 0;
    let mut vertex2 = 0;
    let mut new_vertex1 = StlVertex::zero();
    let mut new_vertex2 = StlVertex::zero();
    {
        let (v1a, v2a) = if edge_a.which_edge < 3 {
            (edge_a.which_edge, (edge_a.which_edge + 1) % 3)
        } else {
            ((edge_a.which_edge + 1) % 3, edge_a.which_edge % 3)
        };
        let (v1b, v2b) = if edge_b.which_edge < 3 {
            (edge_b.which_edge, (edge_b.which_edge + 1) % 3)
        } else {
            ((edge_b.which_edge + 1) % 3, edge_b.which_edge % 3)
        };
        let vertex_of =
            |stl: &StlFile, f: i32, v: i32| stl.facet_start[f as usize].vertex[v as usize];
        let free = |stl: &StlFile, f: i32, v: i32| {
            let n = &stl.neighbors_start[f as usize];
            n.neighbor[v as usize] == -1 && n.neighbor[((v + 2) % 3) as usize] == -1
        };

        if vertex_of(stl, fa, v1a) != vertex_of(stl, fb, v1b) {
            if free(stl, fa, v1a) {
                facet1 = fa;
                vertex1 = v1a;
                new_vertex1 = vertex_of(stl, fb, v1b);
            } else {
                facet1 = fb;
                vertex1 = v1b;
                new_vertex1 = vertex_of(stl, fa, v1a);
            }
        }

        if vertex_of(stl, fa, v2a) != vertex_of(stl, fb, v2b) {
            if free(stl, fa, v2a) {
                facet2 = fa;
                vertex2 = v2a;
                new_vertex2 = vertex_of(stl, fb, v2b);
            } else {
                facet2 = fb;
                vertex2 = v2b;
                new_vertex2 = vertex_of(stl, fa, v2a);
            }
        }
    }

    if facet1 != -1 {
        let mut vnot1 = if facet1 == fa {
            (edge_a.which_edge + 2) % 3
        } else {
            (edge_b.which_edge + 2) % 3
        };
        if (vnot1 + 2) % 3 == vertex1 {
            vnot1 += 3;
        }
        change_vertices(stl, facet1, vnot1, new_vertex1);
    }
    if facet2 != -1 {
        let mut vnot2 = if facet2 == fa {
            (edge_a.which_edge + 2) % 3
        } else {
            (edge_b.which_edge + 2) % 3
        };
        if (vnot2 + 2) % 3 == vertex2 {
            vnot2 += 3;
        }
        change_vertices(stl, facet2, vnot2, new_vertex2);
    }
    stl.stats.edges_fixed += 2;
}

/// Whether two of the facet's vertices are equal (`v0 == v1 || v1 == v2 || v0 == v2`).
fn degenerate(facet: &StlFacet) -> bool {
    facet.vertex[0] == facet.vertex[1]
        || facet.vertex[1] == facet.vertex[2]
        || facet.vertex[0] == facet.vertex[2]
}

/// `stl_check_facets_exact`: removes the degenerate facets, then pairs the edges by their exact
/// keys (see the module documentation).
pub fn stl_check_facets_exact(stl: &mut StlFile) {
    stl.stats.connected_edges = 0;
    stl.stats.connected_facets_1_edge = 0;
    stl.stats.connected_facets_2_edge = 0;
    stl.stats.connected_facets_3_edge = 0;

    let mut i = 0u32;
    while i < stl.stats.number_of_facets {
        if degenerate(&stl.facet_start[i as usize]) {
            stl.stats.number_of_facets -= 1;
            stl.facet_start[i as usize] = stl.facet_start[stl.stats.number_of_facets as usize];
            stl.facet_start.pop();
            stl.neighbors_start.pop();
            stl.stats.facets_removed += 1;
            stl.stats.degenerate_facets += 1;
        } else {
            i += 1;
        }
    }

    let mut hash_table = HashTableEdges::new(stl.facets());
    for neighbor in &mut stl.neighbors_start {
        neighbor.reset();
    }

    for i in 0..stl.stats.number_of_facets {
        let facet = stl.facet_start[i as usize];
        for j in 0..3 {
            let mut edge = HashEdge::new(i as i32, j as i32);
            edge.load_exact(stl, &facet.vertex[j], &facet.vertex[(j + 1) % 3]);
            hash_table.insert_edge(stl, &edge, Match::Exact);
        }
    }
}

/// `stl_check_facets_nearby(stl, tolerance)`: pairs the edges still open by their grid cells and
/// moves their vertices together (see the module documentation).
pub fn stl_check_facets_nearby(stl: &mut StlFile, tolerance: f32) {
    if stl.stats.connected_facets_3_edge == stl.stats.number_of_facets as i32 {
        return;
    }
    let mut hash_table = HashTableEdges::new(stl.facets());
    for i in 0..stl.stats.number_of_facets {
        // A copy: the matching may move this facet's own vertices.
        let facet = stl.facet_start[i as usize];
        for j in 0..3 {
            if stl.neighbors_start[i as usize].neighbor[j] == -1 {
                let mut edge = HashEdge::new(i as i32, j as i32);
                if edge.load_nearby(stl, &facet.vertex[j], &facet.vertex[(j + 1) % 3], tolerance) {
                    hash_table.insert_edge(stl, &edge, Match::Nearby);
                }
            }
        }
    }
}

/// `remove_facet` of `stl_remove_unconnected_facets`: moves the last facet into `facet_number`'s
/// place, updating the neighbours that pointed at it; on a neighbour that does not point back it
/// logs and returns, leaving the arrays one entry longer than the count, as the C++ does.
fn remove_facet(stl: &mut StlFile, facet_number: i32) {
    stl.stats.facets_removed += 1;
    let fnum = facet_number as usize;
    match stl.neighbors_start[fnum].num_neighbors() {
        3 => {
            stl.stats.connected_facets_3_edge -= 1;
            stl.stats.connected_facets_2_edge -= 1;
            stl.stats.connected_facets_1_edge -= 1;
        }
        2 => {
            stl.stats.connected_facets_2_edge -= 1;
            stl.stats.connected_facets_1_edge -= 1;
        }
        1 => stl.stats.connected_facets_1_edge -= 1,
        _ => {}
    }
    stl.stats.number_of_facets = stl.stats.number_of_facets.wrapping_sub(1);
    let last = stl.stats.number_of_facets;
    if facet_number < last as i32 {
        stl.facet_start[fnum] = stl.facet_start[last as usize];
        stl.neighbors_start[fnum] = stl.neighbors_start[last as usize];
        for i in 0..3 {
            // Read afresh each time: in the C++ `neighbors` is a reference to this entry, which the
            // update below writes when the moved facet neighbours its own new place.
            let neighbors = stl.neighbors_start[fnum];
            if neighbors.neighbor[i] != -1 {
                let other = neighbors.neighbor[i] as usize;
                let slot = ((i32::from(neighbors.which_vertex_not[i]) + 1) % 3) as usize;
                let other_face_idx = stl.neighbors_start[other].neighbor[slot];
                if other_face_idx as u32 != last {
                    log(
                        Severity::Info,
                        &format!(
                            "in remove_facet: neighbor = {other_face_idx} numfacets = {last} this \
                             is wrong"
                        ),
                    );
                    return;
                }
                stl.neighbors_start[other].neighbor[slot] = facet_number;
            }
        }
    }
    stl.facet_start.pop();
    stl.neighbors_start.pop();
}

/// `update_connects_remove_1`: one edge of the facet was disconnected.
fn update_connects_remove_1(stl: &mut StlFile, facet_num: i32) {
    match stl.neighbors_start[facet_num as usize].num_neighbors() {
        1 => stl.stats.connected_facets_1_edge -= 1,
        2 => stl.stats.connected_facets_2_edge -= 1,
        3 => stl.stats.connected_facets_3_edge -= 1,
        _ => {}
    }
}

/// `remove_degenerate` of `stl_remove_unconnected_facets`: collapses a degenerate facet, joining
/// the neighbours of its two remaining edges, and removes it.
fn remove_degenerate(stl: &mut StlFile, facet: i32) {
    let f = stl.facet_start[facet as usize];
    let edge_to_collapse: i32 = if f.vertex[0] == f.vertex[1] {
        if f.vertex[1] == f.vertex[2] {
            let nbr = stl.neighbors_start[facet as usize].neighbor;
            if nbr[0] == -1 {
                0
            } else if nbr[1] == -1 {
                1
            } else {
                2
            }
        } else {
            0
        }
    } else if f.vertex[1] == f.vertex[2] {
        1
    } else if f.vertex[2] == f.vertex[0] {
        2
    } else {
        return;
    };

    let edge = [
        ((edge_to_collapse + 1) % 3) as usize,
        ((edge_to_collapse + 2) % 3) as usize,
        edge_to_collapse as usize,
    ];
    let n = stl.neighbors_start[facet as usize];
    let neighbor = [
        n.neighbor[edge[0]],
        n.neighbor[edge[1]],
        n.neighbor[edge[2]],
    ];
    let mut vnot = [
        i32::from(n.which_vertex_not[edge[0]]),
        i32::from(n.which_vertex_not[edge[1]]),
        i32::from(n.which_vertex_not[edge[2]]),
    ];

    if neighbor[0] == -1 && neighbor[1] != -1 {
        update_connects_remove_1(stl, neighbor[1]);
    }
    if neighbor[1] == -1 && neighbor[0] != -1 {
        update_connects_remove_1(stl, neighbor[0]);
    }

    if neighbor[0] >= 0 {
        if neighbor[1] >= 0 {
            if vnot[0] > 2 {
                if vnot[1] > 2 {
                    vnot[0] -= 3;
                    vnot[1] -= 3;
                } else {
                    vnot[1] += 3;
                }
            } else if vnot[1] > 2 {
                vnot[0] += 3;
            }
        }
        let slot = ((vnot[0] + 1) % 3) as usize;
        let n0 = &mut stl.neighbors_start[neighbor[0] as usize];
        n0.neighbor[slot] = if neighbor[0] == neighbor[1] {
            -1
        } else {
            neighbor[1]
        };
        n0.which_vertex_not[slot] = vnot[1] as i8;
    }
    if neighbor[1] >= 0 {
        let slot = ((vnot[1] + 1) % 3) as usize;
        let n1 = &mut stl.neighbors_start[neighbor[1] as usize];
        n1.neighbor[slot] = if neighbor[0] == neighbor[1] {
            -1
        } else {
            neighbor[0]
        };
        n1.which_vertex_not[slot] = vnot[0] as i8;
    }
    if neighbor[2] >= 0 {
        update_connects_remove_1(stl, neighbor[2]);
        let slot = ((vnot[2] + 1) % 3) as usize;
        stl.neighbors_start[neighbor[2] as usize].neighbor[slot] = -1;
    }
    remove_facet(stl, facet);
}

/// `stl_remove_unconnected_facets`: collapses the degenerate facets, then removes the facets with
/// no neighbour when some facet has none.
pub fn stl_remove_unconnected_facets(stl: &mut StlFile) {
    let mut i = 0u32;
    while i < stl.stats.number_of_facets {
        let f = stl.facet_start[i as usize];
        if f.vertex[0] == f.vertex[1] || f.vertex[0] == f.vertex[2] || f.vertex[1] == f.vertex[2] {
            remove_degenerate(stl, i as i32);
        } else {
            i += 1;
        }
    }

    if stl.stats.connected_facets_1_edge < stl.stats.number_of_facets as i32 {
        let mut i = 0u32;
        while i < stl.stats.number_of_facets {
            if stl.neighbors_start[i as usize].num_neighbors() == 0 {
                remove_facet(stl, i as i32);
            } else {
                i += 1;
            }
        }
    }
}

/// `stl_add_facet`: appends a facet (its normal zero) with no neighbours.
pub fn stl_add_facet(stl: &mut StlFile, new_facet: &StlFacet) {
    let mut facet = *new_facet;
    facet.normal = StlVertex::zero();
    stl.facet_start.push(facet);
    stl.neighbors_start.push(StlNeighbors::RESET);
    stl.stats.facets_added += 1;
    stl.stats.number_of_facets += 1;
}

/// `stl_fill_holes`: closes each open edge loop with new facets. libslic3r does not call it
/// (`trianglemesh_repair_on_import` has it under `#if 0`); `stl_repair` does with its flag.
pub fn stl_fill_holes(stl: &mut StlFile) {
    let mut hash_table = HashTableEdges::new(stl.facets());
    for i in 0..stl.stats.number_of_facets {
        let facet = stl.facet_start[i as usize];
        for j in 0..3 {
            if stl.neighbors_start[i as usize].neighbor[j] != -1 {
                continue;
            }
            let mut edge = HashEdge::new(i as i32, j as i32);
            edge.load_exact(stl, &facet.vertex[j], &facet.vertex[(j + 1) % 3]);
            hash_table.insert_edge(stl, &edge, Match::Exact);
        }
    }

    let mut i = 0u32;
    while i < stl.stats.number_of_facets {
        let facet = stl.facet_start[i as usize];
        let neighbors_initial = stl.neighbors_start[i as usize].neighbor;
        let first_facet = i as i32;
        for j in 0..3usize {
            if stl.neighbors_start[i as usize].neighbor[j] != -1 {
                continue;
            }
            // `stl_facet new_facet;` is uninitialised in the C++; only its vertices are read.
            let mut new_facet = StlFacet::ZERO;
            new_facet.vertex[0] = facet.vertex[j];
            new_facet.vertex[1] = facet.vertex[(j + 1) % 3];
            let mut direction = neighbors_initial[(j + 2) % 3] == -1;
            let mut facet_num = i as i32;
            let mut vnot = ((j + 2) % 3) as i32;
            loop {
                // The C++ computes the pivot vertex here too, and does not use it.
                let next_edge = if vnot > 2 {
                    let e = if direction { vnot % 3 } else { (vnot + 2) % 3 };
                    direction = !direction;
                    e
                } else if direction {
                    (vnot + 2) % 3
                } else {
                    vnot
                };
                let neighbors = stl.neighbors_start[facet_num as usize];
                let next_facet = neighbors.neighbor[next_edge as usize];
                if next_facet == -1 {
                    new_facet.vertex[2] =
                        stl.facet_start[facet_num as usize].vertex[(vnot % 3) as usize];
                    stl_add_facet(stl, &new_facet);
                    for k in 0..3 {
                        let added = stl.stats.number_of_facets as i32 - 1;
                        let mut edge = HashEdge::new(added, k as i32);
                        edge.load_exact(stl, &new_facet.vertex[k], &new_facet.vertex[(k + 1) % 3]);
                        hash_table.insert_edge(stl, &edge, Match::Exact);
                    }
                    break;
                }
                vnot = i32::from(neighbors.which_vertex_not[next_edge as usize]);
                facet_num = next_facet;
                if facet_num == first_facet {
                    log(
                        Severity::Info,
                        "Back to the first facet filling holes: probably a mobius part. Try using \
                         a smaller tolerance or don't do a nearby check.",
                    );
                    return;
                }
            }
        }
        i += 1;
    }
}
