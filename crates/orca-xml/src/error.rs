// Copyright (C) 2026 The sketch-orca authors (see AUTHORS)
// SPDX-License-Identifier: AGPL-3.0-only
//
// This file is part of sketch-orca, a modified version of OrcaSlicer. sketch-orca is free software:
// you can redistribute it and/or modify it under the terms of the GNU Affero General Public
// License, version 3, as published by the Free Software Foundation. It is distributed in the hope
// that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// (LICENSE) for more details.

// Carries Expat code: enum XML_Error of expat.h and XML_ErrorString of xmlparse.c (Expat 2.5.0, as
// OrcaSlicer v2.4.2's dependency build compiles it from deps/EXPAT/expat), translated on
// 2026-11-08. Copyright (c) 1998-2000 Thai Open Source Software Center Ltd and Clark Cooper and
// Copyright (c) 2001-2022 Expat maintainers, under the MIT licence (licenses/MIT-expat.txt).

//! `enum XML_Error` and `XML_ErrorString`: the codes a parse ends with and their texts, which the
//! 3MF reader puts in its error messages.

/// `enum XML_Error`, with the C's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
#[allow(missing_docs)]
pub enum XmlError {
    None = 0,
    NoMemory,
    Syntax,
    NoElements,
    InvalidToken,
    UnclosedToken,
    PartialChar,
    TagMismatch,
    DuplicateAttribute,
    JunkAfterDocElement,
    ParamEntityRef,
    UndefinedEntity,
    RecursiveEntityRef,
    AsyncEntity,
    BadCharRef,
    BinaryEntityRef,
    AttributeExternalEntityRef,
    MisplacedXmlPi,
    UnknownEncoding,
    IncorrectEncoding,
    UnclosedCdataSection,
    ExternalEntityHandling,
    NotStandalone,
    UnexpectedState,
    EntityDeclaredInPe,
    FeatureRequiresXmlDtd,
    CantChangeFeatureOnceParsing,
    UnboundPrefix,
    UndeclaringPrefix,
    IncompletePe,
    XmlDecl,
    TextDecl,
    Publicid,
    Suspended,
    NotSuspended,
    Aborted,
    Finished,
    SuspendPe,
    ReservedPrefixXml,
    ReservedPrefixXmlns,
    ReservedNamespaceUri,
    InvalidArgument,
    NoBuffer,
    AmplificationLimitBreach,
}

impl XmlError {
    /// Every code, in the C's order (its number is its index).
    pub const ALL: [XmlError; 44] = [
        XmlError::None,
        XmlError::NoMemory,
        XmlError::Syntax,
        XmlError::NoElements,
        XmlError::InvalidToken,
        XmlError::UnclosedToken,
        XmlError::PartialChar,
        XmlError::TagMismatch,
        XmlError::DuplicateAttribute,
        XmlError::JunkAfterDocElement,
        XmlError::ParamEntityRef,
        XmlError::UndefinedEntity,
        XmlError::RecursiveEntityRef,
        XmlError::AsyncEntity,
        XmlError::BadCharRef,
        XmlError::BinaryEntityRef,
        XmlError::AttributeExternalEntityRef,
        XmlError::MisplacedXmlPi,
        XmlError::UnknownEncoding,
        XmlError::IncorrectEncoding,
        XmlError::UnclosedCdataSection,
        XmlError::ExternalEntityHandling,
        XmlError::NotStandalone,
        XmlError::UnexpectedState,
        XmlError::EntityDeclaredInPe,
        XmlError::FeatureRequiresXmlDtd,
        XmlError::CantChangeFeatureOnceParsing,
        XmlError::UnboundPrefix,
        XmlError::UndeclaringPrefix,
        XmlError::IncompletePe,
        XmlError::XmlDecl,
        XmlError::TextDecl,
        XmlError::Publicid,
        XmlError::Suspended,
        XmlError::NotSuspended,
        XmlError::Aborted,
        XmlError::Finished,
        XmlError::SuspendPe,
        XmlError::ReservedPrefixXml,
        XmlError::ReservedPrefixXmlns,
        XmlError::ReservedNamespaceUri,
        XmlError::InvalidArgument,
        XmlError::NoBuffer,
        XmlError::AmplificationLimitBreach,
    ];

    /// The code's number.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// The code of a number, `None` outside the enumeration.
    pub fn from_code(code: i32) -> Option<XmlError> {
        usize::try_from(code)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }
}

/// `XML_ErrorString(code)`: the code's text, `None` (the C's `NULL`) for `XML_ERROR_NONE` and for
/// a number outside the enumeration.
pub fn error_string(code: i32) -> Option<&'static str> {
    use XmlError::*;
    Some(match XmlError::from_code(code)? {
        None => return Option::None,
        NoMemory => "out of memory",
        Syntax => "syntax error",
        NoElements => "no element found",
        InvalidToken => "not well-formed (invalid token)",
        UnclosedToken => "unclosed token",
        PartialChar => "partial character",
        TagMismatch => "mismatched tag",
        DuplicateAttribute => "duplicate attribute",
        JunkAfterDocElement => "junk after document element",
        ParamEntityRef => "illegal parameter entity reference",
        UndefinedEntity => "undefined entity",
        RecursiveEntityRef => "recursive entity reference",
        AsyncEntity => "asynchronous entity",
        BadCharRef => "reference to invalid character number",
        BinaryEntityRef => "reference to binary entity",
        AttributeExternalEntityRef => "reference to external entity in attribute",
        MisplacedXmlPi => "XML or text declaration not at start of entity",
        UnknownEncoding => "unknown encoding",
        IncorrectEncoding => "encoding specified in XML declaration is incorrect",
        UnclosedCdataSection => "unclosed CDATA section",
        ExternalEntityHandling => "error in processing external entity reference",
        NotStandalone => "document is not standalone",
        UnexpectedState => "unexpected parser state - please send a bug report",
        EntityDeclaredInPe => "entity declared in parameter entity",
        FeatureRequiresXmlDtd => "requested feature requires XML_DTD support in Expat",
        CantChangeFeatureOnceParsing => "cannot change setting once parsing has begun",
        UnboundPrefix => "unbound prefix",
        UndeclaringPrefix => "must not undeclare prefix",
        IncompletePe => "incomplete markup in parameter entity",
        XmlDecl => "XML declaration not well-formed",
        TextDecl => "text declaration not well-formed",
        Publicid => "illegal character(s) in public id",
        Suspended => "parser suspended",
        NotSuspended => "parser not suspended",
        Aborted => "parsing aborted",
        Finished => "parsing finished",
        SuspendPe => "cannot suspend in external parameter entity",
        ReservedPrefixXml => {
            "reserved prefix (xml) must not be undeclared or bound to another namespace name"
        }
        ReservedPrefixXmlns => "reserved prefix (xmlns) must not be declared or undeclared",
        ReservedNamespaceUri => "prefix must not be bound to one of the reserved namespace names",
        InvalidArgument => "invalid argument",
        NoBuffer => "a successful prior call to function XML_GetBuffer is required",
        AmplificationLimitBreach => {
            "limit on input amplification factor (from DTD and entities) breached"
        }
    })
}
