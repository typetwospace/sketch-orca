// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Microsoft STL code: this file is translated from stl/inc/xtree of microsoft/STL at the
// tag vs-2022-17.12 (commit 91e425587a0b9b5ac0d1005844c661fee2e2813b), as MSVC 14.42 ships it
// (VC/Tools/MSVC/14.42.34433/include/xtree: _Tree_unchecked_const_iterator's operator++ and
// operator--, _Tree_node's _Buyheadnode and _Buynode, _Tree_val's _Max, _Min, _Lrotate,
// _Rrotate, _Extract and _Insert_node, and _Tree's _Emplace_hint, insert with a hint, erase,
// clear, lower_bound, _Find_hint, _Lower_bound_duplicate and _Find_lower_bound for a map, unique
// keys), step for step, on 2026-11-13 by orca-o3-voronoi-2. Its copyright holder is Microsoft
// Corporation, and it is under Apache-2.0 WITH LLVM-exception (licenses/Apache-2.0.txt and
// licenses/LLVM-exception.txt).

//! MSVC's `std::map` (`<xtree>`'s red-black tree), as far as Boost.Polygon's Voronoi builder uses
//! it for its beach line: `insert` with a hint, `lower_bound`, `erase` of an iterator, `begin`,
//! `end`, iterator steps and `clear`. It belongs in `orca-stl` (with the STL's other containers);
//! no piece of phase O3 carries it there, so it is translated here and named in orca-o3-voronoi-2's
//! result.
//!
//! The builder's key comparison (`node_comparison_predicate`) is a strict order only on the pairs
//! the builder compares, so a lookup's answer can depend on which keys it is compared with, and in
//! which order. The tree here makes the same comparisons as MSVC's, with the arguments the same
//! way round: the hint is taken by `_Find_hint`'s cases (before the first key, after the last,
//! between the hint and its neighbour, or equivalent to the hint, which inserts nothing), else a
//! `_Find_lower_bound` search from the root; the rotations and recolourings of `_Insert_node` and
//! `_Extract` are MSVC's, so the tree has MSVC's shape at every step and a search meets the same
//! keys.
//!
//! The nodes live in a vector; index 0 is MSVC's head node (`_Myhead`), which is also every nil
//! leaf: its `parent` is the root, its `left` the smallest node and its `right` the largest, and an
//! iterator is a node index, `end()` being 0. An erased node's slot goes on a free list that the
//! next insert takes from, as a freed node's memory may be allocated again; an iterator to an
//! erased element is as invalid as the C++'s.
//!
//! Left out: everything else of `std::map` (the builder uses no other member), the multi-key paths
//! of `_Find_hint` (`_Multi`), the allocator and the iterator debugging of `_ITERATOR_DEBUG_LEVEL`
//! 2 (the oracle builds with 0).

/// `_Redbl`: the colour of a node's link to its parent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Color {
    /// `_Red`.
    Red,
    /// `_Black`.
    Black,
}

/// `_Tree_child`: the side of `_Tree_id::_Parent` a new node goes on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Child {
    /// `_Right`.
    Right,
    /// `_Left`.
    Left,
    /// `_Unused`: the location of a duplicate, never inserted at.
    Unused,
}

/// `_Tree_id`: where a new node goes.
#[derive(Clone, Copy, Debug)]
struct TreeId {
    parent: usize,
    child: Child,
}

/// `_Tree_find_result`: a location, and the lower bound the search found.
#[derive(Clone, Copy, Debug)]
struct FindResult {
    location: TreeId,
    bound: usize,
}

/// The links of a `_Tree_node`.
#[derive(Clone, Copy, Debug)]
struct Links {
    left: usize,
    parent: usize,
    right: usize,
    color: Color,
    isnil: bool,
}

/// The head node's index (`_Myhead`), which `end()` points at.
const HEAD: usize = 0;

/// `std::map<K, V, Compare>` with MSVC's red-black tree. `comp(a, b)` is the C++ `Compare`, "a
/// goes before b".
pub struct StdMap<K, V, C> {
    links: Vec<Links>,
    vals: Vec<Option<(K, V)>>,
    free: Vec<usize>,
    size: usize,
    comp: C,
}

impl<K, V, C: Fn(&K, &K) -> bool> StdMap<K, V, C> {
    /// `map(const key_compare&)`: an empty map (`_Buyheadnode`: the head links to itself, black
    /// and nil).
    pub fn new(comp: C) -> Self {
        Self {
            links: vec![Links {
                left: HEAD,
                parent: HEAD,
                right: HEAD,
                color: Color::Black,
                isnil: true,
            }],
            vals: vec![None],
            free: Vec::new(),
            size: 0,
            comp,
        }
    }

    /// `size()`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.size
    }

    /// `empty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// `begin()`: the smallest node, `end()` when empty.
    #[must_use]
    pub fn begin(&self) -> usize {
        self.links[HEAD].left
    }

    /// `end()`: the head node.
    #[must_use]
    pub fn end(&self) -> usize {
        HEAD
    }

    /// `it->first`.
    #[must_use]
    pub fn key(&self, it: usize) -> &K {
        &self.val(it).0
    }

    /// `const_cast<key_type&>(it->first)`: the key, to change in place (the caller keeps the
    /// order right, as the C++'s does).
    pub fn key_mut(&mut self, it: usize) -> &mut K {
        &mut self.val_mut(it).0
    }

    /// `it->second`.
    #[must_use]
    pub fn value(&self, it: usize) -> &V {
        &self.val(it).1
    }

    /// `it->second`, to change.
    pub fn value_mut(&mut self, it: usize) -> &mut V {
        &mut self.val_mut(it).1
    }

    /// `_Myval` of a node that holds an element.
    fn val(&self, it: usize) -> &(K, V) {
        self.vals[it]
            .as_ref()
            .expect("std::map: an iterator that holds no element")
    }

    /// `_Myval` of a node that holds an element, to change.
    fn val_mut(&mut self, it: usize) -> &mut (K, V) {
        self.vals[it]
            .as_mut()
            .expect("std::map: an iterator that holds no element")
    }

    /// `_Max(pnode)`: the rightmost node of the subtree at `pnode`.
    fn max(&self, mut pnode: usize) -> usize {
        while !self.links[self.links[pnode].right].isnil {
            pnode = self.links[pnode].right;
        }
        pnode
    }

    /// `_Min(pnode)`: the leftmost node of the subtree at `pnode`.
    fn min(&self, mut pnode: usize) -> usize {
        while !self.links[self.links[pnode].left].isnil {
            pnode = self.links[pnode].left;
        }
        pnode
    }

    /// `++it` (`_Tree_unchecked_const_iterator::operator++`): the next node, the head after the
    /// largest.
    #[must_use]
    pub fn next(&self, mut ptr: usize) -> usize {
        if self.links[self.links[ptr].right].isnil {
            // climb looking for right subtree
            let mut pnode;
            loop {
                pnode = self.links[ptr].parent;
                // while (!_Pnode->_Isnil && _Ptr == _Pnode->_Right)
                if self.links[pnode].isnil || ptr != self.links[pnode].right {
                    break;
                }
                ptr = pnode; // ==> parent while right subtree
            }
            ptr = pnode; // ==> parent (head if end())
        } else {
            ptr = self.min(self.links[ptr].right); // ==> smallest of right subtree
        }
        ptr
    }

    /// `--it` (`_Tree_unchecked_const_iterator::operator--`): the previous node, the largest
    /// before `end()`.
    #[must_use]
    pub fn prev(&self, mut ptr: usize) -> usize {
        if self.links[ptr].isnil {
            ptr = self.links[ptr].right; // end() ==> rightmost
        } else if self.links[self.links[ptr].left].isnil {
            // climb looking for left subtree
            let mut pnode;
            loop {
                pnode = self.links[ptr].parent;
                // while (!_Pnode->_Isnil && _Ptr == _Pnode->_Left)
                if self.links[pnode].isnil || ptr != self.links[pnode].left {
                    break;
                }
                ptr = pnode; // ==> parent while left subtree
            }
            if !self.links[ptr].isnil {
                // decrement non-begin()
                ptr = pnode; // ==> parent if not head
            }
        } else {
            ptr = self.max(self.links[ptr].left); // ==> largest of left subtree
        }
        ptr
    }

    /// `_Lrotate(wherenode)`: promotes the right node to the root of the subtree.
    fn lrotate(&mut self, wherenode: usize) {
        let pnode = self.links[wherenode].right;
        self.links[wherenode].right = self.links[pnode].left;

        let pleft = self.links[pnode].left;
        if !self.links[pleft].isnil {
            self.links[pleft].parent = wherenode;
        }

        self.links[pnode].parent = self.links[wherenode].parent;

        let wparent = self.links[wherenode].parent;
        if wherenode == self.links[HEAD].parent {
            self.links[HEAD].parent = pnode;
        } else if wherenode == self.links[wparent].left {
            self.links[wparent].left = pnode;
        } else {
            self.links[wparent].right = pnode;
        }

        self.links[pnode].left = wherenode;
        self.links[wherenode].parent = pnode;
    }

    /// `_Rrotate(wherenode)`: promotes the left node to the root of the subtree.
    fn rrotate(&mut self, wherenode: usize) {
        let pnode = self.links[wherenode].left;
        self.links[wherenode].left = self.links[pnode].right;

        let pright = self.links[pnode].right;
        if !self.links[pright].isnil {
            self.links[pright].parent = wherenode;
        }

        self.links[pnode].parent = self.links[wherenode].parent;

        let wparent = self.links[wherenode].parent;
        if wherenode == self.links[HEAD].parent {
            self.links[HEAD].parent = pnode;
        } else if wherenode == self.links[wparent].right {
            self.links[wparent].right = pnode;
        } else {
            self.links[wparent].left = pnode;
        }

        self.links[pnode].right = wherenode;
        self.links[wherenode].parent = pnode;
    }

    /// `_Buynode(al, myhead, val)`: a red node whose links point at the head, in a free slot.
    fn buynode(&mut self, val: (K, V)) -> usize {
        let links = Links {
            left: HEAD,
            parent: HEAD,
            right: HEAD,
            color: Color::Red,
            isnil: false,
        };
        if let Some(it) = self.free.pop() {
            self.links[it] = links;
            self.vals[it] = Some(val);
            it
        } else {
            self.links.push(links);
            self.vals.push(Some(val));
            self.links.len() - 1
        }
    }

    /// `_Freenode(al, ptr)`: the slot goes back to the free list.
    fn freenode(&mut self, ptr: usize) {
        self.vals[ptr] = None;
        self.free.push(ptr);
    }

    /// `_Insert_node(loc, newnode)`: links the new node at `loc` and rebalances.
    fn insert_node(&mut self, loc: TreeId, newnode: usize) -> usize {
        self.size += 1;
        let head = HEAD;
        self.links[newnode].parent = loc.parent;
        if loc.parent == head {
            // first node in tree, just set head values
            self.links[head].left = newnode;
            self.links[head].parent = newnode;
            self.links[head].right = newnode;
            self.links[newnode].color = Color::Black; // the root is black
            return newnode;
        }

        debug_assert!(loc.child != Child::Unused);
        if loc.child == Child::Right {
            // add to right of loc.parent
            self.links[loc.parent].right = newnode;
            if loc.parent == self.links[head].right {
                // remember rightmost node
                self.links[head].right = newnode;
            }
        } else {
            // add to left of loc.parent
            self.links[loc.parent].left = newnode;
            if loc.parent == self.links[head].left {
                // remember leftmost node
                self.links[head].left = newnode;
            }
        }

        let mut pnode = newnode;
        while self.links[self.links[pnode].parent].color == Color::Red {
            let parent = self.links[pnode].parent;
            let grandparent = self.links[parent].parent;
            if parent == self.links[grandparent].left {
                // fixup red-red in left subtree
                let parent_sibling = self.links[grandparent].right;
                if self.links[parent_sibling].color == Color::Red {
                    // parent's sibling has two red children, blacken both
                    self.links[parent].color = Color::Black;
                    self.links[parent_sibling].color = Color::Black;
                    self.links[grandparent].color = Color::Red;
                    pnode = grandparent;
                } else {
                    // parent's sibling has red and black children
                    if pnode == self.links[parent].right {
                        // rotate right child to left
                        pnode = parent;
                        self.lrotate(pnode);
                    }

                    let p = self.links[pnode].parent;
                    self.links[p].color = Color::Black; // propagate red up
                    let g = self.links[p].parent;
                    self.links[g].color = Color::Red;
                    self.rrotate(g);
                }
            } else {
                // fixup red-red in right subtree
                let parent_sibling = self.links[grandparent].left;
                if self.links[parent_sibling].color == Color::Red {
                    // parent's sibling has two red children, blacken both
                    self.links[parent].color = Color::Black;
                    self.links[parent_sibling].color = Color::Black;
                    self.links[grandparent].color = Color::Red;
                    pnode = grandparent;
                } else {
                    // parent's sibling has red and black children
                    if pnode == self.links[parent].left {
                        // rotate left child to right
                        pnode = parent;
                        self.rrotate(pnode);
                    }

                    let p = self.links[pnode].parent;
                    self.links[p].color = Color::Black; // propagate red up
                    let g = self.links[p].parent;
                    self.links[g].color = Color::Red;
                    self.lrotate(g);
                }
            }
        }

        let root = self.links[head].parent;
        self.links[root].color = Color::Black; // root is always black
        newnode
    }

    /// `_Extract(where)`: unlinks the node and rebalances; returns the erased node.
    #[allow(clippy::too_many_lines)] // one C++ function, translated statement by statement
    fn extract(&mut self, where_: usize) -> usize {
        let erasednode = where_; // node to erase
        let successor = self.next(where_); // save successor iterator for return

        let fixnode; // the node to recolor as needed
        let mut fixnodeparent; // parent of fixnode (which may be nil)
        let mut pnode = erasednode;

        if self.links[self.links[pnode].left].isnil {
            fixnode = self.links[pnode].right; // stitch up right subtree
        } else if self.links[self.links[pnode].right].isnil {
            fixnode = self.links[pnode].left; // stitch up left subtree
        } else {
            // two subtrees, must lift successor node to replace erased
            pnode = successor; // pnode is successor node
            fixnode = self.links[pnode].right; // fixnode is only subtree
        }
        let mut fixnode = fixnode;

        if pnode == erasednode {
            // at most one subtree, relink it
            fixnodeparent = self.links[erasednode].parent;
            if !self.links[fixnode].isnil {
                self.links[fixnode].parent = fixnodeparent; // link up
            }

            if self.links[HEAD].parent == erasednode {
                self.links[HEAD].parent = fixnode; // link down from root
            } else if self.links[fixnodeparent].left == erasednode {
                self.links[fixnodeparent].left = fixnode; // link down to left
            } else {
                self.links[fixnodeparent].right = fixnode; // link down to right
            }

            if self.links[HEAD].left == erasednode {
                self.links[HEAD].left = if self.links[fixnode].isnil {
                    fixnodeparent // smallest is parent of erased node
                } else {
                    self.min(fixnode) // smallest in relinked subtree
                };
            }

            if self.links[HEAD].right == erasednode {
                self.links[HEAD].right = if self.links[fixnode].isnil {
                    fixnodeparent // largest is parent of erased node
                } else {
                    self.max(fixnode) // largest in relinked subtree
                };
            }
        } else {
            // erased has two subtrees, pnode is successor to erased
            let eleft = self.links[erasednode].left;
            self.links[eleft].parent = pnode; // link left up
            self.links[pnode].left = eleft; // link successor down

            if pnode == self.links[erasednode].right {
                fixnodeparent = pnode; // successor is next to erased
            } else {
                // successor further down, link in place of erased
                fixnodeparent = self.links[pnode].parent; // parent is successor's
                if !self.links[fixnode].isnil {
                    self.links[fixnode].parent = fixnodeparent; // link fix up
                }

                self.links[fixnodeparent].left = fixnode; // link fix down
                let eright = self.links[erasednode].right;
                self.links[pnode].right = eright; // link next down
                self.links[eright].parent = pnode; // right up
            }

            let eparent = self.links[erasednode].parent;
            if self.links[HEAD].parent == erasednode {
                self.links[HEAD].parent = pnode; // link down from root
            } else if self.links[eparent].left == erasednode {
                self.links[eparent].left = pnode; // link down to left
            } else {
                self.links[eparent].right = pnode; // link down to right
            }

            self.links[pnode].parent = eparent; // link successor up
            let c = self.links[pnode].color;
            self.links[pnode].color = self.links[erasednode].color; // recolor it
            self.links[erasednode].color = c;
        }

        if self.links[erasednode].color == Color::Black {
            // erasing black link, must recolor/rebalance tree
            while fixnode != self.links[HEAD].parent && self.links[fixnode].color == Color::Black {
                if fixnode == self.links[fixnodeparent].left {
                    // fixup left subtree
                    pnode = self.links[fixnodeparent].right;
                    if self.links[pnode].color == Color::Red {
                        // rotate red up from right subtree
                        self.links[pnode].color = Color::Black;
                        self.links[fixnodeparent].color = Color::Red;
                        self.lrotate(fixnodeparent);
                        pnode = self.links[fixnodeparent].right;
                    }

                    if self.links[pnode].isnil {
                        fixnode = fixnodeparent; // shouldn't happen
                    } else if self.links[self.links[pnode].left].color == Color::Black
                        && self.links[self.links[pnode].right].color == Color::Black
                    {
                        // redden right subtree with black children
                        self.links[pnode].color = Color::Red;
                        fixnode = fixnodeparent;
                    } else {
                        // must rearrange right subtree
                        if self.links[self.links[pnode].right].color == Color::Black {
                            // rotate red up from left sub-subtree
                            let pleft = self.links[pnode].left;
                            self.links[pleft].color = Color::Black;
                            self.links[pnode].color = Color::Red;
                            self.rrotate(pnode);
                            pnode = self.links[fixnodeparent].right;
                        }

                        self.links[pnode].color = self.links[fixnodeparent].color;
                        self.links[fixnodeparent].color = Color::Black;
                        let pright = self.links[pnode].right;
                        self.links[pright].color = Color::Black;
                        self.lrotate(fixnodeparent);
                        break; // tree now recolored/rebalanced
                    }
                } else {
                    // fixup right subtree
                    pnode = self.links[fixnodeparent].left;
                    if self.links[pnode].color == Color::Red {
                        // rotate red up from left subtree
                        self.links[pnode].color = Color::Black;
                        self.links[fixnodeparent].color = Color::Red;
                        self.rrotate(fixnodeparent);
                        pnode = self.links[fixnodeparent].left;
                    }

                    if self.links[pnode].isnil {
                        fixnode = fixnodeparent; // shouldn't happen
                    } else if self.links[self.links[pnode].right].color == Color::Black
                        && self.links[self.links[pnode].left].color == Color::Black
                    {
                        // redden left subtree with black children
                        self.links[pnode].color = Color::Red;
                        fixnode = fixnodeparent;
                    } else {
                        // must rearrange left subtree
                        if self.links[self.links[pnode].left].color == Color::Black {
                            // rotate red up from right sub-subtree
                            let pright = self.links[pnode].right;
                            self.links[pright].color = Color::Black;
                            self.links[pnode].color = Color::Red;
                            self.lrotate(pnode);
                            pnode = self.links[fixnodeparent].left;
                        }

                        self.links[pnode].color = self.links[fixnodeparent].color;
                        self.links[fixnodeparent].color = Color::Black;
                        let pleft = self.links[pnode].left;
                        self.links[pleft].color = Color::Black;
                        self.rrotate(fixnodeparent);
                        break; // tree now recolored/rebalanced
                    }
                }
                fixnodeparent = self.links[fixnode].parent;
            }

            self.links[fixnode].color = Color::Black; // stopping node is black
        }

        if 0 < self.size {
            self.size -= 1;
        }

        erasednode
    }

    /// `_Find_lower_bound(keyval)`: the search from the root, with the location a new key would
    /// take and the first node that does not go before it.
    fn find_lower_bound(&self, keyval: &K) -> FindResult {
        let mut result = FindResult {
            location: TreeId {
                parent: self.links[HEAD].parent,
                child: Child::Right,
            },
            bound: HEAD,
        };
        let mut trynode = result.location.parent;
        while !self.links[trynode].isnil {
            result.location.parent = trynode;
            if (self.comp)(self.key(trynode), keyval) {
                result.location.child = Child::Right;
                trynode = self.links[trynode].right;
            } else {
                result.location.child = Child::Left;
                result.bound = trynode;
                trynode = self.links[trynode].left;
            }
        }
        result
    }

    /// `_Lower_bound_duplicate(bound, keyval)`: the bound is an element equivalent to the key.
    fn lower_bound_duplicate(&self, bound: usize, keyval: &K) -> bool {
        !self.links[bound].isnil && !(self.comp)(keyval, self.key(bound))
    }

    /// `_Find_hint(hint, keyval)` for unique keys: where the key goes, near the hint if it fits
    /// there, and whether an equivalent element is already in the map.
    fn find_hint(&self, hint: usize, keyval: &K) -> (TreeId, bool) {
        let head = HEAD;
        if self.links[hint].isnil {
            // insert at end if after last element
            let root = self.links[head].parent;
            if self.links[root].isnil || (self.comp)(self.key(self.links[head].right), keyval) {
                return (
                    TreeId {
                        parent: self.links[head].right,
                        child: Child::Right,
                    },
                    false,
                );
            }
        } else if hint == self.links[head].left {
            // insert at beginning if less than first element
            if (self.comp)(keyval, self.key(hint)) {
                return (
                    TreeId {
                        parent: hint,
                        child: Child::Left,
                    },
                    false,
                );
            }
        } else if (self.comp)(keyval, self.key(hint)) {
            // keyval < *hint
            let prev = self.prev(hint);
            if (self.comp)(self.key(prev), keyval) {
                // keyval < *hint and keyval > *prev(hint); insert here
                if self.links[self.links[prev].right].isnil {
                    return (
                        TreeId {
                            parent: prev,
                            child: Child::Right,
                        },
                        false,
                    );
                }
                return (
                    TreeId {
                        parent: hint,
                        child: Child::Left,
                    },
                    false,
                );
            }
        } else if (self.comp)(self.key(hint), keyval) {
            // keyval > *hint
            let next = self.next(hint);
            if self.links[next].isnil || (self.comp)(keyval, self.key(next)) {
                // keyval > *hint and keyval < *next(hint); insert here
                if self.links[self.links[hint].right].isnil {
                    return (
                        TreeId {
                            parent: hint,
                            child: Child::Right,
                        },
                        false,
                    );
                }
                return (
                    TreeId {
                        parent: next,
                        child: Child::Left,
                    },
                    false,
                );
            }
        } else {
            // keyval is equivalent to *hint; don't insert
            return (
                TreeId {
                    parent: hint,
                    child: Child::Right,
                },
                true,
            );
        }

        let loc = self.find_lower_bound(keyval);
        if self.lower_bound_duplicate(loc.bound, keyval) {
            return (
                TreeId {
                    parent: loc.bound,
                    child: Child::Unused,
                },
                true,
            );
        }
        (loc.location, false)
    }

    /// `insert(const_iterator where, value_type&& val)` (`_Emplace_hint`): inserts the element
    /// near the hint unless an equivalent key is in the map; returns the inserted node, or the
    /// equivalent one.
    pub fn insert_hint(&mut self, hint: usize, key: K, value: V) -> usize {
        let (location, duplicate) = self.find_hint(hint, &key);
        if duplicate {
            return location.parent;
        }
        let inserted = self.buynode((key, value));
        self.insert_node(location, inserted)
    }

    /// `lower_bound(keyval)`: the first element that does not go before the key, `end()` if none.
    #[must_use]
    pub fn lower_bound(&self, keyval: &K) -> usize {
        self.find_lower_bound(keyval).bound
    }

    /// `erase(const_iterator where)` (`_Erase_unchecked`): removes the element; returns the next
    /// node.
    pub fn erase(&mut self, where_: usize) -> usize {
        let successor = self.next(where_); // save successor iterator for return
        let erasednode = self.extract(where_); // node to erase
        self.freenode(erasednode); // delete erased node
        successor
    }

    /// `clear()`: every node freed, the head linked to itself.
    pub fn clear(&mut self) {
        self.links.truncate(1);
        self.vals.truncate(1);
        self.free.clear();
        let head = &mut self.links[HEAD];
        head.parent = HEAD;
        head.left = HEAD;
        head.right = HEAD;
        self.size = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::{Color, HEAD, StdMap};

    /// Checks the red-black properties and the head's links; returns the black height.
    fn check<K, V, C: Fn(&K, &K) -> bool>(m: &StdMap<K, V, C>, node: usize) -> usize {
        if m.links[node].isnil {
            return 1;
        }
        let l = m.links[node].left;
        let r = m.links[node].right;
        if m.links[node].color == Color::Red {
            assert_eq!(m.links[l].color, Color::Black);
            assert_eq!(m.links[r].color, Color::Black);
        }
        if !m.links[l].isnil {
            assert_eq!(m.links[l].parent, node);
        }
        if !m.links[r].isnil {
            assert_eq!(m.links[r].parent, node);
        }
        let hl = check(m, l);
        let hr = check(m, r);
        assert_eq!(hl, hr);
        hl + usize::from(m.links[node].color == Color::Black)
    }

    fn keys(m: &StdMap<i32, (), fn(&i32, &i32) -> bool>) -> Vec<i32> {
        let mut out = Vec::new();
        let mut it = m.begin();
        while it != m.end() {
            out.push(*m.key(it));
            it = m.next(it);
        }
        out
    }

    #[test]
    fn inserts_and_erases_keep_order_and_balance() {
        let less: fn(&i32, &i32) -> bool = |a, b| a < b;
        let mut m = StdMap::new(less);
        let mut x: u32 = 12345;
        let mut its = Vec::new();
        for i in 0..400 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let k = (x >> 8) as i32 % 1000;
            // Alternate the hints: end(), begin(), and the last insert.
            let hint = match i % 3 {
                0 => m.end(),
                1 => m.begin(),
                _ => its.last().copied().unwrap_or(HEAD),
            };
            its.push(m.insert_hint(hint, k, ()));
            let root = m.links[HEAD].parent;
            check(&m, root);
        }
        let k = keys(&m);
        assert!(k.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(k.len(), m.len());
        for (i, it) in its.iter().enumerate() {
            if i % 2 == 0 && m.vals[*it].is_some() {
                let next = m.next(*it);
                assert_eq!(m.erase(*it), next);
                let root = m.links[HEAD].parent;
                check(&m, root);
            }
        }
        let k2 = keys(&m);
        assert!(k2.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(k2.len(), m.len());
        assert_eq!(m.lower_bound(&-1), m.begin());
        assert_eq!(m.lower_bound(&10_000), m.end());
        m.clear();
        assert!(m.is_empty());
        assert_eq!(m.begin(), m.end());
    }
}
