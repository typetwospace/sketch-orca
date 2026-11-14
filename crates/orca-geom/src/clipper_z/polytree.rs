// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Clipper code: class PolyNode and class PolyTree of deps_src/clipper/clipper.cpp and
// clipper.hpp of OrcaSlicer v2.4.2 (Clipper 6.4.2 with the PrusaSlicer authors' changes) as
// clipper_z.cpp compiles them with CLIPPERLIB_USE_XYZ, translated on 2026-11-13 from the XY
// translation of 2026-11-07. Copyright Angus Johnson 2010-2017 and the PrusaSlicer authors, under
// the Boost Software License, version 1.0 (BSL-1.0; licenses/BSL-1.0.txt).

//! `ClipperLib_Z::PolyTree` and its `PolyNode`s, the XY clipper's `polytree` with Z paths. The C++
//! tree is itself a `PolyNode` (the root) that owns the
//! other nodes in `AllNodes`; here the nodes live in one vector, the root first ([`ROOT`]), and
//! link to each other by index, `Parent` and `Index` included.

use super::Path;

/// A node's index in its [`PolyTree`].
pub type NodeId = usize;

/// The tree's own node: the C++ `PolyTree` object as a `PolyNode`.
pub const ROOT: NodeId = 0;

/// `PolyNode`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PolyNode {
    /// `Contour`.
    pub contour: Path,
    /// `Childs`.
    pub childs: Vec<NodeId>,
    /// `Parent`: `None` for the root (and, in C++, for a node not yet linked).
    pub parent: Option<NodeId>,
    /// `Index`: the node's index in its parent's `Childs`.
    index: usize,
    /// `m_IsOpen`.
    is_open: bool,
}

/// `PolyTree`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolyTree {
    /// The root, then `AllNodes` in their order.
    nodes: Vec<PolyNode>,
}

impl Default for PolyTree {
    fn default() -> Self {
        Self::new()
    }
}

impl PolyTree {
    /// An empty tree.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: vec![PolyNode::default()],
        }
    }

    /// The node `id`.
    #[must_use]
    pub fn node(&self, id: NodeId) -> &PolyNode {
        &self.nodes[id]
    }

    /// The node `id`, to change (to move its contour out, for one).
    pub fn node_mut(&mut self, id: NodeId) -> &mut PolyNode {
        &mut self.nodes[id]
    }

    /// The tree's own node.
    #[must_use]
    pub fn root(&self) -> &PolyNode {
        &self.nodes[ROOT]
    }

    /// `GetFirst()`: the root's first child.
    #[must_use]
    pub fn get_first(&self) -> Option<NodeId> {
        self.nodes[ROOT].childs.first().copied()
    }

    /// `GetNext()`: the next node depth first.
    #[must_use]
    pub fn get_next(&self, id: NodeId) -> Option<NodeId> {
        match self.nodes[id].childs.first() {
            Some(&c) => Some(c),
            None => self.get_next_sibling_up(id),
        }
    }

    /// `GetNextSiblingUp()`.
    fn get_next_sibling_up(&self, id: NodeId) -> Option<NodeId> {
        let node = &self.nodes[id];
        let parent = node.parent?;
        let siblings = &self.nodes[parent].childs;
        if node.index == siblings.len().wrapping_sub(1) {
            self.get_next_sibling_up(parent)
        } else {
            Some(siblings[node.index + 1])
        }
    }

    /// `IsHole()`: true when the node has an even number of ancestors, the root counted.
    #[must_use]
    pub fn is_hole(&self, id: NodeId) -> bool {
        let mut result = true;
        let mut node = self.nodes[id].parent;
        while let Some(n) = node {
            result = !result;
            node = self.nodes[n].parent;
        }
        result
    }

    /// `IsOpen()`.
    #[must_use]
    pub fn is_open(&self, id: NodeId) -> bool {
        self.nodes[id].is_open
    }

    /// `ChildCount()`.
    #[must_use]
    pub fn child_count(&self, id: NodeId) -> usize {
        self.nodes[id].childs.len()
    }

    /// `Total()`: the nodes besides the root, less the hidden outer rectangle a negative offset
    /// leaves when the root's first child is not the first node.
    #[must_use]
    pub fn total(&self) -> i32 {
        let mut result = (self.nodes.len() - 1) as i32;
        if result > 0 && self.nodes[ROOT].childs.first() != Some(&1) {
            result -= 1;
        }
        result
    }

    /// `Clear()`: drops every node and the root's children (not the root's contour).
    pub fn clear(&mut self) {
        self.nodes.truncate(1);
        self.nodes[ROOT].childs.clear();
    }

    /// `RemoveOutermostPolygon()`: with one child that has children, its children take its place;
    /// otherwise the tree is cleared.
    pub fn remove_outermost_polygon(&mut self) {
        if self.child_count(ROOT) == 1 && self.child_count(self.nodes[ROOT].childs[0]) > 0 {
            let outer = self.nodes[ROOT].childs[0];
            let outer_childs = self.nodes[outer].childs.clone();
            let outer_parent = self.nodes[outer].parent;
            self.nodes[ROOT].childs[0] = outer_childs[0];
            self.nodes[outer_childs[0]].parent = outer_parent;
            for &c in &outer_childs[1..] {
                self.add_child(ROOT, c);
            }
        } else {
            self.clear();
        }
    }

    /// `AllNodes.emplace_back(PolyNode())` with its contour: a node not yet linked.
    pub(crate) fn push_node(&mut self, contour: Path) -> NodeId {
        self.nodes.push(PolyNode {
            contour,
            ..PolyNode::default()
        });
        self.nodes.len() - 1
    }

    /// `m_IsOpen = true`.
    pub(crate) fn set_open(&mut self, id: NodeId) {
        self.nodes[id].is_open = true;
    }

    /// `parent.AddChild(child)`.
    pub(crate) fn add_child(&mut self, parent: NodeId, child: NodeId) {
        let cnt = self.nodes[parent].childs.len();
        self.nodes[parent].childs.push(child);
        self.nodes[child].parent = Some(parent);
        self.nodes[child].index = cnt;
    }

    /// `AddPolyNodeToPaths(polynode, ntAny or ntClosed, paths)`.
    pub(crate) fn add_to_paths(&self, id: NodeId, closed_only: bool, paths: &mut Vec<Path>) {
        let node = &self.nodes[id];
        let matches = !closed_only || !node.is_open;
        if !node.contour.is_empty() && matches {
            paths.push(node.contour.clone());
        }
        for &c in &node.childs {
            self.add_to_paths(c, closed_only, paths);
        }
    }
}
