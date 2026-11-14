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
// Carries Boost code: the parser of rapidxml as Boost.PropertyTree 1.84.0 bundles it
// (boost/property_tree/detail/rapidxml.hpp, :1343-2596: parse_error, xml_document::parse, the
// predicates, internal::get_index, insert_coded_character, skip, skip_and_expand_character_refs,
// parse_bom, parse_xml_declaration, parse_comment, parse_doctype, parse_pi,
// parse_and_append_data, parse_cdata, parse_element, parse_node, parse_node_contents,
// parse_node_attributes and the lookup tables), under the flags read_xml passes it
// (parse_comment_nodes and nothing else), translated on 2026-11-11 by orca-ptree-port; the
// recursion of parse_element and parse_node_contents turned into a loop over a stack of open
// elements on 2026-11-11 by orca-xml-stack. Copyright (C) 2006, 2009 Marcin Kalicinski, under the
// Boost Software License, version 1.0 (licenses/BSL-1.0.txt).

//! rapidxml's parser, as Boost 1.84's `property_tree::read_xml` runs it: `parse<Flags>` with
//! `Flags` `parse_comment_nodes` (what `read_xml`'s flags 0 become), on the stream's bytes with a
//! NUL appended.
//!
//! The parser rewrites the buffer in place as rapidxml does, because `read_xml` counts an error's
//! line in the rewritten buffer: a NUL after each element's and attribute's name and after each
//! value, comment and CDATA section, and text and attribute values with their entities and
//! character references expanded where they stand. A pointer of the C++ is an index into the
//! buffer. The DOM becomes a [`Dom`]: its nodes ([`Node`]) and attributes ([`Attribute`]) in two
//! arrays, as the memory pool holds them, the document itself the first node ([`DOCUMENT`]), each
//! node's children a list linked by index as rapidxml links them (`first_node`, `last_node`,
//! `next_sibling`), and names and values where they lie in the buffer.
//!
//! What the flags leave out is not translated: the declaration, doctype and instruction nodes
//! (they are skipped), the closing tag's check, trimming and normalising white space, an element's
//! value (the text of its first data node, which `read_xml` never reads), and the eight-bit mode.
//! Under these flags rapidxml never throws "expected ?>", "expected PI target", "invalid closing
//! tag name" or "expected attribute name" (`parity/HAZARDS.md`, orca-model's 3MF table).
//!
//! In the C++, `parse_element` calls `parse_node_contents`, which calls `parse_node` and so
//! `parse_element` again for each child element: the stack grows by three frames per level, and a
//! nesting deep enough overflows it. Here the parser keeps a stack of its own, the open elements
//! (a `Vec`), and its loop takes a turn of `parse_node_contents` for the innermost one;
//! `parse_element` opens an element with contents, and its closing tag finishes it as
//! `parse_element` would once `parse_node_contents` returned: the NUL after its name, then
//! `append_node` to its parent. Every read, write and error comes in the C++'s order, so the
//! buffer is the same when an error stops the parse, and so is the line counted in it.

/// What a node is (`node_type`): the kinds flags 0 makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeType {
    /// `node_document`: the document, the parent of the top-level nodes.
    Document,
    /// `node_element`: a name, attributes and children.
    Element,
    /// `node_data`: text.
    Data,
    /// `node_cdata`: a CDATA section's text.
    Cdata,
    /// `node_comment`: a comment's text.
    Comment,
}

/// An attribute (`xml_attribute`): its name and value, as offsets and lengths into the buffer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Attribute {
    pub(crate) name: usize,
    pub(crate) name_size: usize,
    pub(crate) value: usize,
    pub(crate) value_size: usize,
}

/// A node (`xml_node`): its name and value as offsets and lengths into the buffer (a name of
/// size 0 for the nodes that have none), its attributes, and its children as a list linked by
/// index into [`Dom::nodes`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    pub(crate) kind: NodeType,
    pub(crate) name: usize,
    pub(crate) name_size: usize,
    pub(crate) value: usize,
    pub(crate) value_size: usize,
    /// The node's attributes are `Dom::attributes[first_attribute..][..attribute_count]`: a
    /// node's attributes are parsed one after another, before any other node's.
    first_attribute: usize,
    attribute_count: usize,
    /// `first_node()`: the first child.
    pub(crate) first_node: Option<usize>,
    /// `m_last_node`, where `append_node` links the next child.
    last_node: Option<usize>,
    /// `next_sibling()`: the next child of the same parent.
    pub(crate) next_sibling: Option<usize>,
}

/// The parsed document (`xml_document` and its memory pool): every node, the document itself
/// first, and every attribute.
#[derive(Clone, Debug)]
pub(crate) struct Dom {
    pub(crate) nodes: Vec<Node>,
    pub(crate) attributes: Vec<Attribute>,
}

/// The document's own node in [`Dom::nodes`]: its children are the top-level nodes.
pub(crate) const DOCUMENT: usize = 0;

impl Dom {
    /// The attributes of node `node`, in order (`first_attribute()` and each `next_attribute()`).
    pub(crate) fn attributes(&self, node: usize) -> &[Attribute] {
        let n = &self.nodes[node];
        &self.attributes[n.first_attribute..n.first_attribute + n.attribute_count]
    }

    /// The children of node `node`, in order (`first_node()` and each `next_sibling()`).
    #[cfg(test)]
    pub(crate) fn children(&self, node: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(self.nodes[node].first_node, |&c| self.nodes[c].next_sibling)
    }
}

/// `parse_error`: its message (`what()`) and where in the buffer the parser was (`where()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ParseError {
    pub(crate) what: &'static str,
    pub(crate) at: usize,
}

/// `BOOST_PROPERTY_TREE_RAPIDXML_PARSE_ERROR(what, where)`.
fn error<T>(what: &'static str, at: usize) -> Result<T, ParseError> {
    Err(ParseError { what, at })
}

/// `xml_document::parse<parse_comment_nodes>(text)`: the document, its top-level nodes the
/// children of [`DOCUMENT`]. `text` ends with the NUL `read_xml` appends, and is rewritten as
/// rapidxml rewrites it.
///
/// While an element is open, its contents are parsed here a turn of `parse_node_contents`' loop
/// at a time ([`Document::parse_node_contents`]); the document's own loop goes on once no element
/// is open.
///
/// # Errors
/// The `parse_error` rapidxml throws, with where it stopped.
///
/// # Panics
/// When `text` does not end with a NUL.
pub(crate) fn parse(text: &mut [u8]) -> Result<Dom, ParseError> {
    assert_eq!(
        text.last(),
        Some(&0),
        "rapidxml parses a NUL-terminated buffer"
    );
    let mut doc = Document {
        buf: text,
        dom: Dom {
            nodes: Vec::new(),
            attributes: Vec::new(),
        },
    };
    doc.allocate_node(NodeType::Document);
    // The elements whose contents are being parsed, outermost first: in the C++, each is a call
    // of `parse_element` waiting in its `parse_node_contents`.
    let mut open: Vec<usize> = Vec::new();
    let mut text = 0;

    // Parse BOM, if any
    doc.parse_bom(&mut text);

    // Parse children
    loop {
        if !open.is_empty() {
            // Inside an element: parse_node_contents' loop goes round
            doc.parse_node_contents(&mut text, &mut open)?;
            continue;
        }

        // Skip whitespace before node
        doc.skip(&mut text, whitespace_pred);
        if doc.buf[text] == 0 {
            break;
        }

        // Parse and append new child
        if doc.buf[text] == b'<' {
            text += 1; // Skip '<'
            doc.parse_node(&mut text, DOCUMENT, &mut open)?;
        } else {
            return error("expected <", text);
        }
    }
    Ok(doc.dom)
}

/// The buffer being parsed (`Ch *text` and what it points into), and the document so far.
struct Document<'a> {
    buf: &'a mut [u8],
    dom: Dom,
}

// ---- Character utility functions --------------------------------------------------------------

/// `internal::get_index`: a byte past ASCII (a negative `char` on MSVC) looks up as `'z'`.
fn get_index(ch: u8) -> usize {
    if ch > 127 {
        usize::from(b'z')
    } else {
        usize::from(ch)
    }
}

/// `whitespace_pred` (space, `\n`, `\r`, `\t`).
fn whitespace_pred(ch: u8) -> bool {
    LOOKUP_WHITESPACE[get_index(ch)] != 0
}

/// `node_name_pred` (anything but space, `\n`, `\r`, `\t`, `/`, `>`, `?` and NUL).
fn node_name_pred(ch: u8) -> bool {
    LOOKUP_NODE_NAME[get_index(ch)] != 0
}

/// `attribute_name_pred` (anything but space, `\n`, `\r`, `\t`, `/`, `<`, `>`, `=`, `?`, `!` and
/// NUL).
fn attribute_name_pred(ch: u8) -> bool {
    LOOKUP_ATTRIBUTE_NAME[get_index(ch)] != 0
}

/// `text_pred` (anything but `<` and NUL).
fn text_pred(ch: u8) -> bool {
    LOOKUP_TEXT[get_index(ch)] != 0
}

/// `text_pure_no_ws_pred` (anything but `<`, NUL and `&`).
fn text_pure_no_ws_pred(ch: u8) -> bool {
    LOOKUP_TEXT_PURE_NO_WS[get_index(ch)] != 0
}

/// `attribute_value_pred<'\''>`.
fn attribute_value_pred_1(ch: u8) -> bool {
    LOOKUP_ATTRIBUTE_DATA_1[get_index(ch)] != 0
}

/// `attribute_value_pure_pred<'\''>`.
fn attribute_value_pure_pred_1(ch: u8) -> bool {
    LOOKUP_ATTRIBUTE_DATA_1_PURE[get_index(ch)] != 0
}

/// `attribute_value_pred<'"'>`.
fn attribute_value_pred_2(ch: u8) -> bool {
    LOOKUP_ATTRIBUTE_DATA_2[get_index(ch)] != 0
}

/// `attribute_value_pure_pred<'"'>`.
fn attribute_value_pure_pred_2(ch: u8) -> bool {
    LOOKUP_ATTRIBUTE_DATA_2_PURE[get_index(ch)] != 0
}

impl Document<'_> {
    // ---- The DOM ------------------------------------------------------------------------------

    /// `allocate_node(type)`: a new node with no name, value, attributes or children, by its
    /// index.
    fn allocate_node(&mut self, kind: NodeType) -> usize {
        self.dom.nodes.push(Node {
            kind,
            name: 0,
            name_size: 0,
            value: 0,
            value_size: 0,
            first_attribute: 0,
            attribute_count: 0,
            first_node: None,
            last_node: None,
            next_sibling: None,
        });
        self.dom.nodes.len() - 1
    }

    /// `allocate_node(type, 0, value, 0, value_size)`, for the nodes that have no name.
    fn allocate_value_node(&mut self, kind: NodeType, value: usize, value_size: usize) -> usize {
        let node = self.allocate_node(kind);
        self.dom.nodes[node].value = value;
        self.dom.nodes[node].value_size = value_size;
        node
    }

    /// `parent->append_node(child)`: `child` becomes the last of `parent`'s children.
    fn append_node(&mut self, parent: usize, child: usize) {
        match self.dom.nodes[parent].last_node {
            Some(last) => self.dom.nodes[last].next_sibling = Some(child),
            None => self.dom.nodes[parent].first_node = Some(child),
        }
        self.dom.nodes[parent].last_node = Some(child);
    }

    // ---- Character utility functions ----------------------------------------------------------

    /// `insert_coded_character`, UTF-8: the code's bytes at `*text`, which moves past them. `code`
    /// is an `unsigned long`, 32 bits.
    fn insert_coded_character(
        &mut self,
        text: &mut usize,
        mut code: u32,
    ) -> Result<(), ParseError> {
        let t = *text;
        // Insert UTF8 sequence
        if code < 0x80 {
            // 1 byte sequence
            self.buf[t] = code as u8;
            *text += 1;
        } else if code < 0x800 {
            // 2 byte sequence
            self.buf[t + 1] = ((code | 0x80) & 0xBF) as u8;
            code >>= 6;
            self.buf[t] = (code | 0xC0) as u8;
            *text += 2;
        } else if code < 0x10000 {
            // 3 byte sequence
            self.buf[t + 2] = ((code | 0x80) & 0xBF) as u8;
            code >>= 6;
            self.buf[t + 1] = ((code | 0x80) & 0xBF) as u8;
            code >>= 6;
            self.buf[t] = (code | 0xE0) as u8;
            *text += 3;
        } else if code < 0x11_0000 {
            // 4 byte sequence
            self.buf[t + 3] = ((code | 0x80) & 0xBF) as u8;
            code >>= 6;
            self.buf[t + 2] = ((code | 0x80) & 0xBF) as u8;
            code >>= 6;
            self.buf[t + 1] = ((code | 0x80) & 0xBF) as u8;
            code >>= 6;
            self.buf[t] = (code | 0xF0) as u8;
            *text += 4;
        } else {
            // Invalid, only codes up to 0x10FFFF are allowed in Unicode
            return error("invalid numeric character entity", t);
        }
        Ok(())
    }

    /// `skip<StopPred>`: past every character the predicate takes.
    fn skip(&self, text: &mut usize, stop_pred: fn(u8) -> bool) {
        let mut tmp = *text;
        while stop_pred(self.buf[tmp]) {
            tmp += 1;
        }
        *text = tmp;
    }

    /// `skip_and_expand_character_refs<StopPred, StopPredPure>` with entity translation on and no
    /// white space normalisation: past every character `stop_pred` takes, the five entities and
    /// character references written in place as their characters. The new end of the expanded
    /// text.
    fn skip_and_expand_character_refs(
        &mut self,
        text: &mut usize,
        stop_pred: fn(u8) -> bool,
        stop_pred_pure: fn(u8) -> bool,
    ) -> Result<usize, ParseError> {
        // Use simple skip until first modification is detected
        self.skip(text, stop_pred_pure);

        // Use translation skip
        let mut src = *text;
        let mut dest = src;
        while stop_pred(self.buf[src]) {
            // Test if replacement is needed
            if self.buf[src] == b'&' {
                match self.buf[src + 1] {
                    // &amp; &apos;
                    b'a' => {
                        if self.buf[src + 2] == b'm'
                            && self.buf[src + 3] == b'p'
                            && self.buf[src + 4] == b';'
                        {
                            self.buf[dest] = b'&';
                            dest += 1;
                            src += 5;
                            continue;
                        }
                        if self.buf[src + 2] == b'p'
                            && self.buf[src + 3] == b'o'
                            && self.buf[src + 4] == b's'
                            && self.buf[src + 5] == b';'
                        {
                            self.buf[dest] = b'\'';
                            dest += 1;
                            src += 6;
                            continue;
                        }
                    }
                    // &quot;
                    b'q' => {
                        if self.buf[src + 2] == b'u'
                            && self.buf[src + 3] == b'o'
                            && self.buf[src + 4] == b't'
                            && self.buf[src + 5] == b';'
                        {
                            self.buf[dest] = b'"';
                            dest += 1;
                            src += 6;
                            continue;
                        }
                    }
                    // &gt;
                    b'g' => {
                        if self.buf[src + 2] == b't' && self.buf[src + 3] == b';' {
                            self.buf[dest] = b'>';
                            dest += 1;
                            src += 4;
                            continue;
                        }
                    }
                    // &lt;
                    b'l' => {
                        if self.buf[src + 2] == b't' && self.buf[src + 3] == b';' {
                            self.buf[dest] = b'<';
                            dest += 1;
                            src += 4;
                            continue;
                        }
                    }
                    // &#...; - assumes ASCII
                    b'#' => {
                        let mut code: u32 = 0;
                        let radix = if self.buf[src + 2] == b'x' {
                            src += 3; // Skip &#x
                            16
                        } else {
                            src += 2; // Skip &#
                            10
                        };
                        loop {
                            let digit = LOOKUP_DIGITS[usize::from(self.buf[src])];
                            if digit == 0xFF {
                                break;
                            }
                            code = code.wrapping_mul(radix).wrapping_add(u32::from(digit));
                            src += 1;
                        }
                        self.insert_coded_character(&mut dest, code)?; // Put character in output
                        if self.buf[src] == b';' {
                            src += 1;
                        } else {
                            return error("expected ;", src);
                        }
                        continue;
                    }
                    // Something else: ignore, just copy '&' verbatim
                    _ => {}
                }
            }

            // No replacement, only copy character
            self.buf[dest] = self.buf[src];
            dest += 1;
            src += 1;
        }

        // Return new end
        *text = src;
        Ok(dest)
    }

    // ---- Internal parsing functions -----------------------------------------------------------

    /// `parse_bom`: a UTF-8 byte order mark at the very start is skipped.
    fn parse_bom(&self, text: &mut usize) {
        let t = *text;
        if self.buf[t] == 0xEF && self.buf[t + 1] == 0xBB && self.buf[t + 2] == 0xBF {
            *text += 3;
        }
    }

    /// `parse_xml_declaration` without `parse_declaration_node`: skipped to its `?>`.
    fn parse_xml_declaration(&self, text: &mut usize) -> Result<Option<usize>, ParseError> {
        // Skip until end of declaration
        while self.buf[*text] != b'?' || self.buf[*text + 1] != b'>' {
            if self.buf[*text] == 0 {
                return error("unexpected end of data", *text);
            }
            *text += 1;
        }
        *text += 2; // Skip '?>'
        Ok(None)
    }

    /// `parse_comment` with `parse_comment_nodes`: a comment node, its value cut with a NUL.
    fn parse_comment(&mut self, text: &mut usize) -> Result<Option<usize>, ParseError> {
        // Remember value start
        let val = *text;

        // Skip until end of comment
        while self.buf[*text] != b'-' || self.buf[*text + 1] != b'-' || self.buf[*text + 2] != b'>'
        {
            if self.buf[*text] == 0 {
                return error("unexpected end of data", *text);
            }
            *text += 1;
        }

        // Create comment node
        let comment = self.allocate_value_node(NodeType::Comment, val, *text - val);

        // Place zero terminator after comment value
        self.buf[*text] = 0;

        *text += 3; // Skip '-->'
        Ok(Some(comment))
    }

    /// `parse_doctype` without `parse_doctype_node`: skipped to its `>`, brackets counted.
    fn parse_doctype(&self, text: &mut usize) -> Result<Option<usize>, ParseError> {
        // Skip to >
        while self.buf[*text] != b'>' {
            // Determine character type
            match self.buf[*text] {
                // If '[' encountered, scan for matching ending ']' using naive algorithm with depth
                b'[' => {
                    *text += 1; // Skip '['
                    let mut depth = 1;
                    while depth > 0 {
                        match self.buf[*text] {
                            b'[' => depth += 1,
                            b']' => depth -= 1,
                            0 => return error("unexpected end of data", *text),
                            _ => {}
                        }
                        *text += 1;
                    }
                }
                // Error on end of text
                0 => return error("unexpected end of data", *text),
                // Other character, skip it
                _ => *text += 1,
            }
        }
        *text += 1; // skip '>'
        Ok(None)
    }

    /// `parse_pi` without `parse_pi_nodes`: skipped to its `?>`.
    fn parse_pi(&self, text: &mut usize) -> Result<Option<usize>, ParseError> {
        // Skip to '?>'
        while self.buf[*text] != b'?' || self.buf[*text + 1] != b'>' {
            if self.buf[*text] == 0 {
                return error("unexpected end of data", *text);
            }
            *text += 1;
        }
        *text += 2; // Skip '?>'
        Ok(None)
    }

    /// `parse_and_append_data`: a data node from the contents' start (white space is not
    /// trimmed), appended to `node`, its end cut with a NUL. The character that ended the data,
    /// which the NUL may have overwritten.
    fn parse_and_append_data(
        &mut self,
        node: usize,
        text: &mut usize,
        contents_start: usize,
    ) -> Result<u8, ParseError> {
        // Backup to contents start if whitespace trimming is disabled
        *text = contents_start;

        // Skip until end of data
        let val = *text;
        let end = self.skip_and_expand_character_refs(text, text_pred, text_pure_no_ws_pred)?;

        // Create new data node
        let data = self.allocate_value_node(NodeType::Data, val, end - val);
        self.append_node(node, data);

        // The element's own value (the first data node's text) is not kept: read_xml never reads
        // it, and setting it writes nothing into the buffer.

        // Place zero terminator after value
        let ch = self.buf[*text];
        self.buf[end] = 0;
        // Return character that ends data; this is required because zero terminator overwritten it
        Ok(ch)
    }

    /// `parse_cdata`: a CDATA node, its value cut with a NUL.
    fn parse_cdata(&mut self, text: &mut usize) -> Result<Option<usize>, ParseError> {
        // Skip until end of cdata
        let val = *text;
        while self.buf[*text] != b']' || self.buf[*text + 1] != b']' || self.buf[*text + 2] != b'>'
        {
            if self.buf[*text] == 0 {
                return error("unexpected end of data", *text);
            }
            *text += 1;
        }

        // Create new cdata node
        let cdata = self.allocate_value_node(NodeType::Cdata, val, *text - val);

        // Place zero terminator after value
        self.buf[*text] = 0;

        *text += 3; // Skip ]]>
        Ok(Some(cdata))
    }

    /// `parse_element`: an element and its attributes, appended to `parent`. An element with
    /// contents (`>`) is pushed onto `open` instead, and its contents are parsed by `parse`'s loop
    /// ([`Self::parse_node_contents`], which calls [`Self::finish_element`] at its closing tag);
    /// one closed by `/>` is finished here.
    fn parse_element(
        &mut self,
        text: &mut usize,
        parent: usize,
        open: &mut Vec<usize>,
    ) -> Result<(), ParseError> {
        // Create element node
        let element = self.allocate_node(NodeType::Element);

        // Extract element name
        let n = *text;
        self.skip(text, node_name_pred);
        if *text == n {
            return error("expected element name", *text);
        }
        self.dom.nodes[element].name = n;
        self.dom.nodes[element].name_size = *text - n;

        // Skip whitespace between element name and attributes or >
        self.skip(text, whitespace_pred);

        // Parse attributes, if any
        self.parse_node_attributes(text, element)?;

        // Determine ending type
        if self.buf[*text] == b'>' {
            *text += 1;
            // parse_node_contents(text, element): taken a turn at a time by `parse`'s loop while
            // the element is open, the rest of this function by `finish_element`
            open.push(element);
            return Ok(());
        } else if self.buf[*text] == b'/' {
            *text += 1;
            if self.buf[*text] != b'>' {
                return error("expected >", *text);
            }
            *text += 1;
        } else {
            return error("expected >", *text);
        }

        self.finish_element(element, parent);
        Ok(())
    }

    /// The end of `parse_element`, once the element's contents are parsed: the NUL after its
    /// name, then the caller's `append_node`.
    fn finish_element(&mut self, element: usize, parent: usize) {
        // Place zero terminator after name
        let Node {
            name, name_size, ..
        } = self.dom.nodes[element];
        self.buf[name + name_size] = 0;

        // Return parsed element
        self.append_node(parent, element);
    }

    /// `parse_node` and its caller's `append_node`: what follows a `<`, by its first characters,
    /// appended to `parent` when it makes a node (an element with contents once it is closed: see
    /// [`Self::parse_element`]).
    fn parse_node(
        &mut self,
        text: &mut usize,
        parent: usize,
        open: &mut Vec<usize>,
    ) -> Result<(), ParseError> {
        if let Some(node) = self.parse_node_switch(text, parent, open)? {
            self.append_node(parent, node);
        }
        Ok(())
    }

    /// `parse_node`'s switch on the node type: the node it makes, or `None` for what is skipped
    /// and for an element, which [`Self::parse_element`] appends itself.
    fn parse_node_switch(
        &mut self,
        text: &mut usize,
        parent: usize,
        open: &mut Vec<usize>,
    ) -> Result<Option<usize>, ParseError> {
        // Parse proper node type
        match self.buf[*text] {
            // <?...
            b'?' => {
                *text += 1; // Skip ?
                let t = *text;
                if (self.buf[t] == b'x' || self.buf[t] == b'X')
                    && (self.buf[t + 1] == b'm' || self.buf[t + 1] == b'M')
                    && (self.buf[t + 2] == b'l' || self.buf[t + 2] == b'L')
                    && whitespace_pred(self.buf[t + 3])
                {
                    // '<?xml ' - xml declaration
                    *text += 4; // Skip 'xml '
                    self.parse_xml_declaration(text)
                } else {
                    // Parse PI
                    self.parse_pi(text)
                }
            }

            // <!...
            b'!' => {
                let t = *text;
                // Parse proper subset of <! node
                match self.buf[t + 1] {
                    // <!-
                    b'-' => {
                        if self.buf[t + 2] == b'-' {
                            // '<!--' - xml comment
                            *text += 3; // Skip '!--'
                            return self.parse_comment(text);
                        }
                    }
                    // <![
                    b'[' => {
                        if self.buf[t + 2] == b'C'
                            && self.buf[t + 3] == b'D'
                            && self.buf[t + 4] == b'A'
                            && self.buf[t + 5] == b'T'
                            && self.buf[t + 6] == b'A'
                            && self.buf[t + 7] == b'['
                        {
                            // '<![CDATA[' - cdata
                            *text += 8; // Skip '![CDATA['
                            return self.parse_cdata(text);
                        }
                    }
                    // <!D
                    b'D' => {
                        if self.buf[t + 2] == b'O'
                            && self.buf[t + 3] == b'C'
                            && self.buf[t + 4] == b'T'
                            && self.buf[t + 5] == b'Y'
                            && self.buf[t + 6] == b'P'
                            && self.buf[t + 7] == b'E'
                            && whitespace_pred(self.buf[t + 8])
                        {
                            // '<!DOCTYPE ' - doctype
                            *text += 9; // skip '!DOCTYPE '
                            return self.parse_doctype(text);
                        }
                    }
                    _ => {}
                }

                // Attempt to skip other, unrecognized node types starting with <!
                *text += 1; // Skip !
                while self.buf[*text] != b'>' {
                    if self.buf[*text] == 0 {
                        return error("unexpected end of data", *text);
                    }
                    *text += 1;
                }
                *text += 1; // Skip '>'
                Ok(None) // No node recognized
            }

            // <...
            _ => self.parse_element(text, parent, open).map(|()| None),
        }
    }

    /// One turn of `parse_node_contents`' loop for the innermost open element: a child (an
    /// element with contents is opened), or data and what follows it, or the closing tag, whose
    /// name is not checked and which finishes the element ([`Self::finish_element`]) and takes it
    /// off `open`.
    fn parse_node_contents(
        &mut self,
        text: &mut usize,
        open: &mut Vec<usize>,
    ) -> Result<(), ParseError> {
        let node = *open.last().expect("an open element");

        // Store start of node contents (whitespace is not skipped)
        let contents_start = *text;
        let mut next_char = self.buf[*text];

        // After data nodes, instead of continuing the loop, control comes back here
        // (`goto after_data_node`).
        loop {
            // Determine what comes next: node closing, child node, data node, or 0?
            match next_char {
                // Node closing or child node
                b'<' => {
                    if self.buf[*text + 1] == b'/' {
                        // Node closing
                        *text += 2; // Skip '</'
                        // No validation, just skip name
                        self.skip(text, node_name_pred);
                        // Skip remaining whitespace after node name
                        self.skip(text, whitespace_pred);
                        if self.buf[*text] != b'>' {
                            return error("expected >", *text);
                        }
                        *text += 1; // Skip '>'

                        // Node closed, finished parsing contents: back in parse_element, which
                        // returns the element to the contents of its parent, or to the document
                        open.pop();
                        let parent = open.last().copied().unwrap_or(DOCUMENT);
                        self.finish_element(node, parent);
                        return Ok(());
                    }
                    // Child node
                    *text += 1; // Skip '<'
                    return self.parse_node(text, node, open);
                }

                // End of data - error
                0 => return error("unexpected end of data", *text),

                // Data node
                _ => {
                    next_char = self.parse_and_append_data(node, text, contents_start)?;
                    // goto after_data_node: bypass regular processing after data nodes
                }
            }
        }
    }

    /// `parse_node_attributes`: the attributes in order, each name and value cut with a NUL.
    fn parse_node_attributes(&mut self, text: &mut usize, node: usize) -> Result<(), ParseError> {
        // The node's attributes are the ones appended from here on (`append_attribute`)
        self.dom.nodes[node].first_attribute = self.dom.attributes.len();

        // For all attributes
        while attribute_name_pred(self.buf[*text]) {
            // Extract attribute name
            let n = *text;
            *text += 1; // Skip first character of attribute name
            self.skip(text, attribute_name_pred);
            // `text == n` ("expected attribute name") cannot be: a name character was just taken.

            // Create new attribute
            self.dom.attributes.push(Attribute {
                name: n,
                name_size: *text - n,
                value: 0,
                value_size: 0,
            });
            self.dom.nodes[node].attribute_count += 1;

            // Skip whitespace after attribute name
            self.skip(text, whitespace_pred);

            // Skip =
            if self.buf[*text] != b'=' {
                return error("expected =", *text);
            }
            *text += 1;

            // Add terminating zero after name
            let attribute = *self
                .dom
                .attributes
                .last()
                .expect("the attribute just appended");
            self.buf[attribute.name + attribute.name_size] = 0;

            // Skip whitespace after =
            self.skip(text, whitespace_pred);

            // Skip quote and remember if it was ' or "
            let quote = self.buf[*text];
            if quote != b'\'' && quote != b'"' {
                return error("expected ' or \"", *text);
            }
            *text += 1;

            // Extract attribute value and expand char refs in it
            let val = *text;
            let end = if quote == b'\'' {
                self.skip_and_expand_character_refs(
                    text,
                    attribute_value_pred_1,
                    attribute_value_pure_pred_1,
                )?
            } else {
                self.skip_and_expand_character_refs(
                    text,
                    attribute_value_pred_2,
                    attribute_value_pure_pred_2,
                )?
            };

            // Set attribute value
            let attribute = self
                .dom
                .attributes
                .last_mut()
                .expect("the attribute just appended");
            attribute.value = val;
            attribute.value_size = end - val;
            let (value, value_size) = (attribute.value, attribute.value_size);

            // Make sure that end quote is present
            if self.buf[*text] != quote {
                return error("expected ' or \"", *text);
            }
            *text += 1; // Skip quote

            // Add terminating zero after value
            self.buf[value + value_size] = 0;

            // Skip whitespace after attribute value
            self.skip(text, whitespace_pred);
        }
        Ok(())
    }
}

// ---- The lookup tables --------------------------------------------------------------------------

/// Whitespace (space `\n` `\r` `\t`).
#[rustfmt::skip]
const LOOKUP_WHITESPACE: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, // 0
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 1
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 2
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 3
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 4
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 5
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 6
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 7
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 8
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 9
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // A
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // B
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // C
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // D
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // E
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // F
];

/// Node name (anything but space `\n` `\r` `\t` `/` `>` `?` `\0`).
#[rustfmt::skip]
const LOOKUP_NODE_NAME: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, // 0
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 1
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, // 2
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, // 3
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 4
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 5
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 6
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 7
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 8
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 9
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // B
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // C
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // D
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // E
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // F
];

/// Text (i.e. PCDATA) (anything but `<` `\0`).
#[rustfmt::skip]
const LOOKUP_TEXT: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 1
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 2
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, // 3
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 4
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 5
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 6
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 7
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 8
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 9
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // B
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // C
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // D
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // E
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // F
];

/// Text (i.e. PCDATA) that does not require processing when ws normalization is disabled
/// (anything but `<` `\0` `&`).
#[rustfmt::skip]
const LOOKUP_TEXT_PURE_NO_WS: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 1
    1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 2
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, // 3
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 4
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 5
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 6
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 7
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 8
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 9
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // B
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // C
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // D
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // E
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // F
];

/// Attribute name (anything but space `\n` `\r` `\t` `/` `<` `>` `=` `?` `!` `\0`).
#[rustfmt::skip]
const LOOKUP_ATTRIBUTE_NAME: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, // 0
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 1
    0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, // 2
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, // 3
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 4
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 5
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 6
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 7
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 8
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 9
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // B
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // C
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // D
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // E
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // F
];

/// Attribute data with single quote (anything but `'` `\0`).
#[rustfmt::skip]
const LOOKUP_ATTRIBUTE_DATA_1: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 1
    1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, // 2
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 3
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 4
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 5
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 6
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 7
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 8
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 9
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // B
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // C
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // D
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // E
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // F
];

/// Attribute data with single quote that does not require processing (anything but `'` `\0`
/// `&`).
#[rustfmt::skip]
const LOOKUP_ATTRIBUTE_DATA_1_PURE: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 1
    1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, // 2
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 3
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 4
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 5
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 6
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 7
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 8
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 9
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // B
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // C
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // D
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // E
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // F
];

/// Attribute data with double quote (anything but `"` `\0`).
#[rustfmt::skip]
const LOOKUP_ATTRIBUTE_DATA_2: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 1
    1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 2
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 3
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 4
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 5
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 6
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 7
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 8
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 9
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // B
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // C
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // D
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // E
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // F
];

/// Attribute data with double quote that does not require processing (anything but `"` `\0`
/// `&`).
#[rustfmt::skip]
const LOOKUP_ATTRIBUTE_DATA_2_PURE: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 1
    1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 2
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 3
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 4
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 5
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 6
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 7
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 8
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 9
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // B
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // C
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // D
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // E
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // F
];

/// Digits (dec and hex, 255 denotes end of numeric character reference), looked up by the byte
/// itself, not by `get_index`.
#[rustfmt::skip]
const LOOKUP_DIGITS: [u8; 256] = [
//   0    1    2    3    4    5    6    7    8    9    A    B    C    D    E    F
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 0
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 1
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 2
     0,   1,   2,   3,   4,   5,   6,   7,   8,   9, 255, 255, 255, 255, 255, 255, // 3
   255,  10,  11,  12,  13,  14,  15, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 4
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 5
   255,  10,  11,  12,  13,  14,  15, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 6
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 7
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 8
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 9
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // A
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // B
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // C
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // D
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // E
   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // F
];

#[cfg(test)]
mod tests {
    use super::{DOCUMENT, Dom, NodeType, ParseError, parse};

    fn run(doc: &[u8]) -> (Result<Dom, ParseError>, Vec<u8>) {
        let mut buf = doc.to_vec();
        buf.push(0);
        let r = parse(&mut buf);
        (r, buf)
    }

    #[test]
    fn the_buffer_is_rewritten_in_place() {
        let (r, buf) = run(b"<a b=\"&amp;x\">t&lt;u</a>");
        let dom = r.unwrap();
        let top: Vec<usize> = dom.children(DOCUMENT).collect();
        assert_eq!(top.len(), 1);
        let a = &dom.nodes[top[0]];
        assert_eq!(a.kind, NodeType::Element);
        assert_eq!(&buf[a.name..a.name + a.name_size], b"a");
        let attr = dom.attributes(top[0])[0];
        assert_eq!(&buf[attr.value..attr.value + attr.value_size], b"&x");
        let children: Vec<usize> = dom.children(top[0]).collect();
        assert_eq!(children.len(), 1);
        let d = &dom.nodes[children[0]];
        assert_eq!(d.kind, NodeType::Data);
        assert_eq!(&buf[d.value..d.value + d.value_size], b"t<u");
        // The NULs after the name, the attribute's name and value, and the text.
        assert_eq!(buf[2], 0);
        assert_eq!(buf[4], 0);
        assert_eq!(&buf[6..9], b"&x\0");
    }

    /// Children in document order, whether an element is appended when it opens or, as here,
    /// once it is closed; each element's attributes its own.
    #[test]
    fn children_and_attributes_keep_their_order() {
        let (r, buf) = run(b"<!--c--><r p='1'>x<a q='2' s='3'><b/>y</a><![CDATA[z]]><c/></r>");
        let dom = r.unwrap();
        let name = |n: usize| {
            let n = &dom.nodes[n];
            match n.kind {
                NodeType::Element => buf[n.name..n.name + n.name_size].to_vec(),
                _ => buf[n.value..n.value + n.value_size].to_vec(),
            }
        };
        let top: Vec<Vec<u8>> = dom.children(DOCUMENT).map(name).collect();
        assert_eq!(top, [b"c".to_vec(), b"r".to_vec()]);
        let r = dom.children(DOCUMENT).nth(1).unwrap();
        let kids: Vec<(NodeType, Vec<u8>)> = dom
            .children(r)
            .map(|c| (dom.nodes[c].kind, name(c)))
            .collect();
        assert_eq!(
            kids,
            [
                (NodeType::Data, b"x".to_vec()),
                (NodeType::Element, b"a".to_vec()),
                (NodeType::Cdata, b"z".to_vec()),
                (NodeType::Element, b"c".to_vec()),
            ]
        );
        let a = dom.children(r).nth(1).unwrap();
        let values = |n: usize| -> Vec<Vec<u8>> {
            dom.attributes(n)
                .iter()
                .map(|t| buf[t.value..t.value + t.value_size].to_vec())
                .collect()
        };
        assert_eq!(values(r), [b"1".to_vec()]);
        assert_eq!(values(a), [b"2".to_vec(), b"3".to_vec()]);
        let a_kids: Vec<Vec<u8>> = dom.children(a).map(name).collect();
        assert_eq!(a_kids, [b"b".to_vec(), b"y".to_vec()]);
    }

    #[test]
    fn errors_say_where_rapidxml_stopped() {
        assert_eq!(
            run(b"x<a/>").0.unwrap_err(),
            ParseError {
                what: "expected <",
                at: 0
            }
        );
        assert_eq!(
            run(b"<a>&#x110000;</a>").0.unwrap_err(),
            ParseError {
                what: "invalid numeric character entity",
                at: 3
            }
        );
        assert_eq!(run(b"<a>").0.unwrap_err().what, "unexpected end of data");
        assert_eq!(run(b"<a b/>").0.unwrap_err().what, "expected =");
    }
}
