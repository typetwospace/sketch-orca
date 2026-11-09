// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Expat code: the parser of xmlparse.c (Expat 2.5.0 without XML_DTD, XML_NS,
// XML_CONTEXT_BYTES or XML_ATTR_INFO, XML_Char char, as OrcaSlicer v2.4.2's dependency build
// compiles it from deps/EXPAT/expat), translated on 2026-11-08. Copyright (c) 1998-2000 Thai Open
// Source Software Center Ltd and Clark Cooper and Copyright (c) 2001-2022 Expat maintainers, under
// the MIT licence (licenses/MIT-expat.txt).

//! `XML_Parser` as the 3MF reader drives it: made by `XML_ParserCreate(NULL)`, with a start
//! element, an end element and a character data handler and no other (the reader sets the entity
//! declaration and external entity handlers to `NULL`), fed by `XML_Parse` in the chunks the zip
//! reader hands it or by `XML_GetBuffer` and `XML_ParseBuffer` whole, stopped by a handler with
//! `XML_StopParser(parser, XML_FALSE)`, and asked for `XML_GetErrorCode` and the line and column of
//! the error.
//!
//! The processors are the C's: `prologInitProcessor`, `prologProcessor` (`doProlog`, the XML
//! declaration, the document type declaration and its internal subset: general entities, the
//! attribute defaults of `ATTLIST` and the group connectors of `ELEMENT`), `contentProcessor`
//! (`doContent`, `storeAtts`, the attribute values and internal entities expanded in place),
//! `cdataSectionProcessor`, `epilogProcessor` and `errorProcessor`. What only a handler this
//! reader leaves unset would see (comments, processing instructions, declarations, the default
//! handler) is skipped exactly where the C skips it for a `NULL` handler. Without `XML_DTD` there
//! are no parameter entities and no amplification accounting.
//!
//! A pointer is an index into the bytes it points into. The document's bytes are the caller's
//! chunk during `XML_Parse`'s direct path and the parser's own buffer otherwise; the C keeps
//! pointers into the caller's chunk after a failed call, and `XML_GetCurrentLineNumber` counts
//! through them while the chunk still lives, so the parser keeps a copy of the bytes it will
//! count when it returns with pointers there ([`Buf::Saved`]).

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::error::XmlError;
use crate::role::*;
use crate::tok::*;

/// `enum XML_Status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// `XML_STATUS_ERROR`.
    Error = 0,
    /// `XML_STATUS_OK`.
    Ok = 1,
    /// `XML_STATUS_SUSPENDED`.
    Suspended = 2,
}

/// `enum XML_Parsing`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parsing {
    /// `XML_INITIALIZED`.
    Initialized,
    /// `XML_PARSING`.
    Parsing,
    /// `XML_FINISHED`.
    Finished,
    /// `XML_SUSPENDED`.
    Suspended,
}

/// What a handler may ask of the parser while it runs: the parsing status that
/// `XML_StopParser` changes, and `XML_GetSpecifiedAttributeCount`.
#[derive(Debug)]
pub struct Ctl {
    parsing: Parsing,
    error_code: XmlError,
    n_specified_atts: i32,
}

impl Ctl {
    /// `XML_StopParser(parser, resumable)`.
    pub fn stop_parser(&mut self, resumable: bool) -> Status {
        match self.parsing {
            Parsing::Suspended => {
                if resumable {
                    self.error_code = XmlError::Suspended;
                    return Status::Error;
                }
                self.parsing = Parsing::Finished;
            }
            Parsing::Finished => {
                self.error_code = XmlError::Finished;
                return Status::Error;
            }
            _ => {
                self.parsing = if resumable {
                    Parsing::Suspended
                } else {
                    Parsing::Finished
                };
            }
        }
        Status::Ok
    }

    /// `XML_GetSpecifiedAttributeCount`: twice the number of attributes the last start tag
    /// specified (the defaults the document type declaration added follow them).
    pub fn specified_attribute_count(&self) -> i32 {
        self.n_specified_atts
    }
}

/// The three handlers the 3MF reader sets. `name` and the attributes are UTF-8, as `XML_Char`
/// strings are, without their terminating NUL; `atts` alternates names and values.
pub trait Handler {
    /// `XML_StartElementHandler`.
    fn start_element(&mut self, ctl: &mut Ctl, name: &[u8], atts: &[&[u8]]);
    /// `XML_EndElementHandler`.
    fn end_element(&mut self, ctl: &mut Ctl, name: &[u8]);
    /// `XML_CharacterDataHandler`.
    fn character_data(&mut self, ctl: &mut Ctl, s: &[u8]);
}

/// `m_processor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Processor {
    PrologInit,
    Prolog,
    Content,
    CdataSection,
    Epilog,
    Error,
}

/// The bytes the position and event pointers point into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Buf {
    /// The parser's buffer (`m_buffer`).
    Internal,
    /// The caller's chunk, during `XML_Parse`'s direct path.
    Caller,
    /// The part of a caller's chunk from the position pointer to the event pointer, kept when
    /// `XML_Parse` returned with its pointers there.
    Saved,
}

/// `ENTITY`, a general entity of the internal subset.
struct Entity {
    /// `textPtr`/`textLen`: the replacement text (UTF-8) of an internal entity.
    text: Option<Rc<[u8]>>,
    /// `notation`: an unparsed entity.
    notation: bool,
    is_internal: bool,
    open: bool,
}

/// `ATTRIBUTE_ID`: its `name[-1]` flag (1 or 2 while the attribute is seen in a start tag).
struct AttributeId {
    name: Vec<u8>,
    flag: u8,
    maybe_tokenized: bool,
}

/// `DEFAULT_ATTRIBUTE`.
#[derive(Clone)]
struct DefaultAtt {
    id: usize,
    value: Option<Vec<u8>>,
    is_cdata: bool,
}

/// `ELEMENT_TYPE`: what `storeAtts` reads of it.
#[derive(Default)]
struct ElementType {
    default_atts: Vec<DefaultAtt>,
}

/// `DTD`, without what only handlers or `XML_DTD` read (`keepProcessing` is always true).
#[derive(Default)]
struct Dtd {
    general_entities: BTreeMap<Vec<u8>, Entity>,
    element_types: BTreeMap<Vec<u8>, ElementType>,
    attribute_ids: Vec<AttributeId>,
    attribute_index: BTreeMap<Vec<u8>, usize>,
    standalone: bool,
    has_param_entity_refs: bool,
}

/// `TAG`: the raw name in the bytes of the entity it was opened in (the C copies it into the
/// tag's buffer when the document's buffer moves, `storeRawNames`), and the name in UTF-8.
struct Tag {
    raw_name: Vec<u8>,
    name: Vec<u8>,
}

/// `INIT_BUFFER_SIZE`.
const INIT_BUFFER_SIZE: i32 = 1024;
/// `INIT_DATA_BUF_SIZE`: the bytes of `m_dataBuf`, which converted character data passes
/// through.
const INIT_DATA_BUF_SIZE: usize = 1024;

/// `XML_Parser`.
pub struct Parser {
    ctl: Ctl,
    processor: Processor,
    encoding: Enc,
    prolog_state: PrologState,
    /// `m_buffer`..`m_bufferLim`; empty while `m_buffer` is `NULL`.
    buffer: Vec<u8>,
    buffer_ptr: usize,
    buffer_end: usize,
    parse_end_ptr: usize,
    parse_end_byte_index: i32,
    ptrs: Buf,
    position_ptr: Option<usize>,
    event_ptr: Option<usize>,
    saved: Vec<u8>,
    saved_base: usize,
    position: Position,
    final_buffer: bool,
    tag_level: i32,
    tag_stack: Vec<Tag>,
    dtd: Dtd,
    decl_entity: Option<Vec<u8>>,
    decl_element_type: Option<Vec<u8>>,
    decl_attribute_id: usize,
    decl_attribute_is_cdata: bool,
    decl_attribute_is_id: bool,
    group_connector: Vec<u8>,
    open_internal_entities: usize,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    /// `XML_ParserCreate(NULL)`.
    pub fn new() -> Parser {
        Parser {
            ctl: Ctl {
                parsing: Parsing::Initialized,
                error_code: XmlError::None,
                n_specified_atts: 0,
            },
            processor: Processor::PrologInit,
            encoding: Enc::Init,
            prolog_state: PrologState::new(),
            buffer: Vec::new(),
            buffer_ptr: 0,
            buffer_end: 0,
            parse_end_ptr: 0,
            parse_end_byte_index: 0,
            ptrs: Buf::Internal,
            position_ptr: None,
            event_ptr: None,
            saved: Vec::new(),
            saved_base: 0,
            position: Position::default(),
            final_buffer: false,
            tag_level: 0,
            tag_stack: Vec::new(),
            dtd: Dtd::default(),
            decl_entity: None,
            decl_element_type: None,
            decl_attribute_id: 0,
            decl_attribute_is_cdata: false,
            decl_attribute_is_id: false,
            group_connector: Vec::new(),
            open_internal_entities: 0,
        }
    }

    /// `XML_GetErrorCode`.
    pub fn error_code(&self) -> XmlError {
        self.ctl.error_code
    }

    /// `XML_StopParser`, called from outside a handler.
    pub fn stop_parser(&mut self, resumable: bool) -> Status {
        self.ctl.stop_parser(resumable)
    }

    /// `XML_GetSpecifiedAttributeCount`.
    pub fn specified_attribute_count(&self) -> i32 {
        self.ctl.n_specified_atts
    }

    /// The parsing status (`XML_GetParsingStatus`'s `parsing`).
    pub fn parsing(&self) -> Parsing {
        self.ctl.parsing
    }

    /// The position update `XML_GetCurrentLineNumber` and `ColumnNumber` make first.
    fn update_to_event(&mut self) {
        let (Some(ev), Some(pos)) = (self.event_ptr, self.position_ptr) else {
            return;
        };
        if ev < pos {
            return;
        }
        match self.ptrs {
            Buf::Internal => {
                self.encoding
                    .update_position(&self.buffer, pos, ev, &mut self.position)
            }
            Buf::Saved => {
                let base = self.saved_base;
                self.encoding.update_position(
                    &self.saved,
                    pos - base,
                    ev - base,
                    &mut self.position,
                );
            }
            Buf::Caller => unreachable!("a call returns with no pointer into the caller's chunk"),
        }
        self.position_ptr = Some(ev);
    }

    /// `XML_GetCurrentLineNumber`: the line of the last event, counted from 1.
    pub fn current_line_number(&mut self) -> u32 {
        self.update_to_event();
        self.position.line.wrapping_add(1)
    }

    /// `XML_GetCurrentColumnNumber`: the column of the last event, counted from 0.
    pub fn current_column_number(&mut self) -> u32 {
        self.update_to_event();
        self.position.column
    }

    /// Before `XML_Parse` returns from its direct path with pointers into `s`: keeps the bytes
    /// `XML_GetCurrentLineNumber` will count.
    fn save_caller(&mut self, s: &[u8]) {
        if self.ptrs != Buf::Caller {
            return;
        }
        self.saved.clear();
        self.saved_base = 0;
        if let (Some(ev), Some(pos)) = (self.event_ptr, self.position_ptr)
            && ev >= pos
        {
            self.saved.extend_from_slice(&s[pos..ev]);
            self.saved_base = pos;
        }
        self.ptrs = Buf::Saved;
    }

    /// The parsing status switch at the start of `XML_Parse` and `XML_ParseBuffer`.
    fn start(&mut self, need_buffer: bool) -> Result<(), Status> {
        match self.ctl.parsing {
            Parsing::Suspended => {
                self.ctl.error_code = XmlError::Suspended;
                return Err(Status::Error);
            }
            Parsing::Finished => {
                self.ctl.error_code = XmlError::Finished;
                return Err(Status::Error);
            }
            Parsing::Initialized => {
                if need_buffer && self.buffer.is_empty() {
                    self.ctl.error_code = XmlError::NoBuffer;
                    return Err(Status::Error);
                }
                // `startParsing` only seeds the hash salt.
            }
            Parsing::Parsing => {}
        }
        self.ctl.parsing = Parsing::Parsing;
        Ok(())
    }

    /// `XML_Parse(parser, s, len, isFinal)`.
    pub fn parse<H: Handler>(&mut self, h: &mut H, s: &[u8], is_final: bool) -> Status {
        let len = s.len() as i32;
        if len < 0 {
            self.ctl.error_code = XmlError::InvalidArgument;
            return Status::Error;
        }
        if let Err(st) = self.start(false) {
            return st;
        }
        if len == 0 {
            self.final_buffer = is_final;
            if !is_final {
                return Status::Ok;
            }
            self.ptrs = Buf::Internal;
            self.position_ptr = Some(self.buffer_ptr);
            self.parse_end_ptr = self.buffer_end;
            let buf = std::mem::take(&mut self.buffer);
            let mut next = self.buffer_ptr;
            let err = self.run(h, &buf, self.buffer_ptr, self.parse_end_ptr, &mut next);
            self.buffer = buf;
            self.buffer_ptr = next;
            self.ctl.error_code = err;
            if err == XmlError::None {
                match self.ctl.parsing {
                    Parsing::Suspended => {
                        let pos = self.position_ptr.unwrap_or(0);
                        self.encoding.update_position(
                            &self.buffer,
                            pos,
                            self.buffer_ptr,
                            &mut self.position,
                        );
                        self.position_ptr = Some(self.buffer_ptr);
                        return Status::Suspended;
                    }
                    Parsing::Initialized | Parsing::Parsing => {
                        self.ctl.parsing = Parsing::Finished;
                        return Status::Ok;
                    }
                    Parsing::Finished => return Status::Ok,
                }
            }
            self.processor = Processor::Error;
            return Status::Error;
        }
        if self.buffer_ptr == self.buffer_end {
            if (len as u32) > (u32::MAX / 2).wrapping_sub(self.parse_end_byte_index as u32) {
                self.ctl.error_code = XmlError::NoMemory;
                self.event_ptr = None;
                self.processor = Processor::Error;
                return Status::Error;
            }
            self.parse_end_byte_index = self.parse_end_byte_index.wrapping_add(len);
            self.ptrs = Buf::Caller;
            self.position_ptr = Some(0);
            self.final_buffer = is_final;
            self.parse_end_ptr = s.len();
            let mut end = 0;
            let err = self.run(h, s, 0, s.len(), &mut end);
            self.ctl.error_code = err;
            let result;
            if err != XmlError::None {
                self.processor = Processor::Error;
                self.save_caller(s);
                return Status::Error;
            } else {
                match self.ctl.parsing {
                    Parsing::Suspended => result = Status::Suspended,
                    Parsing::Initialized | Parsing::Parsing if is_final => {
                        self.ctl.parsing = Parsing::Finished;
                        self.save_caller(s);
                        return Status::Ok;
                    }
                    _ => result = Status::Ok,
                }
            }
            self.encoding.update_position(s, 0, end, &mut self.position);
            let n_left_over = s.len() - end;
            if n_left_over != 0 {
                if self.buffer.is_empty() || n_left_over > self.buffer.len() {
                    let bytes_to_allocate = (len as u32).wrapping_mul(2) as i32;
                    if bytes_to_allocate <= 0 {
                        self.ctl.error_code = XmlError::NoMemory;
                        self.event_ptr = None;
                        self.processor = Processor::Error;
                        self.ptrs = Buf::Internal;
                        return Status::Error;
                    }
                    self.buffer.resize(bytes_to_allocate as usize, 0);
                }
                self.buffer[..n_left_over].copy_from_slice(&s[end..]);
            }
            self.ptrs = Buf::Internal;
            self.buffer_ptr = 0;
            self.buffer_end = n_left_over;
            self.position_ptr = Some(0);
            self.parse_end_ptr = n_left_over;
            self.event_ptr = Some(0);
            result
        } else {
            match self.get_buffer(len) {
                None => Status::Error,
                Some(buff) => {
                    buff.copy_from_slice(s);
                    self.parse_buffer(h, len, is_final)
                }
            }
        }
    }

    /// `XML_ParseBuffer(parser, len, isFinal)`.
    pub fn parse_buffer<H: Handler>(&mut self, h: &mut H, len: i32, is_final: bool) -> Status {
        let mut result = Status::Ok;
        if let Err(st) = self.start(true) {
            return st;
        }
        let start = self.buffer_ptr;
        self.ptrs = Buf::Internal;
        self.position_ptr = Some(start);
        self.buffer_end = (self.buffer_end as isize + len as isize) as usize;
        self.parse_end_ptr = self.buffer_end;
        self.parse_end_byte_index = self.parse_end_byte_index.wrapping_add(len);
        self.final_buffer = is_final;
        let buf = std::mem::take(&mut self.buffer);
        let mut next = self.buffer_ptr;
        let err = self.run(h, &buf, start, self.parse_end_ptr, &mut next);
        self.buffer = buf;
        self.buffer_ptr = next;
        self.ctl.error_code = err;
        if err != XmlError::None {
            self.processor = Processor::Error;
            return Status::Error;
        }
        match self.ctl.parsing {
            Parsing::Suspended => result = Status::Suspended,
            Parsing::Initialized | Parsing::Parsing if is_final => {
                self.ctl.parsing = Parsing::Finished;
                return result;
            }
            _ => {}
        }
        let pos = self.position_ptr.unwrap_or(0);
        self.encoding
            .update_position(&self.buffer, pos, self.buffer_ptr, &mut self.position);
        self.position_ptr = Some(self.buffer_ptr);
        result
    }

    /// `XML_GetBuffer(parser, len)`: room for `len` bytes after what the buffer holds.
    pub fn get_buffer(&mut self, len: i32) -> Option<&mut [u8]> {
        if len < 0 {
            self.ctl.error_code = XmlError::NoMemory;
            return None;
        }
        match self.ctl.parsing {
            Parsing::Suspended => {
                self.ctl.error_code = XmlError::Suspended;
                return None;
            }
            Parsing::Finished => {
                self.ctl.error_code = XmlError::Finished;
                return None;
            }
            _ => {}
        }
        // `EXPAT_SAFE_PTR_DIFF` is 0 while `m_buffer` is `NULL`.
        let lim = self.buffer.len();
        if len as usize > lim - self.buffer_end {
            let kept = self.buffer_end - self.buffer_ptr;
            let needed = (len as u32).wrapping_add(kept as u32) as i32;
            if needed < 0 {
                self.ctl.error_code = XmlError::NoMemory;
                return None;
            }
            if needed as usize <= lim {
                if !self.buffer.is_empty() {
                    self.buffer.copy_within(self.buffer_ptr..self.buffer_end, 0);
                    self.buffer_end = kept;
                    self.buffer_ptr = 0;
                }
            } else {
                let mut size = (lim - self.buffer_ptr) as i32;
                if size == 0 {
                    size = INIT_BUFFER_SIZE;
                }
                loop {
                    size = (2u32.wrapping_mul(size as u32)) as i32;
                    if !(size < needed && size > 0) {
                        break;
                    }
                }
                if size <= 0 {
                    self.ctl.error_code = XmlError::NoMemory;
                    return None;
                }
                let mut new_buf = vec![0u8; size as usize];
                if !self.buffer.is_empty() {
                    new_buf[..kept].copy_from_slice(&self.buffer[self.buffer_ptr..self.buffer_end]);
                    self.buffer_end = kept;
                } else {
                    self.buffer_end = 0;
                }
                self.buffer = new_buf;
                self.buffer_ptr = 0;
            }
            self.event_ptr = None;
            self.position_ptr = None;
        }
        if self.buffer.is_empty() {
            // `m_bufferEnd` is `NULL`.
            return None;
        }
        let end = self.buffer_end;
        Some(&mut self.buffer[end..end + len as usize])
    }

    /// `m_processor(parser, start, end, endPtr)`.
    fn run<H: Handler>(
        &mut self,
        h: &mut H,
        b: &[u8],
        s: usize,
        end: usize,
        next: &mut usize,
    ) -> XmlError {
        match self.processor {
            Processor::PrologInit => {
                // `initializeEncoding`: `XmlInitEncoding(.., NULL)` cannot fail.
                self.encoding = Enc::Init;
                self.processor = Processor::Prolog;
                self.prolog_processor(h, b, s, end, next)
            }
            Processor::Prolog => self.prolog_processor(h, b, s, end, next),
            Processor::Content => self.content_processor(h, b, s, end, next),
            Processor::CdataSection => self.cdata_section_processor(h, b, s, end, next),
            Processor::Epilog => self.epilog_processor(b, s, end, next),
            Processor::Error => self.ctl.error_code,
        }
    }

    fn set_event(&mut self, is_doc: bool, p: usize) {
        if is_doc {
            self.event_ptr = Some(p);
        }
    }

    fn content_processor<H: Handler>(
        &mut self,
        h: &mut H,
        b: &[u8],
        s: usize,
        end: usize,
        next: &mut usize,
    ) -> XmlError {
        let enc = self.encoding;
        self.do_content(h, 0, enc, b, s, end, next, !self.final_buffer)
    }

    fn cdata_section_processor<H: Handler>(
        &mut self,
        h: &mut H,
        b: &[u8],
        s: usize,
        end: usize,
        next: &mut usize,
    ) -> XmlError {
        let enc = self.encoding;
        let mut start = Some(s);
        let result = self.do_cdata_section(h, enc, b, &mut start, end, next, !self.final_buffer);
        if result != XmlError::None {
            return result;
        }
        if let Some(start) = start {
            self.processor = Processor::Content;
            return self.content_processor(h, b, start, end, next);
        }
        result
    }

    fn prolog_processor<H: Handler>(
        &mut self,
        h: &mut H,
        b: &[u8],
        s: usize,
        end: usize,
        next_ptr: &mut usize,
    ) -> XmlError {
        let mut next = s;
        let tok = Enc::prolog_tok(&mut self.encoding, b, s, end, &mut next);
        let enc = self.encoding;
        self.do_prolog(h, enc, b, s, end, tok, next, next_ptr, !self.final_buffer)
    }

    fn epilog_processor(
        &mut self,
        b: &[u8],
        mut s: usize,
        end: usize,
        next_ptr: &mut usize,
    ) -> XmlError {
        self.processor = Processor::Epilog;
        self.event_ptr = Some(s);
        loop {
            let mut next = s;
            let mut enc = self.encoding;
            let tok = Enc::prolog_tok(&mut enc, b, s, end, &mut next);
            match tok {
                t if t == -TOK_PROLOG_S => {
                    *next_ptr = next;
                    return XmlError::None;
                }
                TOK_NONE => {
                    *next_ptr = s;
                    return XmlError::None;
                }
                TOK_PROLOG_S | TOK_PI | TOK_COMMENT => {}
                TOK_INVALID => {
                    self.event_ptr = Some(next);
                    return XmlError::InvalidToken;
                }
                TOK_PARTIAL => {
                    if !self.final_buffer {
                        *next_ptr = s;
                        return XmlError::None;
                    }
                    return XmlError::UnclosedToken;
                }
                TOK_PARTIAL_CHAR => {
                    if !self.final_buffer {
                        *next_ptr = s;
                        return XmlError::None;
                    }
                    return XmlError::PartialChar;
                }
                _ => return XmlError::JunkAfterDocElement,
            }
            s = next;
            self.event_ptr = Some(s);
            match self.ctl.parsing {
                Parsing::Suspended => {
                    *next_ptr = next;
                    return XmlError::None;
                }
                Parsing::Finished => return XmlError::Aborted,
                _ => {}
            }
        }
    }

    /// `poolStoreString`: `ptr..end` in UTF-8.
    fn utf8(enc: Enc, b: &[u8], ptr: usize, end: usize) -> Vec<u8> {
        let mut v = Vec::new();
        enc.append_utf8(b, ptr, end, &mut v);
        v
    }

    /// `getAttributeId`.
    fn get_attribute_id(&mut self, enc: Enc, b: &[u8], start: usize, end: usize) -> usize {
        let name = Self::utf8(enc, b, start, end);
        if let Some(&id) = self.dtd.attribute_index.get(&name) {
            return id;
        }
        let id = self.dtd.attribute_ids.len();
        self.dtd.attribute_ids.push(AttributeId {
            name: name.clone(),
            flag: 0,
            maybe_tokenized: false,
        });
        self.dtd.attribute_index.insert(name, id);
        id
    }

    /// `getElementType` (its `setElementTypePrefix` matters only with namespaces).
    fn get_element_type(&mut self, enc: Enc, b: &[u8], ptr: usize, end: usize) -> Vec<u8> {
        let name = Self::utf8(enc, b, ptr, end);
        self.dtd.element_types.entry(name.clone()).or_default();
        name
    }

    /// `defineAttribute` (the `idAtt` it records matters only to `XML_GetIdAttributeIndex`).
    fn define_attribute(
        &mut self,
        att_id: usize,
        is_cdata: bool,
        is_id: bool,
        value: Option<Vec<u8>>,
    ) {
        let Some(type_name) = self.decl_element_type.clone() else {
            return;
        };
        let et = self.dtd.element_types.entry(type_name).or_default();
        if (value.is_some() || is_id) && et.default_atts.iter().any(|da| da.id == att_id) {
            return;
        }
        et.default_atts.push(DefaultAtt {
            id: att_id,
            value,
            is_cdata,
        });
        if !is_cdata {
            self.dtd.attribute_ids[att_id].maybe_tokenized = true;
        }
    }

    /// `storeAtts`: the start tag's attributes, then the defaults it did not specify, as
    /// alternating names and values; sets `m_nSpecifiedAtts`.
    fn store_atts(
        &mut self,
        enc: Enc,
        b: &[u8],
        att_str: usize,
        tag_name: &[u8],
    ) -> Result<Vec<Vec<u8>>, XmlError> {
        let is_doc = enc == self.encoding;
        let defaults: Vec<DefaultAtt> = self
            .dtd
            .element_types
            .entry(tag_name.to_vec())
            .or_default()
            .default_atts
            .clone();
        let atts = enc.get_atts(b, att_str);
        let mut app: Vec<Vec<u8>> = Vec::with_capacity(2 * (atts.len() + defaults.len()));
        let mut ids: Vec<usize> = Vec::with_capacity(atts.len() + defaults.len());
        for att in &atts {
            let name_end = att.name + enc.name_length(b, att.name);
            let id = self.get_attribute_id(enc, b, att.name, name_end);
            if self.dtd.attribute_ids[id].flag != 0 {
                self.set_event(is_doc, att.name);
                return Err(XmlError::DuplicateAttribute);
            }
            self.dtd.attribute_ids[id].flag = 1;
            ids.push(id);
            app.push(self.dtd.attribute_ids[id].name.clone());
            let value = if !att.normalized {
                let mut is_cdata = true;
                if self.dtd.attribute_ids[id].maybe_tokenized
                    && let Some(da) = defaults.iter().find(|da| da.id == id)
                {
                    is_cdata = da.is_cdata;
                }
                self.store_attribute_value(enc, is_cdata, b, att.value_ptr, att.value_end, false)?
            } else {
                Self::utf8(enc, b, att.value_ptr, att.value_end)
            };
            app.push(value);
        }
        self.ctl.n_specified_atts = app.len() as i32;
        for da in &defaults {
            if self.dtd.attribute_ids[da.id].flag == 0
                && let Some(v) = &da.value
            {
                self.dtd.attribute_ids[da.id].flag = 1;
                ids.push(da.id);
                app.push(self.dtd.attribute_ids[da.id].name.clone());
                app.push(v.clone());
            }
        }
        // Clear the flags of every attribute stored.
        for id in ids {
            self.dtd.attribute_ids[id].flag = 0;
        }
        Ok(app)
    }

    /// `storeAttributeValue`.
    fn store_attribute_value(
        &mut self,
        enc: Enc,
        is_cdata: bool,
        b: &[u8],
        ptr: usize,
        end: usize,
        dtd_pool: bool,
    ) -> Result<Vec<u8>, XmlError> {
        let mut pool = Vec::new();
        let result = self.append_attribute_value(enc, is_cdata, b, ptr, end, &mut pool, dtd_pool);
        if result != XmlError::None {
            return Err(result);
        }
        if !is_cdata && pool.last() == Some(&0x20) {
            pool.pop();
        }
        Ok(pool)
    }

    /// `appendAttributeValue`; `dtd_pool` is `pool == &dtd->pool` (an `ATTLIST` default).
    #[allow(clippy::too_many_arguments)]
    fn append_attribute_value(
        &mut self,
        enc: Enc,
        is_cdata: bool,
        b: &[u8],
        mut ptr: usize,
        end: usize,
        pool: &mut Vec<u8>,
        dtd_pool: bool,
    ) -> XmlError {
        let is_doc = enc == self.encoding;
        let m = enc.minbpc();
        loop {
            let mut next = ptr;
            let tok = enc.attribute_value_tok(b, ptr, end, &mut next);
            match tok {
                TOK_NONE => return XmlError::None,
                TOK_INVALID => {
                    self.set_event(is_doc, next);
                    return XmlError::InvalidToken;
                }
                TOK_PARTIAL => {
                    self.set_event(is_doc, ptr);
                    return XmlError::InvalidToken;
                }
                TOK_CHAR_REF => {
                    let n = enc.char_ref_number(b, ptr);
                    if n < 0 {
                        self.set_event(is_doc, ptr);
                        return XmlError::BadCharRef;
                    }
                    if !(!is_cdata && n == 0x20 && (pool.is_empty() || pool.last() == Some(&0x20)))
                    {
                        let mut buf = [0u8; 4];
                        let k = utf8_encode(n, &mut buf);
                        pool.extend_from_slice(&buf[..k]);
                    }
                }
                TOK_DATA_CHARS => enc.append_utf8(b, ptr, next, pool),
                TOK_TRAILING_CR | TOK_ATTRIBUTE_VALUE_S | TOK_DATA_NEWLINE => {
                    if tok == TOK_TRAILING_CR {
                        next = ptr + m;
                    }
                    if !(!is_cdata && (pool.is_empty() || pool.last() == Some(&0x20))) {
                        pool.push(0x20);
                    }
                }
                TOK_ENTITY_REF => {
                    let ch = enc.predefined_entity_name(b, ptr + m, next - m);
                    if ch != 0 {
                        pool.push(ch);
                    } else {
                        let name = Self::utf8(enc, b, ptr + m, next - m);
                        let check_entity_decl = if dtd_pool {
                            if self.dtd.standalone {
                                self.open_internal_entities == 0
                            } else {
                                !self.dtd.has_param_entity_refs
                            }
                        } else {
                            !self.dtd.has_param_entity_refs || self.dtd.standalone
                        };
                        let Some(entity) = self.dtd.general_entities.get(&name) else {
                            if check_entity_decl {
                                return XmlError::UndefinedEntity;
                            }
                            ptr = next;
                            continue;
                        };
                        if check_entity_decl && !entity.is_internal {
                            return XmlError::EntityDeclaredInPe;
                        }
                        if entity.open {
                            self.set_event(is_doc, ptr);
                            return XmlError::RecursiveEntityRef;
                        }
                        if entity.notation {
                            self.set_event(is_doc, ptr);
                            return XmlError::BinaryEntityRef;
                        }
                        let Some(text) = entity.text.clone() else {
                            self.set_event(is_doc, ptr);
                            return XmlError::AttributeExternalEntityRef;
                        };
                        self.entity_open(&name, true);
                        let result = self.append_attribute_value(
                            Enc::InternalUtf8,
                            is_cdata,
                            &text,
                            0,
                            text.len(),
                            pool,
                            dtd_pool,
                        );
                        self.entity_open(&name, false);
                        if result != XmlError::None {
                            return result;
                        }
                    }
                }
                _ => {
                    self.set_event(is_doc, ptr);
                    return XmlError::UnexpectedState;
                }
            }
            ptr = next;
        }
    }

    fn entity_open(&mut self, name: &[u8], open: bool) {
        if let Some(e) = self.dtd.general_entities.get_mut(name) {
            e.open = open;
        }
    }

    /// `storeEntityValue`: the replacement text of an entity literal (character references
    /// replaced, entity references kept, line ends made line feeds), with the error that ended it.
    fn store_entity_value(
        &mut self,
        enc: Enc,
        b: &[u8],
        mut ptr: usize,
        end: usize,
    ) -> (XmlError, Vec<u8>) {
        let is_doc = enc == self.encoding;
        let m = enc.minbpc();
        let mut pool = Vec::new();
        loop {
            let mut next = ptr;
            let tok = enc.entity_value_tok(b, ptr, end, &mut next);
            match tok {
                TOK_PARAM_ENTITY_REF => {
                    self.event_ptr = Some(ptr);
                    return (XmlError::ParamEntityRef, pool);
                }
                TOK_NONE => return (XmlError::None, pool),
                TOK_ENTITY_REF | TOK_DATA_CHARS => enc.append_utf8(b, ptr, next, &mut pool),
                TOK_TRAILING_CR | TOK_DATA_NEWLINE => {
                    if tok == TOK_TRAILING_CR {
                        next = ptr + m;
                    }
                    pool.push(0xA);
                }
                TOK_CHAR_REF => {
                    let n = enc.char_ref_number(b, ptr);
                    if n < 0 {
                        self.set_event(is_doc, ptr);
                        return (XmlError::BadCharRef, pool);
                    }
                    let mut buf = [0u8; 4];
                    let k = utf8_encode(n, &mut buf);
                    pool.extend_from_slice(&buf[..k]);
                }
                TOK_PARTIAL => {
                    self.set_event(is_doc, ptr);
                    return (XmlError::InvalidToken, pool);
                }
                TOK_INVALID => {
                    self.set_event(is_doc, next);
                    return (XmlError::InvalidToken, pool);
                }
                _ => {
                    self.set_event(is_doc, ptr);
                    return (XmlError::UnexpectedState, pool);
                }
            }
            ptr = next;
        }
    }

    /// `processXmlDecl(parser, 0, s, next)`.
    fn process_xml_decl(&mut self, b: &[u8], s: usize, next: usize) -> XmlError {
        let decl = match parse_xml_decl(self.encoding, b, s, next) {
            Ok(d) => d,
            Err(bad) => {
                self.event_ptr = Some(bad);
                return XmlError::XmlDecl;
            }
        };
        if decl.standalone == 1 {
            self.dtd.standalone = true;
        }
        // No XML declaration handler, no default handler; `m_protocolEncodingName` is `NULL`.
        if let Some(new_encoding) = decl.new_encoding {
            if new_encoding.minbpc() != self.encoding.minbpc()
                || (new_encoding.minbpc() == 2 && new_encoding != self.encoding)
            {
                self.event_ptr = decl.encoding_name;
                return XmlError::IncorrectEncoding;
            }
            self.encoding = new_encoding;
        } else if decl.encoding_name.is_some() {
            // `handleUnknownEncoding` without an unknown-encoding handler.
            self.event_ptr = decl.encoding_name;
            return XmlError::UnknownEncoding;
        }
        XmlError::None
    }

    /// `doProlog`.
    #[allow(clippy::too_many_arguments)]
    fn do_prolog<H: Handler>(
        &mut self,
        h: &mut H,
        mut enc: Enc,
        b: &[u8],
        mut s: usize,
        end: usize,
        mut tok: i32,
        mut next: usize,
        next_ptr: &mut usize,
        have_more: bool,
    ) -> XmlError {
        let is_doc = enc == self.encoding;
        loop {
            self.set_event(is_doc, s);
            if tok <= 0 {
                if have_more && tok != TOK_INVALID {
                    *next_ptr = s;
                    return XmlError::None;
                }
                match tok {
                    TOK_INVALID => {
                        self.set_event(is_doc, next);
                        return XmlError::InvalidToken;
                    }
                    TOK_PARTIAL => return XmlError::UnclosedToken,
                    TOK_PARTIAL_CHAR => return XmlError::PartialChar,
                    t if t == -TOK_PROLOG_S => tok = -tok,
                    TOK_NONE => return XmlError::NoElements,
                    _ => {
                        tok = -tok;
                        next = end;
                    }
                }
            }
            let m = enc.minbpc();
            let role = self.prolog_state.token_role(tok, b, s, next, enc);
            match role {
                ROLE_XML_DECL => {
                    let result = self.process_xml_decl(b, s, next);
                    if result != XmlError::None {
                        return result;
                    }
                    enc = self.encoding;
                }
                ROLE_DOCTYPE_PUBLIC_ID | ROLE_ENTITY_PUBLIC_ID | ROLE_NOTATION_PUBLIC_ID => {
                    if role == ROLE_DOCTYPE_PUBLIC_ID {
                        self.dtd.has_param_entity_refs = true;
                    }
                    if let Err(bad) = enc.is_public_id(b, s, next) {
                        self.set_event(is_doc, bad);
                        return XmlError::Publicid;
                    }
                }
                ROLE_INSTANCE_START => {
                    self.processor = Processor::Content;
                    return self.content_processor(h, b, s, end, next_ptr);
                }
                ROLE_ATTLIST_ELEMENT_NAME => {
                    self.decl_element_type = Some(self.get_element_type(enc, b, s, next));
                }
                ROLE_ATTRIBUTE_NAME => {
                    self.decl_attribute_id = self.get_attribute_id(enc, b, s, next);
                    self.decl_attribute_is_cdata = false;
                    self.decl_attribute_is_id = false;
                }
                ROLE_ATTRIBUTE_TYPE_CDATA => self.decl_attribute_is_cdata = true,
                ROLE_ATTRIBUTE_TYPE_ID => self.decl_attribute_is_id = true,
                ROLE_IMPLIED_ATTRIBUTE_VALUE | ROLE_REQUIRED_ATTRIBUTE_VALUE => {
                    let (id, cdata, is_id) = (
                        self.decl_attribute_id,
                        self.decl_attribute_is_cdata,
                        self.decl_attribute_is_id,
                    );
                    self.define_attribute(id, cdata, is_id, None);
                }
                ROLE_DEFAULT_ATTRIBUTE_VALUE | ROLE_FIXED_ATTRIBUTE_VALUE => {
                    let cdata = self.decl_attribute_is_cdata;
                    let value =
                        match self.store_attribute_value(enc, cdata, b, s + m, next - m, true) {
                            Ok(v) => v,
                            Err(e) => return e,
                        };
                    let id = self.decl_attribute_id;
                    self.define_attribute(id, cdata, false, Some(value));
                }
                ROLE_ENTITY_VALUE => {
                    let (result, value) = self.store_entity_value(enc, b, s + m, next - m);
                    if let Some(name) = &self.decl_entity
                        && let Some(e) = self.dtd.general_entities.get_mut(name)
                    {
                        e.text = Some(Rc::from(value));
                    }
                    if result != XmlError::None {
                        return result;
                    }
                }
                ROLE_DOCTYPE_SYSTEM_ID => self.dtd.has_param_entity_refs = true,
                ROLE_ENTITY_NOTATION_NAME => {
                    if let Some(name) = &self.decl_entity
                        && let Some(e) = self.dtd.general_entities.get_mut(name)
                    {
                        e.notation = true;
                    }
                }
                ROLE_GENERAL_ENTITY_NAME => {
                    if enc.predefined_entity_name(b, s, next) != 0 {
                        self.decl_entity = None;
                    } else {
                        let name = Self::utf8(enc, b, s, next);
                        if self.dtd.general_entities.contains_key(&name) {
                            self.decl_entity = None;
                        } else {
                            let is_internal = self.open_internal_entities == 0;
                            self.dtd.general_entities.insert(
                                name.clone(),
                                Entity {
                                    text: None,
                                    notation: false,
                                    is_internal,
                                    open: false,
                                },
                            );
                            self.decl_entity = Some(name);
                        }
                    }
                }
                ROLE_PARAM_ENTITY_NAME => self.decl_entity = None,
                ROLE_ERROR => {
                    return match tok {
                        TOK_PARAM_ENTITY_REF => XmlError::ParamEntityRef,
                        TOK_XML_DECL => XmlError::MisplacedXmlPi,
                        _ => XmlError::Syntax,
                    };
                }
                ROLE_GROUP_OPEN => {
                    let level = self.prolog_state.level as usize;
                    if self.group_connector.len() <= level {
                        self.group_connector.resize(level + 1, 0);
                    }
                    self.group_connector[level] = 0;
                }
                ROLE_GROUP_SEQUENCE => {
                    let level = self.prolog_state.level as usize;
                    if self.group_connector[level] == b'|' {
                        return XmlError::Syntax;
                    }
                    self.group_connector[level] = b',';
                }
                ROLE_GROUP_CHOICE => {
                    let level = self.prolog_state.level as usize;
                    if self.group_connector[level] == b',' {
                        return XmlError::Syntax;
                    }
                    self.group_connector[level] = b'|';
                }
                // The rest acts only for handlers this reader leaves unset (`DOCTYPE_CLOSE` with
                // `allowClosingDoctype` true, `ENTITY_SYSTEM_ID`, `PARAM_ENTITY_REF` without a
                // not-standalone handler, the declarations of elements and notations).
                _ => {}
            }
            match self.ctl.parsing {
                Parsing::Suspended => {
                    *next_ptr = next;
                    return XmlError::None;
                }
                Parsing::Finished => return XmlError::Aborted,
                _ => {
                    s = next;
                    tok = Enc::prolog_tok(&mut enc, b, s, end, &mut next);
                }
            }
        }
    }

    /// `doCdataSection`; `start` is `*startPtr`, `None` while the section is unclosed.
    #[allow(clippy::too_many_arguments)]
    fn do_cdata_section<H: Handler>(
        &mut self,
        h: &mut H,
        enc: Enc,
        b: &[u8],
        start: &mut Option<usize>,
        end: usize,
        next_ptr: &mut usize,
        have_more: bool,
    ) -> XmlError {
        let is_doc = enc == self.encoding;
        let mut s = start.unwrap_or(0);
        self.set_event(is_doc, s);
        *start = None;
        loop {
            let mut next = s;
            let tok = enc.cdata_section_tok(b, s, end, &mut next);
            match tok {
                TOK_CDATA_SECT_CLOSE => {
                    *start = Some(next);
                    *next_ptr = next;
                    return if self.ctl.parsing == Parsing::Finished {
                        XmlError::Aborted
                    } else {
                        XmlError::None
                    };
                }
                TOK_DATA_NEWLINE => h.character_data(&mut self.ctl, b"\n"),
                TOK_DATA_CHARS => {
                    if !enc.is_utf8() {
                        loop {
                            let mut data = Vec::with_capacity(INIT_DATA_BUF_SIZE);
                            let res = enc.to_utf8(b, &mut s, next, &mut data, INIT_DATA_BUF_SIZE);
                            h.character_data(&mut self.ctl, &data);
                            if res == Convert::Completed || res == Convert::InputIncomplete {
                                break;
                            }
                            self.set_event(is_doc, s);
                        }
                    } else {
                        h.character_data(&mut self.ctl, &b[s..next]);
                    }
                }
                TOK_INVALID => {
                    self.set_event(is_doc, next);
                    return XmlError::InvalidToken;
                }
                TOK_PARTIAL_CHAR => {
                    if have_more {
                        *next_ptr = s;
                        return XmlError::None;
                    }
                    return XmlError::PartialChar;
                }
                TOK_PARTIAL | TOK_NONE => {
                    if have_more {
                        *next_ptr = s;
                        return XmlError::None;
                    }
                    return XmlError::UnclosedCdataSection;
                }
                _ => {
                    self.set_event(is_doc, next);
                    return XmlError::UnexpectedState;
                }
            }
            s = next;
            self.set_event(is_doc, s);
            match self.ctl.parsing {
                Parsing::Suspended => {
                    *next_ptr = next;
                    return XmlError::None;
                }
                Parsing::Finished => return XmlError::Aborted,
                _ => {}
            }
        }
    }

    /// `processInternalEntity(parser, entity, XML_FALSE)`.
    fn process_internal_entity<H: Handler>(
        &mut self,
        h: &mut H,
        name: &[u8],
        text: Rc<[u8]>,
    ) -> XmlError {
        self.entity_open(name, true);
        self.open_internal_entities += 1;
        let mut next = 0;
        let level = self.tag_level;
        let result = self.do_content(
            h,
            level,
            Enc::InternalUtf8,
            &text,
            0,
            text.len(),
            &mut next,
            false,
        );
        if result == XmlError::None {
            if text.len() != next && self.ctl.parsing == Parsing::Suspended {
                // The C resumes the entity in `internalEntityProcessor` on `XML_ResumeParser`,
                // which nothing here calls: a suspended parser only refuses further input.
            } else {
                self.entity_open(name, false);
                self.open_internal_entities -= 1;
            }
        }
        result
    }

    /// The character data handler on `s`, through `m_dataBuf` when the encoding is not UTF-8.
    fn report_data<H: Handler>(&mut self, h: &mut H, enc: Enc, b: &[u8], s: usize, end: usize) {
        if !enc.is_utf8() {
            let mut data = Vec::with_capacity(INIT_DATA_BUF_SIZE);
            let mut p = s;
            enc.to_utf8(b, &mut p, end, &mut data, INIT_DATA_BUF_SIZE);
            h.character_data(&mut self.ctl, &data);
        } else {
            h.character_data(&mut self.ctl, &b[s..end]);
        }
    }

    /// `doContent`.
    #[allow(clippy::too_many_arguments)]
    fn do_content<H: Handler>(
        &mut self,
        h: &mut H,
        start_tag_level: i32,
        enc: Enc,
        b: &[u8],
        mut s: usize,
        end: usize,
        next_ptr: &mut usize,
        have_more: bool,
    ) -> XmlError {
        let is_doc = enc == self.encoding;
        let m = enc.minbpc();
        self.set_event(is_doc, s);
        loop {
            let mut next = s;
            let tok = enc.content_tok(b, s, end, &mut next);
            match tok {
                TOK_TRAILING_CR => {
                    if have_more {
                        *next_ptr = s;
                        return XmlError::None;
                    }
                    h.character_data(&mut self.ctl, b"\n");
                    if start_tag_level == 0 {
                        return XmlError::NoElements;
                    }
                    if self.tag_level != start_tag_level {
                        return XmlError::AsyncEntity;
                    }
                    *next_ptr = end;
                    return XmlError::None;
                }
                TOK_NONE => {
                    if have_more {
                        *next_ptr = s;
                        return XmlError::None;
                    }
                    if start_tag_level > 0 {
                        if self.tag_level != start_tag_level {
                            return XmlError::AsyncEntity;
                        }
                        *next_ptr = s;
                        return XmlError::None;
                    }
                    return XmlError::NoElements;
                }
                TOK_INVALID => {
                    self.set_event(is_doc, next);
                    return XmlError::InvalidToken;
                }
                TOK_PARTIAL => {
                    if have_more {
                        *next_ptr = s;
                        return XmlError::None;
                    }
                    return XmlError::UnclosedToken;
                }
                TOK_PARTIAL_CHAR => {
                    if have_more {
                        *next_ptr = s;
                        return XmlError::None;
                    }
                    return XmlError::PartialChar;
                }
                TOK_ENTITY_REF => {
                    let ch = enc.predefined_entity_name(b, s + m, next - m);
                    if ch != 0 {
                        h.character_data(&mut self.ctl, &[ch]);
                    } else {
                        let name = Self::utf8(enc, b, s + m, next - m);
                        let checked = !self.dtd.has_param_entity_refs || self.dtd.standalone;
                        match self.dtd.general_entities.get(&name) {
                            None if checked => return XmlError::UndefinedEntity,
                            // Skipped: no skipped-entity handler, no default handler.
                            None => {}
                            Some(entity) => {
                                if checked && !entity.is_internal {
                                    return XmlError::EntityDeclaredInPe;
                                }
                                if entity.open {
                                    return XmlError::RecursiveEntityRef;
                                }
                                if entity.notation {
                                    return XmlError::BinaryEntityRef;
                                }
                                // `m_defaultExpandInternalEntities` is true; an external entity
                                // has no handler to go to and is skipped.
                                if let Some(text) = entity.text.clone() {
                                    let result = self.process_internal_entity(h, &name, text);
                                    if result != XmlError::None {
                                        return result;
                                    }
                                }
                            }
                        }
                    }
                }
                TOK_START_TAG_NO_ATTS | TOK_START_TAG_WITH_ATTS => {
                    let raw = s + m;
                    let raw_len = enc.name_length(b, raw);
                    self.tag_level += 1;
                    let name = Self::utf8(enc, b, raw, raw + raw_len);
                    let atts = match self.store_atts(enc, b, s, &name) {
                        Ok(a) => a,
                        Err(e) => {
                            self.tag_stack.push(Tag {
                                raw_name: b[raw..raw + raw_len].to_vec(),
                                name,
                            });
                            return e;
                        }
                    };
                    self.tag_stack.push(Tag {
                        raw_name: b[raw..raw + raw_len].to_vec(),
                        name,
                    });
                    let refs: Vec<&[u8]> = atts.iter().map(|a| a.as_slice()).collect();
                    let tag = self.tag_stack.last().expect("pushed");
                    h.start_element(&mut self.ctl, &tag.name, &refs);
                }
                TOK_EMPTY_ELEMENT_NO_ATTS | TOK_EMPTY_ELEMENT_WITH_ATTS => {
                    let raw = s + m;
                    let name = Self::utf8(enc, b, raw, raw + enc.name_length(b, raw));
                    let atts = match self.store_atts(enc, b, s, &name) {
                        Ok(a) => a,
                        Err(e) => return e,
                    };
                    let refs: Vec<&[u8]> = atts.iter().map(|a| a.as_slice()).collect();
                    h.start_element(&mut self.ctl, &name, &refs);
                    // `*eventPP = *eventEndPP`: the end tag's event is the token's end.
                    self.set_event(is_doc, next);
                    h.end_element(&mut self.ctl, &name);
                    if self.tag_level == 0 && self.ctl.parsing != Parsing::Finished {
                        if self.ctl.parsing == Parsing::Suspended {
                            self.processor = Processor::Epilog;
                        } else {
                            return self.epilog_processor(b, next, end, next_ptr);
                        }
                    }
                }
                TOK_END_TAG => {
                    if self.tag_level == start_tag_level {
                        return XmlError::AsyncEntity;
                    }
                    let raw = s + 2 * m;
                    let len = enc.name_length(b, raw);
                    let tag = self.tag_stack.last().expect("an open tag");
                    if len != tag.raw_name.len() || b[raw..raw + len] != tag.raw_name[..] {
                        self.set_event(is_doc, raw);
                        return XmlError::TagMismatch;
                    }
                    let tag = self.tag_stack.pop().expect("an open tag");
                    self.tag_level -= 1;
                    h.end_element(&mut self.ctl, &tag.name);
                    if self.tag_level == 0 && self.ctl.parsing != Parsing::Finished {
                        if self.ctl.parsing == Parsing::Suspended {
                            self.processor = Processor::Epilog;
                        } else {
                            return self.epilog_processor(b, next, end, next_ptr);
                        }
                    }
                }
                TOK_CHAR_REF => {
                    let n = enc.char_ref_number(b, s);
                    if n < 0 {
                        return XmlError::BadCharRef;
                    }
                    let mut buf = [0u8; 4];
                    let k = utf8_encode(n, &mut buf);
                    h.character_data(&mut self.ctl, &buf[..k]);
                }
                TOK_XML_DECL => return XmlError::MisplacedXmlPi,
                TOK_DATA_NEWLINE => h.character_data(&mut self.ctl, b"\n"),
                TOK_CDATA_SECT_OPEN => {
                    let mut start = Some(next);
                    let result =
                        self.do_cdata_section(h, enc, b, &mut start, end, next_ptr, have_more);
                    if result != XmlError::None {
                        return result;
                    }
                    match start {
                        None => {
                            self.processor = Processor::CdataSection;
                            return result;
                        }
                        Some(n) => next = n,
                    }
                }
                TOK_TRAILING_RSQB => {
                    if have_more {
                        *next_ptr = s;
                        return XmlError::None;
                    }
                    self.report_data(h, enc, b, s, end);
                    if start_tag_level == 0 {
                        self.set_event(is_doc, end);
                        return XmlError::NoElements;
                    }
                    if self.tag_level != start_tag_level {
                        self.set_event(is_doc, end);
                        return XmlError::AsyncEntity;
                    }
                    *next_ptr = end;
                    return XmlError::None;
                }
                TOK_DATA_CHARS => {
                    if !enc.is_utf8() {
                        loop {
                            let mut data = Vec::with_capacity(INIT_DATA_BUF_SIZE);
                            let res = enc.to_utf8(b, &mut s, next, &mut data, INIT_DATA_BUF_SIZE);
                            h.character_data(&mut self.ctl, &data);
                            if res == Convert::Completed || res == Convert::InputIncomplete {
                                break;
                            }
                            self.set_event(is_doc, s);
                        }
                    } else {
                        h.character_data(&mut self.ctl, &b[s..next]);
                    }
                }
                // Processing instructions and comments: no handler, no default handler.
                _ => {}
            }
            s = next;
            self.set_event(is_doc, s);
            match self.ctl.parsing {
                Parsing::Suspended => {
                    *next_ptr = next;
                    return XmlError::None;
                }
                Parsing::Finished => return XmlError::Aborted,
                _ => {}
            }
        }
    }
}
