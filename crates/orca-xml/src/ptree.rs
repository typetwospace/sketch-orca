// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.
//
// Carries Boost code: Boost.PropertyTree 1.84.0's basic_ptree<std::string, std::string> as the 3MF
// reader uses it (boost/property_tree/ptree.hpp and detail/ptree_implementation.hpp: data,
// get_child, get_child_optional, get_value, get_value_optional, get, get_optional, walk_path and
// find), string_path.hpp (reduce, empty), exceptions.hpp and detail/exception_implementation.hpp
// (ptree_error, ptree_bad_data, ptree_bad_path), stream_translator.hpp (customize_stream::extract
// and stream_translator::get_value), id_translator.hpp, detail/xml_parser_read_rapidxml.hpp
// (read_xml_internal and read_xml_node, flags 0), detail/xml_parser_utils.hpp (xmlattr,
// xmlcomment) and detail/file_parser_error.hpp and xml_parser_error.hpp (format_what), with the
// names boost/core/type_name.hpp gives the four types, translated on 2026-11-11 by
// orca-ptree-port; read_xml_node's and walk_path's recursion turned into loops over stacks of
// their own on 2026-11-11 by orca-xml-stack. Copyright (C) 2002-2006 and 2007 Marcin Kalicinski,
// (C) 2009 Sebastian Redl and (C) 2021 Peter Dimov, under the Boost Software License, version 1.0
// (licenses/BSL-1.0.txt).

//! Boost 1.84's `property_tree` as the 3MF reader reads its cut information and height ranges:
//! [`read_xml`] is `read_xml(std::istringstream, ptree)` with flags 0, on rapidxml
//! (`crate::rapidxml`), and a [`Ptree`] answers `get_child`, `get<T>` with and without a default,
//! `data()` and the iteration of its children as `basic_ptree<std::string, std::string>` does.
//!
//! The tree is plain: each node's data and its children in order, each with its key. `get_child`
//! and `get` walk a path split at each `.` (`string_path`), and find the first child with an equal
//! key, in document order (the ordered index's `find` among equal keys). A typed `get` reads the
//! data as `stream_translator` does, `stream >> value` through MSVC's `num_get` (orca-fmt's
//! [`istream`](orca_fmt::istream)), then white space to the end and nothing else; `get` of bytes
//! (`std::string`) takes the data as it is.
//!
//! Strings are bytes, as `std::string` holds them: a key, the data and a `what()` may hold any
//! byte, a NUL included.
//!
//! Nothing here recurses once per level of a tree, where the C++ does: `read_xml` copies
//! rapidxml's nodes into the tree over a stack of its own (`read_xml_node` calls itself for each
//! child element), `walk_path` is a loop, and a [`Ptree`] is cloned, compared, printed and dropped
//! over stacks of their own (by hand, as the derived ones would recurse). So the port reads, keeps
//! and frees a tree as deep as memory allows, where the C++ engine's stack overflows
//! (`parity/HAZARDS.md`, orca-model's 3MF table).

use crate::rapidxml::{self, DOCUMENT, Dom, NodeType};
use orca_fmt::istream::{Extract, get_f32, get_f64, get_i32, get_u64, is_space};
use std::fmt;

/// `basic_ptree<std::string, std::string>`: the data and the children in order, each with its
/// key.
#[derive(Default)]
pub struct Ptree {
    data: Vec<u8>,
    children: Vec<(Vec<u8>, Ptree)>,
}

impl Clone for Ptree {
    /// The tree copied over a stack of its own: each tree being copied, with the index of its next
    /// child to copy, its key and its copy so far, innermost last. A copy goes into its parent's
    /// copy once all of its children are in it.
    fn clone(&self) -> Self {
        struct Frame<'a> {
            from: &'a Ptree,
            next: usize,
            key: &'a [u8],
            copy: Ptree,
        }
        fn frame<'a>(from: &'a Ptree, key: &'a [u8]) -> Frame<'a> {
            Frame {
                from,
                next: 0,
                key,
                copy: Ptree {
                    data: from.data.clone(),
                    children: Vec::with_capacity(from.children.len()),
                },
            }
        }
        let mut stack = vec![frame(self, &[])];
        loop {
            let top = stack.last_mut().expect("the root's frame is popped last");
            let from = top.from;
            if let Some((key, child)) = from.children.get(top.next) {
                top.next += 1;
                stack.push(frame(child, key));
                continue;
            }
            let done = stack.pop().expect("the frame just read");
            match stack.last_mut() {
                Some(parent) => parent.copy.children.push((done.key.to_vec(), done.copy)),
                None => return done.copy,
            }
        }
    }
}

impl Drop for Ptree {
    /// The children are taken out onto a stack of their own, and theirs in turn, so that each tree
    /// dropped here has no children left and dropping a deep tree does not recurse once per level.
    fn drop(&mut self) {
        if self.children.is_empty() {
            return;
        }
        let mut stack: Vec<Ptree> = self.children.drain(..).map(|(_, t)| t).collect();
        while let Some(mut t) = stack.pop() {
            stack.extend(t.children.drain(..).map(|(_, c)| c));
        }
    }
}

impl PartialEq for Ptree {
    /// Equal data and equal children in order, each with an equal key: the pairs of trees still to
    /// compare are kept on a stack of their own.
    fn eq(&self, other: &Self) -> bool {
        let mut stack = vec![(self, other)];
        while let Some((a, b)) = stack.pop() {
            if a.data != b.data || a.children.len() != b.children.len() {
                return false;
            }
            for ((ka, ta), (kb, tb)) in a.children.iter().zip(&b.children) {
                if ka != kb {
                    return false;
                }
                stack.push((ta, tb));
            }
        }
        true
    }
}

impl Eq for Ptree {}

impl fmt::Debug for Ptree {
    /// `Ptree { data: "...", children: [("key", Ptree { ... }), ...] }`, the bytes escaped, written
    /// over a stack of its own: each tree's children still to write, and whether one was written.
    /// The form differs from a derived `Debug` on purpose: the byte strings are escaped as text,
    /// not listed as numbers, and nothing recurses, so a tree 1,000,000 levels deep prints.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Ptree {{ data: \"{}\", children: [",
            self.data.escape_ascii()
        )?;
        let mut stack = vec![(self.children.iter(), false)];
        while let Some((children, written)) = stack.last_mut() {
            if let Some((key, child)) = children.next() {
                if *written {
                    f.write_str(", ")?;
                }
                *written = true;
                write!(
                    f,
                    "(\"{}\", Ptree {{ data: \"{}\", children: [",
                    key.escape_ascii(),
                    child.data.escape_ascii()
                )?;
                stack.push((child.children.iter(), false));
            } else {
                stack.pop();
                f.write_str(if stack.is_empty() { "] }" } else { "] })" })?;
            }
        }
        Ok(())
    }
}

/// `xml_parser_error` (a `file_parser_error`): what rapidxml threw and the line `read_xml`
/// counted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlParserError {
    /// `message()`: rapidxml's `parse_error::what()`.
    pub message: &'static str,
    /// `line()`: the line feeds before where rapidxml stopped, plus one, as an `unsigned long`.
    pub line: u32,
}

impl XmlParserError {
    /// `what()`: `format_what(message, "", line)`, the file named `<unspecified file>` since the
    /// reader parses a stream, and the line only when it is not 0.
    #[must_use]
    pub fn what(&self) -> String {
        let mut s = String::from("<unspecified file>");
        if self.line > 0 {
            s.push('(');
            s.push_str(&self.line.to_string());
            s.push(')');
        }
        s.push_str(": ");
        s.push_str(self.message);
        s
    }
}

impl fmt::Display for XmlParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.what())
    }
}

impl std::error::Error for XmlParserError {}

/// A `ptree_error` that `get_child` or `get` throws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PtreeError {
    /// `ptree_bad_path`: `No such node (<path>)`, the path whole as it was asked.
    BadPath(Vec<u8>),
    /// `ptree_bad_data`: `conversion of data to type "<type>" failed`.
    BadData(Vec<u8>),
}

impl PtreeError {
    /// `what()`.
    #[must_use]
    pub fn what(&self) -> &[u8] {
        match self {
            Self::BadPath(w) | Self::BadData(w) => w,
        }
    }
}

impl fmt::Display for PtreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(self.what()))
    }
}

impl std::error::Error for PtreeError {}

/// A type `get` converts a node's data to, with the translator `translator_between` picks for it.
pub trait PtreeValue: Sized {
    /// `boost::core::type_name<Type>()`, as the conversion error names the type.
    const TYPE_NAME: &'static str;

    /// The translator's `get_value(data)`: the value, or `None` when it refuses the data.
    fn get_value(data: &[u8]) -> Option<Self>;
}

/// `stream_translator::get_value`: `iss >> e`, then `iss >> std::ws` when not at the end; a value
/// only when `num_get` did not fail and nothing but white space follows it.
fn stream_translate<T>(x: Extract<T>, data: &[u8]) -> Option<T> {
    // customized::extract(iss, e): s >> e; if (!s.eof()) s >> std::ws;
    // if (iss.fail() || iss.bad() || iss.get() != Traits::eof()) return none;
    if x.fail {
        return None;
    }
    if data[x.read..].iter().all(|&c| is_space(c)) {
        x.value
    } else {
        None
    }
}

/// `int`: `operator>>(int&)`, a `long` through `_Stolx`.
impl PtreeValue for i32 {
    const TYPE_NAME: &'static str = "int";

    fn get_value(data: &[u8]) -> Option<Self> {
        stream_translate(get_i32(data), data)
    }
}

/// `size_t`, which is `unsigned long long` on 64-bit Windows: `_Stoullx`.
impl PtreeValue for u64 {
    const TYPE_NAME: &'static str = "unsigned long long";

    fn get_value(data: &[u8]) -> Option<Self> {
        stream_translate(get_u64(data), data)
    }
}

/// `float`: `strtof` on the field `num_get` gathers.
impl PtreeValue for f32 {
    const TYPE_NAME: &'static str = "float";

    fn get_value(data: &[u8]) -> Option<Self> {
        stream_translate(get_f32(data), data)
    }
}

/// `double`: `strtod` on the field `num_get` gathers.
impl PtreeValue for f64 {
    const TYPE_NAME: &'static str = "double";

    fn get_value(data: &[u8]) -> Option<Self> {
        stream_translate(get_f64(data), data)
    }
}

/// `std::string`: `id_translator`, which takes the data as it is and never fails.
impl PtreeValue for Vec<u8> {
    const TYPE_NAME: &'static str = "std::string";

    fn get_value(data: &[u8]) -> Option<Self> {
        Some(data.to_vec())
    }
}

/// `string_path<std::string, id_translator<std::string>>` with the separator `.`: the path and
/// where its next part starts.
struct StringPath<'a> {
    value: &'a [u8],
    start: usize,
}

impl<'a> StringPath<'a> {
    fn new(value: &'a [u8]) -> Self {
        Self { value, start: 0 }
    }

    /// `empty()`: no part left.
    fn empty(&self) -> bool {
        self.start == self.value.len()
    }

    /// `reduce()`: the part up to the next separator, which is skipped unless the path ends there.
    fn reduce(&mut self) -> &'a [u8] {
        let next_sep = self.value[self.start..]
            .iter()
            .position(|&c| c == b'.')
            .map_or(self.value.len(), |i| self.start + i);
        let part = &self.value[self.start..next_sep];
        self.start = next_sep;
        if !self.empty() {
            // Unless we're at the end, skip the separator we found.
            self.start += 1;
        }
        part
    }
}

impl Ptree {
    /// An empty tree: no data, no children.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `data()`.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// `size()`: the number of children.
    #[must_use]
    pub fn size(&self) -> usize {
        self.children.len()
    }

    /// The children in order (`begin()` to `end()`), each as its key and its tree.
    pub fn children(&self) -> impl ExactSizeIterator<Item = (&[u8], &Ptree)> {
        self.children.iter().map(|(k, t)| (k.as_slice(), t))
    }

    /// `find(key)`: the first child whose key is equal, in the order the children were added.
    fn find(&self, key: &[u8]) -> Option<&Ptree> {
        self.children
            .iter()
            .find(|(k, _)| k.as_slice() == key)
            .map(|(_, t)| t)
    }

    /// `walk_path(p)`: the node the path names, or `None`; an empty path names this node. The C++
    /// calls itself on the child for the rest of the path; here the loop goes on from the child.
    fn walk_path(&self, p: &mut StringPath<'_>) -> Option<&Ptree> {
        let mut node = self;
        loop {
            if p.empty() {
                // I'm the child we're looking for.
                return Some(node);
            }
            // Recurse down the tree to find the path.
            let fragment = p.reduce();
            // No such child, or not done yet: go on from it.
            node = node.find(fragment)?;
        }
    }

    /// `get_child(path)`.
    ///
    /// # Errors
    /// `ptree_bad_path`, `No such node (<path>)`, when no node has the path.
    pub fn get_child(&self, path: &[u8]) -> Result<&Ptree, PtreeError> {
        let mut p = StringPath::new(path);
        self.walk_path(&mut p).ok_or_else(|| {
            let mut what = b"No such node (".to_vec();
            what.extend_from_slice(path);
            what.push(b')');
            PtreeError::BadPath(what)
        })
    }

    /// `get_child_optional(path)`.
    #[must_use]
    pub fn get_child_optional(&self, path: &[u8]) -> Option<&Ptree> {
        let mut p = StringPath::new(path);
        self.walk_path(&mut p)
    }

    /// `get_value<Type>()`.
    ///
    /// # Errors
    /// `ptree_bad_data`, `conversion of data to type "<type>" failed`, when the translator
    /// refuses the data.
    pub fn get_value<T: PtreeValue>(&self) -> Result<T, PtreeError> {
        if let Some(o) = self.get_value_optional::<T>() {
            return Ok(o);
        }
        let what = format!("conversion of data to type \"{}\" failed", T::TYPE_NAME);
        Err(PtreeError::BadData(what.into_bytes()))
    }

    /// `get_value_optional<Type>()`.
    #[must_use]
    pub fn get_value_optional<T: PtreeValue>(&self) -> Option<T> {
        T::get_value(&self.data)
    }

    /// `get<Type>(path)`: `get_child(path).get_value<Type>()`.
    ///
    /// # Errors
    /// What `get_child` or `get_value` throws.
    pub fn get<T: PtreeValue>(&self, path: &[u8]) -> Result<T, PtreeError> {
        self.get_child(path)?.get_value()
    }

    /// `get<Type>(path, default_value)`: `get_optional<Type>(path).get_value_or(default_value)`;
    /// nothing throws.
    #[must_use]
    pub fn get_or<T: PtreeValue>(&self, path: &[u8], default_value: T) -> T {
        self.get_optional(path).unwrap_or(default_value)
    }

    /// `get_optional<Type>(path)`.
    #[must_use]
    pub fn get_optional<T: PtreeValue>(&self, path: &[u8]) -> Option<T> {
        self.get_child_optional(path)?.get_value_optional()
    }
}

/// `read_xml(stream, pt)` with flags 0 on a `std::istringstream` of `bytes`: the stream's bytes
/// with a NUL appended, parsed by rapidxml with `parse_comment_nodes`, then each top-level node
/// read into a new tree.
///
/// A `std::istringstream` never fails to read, so `xml_parser_error("read error")` cannot be
/// thrown here.
///
/// # Errors
/// The `xml_parser_error` for rapidxml's `parse_error`, with the line feeds before where it
/// stopped counted in the buffer as rapidxml has rewritten it by then.
pub fn read_xml(bytes: &[u8]) -> Result<Ptree, XmlParserError> {
    // Load data into vector
    let mut v = bytes.to_vec();
    v.push(0); // zero-terminate

    match rapidxml::parse(&mut v) {
        // Create ptree from nodes
        Ok(dom) => Ok(read_xml_nodes(&dom, &v)),
        Err(e) => {
            // static_cast<long>(std::count(&v.front(), e.where<Ch>(), Ch('\n')) + 1), then the
            // unsigned long of xml_parser_error.
            let count = v[..e.at].iter().filter(|&&c| c == b'\n').count();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let line = (count as u64).wrapping_add(1) as i32 as u32;
            Err(XmlParserError {
                message: e.what,
                line,
            })
        }
    }
}

/// `Ch *` as `std::string` takes it: the bytes from `at` to the first NUL.
fn c_str(buf: &[u8], at: usize) -> Vec<u8> {
    let end = buf[at..]
        .iter()
        .position(|&c| c == 0)
        .map_or(buf.len(), |i| at + i);
    buf[at..end].to_vec()
}

/// `read_xml_internal`'s loop, `read_xml_node(child, local, 0)` for each of the document's nodes:
/// an element as a child keyed by its name (its attributes under `<xmlattr>` first, then its
/// children), data and CDATA appended to `pt`'s data, a comment as a child keyed
/// `<xmlcomment>`.
///
/// `read_xml_node` calls itself for each child of an element; here the elements being read are
/// kept on a stack of their own, innermost last, each with its key, its tree so far and its next
/// child. The C++ puts an element's tree into its parent first (`pt.push_back`) and fills it in
/// place; here it goes into its parent once it is filled. Its place among its siblings is the same,
/// since nothing else is added to the parent while the element is being read.
fn read_xml_nodes(dom: &Dom, buf: &[u8]) -> Ptree {
    struct Frame {
        key: Vec<u8>,
        pt: Ptree,
        next: Option<usize>,
    }
    // `local`, the tree the document's nodes are read into.
    let mut stack = vec![Frame {
        key: Vec::new(),
        pt: Ptree::new(),
        next: dom.nodes[DOCUMENT].first_node,
    }];
    loop {
        let top = stack.last_mut().expect("local's frame is popped last");
        let Some(child) = top.next else {
            // Every child read: the element's tree goes into its parent's, or `local` is done.
            let done = stack.pop().expect("the frame just read");
            match stack.last_mut() {
                Some(parent) => parent.pt.children.push((done.key, done.pt)),
                None => return done.pt,
            }
            continue;
        };
        let node = &dom.nodes[child];
        top.next = node.next_sibling;
        match node.kind {
            // Element nodes
            NodeType::Element => {
                // Create node
                let mut pt_node = Ptree::new();

                // Copy attributes
                let attributes = dom.attributes(child);
                if !attributes.is_empty() {
                    let mut pt_attr_root = Ptree::new();
                    for attr in attributes {
                        let data = buf[attr.value..attr.value + attr.value_size].to_vec();
                        pt_attr_root.children.push((
                            c_str(buf, attr.name),
                            Ptree {
                                data,
                                children: Vec::new(),
                            },
                        ));
                    }
                    pt_node.children.push((b"<xmlattr>".to_vec(), pt_attr_root));
                }

                // Copy children
                stack.push(Frame {
                    key: c_str(buf, node.name),
                    pt: pt_node,
                    next: node.first_node,
                });
            }

            // Data nodes
            NodeType::Data | NodeType::Cdata => {
                top.pt
                    .data
                    .extend_from_slice(&buf[node.value..node.value + node.value_size]);
            }

            // Comment nodes
            NodeType::Comment => {
                let data = buf[node.value..node.value + node.value_size].to_vec();
                top.pt.children.push((
                    b"<xmlcomment>".to_vec(),
                    Ptree {
                        data,
                        children: Vec::new(),
                    },
                ));
            }

            // Skip other node types
            NodeType::Document => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Ptree, PtreeError, read_xml};

    #[test]
    fn a_document_reads_into_a_tree() {
        let t =
            read_xml(b"<?xml version=\"1.0\"?>\n<r a=\"1\" b='x'><!--c--><v>2.5</v>t</r>").unwrap();
        let r = t.get_child(b"r").unwrap();
        let keys: Vec<&[u8]> = r.children().map(|(k, _)| k).collect();
        assert_eq!(keys, [&b"<xmlattr>"[..], b"<xmlcomment>", b"v"]);
        assert_eq!(r.data(), b"t");
        assert_eq!(t.get::<i32>(b"r.<xmlattr>.a"), Ok(1));
        assert_eq!(t.get::<f64>(b"r.v"), Ok(2.5));
        assert_eq!(t.get_or::<i32>(b"r.<xmlattr>.b", -1), -1);
        assert_eq!(
            t.get::<i32>(b"r.<xmlattr>.b"),
            Err(PtreeError::BadData(
                b"conversion of data to type \"int\" failed".to_vec()
            ))
        );
        assert_eq!(
            t.get_child(b"r.w").unwrap_err().what(),
            b"No such node (r.w)"
        );
        // "r." is "r"; an empty path is the node itself.
        assert!(std::ptr::eq(
            t.get_child(b"r.").unwrap(),
            t.get_child(b"r").unwrap()
        ));
        assert!(std::ptr::eq(t.get_child(b"").unwrap(), &t));
    }

    #[test]
    fn an_error_names_its_line() {
        let e = read_xml(b"<a/>\nx").unwrap_err();
        assert_eq!(e.what(), "<unspecified file>(2): expected <");
    }

    #[test]
    fn a_tree_clones_compares_and_prints_in_order() {
        let t = read_xml(b"<r a='1'><!--c-->x<v>2</v><v/>y</r>").unwrap();
        let c = t.clone();
        assert_eq!(c, t);
        assert_eq!(
            format!("{c:?}"),
            "Ptree { data: \"\", children: [(\"r\", Ptree { data: \"xy\", children: \
             [(\"<xmlattr>\", Ptree { data: \"\", children: [(\"a\", Ptree { data: \"1\", \
             children: [] })] }), (\"<xmlcomment>\", Ptree { data: \"c\", children: [] }), \
             (\"v\", Ptree { data: \"2\", children: [] }), (\"v\", Ptree { data: \"\", \
             children: [] })] })] }"
        );
        let u = read_xml(b"<r a='1'><!--c-->x<v>2</v><v/>z</r>").unwrap();
        assert_ne!(u, t);
        let w = read_xml(b"<r a='1'><!--c-->x<v>2</v><w/>y</r>").unwrap();
        assert_ne!(w, t);
        assert_ne!(Ptree::new(), t);
    }

    /// The levels of the deep documents below.
    const LEVELS: usize = 1_000_000;

    /// Runs `f` on a thread with a 256 KiB stack, where a reader or a tree that recursed once per
    /// level would overflow long before a million levels.
    fn on_a_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(f)
            .expect("a thread")
            .join()
            .expect("the thread's work")
    }

    #[test]
    fn a_million_levels_read_clone_and_drop_on_a_small_stack() {
        on_a_small_stack(|| {
            let mut doc = b"<a>".repeat(LEVELS - 1);
            doc.extend_from_slice(b"<a k='v'>x");
            doc.extend_from_slice(&b"</a>".repeat(LEVELS));
            let t = read_xml(&doc).unwrap();

            // Down the first children: a million `a`s, the last with its attribute and text.
            let mut levels = 0;
            let mut n = &t;
            while let Some((key, child)) = n.children().find(|(k, _)| *k == b"a") {
                assert_eq!(key, b"a");
                levels += 1;
                n = child;
            }
            assert_eq!(levels, LEVELS);
            assert_eq!(n.data(), b"x");
            assert_eq!(n.get::<Vec<u8>>(b"<xmlattr>.k"), Ok(b"v".to_vec()));

            // A path of a million parts.
            let path = vec![&b"a"[..]; LEVELS].join(&b'.');
            assert!(std::ptr::eq(t.get_child(&path).unwrap(), n));

            let c = t.clone();
            assert_eq!(c, t);
            drop(t);
            assert_eq!(c.get::<Vec<u8>>(&path), Ok(b"x".to_vec()));
            drop(c);
        });
    }

    /// `read_xml` on documents of a million levels that fail, against what the recursive reader
    /// orca-ptree-port wrote gave for them (run on a thread with a 4 GiB stack, before
    /// orca-xml-stack made the reader iterative): the same message and the same line, counted in
    /// the buffer as rewritten by then. Each level's `<a\n>` stays open, so its line feed counts;
    /// its text `t&lt;\n` is expanded in place, leaving its old line feed behind the new one; and
    /// every other level's `<b\n/>` is closed, so the NUL after its name takes its line feed.
    #[test]
    fn deep_failures_give_the_recursive_readers_error_and_line() {
        on_a_small_stack(|| {
            // The deepest element fails: "expected =" after the attribute name `d`.
            let mut doc = Vec::new();
            for i in 0..LEVELS {
                doc.extend_from_slice(b"<a\n>t&lt;\n");
                if i % 2 == 0 {
                    doc.extend_from_slice(b"<b\n/>");
                }
            }
            doc.extend_from_slice(b"<c d\n>");
            assert_eq!(read_xml(&doc).unwrap_err().what(), DEEPEST_FAILS);

            // Half the levels closed again, then a closing tag fails: the closed elements' line
            // feeds are gone, the closing tags' count.
            let mut doc = b"<a\n>".repeat(LEVELS);
            doc.extend_from_slice(&b"</a\n>".repeat(LEVELS / 2));
            doc.extend_from_slice(b"</a !");
            assert_eq!(read_xml(&doc).unwrap_err().what(), CLOSING_FAILS);
        });
    }

    /// What the recursive reader gave for the two documents above: three line feeds a level and
    /// two more, and one a level and one more.
    const DEEPEST_FAILS: &str = "<unspecified file>(3000002): expected =";
    const CLOSING_FAILS: &str = "<unspecified file>(1000001): expected >";
}
