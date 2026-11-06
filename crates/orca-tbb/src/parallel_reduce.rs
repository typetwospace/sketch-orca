// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Intel oneTBB code: this file is translated from include/oneapi/tbb/parallel_reduce.h
// (reduction_tree_node with its right zombie, start_reduce, deterministic_reduction_tree_node,
// start_deterministic_reduce, lambda_reduce_body and the parallel_reduce and
// parallel_deterministic_reduce overloads) and the fold_tree of include/oneapi/tbb/
// partitioner.h of oneTBB 2021.5.0 (the tag v2021.5.0 of uxlfoundation/oneTBB), split for split
// and join for join, on 2026-11-06. Its copyright holder is Intel Corporation, and it is under
// Apache-2.0 (licenses/Apache-2.0.txt).

//! `tbb::parallel_reduce` and `tbb::parallel_deterministic_reduce`.
//!
//! `parallel_reduce` splits the range as `parallel_for` does, but splits the body only when a
//! right part starts while its left sibling is still running (oneTBB's "right zombie"). On one
//! thread that happens only when a body, inside a leaf, waits for tasks spawned outside the
//! reduction (an outer task group's): oneTBB then splits the body while it runs, which its Body
//! requirements allow and Rust's borrowing does not, so a serial arena panics there instead.
//! Otherwise the one body runs every leaf from left to right and no join is made: the result is
//! the left fold of the leaves. That fold is the result in a parallel arena too, because it is
//! the result the parity oracle sees: there the leaves run one after another on the calling
//! thread, and loops nested in the body still run on the pool.
//!
//! `parallel_deterministic_reduce` splits the body at every split of the range, so its result
//! is a fixed tree of joins over the leaves: the body of a task joins the body split off for
//! its last right part first, then the one before, down to the first. In a parallel arena the
//! parts of the tree run on the rayon pool and the joins are made in the same order, so the
//! result is the same as on one thread.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::concurrency::{WaitState, context, run_and_wait_serial, run_leaf, spawn_serial};
use crate::partitioner::{DeterministicPartitioner, PartTask, Partitioner, walk};
use crate::range::Range;

/// A reduction body (oneTBB's `parallel_reduce` Body requirements).
pub trait ReduceBody<R>: Sized {
    /// `Body(Body& b, split)`: a new body for a part split off, made from `self`.
    fn split(&mut self) -> Self;

    /// `operator()(Range& r)`: accumulates the leaf `range`.
    fn apply(&mut self, range: R);

    /// `join(Body& rhs)`: takes the result of the body split off for the part to the right.
    fn join(&mut self, right: Self);
}

/// `parallel_reduce(range, body, partitioner)`: runs `body` over the leaves of `range`, left to
/// right. Nothing runs for an empty range.
///
/// # Panics
///
/// Resumes the first panic of the body, after the tasks that had started have finished. In a
/// serial arena, panics if a leaf waits for tasks spawned outside the reduction and so lets a
/// part of the reduction start beside it (see the module documentation).
pub fn parallel_reduce<R, B>(range: R, partitioner: Partitioner, body: &mut B)
where
    R: Range,
    B: ReduceBody<R>,
{
    if range.is_empty() {
        return;
    }
    let context = context();
    let root = PartTask::new(range, partitioner, context.concurrency);
    let wait = WaitState::new();
    if context.serial {
        reduce_serial(root, body, &wait, false);
    } else {
        walk(root, &mut |leaf| {
            run_leaf(&wait, || body.apply(leaf));
        });
    }
    wait.finish();
}

/// `parallel_deterministic_reduce(range, body, partitioner)`: runs `body`, and a body split off
/// at every split, over the leaves of `range`, and joins them back in the order of the split
/// tree. Nothing runs for an empty range.
///
/// # Panics
///
/// Resumes the first panic of a body, after the tasks that had started have finished.
pub fn parallel_deterministic_reduce<R, B>(
    range: R,
    partitioner: DeterministicPartitioner,
    body: &mut B,
) where
    R: Range + Send,
    B: ReduceBody<R> + Send,
{
    if range.is_empty() {
        return;
    }
    let context = context();
    let root = PartTask::new(range, partitioner.into(), context.concurrency);
    let wait = WaitState::new();
    if context.serial {
        reduce_serial(root, body, &wait, true);
    } else {
        deterministic_parallel(root, body, &wait);
    }
    wait.finish();
}

/// `parallel_reduce(range, identity, real_body, reduction, partitioner)`: the value of
/// [`parallel_reduce`] with `lambda_reduce_body`, which starts from `identity`, maps each leaf
/// and the value so far to a new value with `real_body`, and joins two values with `reduction`.
///
/// # Panics
///
/// As [`parallel_reduce`].
pub fn parallel_reduce_fn<R, V, RB, RD>(
    range: R,
    partitioner: Partitioner,
    identity: V,
    real_body: RB,
    reduction: RD,
) -> V
where
    R: Range,
    V: Clone,
    RB: Fn(R, V) -> V,
    RD: Fn(V, V) -> V,
{
    let mut body = LambdaBody::new(&identity, &real_body, &reduction);
    parallel_reduce(range, partitioner, &mut body);
    body.into_value()
}

/// `parallel_deterministic_reduce(range, identity, real_body, reduction, partitioner)`: the
/// value of [`parallel_deterministic_reduce`] with `lambda_reduce_body`.
///
/// # Panics
///
/// As [`parallel_deterministic_reduce`].
pub fn parallel_deterministic_reduce_fn<R, V, RB, RD>(
    range: R,
    partitioner: DeterministicPartitioner,
    identity: V,
    real_body: RB,
    reduction: RD,
) -> V
where
    R: Range + Send,
    V: Clone + Send + Sync,
    RB: Fn(R, V) -> V + Sync,
    RD: Fn(V, V) -> V + Sync,
{
    let mut body = LambdaBody::new(&identity, &real_body, &reduction);
    parallel_deterministic_reduce(range, partitioner, &mut body);
    body.into_value()
}

/// `lambda_reduce_body`: a value, the identity a split body starts from, and the two functions.
struct LambdaBody<'f, V, RB, RD> {
    identity: &'f V,
    real_body: &'f RB,
    reduction: &'f RD,
    /// None only while a function runs, or after one panicked.
    value: Option<V>,
}

const LOST_VALUE: &str = "orca-tbb: a reduction's value was lost to a panic";

impl<'f, V: Clone, RB, RD> LambdaBody<'f, V, RB, RD> {
    fn new(identity: &'f V, real_body: &'f RB, reduction: &'f RD) -> Self {
        LambdaBody {
            identity,
            real_body,
            reduction,
            value: Some(identity.clone()),
        }
    }

    /// `result()`.
    fn into_value(self) -> V {
        self.value.expect(LOST_VALUE)
    }
}

impl<R, V: Clone, RB: Fn(R, V) -> V, RD: Fn(V, V) -> V> ReduceBody<R>
    for LambdaBody<'_, V, RB, RD>
{
    fn split(&mut self) -> Self {
        LambdaBody::new(self.identity, self.real_body, self.reduction)
    }

    fn apply(&mut self, range: R) {
        let value = self.value.take().expect(LOST_VALUE);
        self.value = Some((self.real_body)(range, value));
    }

    fn join(&mut self, right: Self) {
        let left = self.value.take().expect(LOST_VALUE);
        self.value = Some((self.reduction)(left, right.into_value()));
    }
}

/// A body of a reduction in a serial arena: the caller's, or one split off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyId {
    Root,
    Split(usize),
}

/// A node of the reduction tree: oneTBB's `tree_node` with the body it joins into and the body
/// split off for its right part (`reduction_tree_node`'s zombie, or
/// `deterministic_reduction_tree_node`'s right body). Node 0 is the root's `wait_node`.
struct Node {
    parent: Option<usize>,
    /// `m_ref_count`: the children not yet finished.
    refs: u32,
    left: BodyId,
    right: Option<BodyId>,
}

/// A reduction in a serial arena: its bodies and its tree.
struct Reduction<'b, R, B> {
    root: RefCell<&'b mut B>,
    bodies: RefCell<Vec<Option<B>>>,
    nodes: RefCell<Vec<Node>>,
    deterministic: bool,
    wait: Arc<WaitState>,
    range: PhantomData<fn(R)>,
}

const BUSY_BODY: &str = "orca-tbb: a reduction body was needed while it ran a leaf (oneTBB \
                         splits a body while it runs when a leaf waits for outside tasks; a \
                         Rust body cannot be)";

impl<R, B: ReduceBody<R>> Reduction<'_, R, B> {
    fn with_body<T>(&self, id: BodyId, f: impl FnOnce(&mut B) -> T) -> T {
        match id {
            BodyId::Root => {
                let mut root = self.root.try_borrow_mut().expect(BUSY_BODY);
                f(&mut **root)
            }
            BodyId::Split(i) => {
                let mut body = self.bodies.borrow_mut()[i].take().expect(BUSY_BODY);
                let out = f(&mut body);
                self.bodies.borrow_mut()[i] = Some(body);
                out
            }
        }
    }

    fn add_body(&self, body: B) -> BodyId {
        let mut bodies = self.bodies.borrow_mut();
        bodies.push(Some(body));
        BodyId::Split(bodies.len() - 1)
    }

    fn add_node(&self, node: Node) -> usize {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(node);
        nodes.len() - 1
    }

    /// `fold_tree`: a child of `node` has finished. A node whose children have all finished
    /// joins its right body into its left one (unless the reduction is cancelled) and tells its
    /// parent, up to the root's wait node.
    fn fold(&self, mut node: usize) {
        loop {
            let (parent, left, right) = {
                let mut nodes = self.nodes.borrow_mut();
                let n = &mut nodes[node];
                n.refs -= 1;
                if n.refs > 0 {
                    return;
                }
                (n.parent, n.left, n.right.take())
            };
            let Some(parent) = parent else { return };
            if let Some(BodyId::Split(i)) = right {
                let right_body = self.bodies.borrow_mut()[i].take();
                if let Some(right_body) = right_body
                    && !self.wait.is_cancelled()
                {
                    self.with_body(left, |l| l.join(right_body));
                }
            }
            node = parent;
        }
    }
}

/// A task of a reduction in a serial arena: `start_reduce` or `start_deterministic_reduce`.
struct ReduceTask<R> {
    part: PartTask<R>,
    body: BodyId,
    /// `my_parent`, a node index.
    parent: usize,
    /// `is_right_child`.
    right_child: bool,
}

fn reduce_serial<R: Range, B: ReduceBody<R>>(
    root: PartTask<R>,
    body: &mut B,
    wait: &Arc<WaitState>,
    deterministic: bool,
) {
    let reduction = Reduction {
        root: RefCell::new(body),
        bodies: RefCell::new(Vec::new()),
        nodes: RefCell::new(vec![Node {
            parent: None,
            refs: 1,
            left: BodyId::Root,
            right: None,
        }]),
        deterministic,
        wait: Arc::clone(wait),
        range: PhantomData,
    };
    let task = ReduceTask {
        part: root,
        body: BodyId::Root,
        parent: 0,
        right_child: false,
    };
    run_and_wait_serial(wait, || reduce_task_serial(task, &reduction));
}

/// `start_reduce::execute` and `start_deterministic_reduce::execute` in a serial arena.
fn reduce_task_serial<'a, R: Range + 'a, B: ReduceBody<R>>(
    mut task: ReduceTask<R>,
    reduction: &'a Reduction<'_, R, B>,
) {
    task.part.start();
    if !reduction.deterministic
        && task.right_child
        && reduction.nodes.borrow()[task.parent].refs == 2
    {
        // The left sibling is still running: a zombie body for this part.
        let zombie = reduction.with_body(task.body, B::split);
        let id = reduction.add_body(zombie);
        reduction.nodes.borrow_mut()[task.parent].right = Some(id);
        task.body = id;
    }
    while task.part.wants_split() {
        // offer_work: the deterministic tree node splits the body before the range splits; the
        // other node keeps the body, which the right child shares.
        let (child_body, right) = if reduction.deterministic {
            let split = reduction.with_body(task.body, B::split);
            let id = reduction.add_body(split);
            (id, Some(id))
        } else {
            (task.body, None)
        };
        let part = task.part.split_child();
        let node = reduction.add_node(Node {
            parent: Some(task.parent),
            refs: 2,
            left: task.body,
            right,
        });
        task.parent = node;
        task.right_child = false;
        let child = ReduceTask {
            part,
            body: child_body,
            parent: node,
            right_child: true,
        };
        // SAFETY: reduce_serial waits for the reduction's group before `reduction` goes away.
        unsafe {
            spawn_serial(
                &reduction.wait,
                Box::new(move || reduce_task_serial(child, reduction)),
            );
        }
    }
    for leaf in task.part.into_leaves() {
        reduction.with_body(task.body, |b| b.apply(leaf));
        if reduction.wait.is_cancelled() {
            break;
        }
    }
    reduction.fold(task.parent);
}

/// `start_deterministic_reduce::execute` in a parallel arena: a body split off for each right
/// part, the parts on the rayon pool, then the joins from the last split to the first.
fn deterministic_parallel<R: Range + Send, B: ReduceBody<R> + Send>(
    mut task: PartTask<R>,
    body: &mut B,
    wait: &Arc<WaitState>,
) {
    task.start();
    let mut children = Vec::new();
    let mut rights = Vec::new();
    while task.wants_split() {
        rights.push(body.split());
        children.push(task.split_child());
    }
    let run_own = |task: PartTask<R>, body: &mut B| {
        for leaf in task.into_leaves() {
            if !run_leaf(wait, || body.apply(leaf)) {
                break;
            }
        }
    };
    if children.is_empty() {
        run_own(task, body);
        return;
    }
    rayon::scope(|s| {
        for (child, right) in children.into_iter().zip(rights.iter_mut()) {
            s.spawn(move |_| deterministic_parallel(child, right, wait));
        }
        run_own(task, &mut *body);
    });
    for right in rights.into_iter().rev() {
        if wait.is_cancelled() {
            break;
        }
        body.join(right);
    }
}
