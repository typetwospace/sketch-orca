// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Expat code: xmlrole.c, the prolog's state machine (Expat 2.5.0 without XML_DTD, as
// OrcaSlicer v2.4.2's dependency build compiles it from deps/EXPAT/expat), translated on
// 2026-11-08. Copyright (c) 1998-2000 Thai Open Source Software Center Ltd and Clark Cooper and
// Copyright (c) 2001-2022 Expat maintainers, under the MIT licence (licenses/MIT-expat.txt).

//! `XmlTokenRole`: what each token of the prolog means where it stands, from the XML
//! declaration through the document type declaration and its internal subset to the root
//! element's start. Each C handler is a [`State`]; `common` sends a token no state expects to
//! `error`, whose role is `XML_ROLE_ERROR`. Without `XML_DTD` there are no conditional sections
//! and no external subset: the end of a declaration returns to the internal subset.

use crate::tok::*;

// Roles (`XML_ROLE_*` of xmlrole.h, in its order without the XML_DTD ones); the attribute types
// past ID are named for the record, the parser telling them apart only from CDATA and ID.
pub(crate) const ROLE_ERROR: i32 = -1;
pub(crate) const ROLE_NONE: i32 = 0;
pub(crate) const ROLE_XML_DECL: i32 = 1;
pub(crate) const ROLE_INSTANCE_START: i32 = 2;
pub(crate) const ROLE_DOCTYPE_NONE: i32 = 3;
pub(crate) const ROLE_DOCTYPE_NAME: i32 = 4;
pub(crate) const ROLE_DOCTYPE_SYSTEM_ID: i32 = 5;
pub(crate) const ROLE_DOCTYPE_PUBLIC_ID: i32 = 6;
pub(crate) const ROLE_DOCTYPE_INTERNAL_SUBSET: i32 = 7;
pub(crate) const ROLE_DOCTYPE_CLOSE: i32 = 8;
pub(crate) const ROLE_GENERAL_ENTITY_NAME: i32 = 9;
pub(crate) const ROLE_PARAM_ENTITY_NAME: i32 = 10;
pub(crate) const ROLE_ENTITY_NONE: i32 = 11;
pub(crate) const ROLE_ENTITY_VALUE: i32 = 12;
pub(crate) const ROLE_ENTITY_SYSTEM_ID: i32 = 13;
pub(crate) const ROLE_ENTITY_PUBLIC_ID: i32 = 14;
pub(crate) const ROLE_ENTITY_COMPLETE: i32 = 15;
pub(crate) const ROLE_ENTITY_NOTATION_NAME: i32 = 16;
pub(crate) const ROLE_NOTATION_NONE: i32 = 17;
pub(crate) const ROLE_NOTATION_NAME: i32 = 18;
pub(crate) const ROLE_NOTATION_SYSTEM_ID: i32 = 19;
pub(crate) const ROLE_NOTATION_NO_SYSTEM_ID: i32 = 20;
pub(crate) const ROLE_NOTATION_PUBLIC_ID: i32 = 21;
pub(crate) const ROLE_ATTRIBUTE_NAME: i32 = 22;
pub(crate) const ROLE_ATTRIBUTE_TYPE_CDATA: i32 = 23;
pub(crate) const ROLE_ATTRIBUTE_TYPE_ID: i32 = 24;
#[allow(dead_code)]
pub(crate) const ROLE_ATTRIBUTE_TYPE_IDREF: i32 = 25;
#[allow(dead_code)]
pub(crate) const ROLE_ATTRIBUTE_TYPE_IDREFS: i32 = 26;
#[allow(dead_code)]
pub(crate) const ROLE_ATTRIBUTE_TYPE_ENTITY: i32 = 27;
#[allow(dead_code)]
pub(crate) const ROLE_ATTRIBUTE_TYPE_ENTITIES: i32 = 28;
#[allow(dead_code)]
pub(crate) const ROLE_ATTRIBUTE_TYPE_NMTOKEN: i32 = 29;
#[allow(dead_code)]
pub(crate) const ROLE_ATTRIBUTE_TYPE_NMTOKENS: i32 = 30;
pub(crate) const ROLE_ATTRIBUTE_ENUM_VALUE: i32 = 31;
pub(crate) const ROLE_ATTRIBUTE_NOTATION_VALUE: i32 = 32;
pub(crate) const ROLE_ATTLIST_NONE: i32 = 33;
pub(crate) const ROLE_ATTLIST_ELEMENT_NAME: i32 = 34;
pub(crate) const ROLE_IMPLIED_ATTRIBUTE_VALUE: i32 = 35;
pub(crate) const ROLE_REQUIRED_ATTRIBUTE_VALUE: i32 = 36;
pub(crate) const ROLE_DEFAULT_ATTRIBUTE_VALUE: i32 = 37;
pub(crate) const ROLE_FIXED_ATTRIBUTE_VALUE: i32 = 38;
pub(crate) const ROLE_ELEMENT_NONE: i32 = 39;
pub(crate) const ROLE_ELEMENT_NAME: i32 = 40;
pub(crate) const ROLE_CONTENT_ANY: i32 = 41;
pub(crate) const ROLE_CONTENT_EMPTY: i32 = 42;
pub(crate) const ROLE_CONTENT_PCDATA: i32 = 43;
pub(crate) const ROLE_GROUP_OPEN: i32 = 44;
pub(crate) const ROLE_GROUP_CLOSE: i32 = 45;
pub(crate) const ROLE_GROUP_CLOSE_REP: i32 = 46;
pub(crate) const ROLE_GROUP_CLOSE_OPT: i32 = 47;
pub(crate) const ROLE_GROUP_CLOSE_PLUS: i32 = 48;
pub(crate) const ROLE_GROUP_CHOICE: i32 = 49;
pub(crate) const ROLE_GROUP_SEQUENCE: i32 = 50;
pub(crate) const ROLE_CONTENT_ELEMENT: i32 = 51;
pub(crate) const ROLE_CONTENT_ELEMENT_REP: i32 = 52;
pub(crate) const ROLE_CONTENT_ELEMENT_OPT: i32 = 53;
pub(crate) const ROLE_CONTENT_ELEMENT_PLUS: i32 = 54;
pub(crate) const ROLE_PI: i32 = 55;
pub(crate) const ROLE_COMMENT: i32 = 56;
pub(crate) const ROLE_PARAM_ENTITY_REF: i32 = 57;

/// The C's handlers, one a state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Prolog0,
    Prolog1,
    Prolog2,
    Doctype0,
    Doctype1,
    Doctype2,
    Doctype3,
    Doctype4,
    Doctype5,
    InternalSubset,
    Entity0,
    Entity1,
    Entity2,
    Entity3,
    Entity4,
    Entity5,
    Entity6,
    Entity7,
    Entity8,
    Entity9,
    Entity10,
    Notation0,
    Notation1,
    Notation2,
    Notation3,
    Notation4,
    Attlist0,
    Attlist1,
    Attlist2,
    Attlist3,
    Attlist4,
    Attlist5,
    Attlist6,
    Attlist7,
    Attlist8,
    Attlist9,
    Element0,
    Element1,
    Element2,
    Element3,
    Element4,
    Element5,
    Element6,
    Element7,
    DeclClose,
    Error,
}

/// `PROLOG_STATE`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PrologState {
    handler: State,
    /// `level`: how deep the element declaration's groups nest.
    pub level: u32,
    role_none: i32,
}

impl PrologState {
    /// `XmlPrologStateInit`.
    pub(crate) fn new() -> Self {
        PrologState {
            handler: State::Prolog0,
            level: 0,
            role_none: 0,
        }
    }

    /// `common`.
    fn common(&mut self) -> i32 {
        self.handler = State::Error;
        ROLE_ERROR
    }

    /// `setTopLevel`.
    fn set_top_level(&mut self) {
        self.handler = State::InternalSubset;
    }

    fn decl_close(&mut self, role_none: i32, role: i32) -> i32 {
        self.handler = State::DeclClose;
        self.role_none = role_none;
        role
    }

    /// `XmlTokenRole(state, tok, ptr, end, enc)`.
    pub(crate) fn token_role(
        &mut self,
        tok: i32,
        b: &[u8],
        ptr: usize,
        end: usize,
        enc: Enc,
    ) -> i32 {
        use State::*;
        let m = enc.minbpc();
        let named = |kw: &[u8]| enc.name_matches_ascii(b, ptr, end, kw);
        let decl_named = |kw: &[u8]| enc.name_matches_ascii(b, ptr + 2 * m, end, kw);
        let pound_named = |kw: &[u8]| enc.name_matches_ascii(b, ptr + m, end, kw);
        match self.handler {
            Prolog0 | Prolog1 => match tok {
                TOK_PROLOG_S => {
                    self.handler = Prolog1;
                    return ROLE_NONE;
                }
                TOK_XML_DECL if self.handler == Prolog0 => {
                    self.handler = Prolog1;
                    return ROLE_XML_DECL;
                }
                TOK_PI => {
                    self.handler = Prolog1;
                    return ROLE_PI;
                }
                TOK_COMMENT => {
                    self.handler = Prolog1;
                    return ROLE_COMMENT;
                }
                TOK_BOM => return ROLE_NONE,
                TOK_DECL_OPEN => {
                    if decl_named(b"DOCTYPE") {
                        self.handler = Doctype0;
                        return ROLE_DOCTYPE_NONE;
                    }
                }
                TOK_INSTANCE_START => {
                    self.handler = Error;
                    return ROLE_INSTANCE_START;
                }
                _ => {}
            },
            Prolog2 => match tok {
                TOK_PROLOG_S => return ROLE_NONE,
                TOK_PI => return ROLE_PI,
                TOK_COMMENT => return ROLE_COMMENT,
                TOK_INSTANCE_START => {
                    self.handler = Error;
                    return ROLE_INSTANCE_START;
                }
                _ => {}
            },
            Doctype0 => match tok {
                TOK_PROLOG_S => return ROLE_DOCTYPE_NONE,
                TOK_NAME => {
                    self.handler = Doctype1;
                    return ROLE_DOCTYPE_NAME;
                }
                _ => {}
            },
            Doctype1 => match tok {
                TOK_PROLOG_S => return ROLE_DOCTYPE_NONE,
                TOK_OPEN_BRACKET => {
                    self.handler = InternalSubset;
                    return ROLE_DOCTYPE_INTERNAL_SUBSET;
                }
                TOK_DECL_CLOSE => {
                    self.handler = Prolog2;
                    return ROLE_DOCTYPE_CLOSE;
                }
                TOK_NAME => {
                    if named(b"SYSTEM") {
                        self.handler = Doctype3;
                        return ROLE_DOCTYPE_NONE;
                    }
                    if named(b"PUBLIC") {
                        self.handler = Doctype2;
                        return ROLE_DOCTYPE_NONE;
                    }
                }
                _ => {}
            },
            Doctype2 => match tok {
                TOK_PROLOG_S => return ROLE_DOCTYPE_NONE,
                TOK_LITERAL => {
                    self.handler = Doctype3;
                    return ROLE_DOCTYPE_PUBLIC_ID;
                }
                _ => {}
            },
            Doctype3 => match tok {
                TOK_PROLOG_S => return ROLE_DOCTYPE_NONE,
                TOK_LITERAL => {
                    self.handler = Doctype4;
                    return ROLE_DOCTYPE_SYSTEM_ID;
                }
                _ => {}
            },
            Doctype4 => match tok {
                TOK_PROLOG_S => return ROLE_DOCTYPE_NONE,
                TOK_OPEN_BRACKET => {
                    self.handler = InternalSubset;
                    return ROLE_DOCTYPE_INTERNAL_SUBSET;
                }
                TOK_DECL_CLOSE => {
                    self.handler = Prolog2;
                    return ROLE_DOCTYPE_CLOSE;
                }
                _ => {}
            },
            Doctype5 => match tok {
                TOK_PROLOG_S => return ROLE_DOCTYPE_NONE,
                TOK_DECL_CLOSE => {
                    self.handler = Prolog2;
                    return ROLE_DOCTYPE_CLOSE;
                }
                _ => {}
            },
            InternalSubset => match tok {
                TOK_PROLOG_S => return ROLE_NONE,
                TOK_DECL_OPEN => {
                    if decl_named(b"ENTITY") {
                        self.handler = Entity0;
                        return ROLE_ENTITY_NONE;
                    }
                    if decl_named(b"ATTLIST") {
                        self.handler = Attlist0;
                        return ROLE_ATTLIST_NONE;
                    }
                    if decl_named(b"ELEMENT") {
                        self.handler = Element0;
                        return ROLE_ELEMENT_NONE;
                    }
                    if decl_named(b"NOTATION") {
                        self.handler = Notation0;
                        return ROLE_NOTATION_NONE;
                    }
                }
                TOK_PI => return ROLE_PI,
                TOK_COMMENT => return ROLE_COMMENT,
                TOK_PARAM_ENTITY_REF => return ROLE_PARAM_ENTITY_REF,
                TOK_CLOSE_BRACKET => {
                    self.handler = Doctype5;
                    return ROLE_DOCTYPE_NONE;
                }
                TOK_NONE => return ROLE_NONE,
                _ => {}
            },
            Entity0 => match tok {
                TOK_PROLOG_S => return ROLE_ENTITY_NONE,
                TOK_PERCENT => {
                    self.handler = Entity1;
                    return ROLE_ENTITY_NONE;
                }
                TOK_NAME => {
                    self.handler = Entity2;
                    return ROLE_GENERAL_ENTITY_NAME;
                }
                _ => {}
            },
            Entity1 => match tok {
                TOK_PROLOG_S => return ROLE_ENTITY_NONE,
                TOK_NAME => {
                    self.handler = Entity7;
                    return ROLE_PARAM_ENTITY_NAME;
                }
                _ => {}
            },
            Entity2 | Entity7 => match tok {
                TOK_PROLOG_S => return ROLE_ENTITY_NONE,
                TOK_NAME => {
                    let general = self.handler == Entity2;
                    if named(b"SYSTEM") {
                        self.handler = if general { Entity4 } else { Entity9 };
                        return ROLE_ENTITY_NONE;
                    }
                    if named(b"PUBLIC") {
                        self.handler = if general { Entity3 } else { Entity8 };
                        return ROLE_ENTITY_NONE;
                    }
                }
                TOK_LITERAL => return self.decl_close(ROLE_ENTITY_NONE, ROLE_ENTITY_VALUE),
                _ => {}
            },
            Entity3 | Entity8 => match tok {
                TOK_PROLOG_S => return ROLE_ENTITY_NONE,
                TOK_LITERAL => {
                    self.handler = if self.handler == Entity3 {
                        Entity4
                    } else {
                        Entity9
                    };
                    return ROLE_ENTITY_PUBLIC_ID;
                }
                _ => {}
            },
            Entity4 | Entity9 => match tok {
                TOK_PROLOG_S => return ROLE_ENTITY_NONE,
                TOK_LITERAL => {
                    self.handler = if self.handler == Entity4 {
                        Entity5
                    } else {
                        Entity10
                    };
                    return ROLE_ENTITY_SYSTEM_ID;
                }
                _ => {}
            },
            Entity5 => match tok {
                TOK_PROLOG_S => return ROLE_ENTITY_NONE,
                TOK_DECL_CLOSE => {
                    self.set_top_level();
                    return ROLE_ENTITY_COMPLETE;
                }
                TOK_NAME => {
                    if named(b"NDATA") {
                        self.handler = Entity6;
                        return ROLE_ENTITY_NONE;
                    }
                }
                _ => {}
            },
            Entity6 => match tok {
                TOK_PROLOG_S => return ROLE_ENTITY_NONE,
                TOK_NAME => return self.decl_close(ROLE_ENTITY_NONE, ROLE_ENTITY_NOTATION_NAME),
                _ => {}
            },
            Entity10 => match tok {
                TOK_PROLOG_S => return ROLE_ENTITY_NONE,
                TOK_DECL_CLOSE => {
                    self.set_top_level();
                    return ROLE_ENTITY_COMPLETE;
                }
                _ => {}
            },
            Notation0 => match tok {
                TOK_PROLOG_S => return ROLE_NOTATION_NONE,
                TOK_NAME => {
                    self.handler = Notation1;
                    return ROLE_NOTATION_NAME;
                }
                _ => {}
            },
            Notation1 => match tok {
                TOK_PROLOG_S => return ROLE_NOTATION_NONE,
                TOK_NAME => {
                    if named(b"SYSTEM") {
                        self.handler = Notation3;
                        return ROLE_NOTATION_NONE;
                    }
                    if named(b"PUBLIC") {
                        self.handler = Notation2;
                        return ROLE_NOTATION_NONE;
                    }
                }
                _ => {}
            },
            Notation2 => match tok {
                TOK_PROLOG_S => return ROLE_NOTATION_NONE,
                TOK_LITERAL => {
                    self.handler = Notation4;
                    return ROLE_NOTATION_PUBLIC_ID;
                }
                _ => {}
            },
            Notation3 => match tok {
                TOK_PROLOG_S => return ROLE_NOTATION_NONE,
                TOK_LITERAL => return self.decl_close(ROLE_NOTATION_NONE, ROLE_NOTATION_SYSTEM_ID),
                _ => {}
            },
            Notation4 => match tok {
                TOK_PROLOG_S => return ROLE_NOTATION_NONE,
                TOK_LITERAL => return self.decl_close(ROLE_NOTATION_NONE, ROLE_NOTATION_SYSTEM_ID),
                TOK_DECL_CLOSE => {
                    self.set_top_level();
                    return ROLE_NOTATION_NO_SYSTEM_ID;
                }
                _ => {}
            },
            Attlist0 => match tok {
                TOK_PROLOG_S => return ROLE_ATTLIST_NONE,
                TOK_NAME => {
                    self.handler = Attlist1;
                    return ROLE_ATTLIST_ELEMENT_NAME;
                }
                _ => {}
            },
            Attlist1 => match tok {
                TOK_PROLOG_S => return ROLE_ATTLIST_NONE,
                TOK_DECL_CLOSE => {
                    self.set_top_level();
                    return ROLE_ATTLIST_NONE;
                }
                TOK_NAME => {
                    self.handler = Attlist2;
                    return ROLE_ATTRIBUTE_NAME;
                }
                _ => {}
            },
            Attlist2 => match tok {
                TOK_PROLOG_S => return ROLE_ATTLIST_NONE,
                TOK_NAME => {
                    const TYPES: [&[u8]; 8] = [
                        b"CDATA",
                        b"ID",
                        b"IDREF",
                        b"IDREFS",
                        b"ENTITY",
                        b"ENTITIES",
                        b"NMTOKEN",
                        b"NMTOKENS",
                    ];
                    for (i, kw) in TYPES.iter().enumerate() {
                        if named(kw) {
                            self.handler = Attlist8;
                            return ROLE_ATTRIBUTE_TYPE_CDATA + i as i32;
                        }
                    }
                    if named(b"NOTATION") {
                        self.handler = Attlist5;
                        return ROLE_ATTLIST_NONE;
                    }
                }
                TOK_OPEN_PAREN => {
                    self.handler = Attlist3;
                    return ROLE_ATTLIST_NONE;
                }
                _ => {}
            },
            Attlist3 => match tok {
                TOK_PROLOG_S => return ROLE_ATTLIST_NONE,
                TOK_NMTOKEN | TOK_NAME => {
                    self.handler = Attlist4;
                    return ROLE_ATTRIBUTE_ENUM_VALUE;
                }
                _ => {}
            },
            Attlist4 => match tok {
                TOK_PROLOG_S => return ROLE_ATTLIST_NONE,
                TOK_CLOSE_PAREN => {
                    self.handler = Attlist8;
                    return ROLE_ATTLIST_NONE;
                }
                TOK_OR => {
                    self.handler = Attlist3;
                    return ROLE_ATTLIST_NONE;
                }
                _ => {}
            },
            Attlist5 => match tok {
                TOK_PROLOG_S => return ROLE_ATTLIST_NONE,
                TOK_OPEN_PAREN => {
                    self.handler = Attlist6;
                    return ROLE_ATTLIST_NONE;
                }
                _ => {}
            },
            Attlist6 => match tok {
                TOK_PROLOG_S => return ROLE_ATTLIST_NONE,
                TOK_NAME => {
                    self.handler = Attlist7;
                    return ROLE_ATTRIBUTE_NOTATION_VALUE;
                }
                _ => {}
            },
            Attlist7 => match tok {
                TOK_PROLOG_S => return ROLE_ATTLIST_NONE,
                TOK_CLOSE_PAREN => {
                    self.handler = Attlist8;
                    return ROLE_ATTLIST_NONE;
                }
                TOK_OR => {
                    self.handler = Attlist6;
                    return ROLE_ATTLIST_NONE;
                }
                _ => {}
            },
            Attlist8 => match tok {
                TOK_PROLOG_S => return ROLE_ATTLIST_NONE,
                TOK_POUND_NAME => {
                    if pound_named(b"IMPLIED") {
                        self.handler = Attlist1;
                        return ROLE_IMPLIED_ATTRIBUTE_VALUE;
                    }
                    if pound_named(b"REQUIRED") {
                        self.handler = Attlist1;
                        return ROLE_REQUIRED_ATTRIBUTE_VALUE;
                    }
                    if pound_named(b"FIXED") {
                        self.handler = Attlist9;
                        return ROLE_ATTLIST_NONE;
                    }
                }
                TOK_LITERAL => {
                    self.handler = Attlist1;
                    return ROLE_DEFAULT_ATTRIBUTE_VALUE;
                }
                _ => {}
            },
            Attlist9 => match tok {
                TOK_PROLOG_S => return ROLE_ATTLIST_NONE,
                TOK_LITERAL => {
                    self.handler = Attlist1;
                    return ROLE_FIXED_ATTRIBUTE_VALUE;
                }
                _ => {}
            },
            Element0 => match tok {
                TOK_PROLOG_S => return ROLE_ELEMENT_NONE,
                TOK_NAME => {
                    self.handler = Element1;
                    return ROLE_ELEMENT_NAME;
                }
                _ => {}
            },
            Element1 => match tok {
                TOK_PROLOG_S => return ROLE_ELEMENT_NONE,
                TOK_NAME => {
                    if named(b"EMPTY") {
                        return self.decl_close(ROLE_ELEMENT_NONE, ROLE_CONTENT_EMPTY);
                    }
                    if named(b"ANY") {
                        return self.decl_close(ROLE_ELEMENT_NONE, ROLE_CONTENT_ANY);
                    }
                }
                TOK_OPEN_PAREN => {
                    self.handler = Element2;
                    self.level = 1;
                    return ROLE_GROUP_OPEN;
                }
                _ => {}
            },
            Element2 => match tok {
                TOK_PROLOG_S => return ROLE_ELEMENT_NONE,
                TOK_POUND_NAME => {
                    if pound_named(b"PCDATA") {
                        self.handler = Element3;
                        return ROLE_CONTENT_PCDATA;
                    }
                }
                TOK_OPEN_PAREN => {
                    self.level = 2;
                    self.handler = Element6;
                    return ROLE_GROUP_OPEN;
                }
                TOK_NAME | TOK_NAME_QUESTION | TOK_NAME_ASTERISK | TOK_NAME_PLUS => {
                    self.handler = Element7;
                    return content_element_role(tok);
                }
                _ => {}
            },
            Element3 => match tok {
                TOK_PROLOG_S => return ROLE_ELEMENT_NONE,
                TOK_CLOSE_PAREN => return self.decl_close(ROLE_ELEMENT_NONE, ROLE_GROUP_CLOSE),
                TOK_CLOSE_PAREN_ASTERISK => {
                    return self.decl_close(ROLE_ELEMENT_NONE, ROLE_GROUP_CLOSE_REP);
                }
                TOK_OR => {
                    self.handler = Element4;
                    return ROLE_ELEMENT_NONE;
                }
                _ => {}
            },
            Element4 => match tok {
                TOK_PROLOG_S => return ROLE_ELEMENT_NONE,
                TOK_NAME => {
                    self.handler = Element5;
                    return ROLE_CONTENT_ELEMENT;
                }
                _ => {}
            },
            Element5 => match tok {
                TOK_PROLOG_S => return ROLE_ELEMENT_NONE,
                TOK_CLOSE_PAREN_ASTERISK => {
                    return self.decl_close(ROLE_ELEMENT_NONE, ROLE_GROUP_CLOSE_REP);
                }
                TOK_OR => {
                    self.handler = Element4;
                    return ROLE_ELEMENT_NONE;
                }
                _ => {}
            },
            Element6 => match tok {
                TOK_PROLOG_S => return ROLE_ELEMENT_NONE,
                TOK_OPEN_PAREN => {
                    self.level += 1;
                    return ROLE_GROUP_OPEN;
                }
                TOK_NAME | TOK_NAME_QUESTION | TOK_NAME_ASTERISK | TOK_NAME_PLUS => {
                    self.handler = Element7;
                    return content_element_role(tok);
                }
                _ => {}
            },
            Element7 => match tok {
                TOK_PROLOG_S => return ROLE_ELEMENT_NONE,
                TOK_CLOSE_PAREN
                | TOK_CLOSE_PAREN_ASTERISK
                | TOK_CLOSE_PAREN_QUESTION
                | TOK_CLOSE_PAREN_PLUS => {
                    self.level = self.level.wrapping_sub(1);
                    if self.level == 0 {
                        self.handler = DeclClose;
                        self.role_none = ROLE_ELEMENT_NONE;
                    }
                    return match tok {
                        TOK_CLOSE_PAREN => ROLE_GROUP_CLOSE,
                        TOK_CLOSE_PAREN_ASTERISK => ROLE_GROUP_CLOSE_REP,
                        TOK_CLOSE_PAREN_QUESTION => ROLE_GROUP_CLOSE_OPT,
                        _ => ROLE_GROUP_CLOSE_PLUS,
                    };
                }
                TOK_COMMA => {
                    self.handler = Element6;
                    return ROLE_GROUP_SEQUENCE;
                }
                TOK_OR => {
                    self.handler = Element6;
                    return ROLE_GROUP_CHOICE;
                }
                _ => {}
            },
            DeclClose => match tok {
                TOK_PROLOG_S => return self.role_none,
                TOK_DECL_CLOSE => {
                    self.set_top_level();
                    return self.role_none;
                }
                _ => {}
            },
            Error => return ROLE_NONE,
        }
        self.common()
    }
}

/// The role of a content particle's name, by its suffix.
fn content_element_role(tok: i32) -> i32 {
    match tok {
        TOK_NAME_QUESTION => ROLE_CONTENT_ELEMENT_OPT,
        TOK_NAME_ASTERISK => ROLE_CONTENT_ELEMENT_REP,
        TOK_NAME_PLUS => ROLE_CONTENT_ELEMENT_PLUS,
        _ => ROLE_CONTENT_ELEMENT,
    }
}
