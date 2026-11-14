// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries OrcaSlicer code: src/libslic3r/Arachne/utils/HalfEdge.hpp lines 1-39, HalfEdgeGraph.hpp
// lines 1-31 and HalfEdgeNode.hpp lines 1-38 whole, of OrcaSlicer v2.4.2, translated on 2026-11-13
// by arachne-1. Copyright Ultimaker B.V. (2020, CuraEngine), with the PrusaSlicer authors' (Prusa
// Research) and the OrcaSlicer authors' changes, under the GNU Affero General Public License,
// version 3 (AGPL-3.0; LICENSE).

//! The half-edge graph templates: `HalfEdge`, `HalfEdgeNode` and `HalfEdgeGraph`, which the
//! skeletal trapezoidation's graph (the piece arachne-2's) instantiates.
//!
//! The C++ graph keeps its edges and nodes in two `std::list`s and links them by pointers: an
//! edge's twin, next and previous edges and its two nodes, a node's incident edge. A pointer to an
//! element stays valid while other elements are added and erased, and the code compares elements
//! by address (`operator==`). Here each list is a [`StableList`]: the elements live in slots that
//! are never reused, linked in the list's order, and a pointer is the element's slot, an
//! [`EdgeId`] or a [`NodeId`] (`None` for `nullptr`). So a handle stays valid as the list changes,
//! handles compare as addresses do, and a handle to an erased element (a dangling pointer in C++)
//! is known: [`StableList::get`] gives `None` and [`StableList::positions`] gives -2, as the probe
//! writes it (`parity/sop1/arachne.md`, "a graph"). `push_front`, `push_back`, `insert` and
//! `erase` are `std::list`'s `emplace_front`, `emplace_back`, `emplace` and `erase`, in the same
//! places of the order.
//!
//! The C++ templates take the derived node and edge types as parameters (so the pointers have the
//! derived types); the derived types' methods, which follow the pointers, are the piece arachne-2's
//! and take the graph with a handle.

use core::fmt::Debug;
use core::hash::Hash;
use core::marker::PhantomData;
use core::ops::{Index, IndexMut};

use orca_geom::point::Point;

/// A handle on an element of a [`StableList`]: its slot.
pub trait ListKey: Copy + Eq + Hash + Debug {
    /// The handle of a slot.
    fn from_slot(slot: u32) -> Self;
    /// The handle's slot.
    fn slot(self) -> u32;
}

/// An edge of a [`HalfEdgeGraph`]: `edge_t*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeId(u32);

/// A node of a [`HalfEdgeGraph`]: `node_t*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl ListKey for EdgeId {
    fn from_slot(slot: u32) -> Self {
        Self(slot)
    }
    fn slot(self) -> u32 {
        self.0
    }
}

impl ListKey for NodeId {
    fn from_slot(slot: u32) -> Self {
        Self(slot)
    }
    fn slot(self) -> u32 {
        self.0
    }
}

/// No slot: the end of the list.
const NIL: u32 = u32::MAX;

#[derive(Clone, Debug)]
struct Slot<T> {
    /// The element, `None` once erased.
    value: Option<T>,
    prev: u32,
    next: u32,
}

/// `std::list<T>` with handles that stay valid: each element in a slot of its own that is never
/// reused, the slots linked in the list's order.
#[derive(Clone, Debug)]
pub struct StableList<K, T> {
    slots: Vec<Slot<T>>,
    head: u32,
    tail: u32,
    len: usize,
    key: PhantomData<fn() -> K>,
}

impl<K: ListKey, T> Default for StableList<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: ListKey, T> StableList<K, T> {
    /// An empty list.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            head: NIL,
            tail: NIL,
            len: 0,
            key: PhantomData,
        }
    }

    /// `size()`.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// `empty()`.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn key(slot: u32) -> Option<K> {
        if slot == NIL {
            None
        } else {
            Some(K::from_slot(slot))
        }
    }

    fn live(&self, k: K) -> &Slot<T> {
        match self.slots.get(k.slot() as usize) {
            Some(s) if s.value.is_some() => s,
            _ => panic!("{k:?} is not in the list"),
        }
    }

    /// The first element, `None` when the list is empty (`&front()`).
    #[must_use]
    pub fn front(&self) -> Option<K> {
        Self::key(self.head)
    }

    /// The last element, `None` when the list is empty (`&back()`).
    #[must_use]
    pub fn back(&self) -> Option<K> {
        Self::key(self.tail)
    }

    /// The element after `k`, `None` at the end (`++it`).
    ///
    /// # Panics
    /// When `k` is not in the list.
    #[must_use]
    pub fn next(&self, k: K) -> Option<K> {
        Self::key(self.live(k).next)
    }

    /// The element before `k`, `None` at the start (`--it`).
    ///
    /// # Panics
    /// When `k` is not in the list.
    #[must_use]
    pub fn prev(&self, k: K) -> Option<K> {
        Self::key(self.live(k).prev)
    }

    /// Whether `k` is an element of the list (not erased).
    #[must_use]
    pub fn contains(&self, k: K) -> bool {
        self.slots
            .get(k.slot() as usize)
            .is_some_and(|s| s.value.is_some())
    }

    /// The element `k`, `None` when it is not in the list.
    #[must_use]
    pub fn get(&self, k: K) -> Option<&T> {
        self.slots
            .get(k.slot() as usize)
            .and_then(|s| s.value.as_ref())
    }

    /// The element `k`, to change it; `None` when it is not in the list.
    pub fn get_mut(&mut self, k: K) -> Option<&mut T> {
        self.slots
            .get_mut(k.slot() as usize)
            .and_then(|s| s.value.as_mut())
    }

    /// Links a new slot holding `value` between `prev` and `next` (either `NIL`).
    fn link(&mut self, value: T, prev: u32, next: u32) -> K {
        let slot = u32::try_from(self.slots.len())
            .ok()
            .filter(|s| *s != NIL)
            .expect("a list of fewer than 2^32 - 1 elements");
        self.slots.push(Slot {
            value: Some(value),
            prev,
            next,
        });
        if prev == NIL {
            self.head = slot;
        } else {
            self.slots[prev as usize].next = slot;
        }
        if next == NIL {
            self.tail = slot;
        } else {
            self.slots[next as usize].prev = slot;
        }
        self.len += 1;
        K::from_slot(slot)
    }

    /// `emplace_front(value)`: the new first element.
    pub fn push_front(&mut self, value: T) -> K {
        let next = self.head;
        self.link(value, NIL, next)
    }

    /// `emplace_back(value)`: the new last element.
    pub fn push_back(&mut self, value: T) -> K {
        let prev = self.tail;
        self.link(value, prev, NIL)
    }

    /// `emplace(pos, value)`: a new element before `pos`, or at the end for `None` (`end()`).
    ///
    /// # Panics
    /// When `pos` is not in the list.
    pub fn insert(&mut self, pos: Option<K>, value: T) -> K {
        match pos {
            None => self.push_back(value),
            Some(k) => {
                let prev = self.live(k).prev;
                self.link(value, prev, k.slot())
            }
        }
    }

    /// `erase(it)`: removes `k` and returns the element after it (`None` at the end). Its handle
    /// stays known as erased.
    ///
    /// # Panics
    /// When `k` is not in the list.
    pub fn erase(&mut self, k: K) -> Option<K> {
        let (prev, next) = {
            let s = self.live(k);
            (s.prev, s.next)
        };
        if prev == NIL {
            self.head = next;
        } else {
            self.slots[prev as usize].next = next;
        }
        if next == NIL {
            self.tail = prev;
        } else {
            self.slots[next as usize].prev = prev;
        }
        let s = &mut self.slots[k.slot() as usize];
        s.value = None;
        s.prev = NIL;
        s.next = NIL;
        self.len -= 1;
        Self::key(next)
    }

    /// `clear()`: every element erased; the handles stay known as erased.
    pub fn clear(&mut self) {
        for s in &mut self.slots {
            s.value = None;
            s.prev = NIL;
            s.next = NIL;
        }
        self.head = NIL;
        self.tail = NIL;
        self.len = 0;
    }

    /// The handles in the list's order.
    #[must_use]
    pub fn keys(&self) -> Keys<'_, K, T> {
        Keys {
            list: self,
            at: self.head,
        }
    }

    /// The handles and the elements in the list's order.
    pub fn iter(&self) -> impl Iterator<Item = (K, &T)> + '_ {
        self.keys().map(move |k| (k, &self[k]))
    }

    /// The elements in the list's order.
    pub fn values(&self) -> impl Iterator<Item = &T> + '_ {
        self.keys().map(move |k| &self[k])
    }

    /// Calls `f` on each handle and element in the list's order, the element to change.
    pub fn for_each_mut(&mut self, mut f: impl FnMut(K, &mut T)) {
        let mut at = self.head;
        while at != NIL {
            let s = &mut self.slots[at as usize];
            let next = s.next;
            if let Some(v) = s.value.as_mut() {
                f(K::from_slot(at), v);
            }
            at = next;
        }
    }

    /// Each element's position in the list, for writing pointers as the probe does.
    #[must_use]
    pub fn positions(&self) -> Positions<K> {
        let mut of_slot = vec![-2i32; self.slots.len()];
        for (i, k) in self.keys().enumerate() {
            of_slot[k.slot() as usize] = i32::try_from(i).unwrap_or(i32::MAX);
        }
        Positions {
            of_slot,
            key: PhantomData,
        }
    }

    /// The `i`-th element of the list's order, `None` past its end.
    #[must_use]
    pub fn nth(&self, i: usize) -> Option<K> {
        self.keys().nth(i)
    }
}

impl<K: ListKey, T> Index<K> for StableList<K, T> {
    type Output = T;

    fn index(&self, k: K) -> &T {
        match self.get(k) {
            Some(v) => v,
            None => panic!("{k:?} is not in the list"),
        }
    }
}

impl<K: ListKey, T> IndexMut<K> for StableList<K, T> {
    fn index_mut(&mut self, k: K) -> &mut T {
        match self.get_mut(k) {
            Some(v) => v,
            None => panic!("{k:?} is not in the list"),
        }
    }
}

/// The handles of a [`StableList`] in its order.
pub struct Keys<'a, K, T> {
    list: &'a StableList<K, T>,
    at: u32,
}

impl<K: ListKey, T> Iterator for Keys<'_, K, T> {
    type Item = K;

    fn next(&mut self) -> Option<K> {
        if self.at == NIL {
            return None;
        }
        let k = K::from_slot(self.at);
        self.at = self.list.slots[self.at as usize].next;
        Some(k)
    }
}

/// Each element's position in a [`StableList`] when it was taken
/// ([`StableList::positions`]).
#[derive(Clone, Debug)]
pub struct Positions<K> {
    of_slot: Vec<i32>,
    key: PhantomData<fn() -> K>,
}

impl<K: ListKey> Positions<K> {
    /// A pointer as the probe writes it: the position of `k` in its list, -1 for `None`
    /// (`nullptr`), -2 for an element no longer in the list.
    #[must_use]
    pub fn index(&self, k: Option<K>) -> i32 {
        match k {
            None => -1,
            Some(k) => self.of_slot.get(k.slot() as usize).copied().unwrap_or(-2),
        }
    }
}

/// `HalfEdge<node_data_t, edge_data_t, derived_node_t, derived_edge_t>`: an edge's data and its
/// pointers, all `nullptr` at first.
#[derive(Clone, Debug)]
pub struct HalfEdge<E> {
    /// `data`.
    pub data: E,
    /// `twin`.
    pub twin: Option<EdgeId>,
    /// `next`.
    pub next: Option<EdgeId>,
    /// `prev`.
    pub prev: Option<EdgeId>,
    /// `from`.
    pub from: Option<NodeId>,
    /// `to`.
    pub to: Option<NodeId>,
}

impl<E> HalfEdge<E> {
    /// `HalfEdge(data)`.
    #[must_use]
    pub const fn new(data: E) -> Self {
        Self {
            data,
            twin: None,
            next: None,
            prev: None,
            from: None,
            to: None,
        }
    }
}

/// `HalfEdgeNode<node_data_t, edge_data_t, derived_node_t, derived_edge_t>`: a node's data, its
/// point and its incident edge, `nullptr` at first.
#[derive(Clone, Debug)]
pub struct HalfEdgeNode<N> {
    /// `data`.
    pub data: N,
    /// `p`.
    pub p: Point,
    /// `incident_edge`.
    pub incident_edge: Option<EdgeId>,
}

impl<N> HalfEdgeNode<N> {
    /// `HalfEdgeNode(data, p)`.
    #[must_use]
    pub const fn new(data: N, p: Point) -> Self {
        Self {
            data,
            p,
            incident_edge: None,
        }
    }
}

/// `HalfEdgeGraph::Edges`: `std::list<edge_t>`.
pub type Edges<E> = StableList<EdgeId, HalfEdge<E>>;
/// `HalfEdgeGraph::Nodes`: `std::list<node_t>`.
pub type Nodes<N> = StableList<NodeId, HalfEdgeNode<N>>;

/// `HalfEdgeGraph<node_data_t, edge_data_t, derived_node_t, derived_edge_t>`: the edges and the
/// nodes.
#[derive(Clone, Debug)]
pub struct HalfEdgeGraph<N, E> {
    /// `edges`.
    pub edges: Edges<E>,
    /// `nodes`.
    pub nodes: Nodes<N>,
}

impl<N, E> Default for HalfEdgeGraph<N, E> {
    fn default() -> Self {
        Self {
            edges: StableList::new(),
            nodes: StableList::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_survive_erasure_and_keep_the_order() {
        let mut l: StableList<EdgeId, i32> = StableList::new();
        let a = l.push_back(1);
        let b = l.push_back(2);
        let c = l.push_front(0);
        let d = l.insert(Some(b), 5);
        assert_eq!(l.values().copied().collect::<Vec<_>>(), vec![0, 1, 5, 2]);
        assert_eq!(l.erase(a), Some(d));
        assert!(!l.contains(a));
        assert_eq!(l.get(a), None);
        assert_eq!(l.values().copied().collect::<Vec<_>>(), vec![0, 5, 2]);
        let pos = l.positions();
        assert_eq!(pos.index(Some(c)), 0);
        assert_eq!(pos.index(Some(b)), 2);
        assert_eq!(pos.index(Some(a)), -2);
        assert_eq!(pos.index(None), -1);
        assert_eq!(l.erase(b), None);
        assert_eq!(l.back(), Some(d));
        l.for_each_mut(|_, v| *v *= 10);
        assert_eq!(l.values().copied().collect::<Vec<_>>(), vec![0, 50]);
        assert_eq!(l.len(), 2);
        l.clear();
        assert!(l.is_empty());
        assert!(!l.contains(c));
        let e = l.push_back(7);
        assert_ne!(e, c);
        assert_eq!(l.front(), Some(e));
    }

    #[test]
    fn a_graph_starts_unlinked() {
        let mut g: HalfEdgeGraph<(), u8> = HalfEdgeGraph::default();
        let n = g.nodes.push_back(HalfEdgeNode::new((), Point::new(1, 2)));
        let e = g.edges.push_front(HalfEdge::new(3));
        g.edges[e].from = Some(n);
        g.nodes[n].incident_edge = Some(e);
        assert_eq!(g.edges[e].to, None);
        assert_eq!(g.nodes[n].p, Point::new(1, 2));
    }
}
