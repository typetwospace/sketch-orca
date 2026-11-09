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
// xml.sop1: expat 2.5.0, the dependency build's, as the 3MF reader parses with it (parity/SOP1.md,
// "Probe files"), recorded by the o2-model task (2026-11-08); orca-xml's tests/probes.rs replays
// it. Each record holds its inputs, then what expat did with them:
//
//   XPRS  a document parsed by a parser made as bbs_3mf.cpp makes one (XML_ParserCreate(NULL), the
//         start, end and character data handlers, no entity declaration or external entity
//         handler), fed in one of five ways: XML_Parse chunk by chunk with the last chunk final
//         (the model files, through miniz's extraction callback), XML_GetBuffer and XML_ParseBuffer
//         whole and final (the configuration files), XML_Parse with no chunk final (an entry
//         shorter than its stated size), the same then XML_Parse(NULL, 0, 1), and XML_GetBuffer
//         and XML_ParseBuffer chunk by chunk. The chunks repeat a pattern of sizes. A handler may
//         call XML_StopParser(parser, false) at a chosen callback, and at every later one, as
//         _stop_xml_parser does. Recorded: each call's length and status (the feeding stops at
//         the first call that fails or after a stop, as the reader's callback throws then), every
//         callback with its name or data, XML_GetSpecifiedAttributeCount and the attributes in a
//         start handler, and what a stop returned; then XML_GetErrorCode, XML_ErrorString,
//         XML_GetCurrentLineNumber, XML_GetCurrentColumnNumber, the parsing status and
//         XML_GetSpecifiedAttributeCount.
//   XERR  XML_ErrorString of every code and one either side.
//
// The documents are written by hand (the 3MF reader's files, each encoding expat knows and the
// errors it reports, declarations, entities, character references, CDATA sections, line ends),
// mutated from those with a seed, and generated like the 3MF reader's model and configuration
// files. The committed set takes the hand-written documents and a few of the rest.
#include "probe.hpp"

#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include <expat.h>

namespace probe {

namespace {

void add(Sets &sets, const char (&kind)[5], const Payload &p, bool committed)
{
    if (committed)
        sets.committed.add(kind, p);
    sets.full.add(kind, p);
}

template<size_t N> std::string lit(const char (&s)[N]) { return std::string(s, N - 1); }

// ---- XPRS ------------------------------------------------------------------------------------

// The feeding: how the document reaches the parser.
enum Mode : uint8_t {
    PARSE_CHUNKS     = 0, // XML_Parse per chunk, the last final
    BUFFER_WHOLE     = 1, // XML_GetBuffer(size), XML_ParseBuffer(size, 1)
    PARSE_NOT_FINAL  = 2, // XML_Parse per chunk, none final
    PARSE_THEN_FINAL = 3, // as 2, then XML_Parse(NULL, 0, 1)
    BUFFER_CHUNKS    = 4, // XML_GetBuffer and XML_ParseBuffer per chunk, the last final
};

struct Case
{
    std::string           label;
    uint8_t               mode = PARSE_CHUNKS;
    std::string           doc;
    std::vector<uint32_t> pattern; // chunk sizes, repeated; empty: the document in one chunk
    int32_t               stop_at    = -1;
    bool                  stop_after = false;
};

std::vector<uint32_t> chunk_sizes(size_t n, const std::vector<uint32_t> &pattern)
{
    std::vector<uint32_t> out;
    if (pattern.empty()) {
        if (n != 0)
            out.push_back(uint32_t(n));
        return out;
    }
    size_t at = 0;
    for (size_t i = 0; at < n; ++i) {
        size_t k = pattern[i % pattern.size()];
        if (k > n - at)
            k = n - at;
        out.push_back(uint32_t(k));
        at += k;
    }
    return out;
}

// The handlers' state: the callbacks so far and the stop plan.
struct Rec
{
    XML_Parser parser = nullptr;
    Payload    events;
    uint64_t   count      = 0;
    int32_t    stop_at    = -1;
    bool       stop_after = false;
    bool       stopped    = false; // the importer's parse_error()

    void after()
    {
        const int64_t index = int64_t(count);
        ++count;
        if (stop_at >= 0 && (index == stop_at || (stop_after && index > stop_at))) {
            stopped = true;
            events.i32(int32_t(XML_StopParser(parser, XML_FALSE)));
        } else
            events.i32(-1);
    }
};

void XMLCALL on_start(void *user, const XML_Char *name, const XML_Char **atts)
{
    Rec &r = *static_cast<Rec *>(user);
    r.events.u8('S').str(name).i32(XML_GetSpecifiedAttributeCount(r.parser));
    uint64_t n = 0;
    while (atts[n] != nullptr)
        ++n;
    r.events.u64(n);
    for (uint64_t i = 0; i < n; ++i)
        r.events.str(atts[i]);
    r.after();
}

void XMLCALL on_end(void *user, const XML_Char *name)
{
    Rec &r = *static_cast<Rec *>(user);
    r.events.u8('E').str(name);
    r.after();
}

void XMLCALL on_chars(void *user, const XML_Char *s, int len)
{
    Rec &r = *static_cast<Rec *>(user);
    r.events.u8('C').str(std::string(s, size_t(len)));
    r.after();
}

// Parses `c` and returns its record and the number of callbacks.
std::pair<Payload, uint64_t> run_case(const Case &c)
{
    Rec        r;
    XML_Parser p = XML_ParserCreate(nullptr);
    if (p == nullptr)
        throw std::runtime_error("XML_ParserCreate failed");
    r.parser     = p;
    r.stop_at    = c.stop_at;
    r.stop_after = c.stop_after;
    XML_SetUserData(p, &r);
    XML_SetElementHandler(p, on_start, on_end);
    XML_SetCharacterDataHandler(p, on_chars);
    XML_SetEntityDeclHandler(p, nullptr);
    XML_SetExternalEntityRefHandler(p, nullptr);

    Payload                     calls;
    uint64_t                    ncalls      = 0;
    bool                        buffer_null = false;
    const std::vector<uint32_t> sizes       = chunk_sizes(c.doc.size(), c.pattern);
    // Each chunk in a block of its own, alive until the end: the parser's pointers may stay in
    // the last one, as they stay in miniz's buffer while the reader asks for the line.
    std::vector<std::string> keep;
    keep.reserve(sizes.size());
    if (c.mode == BUFFER_WHOLE) {
        void *b = XML_GetBuffer(p, int(c.doc.size()));
        if (b == nullptr)
            buffer_null = true;
        else {
            std::memcpy(b, c.doc.data(), c.doc.size());
            const int st = int(XML_ParseBuffer(p, int(c.doc.size()), 1));
            calls.u32(uint32_t(c.doc.size())).i32(st);
            ++ncalls;
        }
    } else if (c.mode == BUFFER_CHUNKS) {
        size_t at = 0;
        for (size_t i = 0; i < sizes.size(); ++i) {
            const uint32_t n = sizes[i];
            void          *b = XML_GetBuffer(p, int(n));
            if (b == nullptr) {
                buffer_null = true;
                break;
            }
            std::memcpy(b, c.doc.data() + at, n);
            at += n;
            const int st = int(XML_ParseBuffer(p, int(n), i + 1 == sizes.size() ? 1 : 0));
            calls.u32(n).i32(st);
            ++ncalls;
            if (st != XML_STATUS_OK || r.stopped)
                break;
        }
    } else {
        size_t at    = 0;
        bool   going = true;
        for (size_t i = 0; i < sizes.size(); ++i) {
            const uint32_t n = sizes[i];
            keep.emplace_back(c.doc.substr(at, n));
            at += n;
            const int fin = c.mode == PARSE_CHUNKS && i + 1 == sizes.size() ? 1 : 0;
            const int st  = int(XML_Parse(p, keep.back().data(), int(n), fin));
            calls.u32(n).i32(st);
            ++ncalls;
            if (st != XML_STATUS_OK || r.stopped) {
                going = false;
                break;
            }
        }
        if (c.mode == PARSE_THEN_FINAL && going) {
            const int st = int(XML_Parse(p, nullptr, 0, 1));
            calls.u32(0).i32(st);
            ++ncalls;
        }
    }

    const XML_Error   code = XML_GetErrorCode(p);
    const XML_LChar  *text = XML_ErrorString(code);
    const XML_Size    line = XML_GetCurrentLineNumber(p);
    const XML_Size    col  = XML_GetCurrentColumnNumber(p);
    XML_ParsingStatus status;
    XML_GetParsingStatus(p, &status);
    const int spec = XML_GetSpecifiedAttributeCount(p);
    XML_ParserFree(p);

    Payload out;
    out.str(c.label).u8(c.mode).str(c.doc).u32s(c.pattern).i32(c.stop_at).boolean(c.stop_after);
    out.boolean(buffer_null).u64(ncalls);
    out.bytes += calls.bytes;
    out.u64(r.count);
    out.bytes += r.events.bytes;
    out.i32(int32_t(code)).boolean(text != nullptr).str(text != nullptr ? text : "");
    out.u32(uint32_t(line)).u32(uint32_t(col)).i32(int32_t(status.parsing)).i32(spec);
    return {out, r.count};
}

// ---- Documents -------------------------------------------------------------------------------

struct Doc
{
    std::string label;
    std::string text;
};

// A valid UTF-8 string's code points.
std::vector<uint32_t> code_points(const std::string &s)
{
    std::vector<uint32_t> out;
    for (size_t i = 0; i < s.size();) {
        const unsigned char c = (unsigned char) s[i];
        uint32_t            v;
        size_t              n;
        if (c < 0x80) {
            v = c;
            n = 1;
        } else if (c < 0xE0) {
            v = c & 0x1F;
            n = 2;
        } else if (c < 0xF0) {
            v = c & 0x0F;
            n = 3;
        } else {
            v = c & 0x07;
            n = 4;
        }
        for (size_t k = 1; k < n; ++k)
            v = (v << 6) | ((unsigned char) s[i + k] & 0x3F);
        out.push_back(v);
        i += n;
    }
    return out;
}

// `s` (valid UTF-8) in UTF-16, little- or big-endian, with a byte order mark if asked.
std::string utf16(const std::string &s, bool big, bool bom)
{
    std::string out;
    auto        unit = [&](uint32_t u) {
        if (big) {
            out.push_back(char(u >> 8));
            out.push_back(char(u & 0xFF));
        } else {
            out.push_back(char(u & 0xFF));
            out.push_back(char(u >> 8));
        }
    };
    if (bom)
        unit(0xFEFF);
    for (uint32_t v : code_points(s)) {
        if (v >= 0x10000) {
            v -= 0x10000;
            unit(0xD800 | (v >> 10));
            unit(0xDC00 | (v & 0x3FF));
        } else
            unit(v);
    }
    return out;
}

const char *const NS = " xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\" "
                       "xmlns:BambuStudio=\"http://schemas.bambulab.com/package/2021\" "
                       "xmlns:p=\"http://schemas.microsoft.com/3dmanufacturing/production/2015/06\" "
                       "requiredextensions=\"p\"";

std::vector<Doc> hand_docs()
{
    std::vector<Doc> d;
    auto             put = [&](const char *label, std::string text) { d.push_back({label, std::move(text)}); };

    // The 3MF reader's own files.
    put("3mf: a Bambu main model with a component",
        std::string("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\"") + NS +
            ">\n <metadata name=\"Application\">BambuStudio-02.00.00.95</metadata>\n"
            " <metadata name=\"BambuStudio:3mfVersion\">1</metadata>\n"
            " <metadata name=\"Title\">Cube &amp; Co &#233;</metadata>\n"
            " <metadata name=\"Description\"></metadata>\n"
            " <resources>\n"
            "  <object id=\"2\" p:UUID=\"00000001-61cb-4c03-9d28-80fed5dfa1dc\" type=\"model\">\n"
            "   <components>\n"
            "    <component p:path=\"/3D/Objects/object_1.model\" objectid=\"1\" "
            "p:UUID=\"00010000-b206-40ff-9872-83e8017abed1\" transform=\"1 0 0 0 1 0 0 0 1 0 0 0\"/>\n"
            "   </components>\n"
            "  </object>\n"
            " </resources>\n"
            " <build p:UUID=\"2c7c17d8-22b5-4d84-8835-1976022ea369\">\n"
            "  <item objectid=\"2\" p:UUID=\"00000002-b1ec-4553-aec9-835e5b724bb4\" "
            "transform=\"1 0 0 0 1 0 0 0 1 128 128 10\" printable=\"1\"/>\n"
            " </build>\n"
            "</model>\n");
    put("3mf: a Bambu object model with painting",
        std::string("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<model unit=\"millimeter\" xml:lang=\"en-US\"") + NS +
            ">\r\n <metadata name=\"BambuStudio:3mfVersion\">1</metadata>\r\n <resources>\r\n"
            "  <object id=\"1\" p:UUID=\"00010000-81cb-4c03-9d28-80fed5dfa1dc\" type=\"model\">\r\n"
            "   <mesh>\r\n    <vertices>\r\n"
            "     <vertex x=\"-10\" y=\"-10\" z=\"0\"/>\r\n"
            "     <vertex x=\"10\" y=\"-10\" z=\"0\"/>\r\n"
            "     <vertex x=\"10\" y=\"10\" z=\"0\"/>\r\n"
            "     <vertex x=\"-10\" y=\"10\" z=\"20.000001\"/>\r\n"
            "    </vertices>\r\n    <triangles>\r\n"
            "     <triangle v1=\"0\" v2=\"1\" v3=\"2\" paint_color=\"4\"/>\r\n"
            "     <triangle v1=\"0\" v2=\"2\" v3=\"3\" paint_supports=\"8\" paint_seam=\"0C\"/>\r\n"
            "     <triangle v1=\"0\" v2=\"3\" v3=\"1\" paint_color=\"1C3D5\" face_property=\"1\"/>\r\n"
            "     <triangle v1=\"1\" v2=\"3\" v3=\"2\"/>\r\n"
            "    </triangles>\r\n   </mesh>\r\n  </object>\r\n </resources>\r\n</model>\r\n");
    put("3mf: model_settings.config",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config>\n  <object id=\"2\">\n"
        "    <metadata key=\"name\" value=\"Cube\"/>\n    <metadata key=\"extruder\" value=\"1\"/>\n"
        "    <part id=\"1\" subtype=\"normal_part\">\n      <metadata key=\"name\" value=\"Cube\"/>\n"
        "      <metadata key=\"matrix\" value=\"1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1\"/>\n"
        "      <metadata key=\"source_file\" value=\"C:\\Users\\a\\Cube &amp; co.stl\"/>\n"
        "      <mesh_stat edges_fixed=\"0\" degenerate_facets=\"0\" facets_removed=\"0\" facets_reversed=\"0\" "
        "backwards_edges=\"0\"/>\n    </part>\n    <part id=\"2\" subtype=\"modifier_part\">\n"
        "      <metadata key=\"sparse_infill_density\" value=\"40%\"/>\n    </part>\n  </object>\n  <plate>\n"
        "    <metadata key=\"plater_id\" value=\"1\"/>\n    <metadata key=\"plater_name\" value=\"\"/>\n"
        "    <metadata key=\"locked\" value=\"false\"/>\n    <model_instance>\n"
        "      <metadata key=\"object_id\" value=\"2\"/>\n      <metadata key=\"instance_id\" value=\"0\"/>\n"
        "    </model_instance>\n  </plate>\n  <assemble>\n"
        "   <assemble_item object_id=\"2\" instance_id=\"0\" transform=\"1 0 0 0 1 0 0 0 1 0 0 0\" offset=\"0 0 0\" />\n"
        "  </assemble>\n</config>\n");
    put("3mf: relationships",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n"
        " <Relationship Target=\"/3D/3dmodel.model\" Id=\"rel-1\" "
        "Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>\n"
        " <Relationship Target=\"/Metadata/plate_1.png\" Id=\"rel-2\" "
        "Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail\"/>\n"
        "</Relationships>");
    put("3mf: content types",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n"
        " <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n"
        " <Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\n"
        " <Default Extension=\"png\" ContentType=\"image/png\"/>\n</Types>");
    put("3mf: a plain model with an inline mesh",
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n"
        "<model xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\" unit=\"inch\" xml:lang=\"en-US\">\n"
        "  <resources>\n    <basematerials id=\"5\"><base name=\"Red\" displaycolor=\"#FF0000FF\"/></basematerials>\n"
        "    <object id=\"1\" type=\"model\" name=\"Box &#233;\" pid=\"5\" pindex=\"0\">\n"
        "      <mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\" /><vertex x=\"10.5\" y=\"0\" z=\"0\" />"
        "<vertex x=\"0\" y=\"1e1\" z=\"0\" /><vertex x=\"0\" y=\"0\" z=\"-.5E+1\" /></vertices>\n"
        "      <triangles><triangle v1=\"0\" v2=\"2\" v3=\"1\" /><triangle v1=\"0\" v2=\"1\" v3=\"3\" />"
        "<triangle v1=\"1\" v2=\"2\" v3=\"3\" /><triangle v1=\"2\" v2=\"0\" v3=\"3\" /></triangles></mesh>\n"
        "    </object>\n    <object id=\"3\" type=\"model\"><components>"
        "<component objectid=\"1\" transform=\"0.5 0 0 0 0.5 0 0 0 0.5 1 2 3\"/><component objectid=\"1\"/>"
        "</components></object>\n  </resources>\n"
        "  <build><item objectid=\"3\" transform=\"1 0 0 0 1 0 0 0 1 50 50 0\"/><item objectid=\"1\"/></build>\n"
        "</model>\n");
    put("3mf: a PrusaSlicer model config",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config>\n <object id=\"1\" instances_count=\"1\">\n"
        "  <metadata type=\"object\" key=\"name\" value=\"box\"/>\n  <volume firstid=\"0\" lastid=\"11\">\n"
        "   <metadata type=\"volume\" key=\"modifier\" value=\"0\"/>\n   <mesh edges_fixed=\"0\"/>\n"
        "  </volume>\n </object>\n</config>\n");
    put("3mf: cut information",
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<objects>\n <object id=\"1\">\n"
        "  <cut_id id=\"0\" check_sum=\"1\" connectors_cnt=\"0\"/>\n </object>\n</objects>\n");
    put("3mf: slice info",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config>\n  <header>\n"
        "    <header_item key=\"X-BBL-Client-Type\" value=\"slicer\"/>\n"
        "    <header_item key=\"X-BBL-Client-Version\" value=\"02.00.00.95\"/>\n  </header>\n</config>\n");

    // Content.
    put("an empty element", "<r/>");
    put("text and elements", "<r>a<b>c</b>d<e></e></r>");
    put("an XML declaration and white space", "<?xml version=\"1.0\"?>\n\n<r>\n  <a x=\"1\" y='2'/>\n</r>\n \t\n");
    put("comments and processing instructions",
        "<?xml version=\"1.0\"?><!--c--><?p d?>\n<r><!-- x --><?q?>t<?xml-stylesheet href=\"a\"?>u</r><!--e--><?z  z ?> \n");
    put("a CDATA section", "<r>a<![CDATA[x]]b]]]>c<>&amp;\r\nx\ry\n]]>b<![CDATA[]]></r>");
    put("line ends in text", "<r>a\r\nb\rc\n\r\r\nd\r</r>\r");
    put("line ends in attributes", "<r a=\"x\r\ny\rz\n\tw\" b='&#13;&#10;&#9;' c=\"\r\"/>");
    put("predefined entities", "<r a=\"&lt;&gt;&amp;&quot;&apos;\">&lt;&gt;&amp;&quot;&apos;</r>");
    put("character references",
        "<r a=\"&#65;&#x42;&#x10FFFF;\">&#65;&#x42;&#x10FFFF;&#128512;&#xe9;&#9;&#10;&#13;&#x20;</r>");
    {
        std::string s = "<r>";
        for (int i = 0; i < 400; ++i)
            s += i % 7 == 0 ? "line &amp; \xC3\xA9\n" : "text-";
        put("long text", s + "</r>");
    }
    {
        std::string s = "<r";
        for (int i = 0; i < 40; ++i)
            s += " a" + std::to_string(i) + "=\"v" + std::to_string(i * 7) + "\"";
        put("forty attributes", s + "><b/></r>");
    }
    put("a long attribute value", "<r a=\"" + std::string(3000, 'v') + "\" b='&amp;" + std::string(1500, ' ') + "x'/>");
    {
        const std::string n(2000, 'n');
        put("a long name", "<" + n + " a=\"1\">x</" + n + ">");
    }
    {
        std::string s;
        for (int i = 0; i < 300; ++i)
            s += "<e" + std::to_string(i % 10) + ">";
        for (int i = 299; i >= 0; --i)
            s += "</e" + std::to_string(i % 10) + ">";
        put("deep nesting", s);
    }
    put("colons, dots and dashes in names", "<a:b c:d=\"1\" e.f-g_h=\"2\"><_x.y-z/><:c/><x:/></a:b>");
    put("non-ASCII names",
        "<\xC3\xA9 a\xC3\xA9=\"1\"><\xE4\xB8\xAD\xE6\x96\x87 \xE5\xB1\x9E\xE6\x80\xA7=\"\xE5\x80\xBC\"/>"
        "<a\xC2\xB7"
        "b/></\xC3\xA9>");

    // The document type declaration.
    put("a document type declaration with defaults and entities",
        "<?xml version=\"1.0\" standalone=\"yes\"?>\n<!DOCTYPE r [\n<!ELEMENT r (#PCDATA|a|b)*>\n<!ELEMENT a EMPTY>\n"
        "<!ELEMENT b ANY>\n<!ELEMENT c (a,(b|a)?,a+)*>\n"
        "<!ATTLIST r id ID #IMPLIED k CDATA \"d  e\" t NMTOKENS \" x  y \" f (one|two) \"two\" "
        "g CDATA #FIXED \"fixed\" h CDATA #REQUIRED>\n<!ATTLIST a k CDATA 'v&amp;&#38;&#x26;'>\n"
        "<!ENTITY e1 \"one\">\n<!ENTITY e2 \"&e1; and &#60;b/&#62;\">\n<!ENTITY e3 '<a k=\"1\"/>text<b>in</b>'>\n"
        "<!NOTATION n SYSTEM \"n\">\n<!ENTITY u SYSTEM \"u.bin\" NDATA n>\n<!ENTITY x SYSTEM \"x.xml\">\n"
        "<!ENTITY p PUBLIC \"-//X//Y\" \"p.xml\">\n<!-- comment -->\n<?pi data?>\n]>\n"
        "<r h=\"1\" t=\"  a\tb\nc  \" id=\" i1 \">&e1;&e2;&e3;<a/><a k=\"z\"/></r>");
    put("entities in attributes",
        "<!DOCTYPE r [<!ENTITY q \"a&#9;b&#38;amp;\"><!ENTITY w \"&q; &q;\">]><r a=\"&q;&#9;&#10;x&#13;\" b=\"&w;\" c=\" &q; \"/>");
    put("an external entity in content", "<!DOCTYPE r [<!ENTITY x SYSTEM \"x.xml\">]><r>a&x;b</r>");
    put("an external entity in an attribute", "<!DOCTYPE r [<!ENTITY x SYSTEM \"x.xml\">]><r a=\"&x;\"/>");
    put("an unparsed entity in content", "<!DOCTYPE r [<!NOTATION n SYSTEM \"n\"><!ENTITY u SYSTEM \"u\" NDATA n>]><r>&u;</r>");
    put("a recursive entity", "<!DOCTYPE r [<!ENTITY a \"x&b;\"><!ENTITY b \"&a;y\">]><r>&a;</r>");
    put("a recursive entity in an attribute", "<!DOCTYPE r [<!ENTITY a \"x&b;\"><!ENTITY b \"&a;y\">]><r v=\"&a;\"/>");
    put("an undefined entity", "<r>&nope;</r>");
    put("an undefined entity with an external subset", "<!DOCTYPE r SYSTEM \"r.dtd\"><r>a&nope;b</r>");
    put("an undefined entity, standalone, with an external subset",
        "<?xml version=\"1.0\" standalone=\"yes\"?><!DOCTYPE r SYSTEM \"r.dtd\"><r>&nope;</r>");
    put("an undefined entity in an attribute with an external subset", "<!DOCTYPE r SYSTEM \"r.dtd\"><r a=\"&nope;\"/>");
    put("an entity holding < in an attribute", "<!DOCTYPE r [<!ENTITY lt2 \"&#60;\">]><r a=\"&lt2;\"/>");
    put("a predefined entity redeclared", "<!DOCTYPE r [<!ENTITY lt \"&#38;#60;\"><!ENTITY amp \"x\">]><r a=\"&lt;\">&lt;&amp;</r>");
    put("a parameter entity", "<!DOCTYPE r [<!ENTITY % pe \"<!ENTITY e 'x'>\">%pe;]><r>&e;</r>");
    put("a parameter entity in an entity value", "<!DOCTYPE r [<!ENTITY % pe \"x\"><!ENTITY e \"%pe;\">]><r/>");
    put("a public identifier", "<!DOCTYPE r PUBLIC \"-//W3C//DTD X 1.0//EN\" \"http://x/y.dtd\"><r/>");
    put("a bad public identifier", "<!DOCTYPE r PUBLIC \"a{b}\" \"x\"><r/>");
    put("element declarations",
        "<!DOCTYPE r [<!ELEMENT r (a|b|c)+><!ELEMENT a (#PCDATA)><!ELEMENT b (#PCDATA|a)*><!ELEMENT c ((a,b)|(b,a))?>"
        "<!ELEMENT d (a*,b+,c?)>]><r><a>1</a><b>2<a/></b></r>");
    put("attribute types and normalization",
        "<!DOCTYPE r [<!ATTLIST r a NMTOKEN \" x \" b NMTOKENS \"  p   q  \" c ENTITY #IMPLIED d IDREFS #IMPLIED "
        "e NOTATION (n|m) #IMPLIED f CDATA \"  s  \" a CDATA \"second\">]>"
        "<r b=\"  u   v \" d=\" i  j \" f=\" t \"/>");
    put("a default that is given", "<!DOCTYPE r [<!ATTLIST r k CDATA \"d\" l CDATA \"e\">]><r k=\"spec\"><r l=\"x\" k=\"y\"/></r>");
    put("an entity with markup", "<!DOCTYPE r [<!ENTITY e '<a k=\"1\">x<b/>y</a>&#38;amp;'>]><r>1&e;2&e;3</r>");
    put("an entity that opens an element", "<!DOCTYPE r [<!ENTITY s \"<b>\">]><r>&s;</b></r>");
    put("an entity that closes an element", "<!DOCTYPE r [<!ENTITY s \"</r>\">]><r>&s;");
    put("a syntax error in the subset", "<!DOCTYPE r [<!ELEMENT r (a,|b)>]><r/>");
    put("an unended subset", "<!DOCTYPE r [<!ENTITY e \"x\">");
    put("two document types", "<!DOCTYPE r><!DOCTYPE r><r/>");

    // Character references that fail.
    put("character reference 0", "<r>&#0;</r>");
    put("character reference D800", "<r>&#xD800;</r>");
    put("character reference FFFE", "<r>&#xFFFE;</r>");
    put("character reference 110000", "<r>&#x110000;</r>");
    put("a huge character reference", "<r>&#99999999999;</r>");
    put("an empty hexadecimal reference", "<r>&#x;</r>");
    put("an unended character reference", "<r>&#65</r>");
    put("character reference 0 in an attribute", "<r a=\"&#0;\"/>");

    // Errors.
    put("a mismatched tag", "<r>\n  <a></b>\n</r>");
    put("an unclosed element", "<r>");
    put("an unended start tag", "<r");
    put("an unended attribute", "<r a=\"1");
    put("a duplicate attribute", "<r a=\"1\" a=\"2\"/>");
    put("an unquoted attribute", "<r a=1/>");
    put("< in an attribute", "<r a=\"<\"/>");
    put("attributes without space", "<r a=\"x\"b=\"y\"/>");
    put("two root elements", "<r/><r/>");
    put("text after the root", "<r/>text");
    put("text before the root", "text<r/>");
    put("an empty document", "");
    put("white space only", "  \n\t ");
    put("a comment only", "<!-- only -->");
    put("]]> in content", "<r>]]></r>");
    put("-- in a comment", "<r><!-- a -- b --></r>");
    put("an XML declaration in content", "<r><?xml version=\"1.0\"?></r>");
    put("an XML declaration after the root", "<r/><?xml version=\"1.0\"?>");
    put("an XML declaration after space", " <?xml version=\"1.0\"?><r/>");
    put("an XML declaration without a version", "<?xml?><r/>");
    put("a bad standalone", "<?xml version=\"1.0\" standalone=\"maybe\"?><r/>");
    put("an encoding before the version", "<?xml encoding=\"UTF-8\"?><r/>");
    put("mismatched quotes in the declaration", "<?xml version='1.0\" encoding=\"UTF-8\"?><r/>");
    put("no space in the declaration", "<?xml version=\"1.0\"encoding=\"UTF-8\"?><r/>");
    put("an empty encoding name", "<?xml version=\"1.0\" encoding=\"\"?><r/>");
    put("a lone ampersand", "<r>&</r>");
    put("an unended entity reference", "<r>&a</r>");
    put("an empty entity reference", "<r>&;</r>");
    put("an end tag only", "</r>");
    put("space in tags", "<r ></r ><!-- -->");
    put("space after the slash", "<r/ >");
    put("a name starting with a digit", "<1r/>");
    put("an attribute name starting with a digit", "<r 1a=\"x\"/>");
    put("an unclosed CDATA section", "<r><![CDATA[x</r>");
    put("NUL in text", lit("<r>a\0b</r>"));
    put("a control character in text", "<r>\x01</r>");
    put("a control character in an attribute", "<r a=\"\x0C\"/>");
    put("a reserved processing instruction target", "<r><?XmL x?></r>");

    // UTF-8.
    put("UTF-8 with a byte order mark",
        "\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"UTF-8\"?><r a=\"\xC3\xA9\">\xC3\xA9\xE4\xB8\xAD\xF0\x9F\x98\x80</r>");
    put("an overlong UTF-8 sequence", "<r>\xC0\x80</r>");
    put("a UTF-8 surrogate", "<r>\xED\xA0\x80</r>");
    put("UTF-8 past U+10FFFF", "<r>\xF4\x90\x80\x80</r>");
    put("UTF-8 U+FFFE", "<r>\xEF\xBF\xBE</r>");
    put("UTF-8 U+FFFF", "<r>\xEF\xBF\xBF</r>");
    put("a lone continuation byte", "<r>\x80</r>");
    put("a cut two-byte sequence", "<r>\xC3</r>");
    put("a cut three-byte sequence", "<r>\xE4\xB8</r>");
    put("a cut sequence at the end", "<r>\xE4\xB8");
    put("a name with U+00D7", "<a\xC3\x97"
                              "b/>");
    put("a name starting with U+00B7", "<\xC2\xB7"
                                       "a/>");
    put("a name starting with a combining mark", "<\xCC\x80/>");
    put("a byte order mark only", "\xEF\xBB\xBF");
    put("a cut byte order mark", "\xEF\xBB");

    // Declared encodings.
    {
        std::string s = "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><r a=\"\xE9\xFF\">\xE0\xE9\xE8";
        for (int i = 0; i < 700; ++i)
            s += i % 5 == 0 ? "\xA0\n" : "ab\xE7";
        put("ISO-8859-1", s + "</r>");
    }
    put("iso-8859-1 in lower case", "<?xml version=\"1.0\" encoding=\"iso-8859-1\"?><\xE9 \xE9=\"\xB5\"/>");
    put("US-ASCII with a high byte", "<?xml version=\"1.0\" encoding=\"US-ASCII\"?><r>\xE9</r>");
    put("us-ascii", "<?xml version=\"1.0\" encoding=\"us-ascii\"?><r>abc</r>");
    put("an unknown encoding", "<?xml version=\"1.0\" encoding=\"windows-1252\"?><r/>");
    put("UTF8 without a dash", "<?xml version=\"1.0\" encoding=\"UTF8\"?><r/>");
    put("UTF-16 declared in UTF-8", "<?xml version=\"1.0\" encoding=\"UTF-16\"?><r/>");
    put("UTF-8 declared after a UTF-8 byte order mark", "\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"utf-8\"?><r/>");
    put("ISO-8859-1 declared after a UTF-8 byte order mark",
        "\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><r>\xC3\xA9</r>");
    {
        const std::string body = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><r a=\"\xC3\xA9\xF0\x9F\x98\x80\">text \xC3\xA9 "
                                 "\xF0\x9F\x98\x80 \xE4\xB8\xAD<b/>\r\n&amp;&#233;</r>";
        put("UTF-16LE with a byte order mark", utf16(body, false, true));
        put("UTF-16BE with a byte order mark", utf16(body, true, true));
        put("UTF-16LE without a byte order mark", utf16("<r>x\xC3\xA9</r>", false, false));
        put("UTF-16BE without a byte order mark", utf16("<r>x\xC3\xA9</r>", true, false));
        put("UTF-16LE declared UTF-16LE", utf16("<?xml version=\"1.0\" encoding=\"UTF-16LE\"?><r/>", false, true));
        put("UTF-16BE declared UTF-8", utf16("<?xml version=\"1.0\" encoding=\"UTF-8\"?><r/>", true, true));
        put("UTF-16LE declared ISO-8859-1", utf16("<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><r/>", false, false));
        std::string cut = utf16(body, false, true);
        cut.pop_back();
        put("UTF-16LE cut in a character", cut);
        std::string lone = utf16("<r>ab</r>", true, true);
        lone[8]          = '\xD8';
        lone[9]          = '\x00';
        put("UTF-16BE with a lone surrogate", lone);
        std::string text = "<r k=\"\xE4\xB8\xAD\">";
        for (int i = 0; i < 300; ++i)
            text += i % 9 == 0 ? "\xF0\x9F\x98\x80\n" : "\xC3\xA9x";
        put("long UTF-16LE text", utf16(text + "</r>", false, true));
        put("UTF-16BE with a name in U+4E2D", utf16("<\xE4\xB8\xAD \xE4\xB8\xAD=\"1\"/>", true, false));
    }
    return d;
}

// ---- Generated documents ---------------------------------------------------------------------

std::string space(SplitMix &g)
{
    static const char *const w[] = {" ", " ", " ", "  ", "\t", "\n", "\r\n", "\n   "};
    return w[g.below(8)];
}

std::string line_end(SplitMix &g)
{
    static const char *const w[] = {"\n", "\n", "\n", "\r\n", "", "\n  "};
    return w[g.below(6)];
}

std::string number(SplitMix &g)
{
    char         b[64];
    const double v = double(int64_t(g.below(2000001)) - 1000000);
    switch (g.below(4)) {
    case 0: std::snprintf(b, sizeof b, "%d", int(g.below(401)) - 200); break;
    case 1: std::snprintf(b, sizeof b, "%.9g", v / 997.0); break;
    case 2: std::snprintf(b, sizeof b, "%.6f", v / 31.0); break;
    default: std::snprintf(b, sizeof b, "%.3e", v / 7.0); break;
    }
    return b;
}

std::string attr(SplitMix &g, const std::string &name, const std::string &value)
{
    const char  q = g.below(5) == 0 ? '\'' : '"';
    std::string s = space(g);
    s += name;
    s += g.below(9) == 0 ? " = " : "=";
    s += q;
    s += value;
    s += q;
    return s;
}

std::string text(SplitMix &g)
{
    static const char *const t[] = {"Cube", " ", "&amp;", "&lt;", "&#233;", "\xC3\xA9", "\xE4\xB8\xAD", "&quot;", "x", "\n", "&#x1F600;"};
    std::string              s;
    const uint64_t           n = g.below(8);
    for (uint64_t i = 0; i < n; ++i)
        s += t[g.below(11)];
    return s;
}

std::string transform(SplitMix &g)
{
    std::string s;
    for (int i = 0; i < 12; ++i) {
        if (i != 0)
            s += ' ';
        s += number(g);
    }
    return s;
}

std::string uuid(SplitMix &g)
{
    char           b[64];
    const unsigned a  = unsigned(g.next() & 0xFFFFFFFFu);
    const unsigned b1 = unsigned(g.below(65536));
    const unsigned b2 = unsigned(g.below(65536));
    const unsigned b3 = unsigned(g.below(65536));
    const auto     c  = (unsigned long long) (g.next() & 0xFFFFFFFFFFFFull);
    std::snprintf(b, sizeof b, "%08x-%04x-%04x-%04x-%012llx", a, b1, b2, b3, c);
    return b;
}

std::string hex_paint(SplitMix &g)
{
    static const char *const h = "0123456789ABCDEF";
    std::string              s;
    const uint64_t           n = 1 + g.below(12);
    for (uint64_t i = 0; i < n; ++i)
        s += h[g.below(16)];
    return s;
}

std::string gen_model(SplitMix &g, size_t nv, size_t nt)
{
    std::string s;
    if (g.below(4) == 0)
        s += "\xEF\xBB\xBF";
    s += "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
    s += line_end(g);
    s += "<model";
    s += attr(g, "unit", "millimeter");
    s += attr(g, "xml:lang", "en-US");
    s += NS;
    s += ">";
    s += line_end(g);
    s += " <metadata";
    s += attr(g, "name", "Application");
    s += ">BambuStudio-01.09.00.70</metadata>";
    s += line_end(g);
    s += " <metadata name=\"Title\">";
    s += text(g);
    s += "</metadata>";
    s += line_end(g);
    s += " <resources>";
    s += line_end(g);
    s += "  <object";
    s += attr(g, "id", "1");
    s += attr(g, "p:UUID", uuid(g));
    s += attr(g, "type", "model");
    s += ">";
    s += line_end(g);
    s += "   <mesh>";
    s += line_end(g);
    s += "    <vertices>";
    s += line_end(g);
    for (size_t i = 0; i < nv; ++i) {
        s += "     <vertex";
        s += attr(g, "x", number(g));
        s += attr(g, "y", number(g));
        s += attr(g, "z", number(g));
        s += g.below(9) == 0 ? " />" : "/>";
        s += line_end(g);
    }
    s += "    </vertices>";
    s += line_end(g);
    s += "    <triangles>";
    s += line_end(g);
    for (size_t i = 0; i < nt; ++i) {
        s += "     <triangle";
        s += attr(g, "v1", std::to_string(g.below(nv + 1)));
        s += attr(g, "v2", std::to_string(g.below(nv + 1)));
        s += attr(g, "v3", std::to_string(g.below(nv + 1)));
        if (g.below(4) == 0)
            s += attr(g, "paint_color", hex_paint(g));
        if (g.below(6) == 0)
            s += attr(g, "paint_supports", hex_paint(g));
        s += "/>";
        s += line_end(g);
    }
    s += "    </triangles>";
    s += line_end(g);
    s += "   </mesh>";
    s += line_end(g);
    s += "  </object>";
    s += line_end(g);
    s += "  <object id=\"2\" type=\"model\">";
    s += line_end(g);
    s += "   <components>";
    s += line_end(g);
    const uint64_t nc = 1 + g.below(3);
    for (uint64_t i = 0; i < nc; ++i) {
        s += "    <component";
        s += attr(g, "p:path", "/3D/Objects/object_1.model");
        s += attr(g, "objectid", "1");
        s += attr(g, "transform", transform(g));
        s += "/>";
        s += line_end(g);
    }
    s += "   </components>";
    s += line_end(g);
    s += "  </object>";
    s += line_end(g);
    s += " </resources>";
    s += line_end(g);
    s += " <build>";
    s += line_end(g);
    s += "  <item";
    s += attr(g, "objectid", "2");
    s += attr(g, "transform", transform(g));
    s += attr(g, "printable", "1");
    s += "/>";
    s += line_end(g);
    s += " </build>";
    s += line_end(g);
    s += "</model>";
    s += line_end(g);
    return s;
}

std::string gen_config(SplitMix &g, size_t objects)
{
    std::string s = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
    s += line_end(g);
    s += "<config>";
    s += line_end(g);
    for (size_t o = 0; o < objects; ++o) {
        s += "  <object";
        s += attr(g, "id", std::to_string(o + 1));
        s += ">";
        s += line_end(g);
        const uint64_t nm = g.below(5);
        for (uint64_t i = 0; i < nm; ++i) {
            s += "    <metadata";
            s += attr(g, "key", "name");
            s += attr(g, "value", text(g));
            s += "/>";
            s += line_end(g);
        }
        const uint64_t np = 1 + g.below(3);
        for (uint64_t i = 0; i < np; ++i) {
            s += "    <part";
            s += attr(g, "id", std::to_string(i + 1));
            s += attr(g, "subtype", g.below(3) == 0 ? "modifier_part" : "normal_part");
            s += ">";
            s += line_end(g);
            s += "      <metadata";
            s += attr(g, "key", "matrix");
            s += attr(g, "value", transform(g));
            s += "/>";
            s += line_end(g);
            s += "    </part>";
            s += line_end(g);
        }
        s += "  </object>";
        s += line_end(g);
    }
    s += "  <plate>";
    s += line_end(g);
    s += "    <metadata key=\"plater_id\" value=\"1\"/>";
    s += line_end(g);
    s += "  </plate>";
    s += line_end(g);
    s += "</config>";
    s += line_end(g);
    return s;
}

std::string mutate(SplitMix &g, std::string s)
{
    static const char *const tokens[] = {"<",     ">",     "&",     ";",  "\"",   "'",    "=",    "/",         "!",
                                         "?",     "[",     "]",     "-",  "#",    "\r",   "\n",   "\t",        " ",
                                         "\x80",  "\xC3",  "\xFF",  "&amp;", "&#", "&#x", "<!--", "-->",       "]]>",
                                         "<![CDATA[", "&e1;", "</", "<r>", "x",  ":",    "\xEF\xBB\xBF", "%",
                                         "<?xml version=\"1.0\"?>"};
    const size_t             n_tokens = sizeof tokens / sizeof tokens[0];
    const uint64_t           edits    = 1 + g.below(3);
    for (uint64_t k = 0; k < edits; ++k) {
        const size_t at = size_t(g.below(s.size() + 1));
        switch (g.below(6)) {
        case 0: s.erase(at, size_t(1 + g.below(4))); break;
        case 1:
        case 2: s.insert(at, tokens[g.below(n_tokens)]); break;
        case 3:
            if (at < s.size())
                s[at] = char(g.next());
            break;
        case 4: {
            const std::string piece = s.substr(at, size_t(1 + g.below(16)));
            s.insert(at, piece);
            break;
        }
        default:
            if (g.below(4) == 0)
                s.resize(at);
            else
                s.insert(at, 1, '\0');
            break;
        }
    }
    return s;
}

std::vector<uint32_t> seeded_pattern(SplitMix &g, size_t n)
{
    std::vector<uint32_t> p(size_t(1 + g.below(4)));
    const uint64_t        top = n < 4 ? 2 : n / 2;
    for (uint32_t &x : p)
        x = uint32_t(1 + g.below(g.below(3) == 0 ? 16 : top));
    return p;
}

// ---- Recording -------------------------------------------------------------------------------

Case make(const Doc &d, uint8_t mode, std::vector<uint32_t> pattern, int32_t stop_at = -1, bool stop_after = false)
{
    Case c;
    c.label      = d.label;
    c.mode       = mode;
    c.doc        = d.text;
    c.pattern    = std::move(pattern);
    c.stop_at    = stop_at;
    c.stop_after = stop_after;
    return c;
}

void put(Sets &sets, const Case &c, bool committed) { add(sets, "XPRS", run_case(c).first, committed); }

void record_docs(Sets &sets)
{
    SplitMix               g(0x584D4C0001ull);
    const std::vector<Doc> docs = hand_docs();
    // Every hand-written document, fed each way.
    for (const Doc &d : docs) {
        const size_t n = d.text.size();
        put(sets, make(d, PARSE_CHUNKS, {}), true);
        put(sets, make(d, BUFFER_WHOLE, {}), n <= 2000);
        if (n <= 700)
            put(sets, make(d, PARSE_CHUNKS, {1}), true);
        else if (n <= 8000)
            put(sets, make(d, PARSE_CHUNKS, {1}), false);
        put(sets, make(d, PARSE_CHUNKS, seeded_pattern(g, n)), n <= 1500);
        put(sets, make(d, BUFFER_CHUNKS, seeded_pattern(g, n)), n <= 1200);
        put(sets, make(d, PARSE_NOT_FINAL, seeded_pattern(g, n)), n <= 200);
        put(sets, make(d, PARSE_THEN_FINAL, seeded_pattern(g, n)), n <= 600);
        put(sets, make(d, BUFFER_CHUNKS, {1}), false);
        for (uint32_t k : {2u, 3u, 7u, 64u})
            put(sets, make(d, k % 2 == 0 ? PARSE_CHUNKS : BUFFER_CHUNKS, {k}), false);
    }
    // Stops: at the first callback, the second, a third of the way, half way and the last, alone
    // or with a stop at every later one.
    for (size_t i = 0; i < docs.size(); ++i) {
        const Doc     &d = docs[i];
        const uint64_t e = run_case(make(d, PARSE_CHUNKS, {})).second;
        if (e == 0)
            continue;
        const bool                 small = d.text.size() <= 1500;
        const std::vector<int32_t> at    = {0, 1, int32_t(e / 3), int32_t(e / 2), int32_t(e - 1)};
        for (size_t k = 0; k < at.size(); ++k) {
            if (uint64_t(at[k]) >= e)
                continue;
            const bool commit = small && i % 4 == 0 && k != 1;
            put(sets, make(d, PARSE_CHUNKS, {}, at[k], false), commit);
            put(sets, make(d, PARSE_CHUNKS, {}, at[k], true), commit && k == 2);
            put(sets, make(d, BUFFER_WHOLE, {}, at[k], k % 2 == 0), commit && k == 0);
            put(sets, make(d, PARSE_CHUNKS, seeded_pattern(g, d.text.size()), at[k], k % 2 == 1), false);
        }
    }
    // Mutations of each.
    for (size_t i = 0; i < docs.size(); ++i) {
        for (int m = 0; m < 14; ++m) {
            const Doc  mutant{docs[i].label + " (mutated)", mutate(g, docs[i].text)};
            const bool commit = m == 0 && mutant.text.size() <= 500;
            put(sets, make(mutant, PARSE_CHUNKS, seeded_pattern(g, mutant.text.size())), commit);
            put(sets, make(mutant, BUFFER_WHOLE, {}), commit && i % 2 == 0);
            if (m % 3 == 0)
                put(sets, make(mutant, BUFFER_CHUNKS, seeded_pattern(g, mutant.text.size())), false);
        }
    }
}

void record_generated(Sets &sets)
{
    SplitMix g(0x584D4C0002ull);
    // Small ones, committed.
    for (int i = 0; i < 3; ++i) {
        const size_t nv = size_t(4 + g.below(6));
        const size_t nt = size_t(4 + g.below(8));
        const Doc    model{"generated model, small", gen_model(g, nv, nt)};
        put(sets, make(model, PARSE_CHUNKS, seeded_pattern(g, model.text.size())), true);
        put(sets, make(model, BUFFER_WHOLE, {}), i == 0);
        const size_t objects = size_t(1 + g.below(2));
        const Doc    config{"generated config, small", gen_config(g, objects)};
        put(sets, make(config, BUFFER_WHOLE, {}), true);
    }
    // Larger ones: miniz's chunks (its 64 KiB reads of a stored entry, its 32 KiB dictionary of a
    // deflated one), seeded chunks, and whole.
    for (int i = 0; i < 24; ++i) {
        const size_t nv = size_t(20 + g.below(i < 20 ? 600 : 4000));
        const Doc    model{"generated model", gen_model(g, nv, nv * 2)};
        put(sets, make(model, PARSE_CHUNKS, {i % 2 == 0 ? 65536u : 32768u}), false);
        put(sets, make(model, PARSE_CHUNKS, seeded_pattern(g, model.text.size())), false);
        if (i % 3 == 0)
            put(sets, make(model, BUFFER_WHOLE, {}), false);
        if (i % 4 == 0) {
            const Doc mutant{"generated model (mutated)", mutate(g, model.text)};
            put(sets, make(mutant, PARSE_CHUNKS, {32768u}), false);
        }
        const size_t objects = size_t(1 + g.below(30));
        const Doc    config{"generated config", gen_config(g, objects)};
        put(sets, make(config, BUFFER_WHOLE, {}), false);
        put(sets, make(config, PARSE_CHUNKS, seeded_pattern(g, config.text.size())), false);
    }
}

// ---- XERR ------------------------------------------------------------------------------------

void record_err(Sets &sets)
{
    Payload p;
    p.u64(46);
    for (int code = -1; code <= 44; ++code) {
        const XML_LChar *s = XML_ErrorString(XML_Error(code));
        p.i32(code).boolean(s != nullptr).str(s != nullptr ? s : "");
    }
    add(sets, "XERR", p, true);
}

bool run_xml(const Options &options, std::string &error)
{
    Sets sets;
    head(sets, "xml");
    try {
        record_docs(sets);
        record_generated(sets);
        record_err(sets);
    } catch (const std::exception &e) {
        error = e.what();
        return false;
    }
    return save(sets, options, "xml.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"xml", false, run_xml};
    return t;
}

} // namespace probe
