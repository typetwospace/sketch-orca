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
// ptree.sop1: Boost 1.84's property_tree as the 3MF reader uses it (parity/SOP1.md, "Probe files"),
// recorded by orca-ptree-probe (2026-11-10); the port of read_xml and get<T> replays it. The reader
// reads the cut information and the height ranges with read_xml(std::istringstream, ptree) and
// flags 0 (rapidxml with parse_comment_nodes and nothing else) and asks the tree with get_child,
// get<T> through stream_translator (MSVC 14.42's num_get for long, unsigned long long, float and
// double), get<T> with a default, data() and the iteration of a node's children
// (src/libslic3r/Format/bbs_3mf.cpp:2559-2616 and :2885-2945). Each record holds its inputs, then
// what the build did with them:
//
//   PTXR  a document through read_xml exactly as the reader calls it: the exception's what(), or
//         the whole tree in document order (each node's key, data and number of children).
//   PTGT  a document through read_xml, then queries on the tree it gives: each made on a node
//         reached by child indexes from the root, with the call, its path and default, and the
//         answer or the what() of what it threw.
//
// The cases come from reading Boost's and MSVC's sources branch by branch, each from a fixed table
// here: rapidxml's documents under flags 0 (PTXR), each parse_error it can throw under them with
// the line read_xml counts (in the buffer rapidxml has already rewritten in place), paths, numbers
// under each of the four types the reader converts to, and the reader's own calls on the fixtures
// of 3mf-2-numbers, its 25 variants (model.cpp, number_variants) and the other fixtures whose cut
// information or height ranges reach read_xml (PTGT). The full set adds seeded mutations of the
// documents, seeded number texts and larger generated documents; the committed set is every case
// of the tables.
#include "probe.hpp"

#include <cstdio>
#include <cstring>
#include <exception>
#include <iterator>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include <boost/property_tree/ptree.hpp>
#include <boost/property_tree/xml_parser.hpp>

namespace probe {

namespace {

namespace pt = boost::property_tree;

void add(Sets &sets, const char (&kind)[5], const Payload &p, bool committed)
{
    if (committed)
        sets.committed.add(kind, p);
    sets.full.add(kind, p);
}

template<size_t N> std::string lit(const char (&s)[N]) { return std::string(s, N - 1); }

// ---- Reading ---------------------------------------------------------------------------------

// read_xml as the reader calls it: the bytes in a std::istringstream, flags 0. False, with the
// exception's what(), when it throws.
bool read(const std::string &bytes, pt::ptree &tree, std::string &what)
{
    std::istringstream iss(bytes);
    try {
        pt::read_xml(iss, tree);
        return true;
    } catch (const std::exception &e) {
        what = e.what();
        return false;
    }
}

// A node and everything under it, in document order: key, data, number of children, children.
void put_node(Payload &p, const std::string &key, const pt::ptree &t)
{
    p.str(key).str(t.data()).u64(uint64_t(t.size()));
    for (const auto &c : t)
        put_node(p, c.first, c.second);
}

struct Doc
{
    std::string label;
    std::string text;
};

// ---- PTXR ------------------------------------------------------------------------------------

Payload ptxr(const Doc &d)
{
    Payload p;
    p.str(d.label).str(d.text);
    pt::ptree   tree;
    std::string what;
    if (!read(d.text, tree, what))
        p.u8(1).str(what);
    else {
        p.u8(0);
        put_node(p, std::string(), tree);
    }
    return p;
}

// ---- PTGT ------------------------------------------------------------------------------------

// The calls a query makes on its node: the reader's own.
enum Call : uint8_t {
    GET_CHILD  = 0,  // get_child(path)
    GET_INT    = 1,  // get<int>(path)
    GET_SIZE   = 2,  // get<size_t>(path)
    GET_FLOAT  = 3,  // get<float>(path)
    GET_DOUBLE = 4,  // get<double>(path)
    GET_STRING = 5,  // get<std::string>(path)
    DEF_INT    = 6,  // get<int>(path, default)
    DEF_SIZE   = 7,  // get<size_t>(path, default)
    DEF_FLOAT  = 8,  // get<float>(path, default)
    DEF_DOUBLE = 9,  // get<double>(path, default)
    DEF_STRING = 10, // get<std::string>(path, default)
    DATA       = 11, // data()
    CHILDREN   = 12, // for (const auto &c : node): c.first, c.second
};

struct Query
{
    std::vector<uint64_t> at; // child indexes from the root to the node the call is made on
    uint8_t               call = DATA;
    std::string           path;
    int32_t               d_int    = 0;
    uint64_t              d_size   = 0;
    float                 d_float  = 0;
    double                d_double = 0;
    std::string           d_string;
};

Query q(std::vector<uint64_t> at, uint8_t call, std::string path = std::string())
{
    Query r;
    r.at   = std::move(at);
    r.call = call;
    r.path = std::move(path);
    return r;
}
Query q_int(std::vector<uint64_t> at, std::string path, int32_t d)
{
    Query r = q(std::move(at), DEF_INT, std::move(path));
    r.d_int = d;
    return r;
}
Query q_size(std::vector<uint64_t> at, std::string path, uint64_t d)
{
    Query r  = q(std::move(at), DEF_SIZE, std::move(path));
    r.d_size = d;
    return r;
}
Query q_float(std::vector<uint64_t> at, std::string path, float d)
{
    Query r   = q(std::move(at), DEF_FLOAT, std::move(path));
    r.d_float = d;
    return r;
}
Query q_double(std::vector<uint64_t> at, std::string path, double d)
{
    Query r    = q(std::move(at), DEF_DOUBLE, std::move(path));
    r.d_double = d;
    return r;
}
Query q_string(std::vector<uint64_t> at, std::string path, std::string d)
{
    Query r    = q(std::move(at), DEF_STRING, std::move(path));
    r.d_string = std::move(d);
    return r;
}

const pt::ptree *node_at(const pt::ptree &root, const std::vector<uint64_t> &at)
{
    const pt::ptree *n = &root;
    for (uint64_t i : at) {
        if (i >= n->size())
            return nullptr;
        auto it = n->begin();
        std::advance(it, ptrdiff_t(i));
        n = &it->second;
    }
    return n;
}

// The child indexes from `from` to the node at `target`, which get_child returned.
bool locate(const pt::ptree &from, const pt::ptree *target, std::vector<uint64_t> &out)
{
    if (&from == target)
        return true;
    uint64_t i = 0;
    for (const auto &c : from) {
        out.push_back(i);
        if (locate(c.second, target, out))
            return true;
        out.pop_back();
        ++i;
    }
    return false;
}

void put_query(Payload &p, const pt::ptree &root, const Query &x)
{
    p.u64s(x.at).u8(x.call).str(x.path);
    switch (x.call) {
    case DEF_INT: p.i32(x.d_int); break;
    case DEF_SIZE: p.u64(x.d_size); break;
    case DEF_FLOAT: p.f32(x.d_float); break;
    case DEF_DOUBLE: p.f64(x.d_double); break;
    case DEF_STRING: p.str(x.d_string); break;
    default: break;
    }
    const pt::ptree *n = node_at(root, x.at);
    if (n == nullptr)
        throw std::runtime_error("a query's node is not in its tree");
    Payload     a;
    std::string what;
    bool        threw = false;
    try {
        switch (x.call) {
        case GET_CHILD: {
            const pt::ptree      &c = n->get_child(x.path);
            std::vector<uint64_t> where;
            if (!locate(*n, &c, where))
                throw std::runtime_error("get_child gave a node outside its tree");
            a.u64s(where);
            break;
        }
        case GET_INT: a.i32(n->get<int>(x.path)); break;
        case GET_SIZE: a.u64(uint64_t(n->get<size_t>(x.path))); break;
        case GET_FLOAT: a.f32(n->get<float>(x.path)); break;
        case GET_DOUBLE: a.f64(n->get<double>(x.path)); break;
        case GET_STRING: a.str(n->get<std::string>(x.path)); break;
        case DEF_INT: a.i32(n->get<int>(x.path, int(x.d_int))); break;
        case DEF_SIZE: a.u64(uint64_t(n->get<size_t>(x.path, size_t(x.d_size)))); break;
        case DEF_FLOAT: a.f32(n->get<float>(x.path, x.d_float)); break;
        case DEF_DOUBLE: a.f64(n->get<double>(x.path, x.d_double)); break;
        case DEF_STRING: a.str(n->get<std::string>(x.path, x.d_string)); break;
        case DATA: a.str(n->data()); break;
        case CHILDREN:
            a.u64(uint64_t(n->size()));
            for (const auto &c : *n)
                a.str(c.first).str(c.second.data()).u64(uint64_t(c.second.size()));
            break;
        default: throw std::runtime_error("an unknown call");
        }
    } catch (const pt::ptree_error &e) {
        threw = true;
        what  = e.what();
    }
    if (threw)
        p.u8(1).str(what);
    else {
        p.u8(0);
        p.bytes += a.bytes;
    }
}

// The queries a record makes, from the tree read_xml gave (empty when it threw).
using Plan = std::vector<Query> (*)(const pt::ptree &);

Payload ptgt(const std::string &label, const std::string &bytes, const std::vector<Query> &fixed, Plan plan = nullptr)
{
    Payload p;
    p.str(label).str(bytes);
    pt::ptree   tree;
    std::string what;
    if (!read(bytes, tree, what)) {
        p.u8(1).str(what);
        return p;
    }
    std::vector<Query> qs = fixed;
    if (plan != nullptr) {
        std::vector<Query> more = plan(tree);
        qs.insert(qs.end(), more.begin(), more.end());
    }
    p.u8(0).u64(uint64_t(qs.size()));
    for (const Query &x : qs)
        put_query(p, tree, x);
    return p;
}

// ---- (a) Documents ---------------------------------------------------------------------------

std::vector<Doc> tree_docs()
{
    std::vector<Doc> d;
    auto             put = [&](const char *label, std::string text) { d.push_back({label, std::move(text)}); };

    // The prolog: what rapidxml skips under flags 0.
    put("empty input", "");
    put("white space only", " \t\r\n ");
    put("one empty element", "<a/>");
    put("an element pair with nothing in it", "<a></a>");
    put("a declaration, the root and a line feed", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<a/>\n");
    put("a declaration in upper case with single quotes", "<?XML version='1.0'?><a/>");
    put("<?xml?> with no space after xml is an instruction, skipped", "<?xml?><a/>");
    put("<?xml-stylesheet?>, an instruction, skipped", "<?xml-stylesheet href=\"a\"?><a/>");
    put("a declaration after the root, skipped", "<a/><?xml version=\"1.0\"?>");
    put("a comment before the declaration", "<!--c--><?xml version=\"1.0\"?><a/>");
    put("processing instructions, skipped", "<?pi x?><a><?pi data?>t<?p?>u<?q a?b?></a>");
    put("a doctype, skipped", "<!DOCTYPE a><a/>");
    put("a doctype with an internal subset; its entity is not known", "<!DOCTYPE a [ <!ENTITY e \"x\"> <!ELEMENT a (#PCDATA)> ]><a>&e;</a>");
    put("a doctype with nested brackets", "<!DOCTYPE a [ [ [] ] ] ><a/>");
    put("a doctype with > inside its subset", "<!DOCTYPE a [<!ENTITY e \"a>b\">]><a/>");
    put("<!DOCTYPE with no space after it, skipped as unknown", "<!DOCTYPEa><a/>");
    put("unknown <! declarations, skipped", "<!ELEMENT a ANY><a><!foo bar>x<!-y-></a>");

    // Comments and CDATA.
    put("comments kept as <xmlcomment>", "<!-- top --><a><!--in-->t<!---->u</a><!--after-->");
    put("a comment holding --", "<a><!-- x -- y --></a>");
    put("a comment ending --->", "<a><!--x---></a>");
    put("a comment of markup", "<a><!--<b>&amp;</b>--></a>");
    put("CDATA joins the data", "<a><![CDATA[<b>&amp;]]></a>");
    put("CDATA holding ]] and ]", "<a><![CDATA[x]]y]]]></a>");
    put("empty CDATA", "<a>x<![CDATA[]]>y</a>");
    put("CDATA at the top level goes into the root's data", "<![CDATA[top]]><a/><![CDATA[2]]>");
    put("<![CDATAx>, skipped as unknown", "<a><![CDATAx>y</a>");

    // Attributes.
    put("attributes in either quote", "<a b=\"1\" c='2' d=\"it's\" e='say \"x\"'/>");
    put("attributes with no space between them", "<a b=\"1\"c='2'/>");
    put("white space around =", "<a b \t=\r\n \"1\"/>");
    put("a repeated attribute", "<a b=\"1\" b=\"2\"/>");
    put("empty attribute values", "<a b=\"\" c=''/>");
    put("attribute values keep their white space", "<a b=\" x \t\n y \r\n\"/>");
    put("attribute names of odd characters", "<a b:c=\"1\" _-.9=\"2\" \"q=\"3\" []{}=\"4\"/>");
    put("attributes, text and children", "<a x=\"1\">t<b y=\"2\">u</b>v</a>");
    put("an attribute and a child of the same name", "<a b=\"1\"><b>2</b></a>");

    // Elements.
    put("element names of odd characters", "<r><p:UUID/><a=b/><1/><<x/><a\"b=\"1\"/>-<_/></r>");
    put("nested and repeated elements", "<r><a><b/></a><a/><b>x</b><a>y</a></r>");
    put("mixed content", "<a>x<b/>y<c>z</c>w</a>");
    put("mixed content with CDATA and comments", "<a>x<![CDATA[c]]>y<!--k-->z</a>");
    put("white space between children joins the data", "<a>\n  <b/>\n  <c/>\n</a>\n");
    put("white space in the data, not trimmed or normalised", "<a> \t x \n  y \r </a>");
    put("CR LF and a lone CR kept", "<a>1\r\n2\r3\n</a>");
    put("two roots", "<a>1</a><b>2</b>");
    put("a closing tag of another name", "<a>x</b>");
    put("an empty closing tag", "<a>x</>");
    put("white space in a closing tag", "<a>x</a \n\t>");
    put("a closing tag of odd characters", "<a>x</a=\"b\"\n>");
    put("a tab before />", "<a\t/>");

    // Entities and character references.
    put("the five named entities in text", "<a>&lt;&gt;&amp;&quot;&apos;</a>");
    put("the five named entities in attributes", "<a b=\"&lt;&gt;&amp;&quot;&apos;\" c='&apos;&quot;'/>");
    put("entities without ; or in another case, kept", "<a>&amp &AMP; &lt &gt &apos &quot &Amp; &ap;</a>");
    put("unknown entities kept as written", "<a b=\"&nbsp;&e;\">&nbsp;&copy;&unknown;</a>");
    put("a lone & kept", "<a b=\"&\">a & b &</a>");
    put("decimal references at each UTF-8 length",
        "<a>&#65;&#127;&#128;&#233;&#2047;&#2048;&#8364;&#65535;&#65536;&#128512;&#1114111;</a>");
    put("hexadecimal references at each UTF-8 length",
        "<a>&#x41;&#x7f;&#x80;&#xE9;&#x7FF;&#x800;&#x20AC;&#xffff;&#x10000;&#x1F600;&#x10FFFF;</a>");
    put("references to surrogates and noncharacters", "<a>&#xD800;&#xDFFF;&#xFFFE;&#xFDD0;</a>");
    put("a reference to 0, or with no digits, gives a NUL", "<a b=\"&#0;\">x&#0;y&#;z&#x;w</a>");
    put("decimal references take hexadecimal letters", "<a>&#1a;&#6F;&#9f;</a>");
    put("references past 32 bits wrap", "<a>&#4294967361;&#x100000041;&#4294967296;</a>");
    put("references with leading zeros", "<a>&#0000065;&#x00042;</a>");
    put("references in attribute values", "<a b=\"&#65;&#x42;&#10;&#13;&#9;\" c='&#39;'/>");

    // Bytes.
    put("bytes past ASCII in names, attributes and text",
        "<\xC3\xA9l\xC3\xA9 \xC3\xA0=\"\xE2\x82\xAC\">\xF0\x9F\x98\x80\xFF\xFE\x80</\xC3\xA9l\xC3\xA9>");
    put("control bytes in text and attributes", "<a b=\"\x01\x1F\x7F\">\x01\x08\x0B\x0C\x1B\x7F</a>");
    put("a UTF-8 byte order mark", "\xEF\xBB\xBF<a/>");
    put("a byte order mark inside the text",
        "<a>\xEF\xBB\xBF"
        "x</a>");
    put("a NUL after the root ends the parse", lit("<a/>\0junk<<<"));
    put("a NUL before the root ends it at once", lit("\0<a/>"));

    // Larger ones, made here.
    {
        std::string s;
        for (int i = 0; i < 200; ++i)
            s += "<d" + std::to_string(i % 3) + ">";
        s += "x";
        for (int i = 199; i >= 0; --i)
            s += "</d" + std::to_string(i % 3) + ">";
        put("nesting 200 deep", s);
    }
    {
        std::string s = "<r>";
        for (int i = 0; i < 300; ++i)
            s += "<c" + std::to_string(i % 4) + " i=\"" + std::to_string(i) + "\">" + std::to_string(i * 7) + "</c" + std::to_string(i % 4) + ">";
        put("300 children", s + "</r>");
    }
    put("a long name, value and text",
        "<" + std::string(1500, 'n') + " " + std::string(1000, 'a') + "=\"" + std::string(1500, 'v') + "\">" + std::string(1500, 't') + "</" +
            std::string(1500, 'n') + ">");
    return d;
}

// ---- (b) Errors and their lines --------------------------------------------------------------

// Every parse_error rapidxml can throw under flags 0, one or more documents for each place that
// throws it, and the line read_xml counts: the line feeds from the start of the buffer to where
// rapidxml stopped, in the buffer as rapidxml has rewritten it by then (names and attribute values
// cut with a NUL, text and attribute values expanded in place, their old bytes left behind).
std::vector<Doc> error_docs()
{
    std::vector<Doc> d;
    auto             put = [&](const char *label, std::string text) { d.push_back({label, std::move(text)}); };

    // expected < (rapidxml.hpp:1393), the document's top level.
    put("expected < at line 1: text before the root", "x<a/>");
    put("expected < at line 2: text after the root", "<a/>\nx");
    put("expected < at line 6", "<a/>\n\n\n\n\nx");
    put("expected <: a stray >", "<a/>>");
    put("expected <: a UTF-16 byte order mark", lit("\xFF\xFE<\0a\0/\0>\0"));
    put("expected <: a byte order mark after white space", " \xEF\xBB\xBF<a/>");
    put("expected <: two byte order marks", "\xEF\xBB\xBF\xEF\xBB\xBF<a/>");
    put("expected <: a doctype whose subset quotes ]", "<!DOCTYPE a [<!ENTITY e \"]\">]><a/>");

    // invalid numeric character entity (:1536), past U+10FFFF.
    put("invalid numeric character entity: &#x110000; in text", "<a>&#x110000;</a>");
    put("invalid numeric character entity: &#1114112; in an attribute", "<a b=\"&#1114112;\"/>");
    put("invalid numeric character entity: &#99999999;", "<a>x&#99999999;</a>");

    // expected ; (:1667).
    put("expected ;: &#65 before <", "<a>&#65</a>");
    put("expected ;: &#X41; (an upper-case X)", "<a>&#X41;</a>");
    put("expected ;: &#65 at the end of an attribute", "<a b=\"&#65\"/>");
    put("expected ;: &#-1;", "<a>&#-1;</a>");
    put("expected ;: &#65 ;", "<a>&#65 ;</a>");
    put("expected ;: &#x at the end of an attribute", "<a b='&#x'/>");

    // unexpected end of data, at each place that throws it.
    put("unexpected end of data: an open declaration (:1742)", "<?xml version=\"1.0\"");
    put("unexpected end of data: an open declaration in an element (:1742)", "<a><?xml x");
    put("unexpected end of data: an open comment (:1791)", "<a><!-- x");
    put("unexpected end of data: an open comment ending -- (:1791)", "<a><!-- x --");
    put("unexpected end of data: an open doctype subset (:1833)", "<!DOCTYPE a [ <!ENTITY");
    put("unexpected end of data: an open doctype (:1843)", "<!DOCTYPE a");
    put("unexpected end of data: an open instruction (:1924)", "<?pi x");
    put("unexpected end of data: <?> (:1924)", "<?>");
    put("unexpected end of data: open CDATA (:2015)", "<a><![CDATA[x]]");
    put("unexpected end of data: an open <! declaration (:2155)", "<!foo");
    put("unexpected end of data at line 1: an open element (:2224)", "<a>");
    put("unexpected end of data at line 2 (:2224)", "<a>\n");
    put("unexpected end of data at line 5 (:2224)", "<a>\n<b>x</b>\n\n\n");
    put("unexpected end of data: an open element holding a child (:2224)", "<a><b/>");
    put("unexpected end of data: a NUL inside an element (:2224)", lit("<a>x\0</a>"));
    put("unexpected end of data: a NUL inside a comment (:1791)", lit("<a><!--x\0--></a>"));

    // expected element name (:2042).
    put("expected element name: <>", "<>");
    put("expected element name: < a", "< a/>");
    put("expected element name: a closing tag at the top level", "</a>");
    put("expected element name: < and a line feed in an element, line 2", "<a>\n<\n/></a>");
    put("expected element name: < / in an element", "<a><b></b>< /></a>");

    // expected > (:2061, :2065, :2209).
    put("expected > at line 1: / then a space (:2061)", "<a/ >");
    put("expected > at line 2: / then x (:2061)", "<a\n/x>");
    put("expected > at line 4: ? after the attributes (:2065)", "<r>\n\n\n<a b=\"1\"?></r>");
    put("expected >: ! after an attribute (:2065)", "<a b=\"1\"!>");
    put("unexpected end of data, not expected >: ! after the name is part of the name (:2224)", "<a!>");
    put("expected >: a NUL in a tag (:2065)", lit("<a\0/>"));
    put("expected >: a closing tag with two names (:2209)", "<a></a b>");
    put("expected >: an open closing tag (:2209)", "<a></a");

    // expected = (:2259).
    put("expected =: an attribute with no value", "<a b/>");
    put("expected =: two names", "<a b c=\"1\"/>");
    put("expected =: an attribute name at the end", "<a b");

    // expected ' or " (:2272, :2288).
    put("expected ' or \": a value with no quote (:2272)", "<a b=1/>");
    put("expected ' or \": = at the end (:2272)", "<a b=");
    put("expected ' or \": an unterminated value (:2288)", "<a b=\"1/>");
    put("expected ' or \": a single quote closed by a double (:2288)", "<a b='1\"/>");
    put("expected ' or \": a NUL in a value (:2288)", lit("<a b=\"x\0\"/>"));

    // The line, counted in the buffer rapidxml has rewritten.
    put("line: a line feed after a finished element's name is not counted", "<r><b\n/>\n</r>\nx");
    put("line: a line feed after an attribute's name is not counted", "<r b\n=\"1\"/>\nx");
    put("line: a line feed after an open element's name is counted", "<r\n><b/>\nx");
    put("line: a line feed after a closed root's name is not counted", "<r\n></r>\nx");
    put("line: text expanded in place counts its line feeds again", "<r>&amp;\n\n</r>\nx");
    put("line: an attribute value expanded in place counts its line feeds again", "<r b=\"&amp;\n\"/>\nx");
    put("line: &#10; adds a line", "<r>&#10;&#10;</r>x");
    put("line: an invalid reference is counted to where it would be written", "<r>&amp;\n&#x110000;</r>");
    put("line: an invalid reference after &#10;", "<r>&#10;&#x110000;</r>");
    put("line: an invalid reference after expanded text and line feeds", "<r>&amp;&amp;\n\n&#x110000;</r>");
    put("line: expected ; after expanded text and a line feed", "<r>&amp;\n&#65</r>");
    put("line: the end of expanded text written over a line feed", "<r>&amp;\n\n\n\n\n</r>\nx");
    put("line: comments and CDATA keep their line feeds", "<r><!--\n--><![CDATA[\n]]></r>\nx");
    put("line: only line feeds count, not CR", "<a/>\r\n\r\rx");
    put("line: past 1000", "<a/>" + std::string(1500, '\n') + "x");
    return d;
}

// ---- (c, e) Paths and the calls ----------------------------------------------------------------

void record_paths(Sets &sets)
{
    // r's children: <xmlattr> 0, x 1, x 2, e 3, s 4, <xmlcomment> 5, dotted.name 6, e-acute 7, n 8.
    const std::string c1 = "<r a=\"1\" b=\" 2 \"><x>first<y>deep</y></x><x>second<z>other</z></x><e/><s>  text  </s><!--note-->"
                           "<dotted.name>d</dotted.name><\xC3\xA9>u</\xC3\xA9><n><m><k>v</k></m></n></r>";
    std::vector<Query> a;
    a.push_back(q({}, GET_CHILD, ""));
    a.push_back(q({}, GET_CHILD, "r"));
    a.push_back(q({}, GET_CHILD, "r.x"));
    a.push_back(q({}, GET_CHILD, "r.x.y"));
    a.push_back(q({}, GET_CHILD, "r.x.z"));
    a.push_back(q({}, GET_CHILD, "r.<xmlattr>"));
    a.push_back(q({}, GET_CHILD, "r.<xmlattr>.b"));
    a.push_back(q({}, GET_INT, "r.<xmlattr>.a"));
    a.push_back(q({}, GET_INT, "r.<xmlattr>.b"));
    a.push_back(q({}, GET_STRING, "r.<xmlattr>.b"));
    a.push_back(q({}, GET_INT, "r.<xmlattr>.c"));
    a.push_back(q({}, GET_CHILD, "r."));
    a.push_back(q({}, GET_CHILD, ".r"));
    a.push_back(q({}, GET_CHILD, "r..x"));
    a.push_back(q({}, GET_CHILD, "r.x."));
    a.push_back(q({}, GET_CHILD, "."));
    a.push_back(q({}, GET_CHILD, "r.<xmlcomment>"));
    a.push_back(q({}, GET_STRING, "r.<xmlcomment>"));
    a.push_back(q({}, GET_CHILD, "r.dotted.name"));
    a.push_back(q({}, GET_CHILD, "r.dotted"));
    a.push_back(q({}, GET_STRING, "r.\xC3\xA9"));
    a.push_back(q({}, GET_CHILD, "R"));
    a.push_back(q({}, GET_CHILD, "r.n.m.k.q.w"));
    a.push_back(q({}, GET_STRING, "r.n.m.k"));
    a.push_back(q({}, GET_STRING, "r.x"));
    a.push_back(q({}, GET_STRING, "r.s"));
    a.push_back(q({}, GET_STRING, "r"));
    a.push_back(q({}, GET_STRING, ""));
    for (uint8_t c : {GET_INT, GET_SIZE, GET_FLOAT, GET_DOUBLE, GET_STRING})
        a.push_back(q({}, c, "r.e"));
    for (uint8_t c : {GET_INT, GET_SIZE, GET_FLOAT, GET_DOUBLE, GET_STRING})
        a.push_back(q({}, c, "r.missing"));
    a.push_back(q_string({}, "r.missing", "dflt"));
    a.push_back(q_string({}, "r.s", "dflt"));
    a.push_back(q_int({}, "r.s", 7));
    a.push_back(q_int({}, "r.missing", 7));
    a.push_back(q_int({}, "r.<xmlattr>.a", 7));
    a.push_back(q_size({}, "r.<xmlattr>.b", 9));
    a.push_back(q_size({}, "r.x", 9));
    a.push_back(q_float({}, "r.x", 0.25f));
    a.push_back(q_float({}, "r.<xmlattr>.a", 0.25f));
    a.push_back(q_double({}, "r.<xmlattr>.a", 0.5));
    a.push_back(q_double({}, "nope", -0.0));
    // On a leaf (y), on the first x and on the second.
    a.push_back(q({0, 1, 0}, GET_CHILD, "q"));
    a.push_back(q({0, 1, 0}, GET_CHILD, ""));
    a.push_back(q({0, 1, 0}, DATA));
    a.push_back(q({0, 1, 0}, CHILDREN));
    a.push_back(q({0, 1}, GET_CHILD, "y"));
    a.push_back(q({0, 1}, GET_CHILD, "z"));
    a.push_back(q({0, 1}, DATA));
    a.push_back(q({0, 1}, GET_STRING, ""));
    a.push_back(q({0, 2}, GET_CHILD, "z"));
    a.push_back(q({0, 2}, GET_STRING, "z"));
    a.push_back(q({0, 0}, GET_INT, "a"));
    a.push_back(q({0, 0}, GET_CHILD, "<xmlattr>"));
    a.push_back(q({0, 5}, DATA));
    a.push_back(q({}, CHILDREN));
    a.push_back(q({0}, CHILDREN));
    a.push_back(q({0, 0}, CHILDREN));
    a.push_back(q({}, DATA));
    add(sets, "PTGT", ptgt("paths: separators, <xmlattr>, <xmlcomment>, missing nodes, leaves", c1, a), true);

    // The first of several children of one name; attributes of one name.
    const std::string  c2 = "<r q=\"1\" q=\"2\"><a>1</a><a>2</a><b><a>3</a></b><a><c>4</c></a></r>";
    std::vector<Query> b;
    b.push_back(q({}, GET_STRING, "r.a"));
    b.push_back(q({}, GET_STRING, "r.a.c"));
    b.push_back(q({}, GET_CHILD, "r.a"));
    b.push_back(q({}, GET_STRING, "r.b.a"));
    b.push_back(q({}, GET_STRING, "r.<xmlattr>.q"));
    b.push_back(q({0}, CHILDREN));
    b.push_back(q({0, 0}, CHILDREN));
    b.push_back(q({0, 4}, GET_STRING, "c"));
    b.push_back(q({0, 4}, GET_INT, "c"));
    add(sets, "PTGT", ptgt("paths: the first of several children of one name", c2, b), true);

    // The top level: a comment child and CDATA in the root's data.
    const std::string  c3 = "<!--c--><![CDATA[top]]><a>1</a><b x=\"y\"/>";
    std::vector<Query> c;
    c.push_back(q({}, DATA));
    c.push_back(q({}, GET_STRING, ""));
    c.push_back(q({}, GET_CHILD, "<xmlcomment>"));
    c.push_back(q({}, GET_STRING, "b.<xmlattr>.x"));
    c.push_back(q({}, GET_INT, "a"));
    c.push_back(q({}, CHILDREN));
    add(sets, "PTGT", ptgt("paths: the top level", c3, c), true);

    // Data around children, CDATA and references, as get<T> and get<std::string> see it.
    const std::string c4 = lit("<d><a> 5 <b/> </a><c>1<b/>2</c><e>&#32;7&#9;</e><f><![CDATA[ 8 ]]></f><g>5&#0;</g><h>&amp;&lt;x&gt;</h>"
                               "<i>\r\n9\r\n</i><j b=\"&#10;3&#10;\"/></d>");
    std::vector<Query> e;
    for (uint64_t i = 0; i < 8; ++i)
        for (uint8_t k : {GET_INT, GET_SIZE, GET_FLOAT, GET_DOUBLE, GET_STRING})
            e.push_back(q({0, i}, k, ""));
    for (uint8_t k : {GET_INT, GET_DOUBLE, GET_STRING})
        e.push_back(q({0, 7}, k, "<xmlattr>.b"));
    e.push_back(q({0}, DATA));
    add(sets, "PTGT", ptgt("data: text around children, CDATA, references and a NUL", c4, e), true);

    // An empty tree: what the reader's first call gets.
    std::vector<Query> f;
    f.push_back(q({}, GET_CHILD, "objects"));
    f.push_back(q({}, CHILDREN));
    f.push_back(q({}, DATA));
    f.push_back(q_int({}, "<xmlattr>.id", -1));
    add(sets, "PTGT", ptgt("an empty document", "", f), true);
    add(sets, "PTGT", ptgt("a document read_xml refuses", "<objects><object id=\"1\"></objects", f), true);
}

// ---- (d) Numbers -----------------------------------------------------------------------------

// Decimal digits of n * 5^k, most significant first.
std::string times_pow5(uint64_t n, int k)
{
    std::vector<int> d; // least significant first
    if (n == 0)
        d.push_back(0);
    for (; n != 0; n /= 10)
        d.push_back(int(n % 10));
    for (int i = 0; i < k; ++i) {
        int carry = 0;
        for (int &x : d) {
            const int v = x * 5 + carry;
            x           = v % 10;
            carry       = v / 10;
        }
        for (; carry != 0; carry /= 10)
            d.push_back(carry % 10);
    }
    std::string s;
    for (auto it = d.rbegin(); it != d.rend(); ++it)
        s.push_back(char('0' + *it));
    return s;
}

// Decimal digits of n * 2^k, most significant first.
std::string times_pow2(uint64_t n, int k)
{
    std::vector<int> d; // least significant first
    if (n == 0)
        d.push_back(0);
    for (; n != 0; n /= 10)
        d.push_back(int(n % 10));
    for (int i = 0; i < k; ++i) {
        int carry = 0;
        for (int &x : d) {
            const int v = x * 2 + carry;
            x           = v % 10;
            carry       = v / 10;
        }
        if (carry != 0)
            d.push_back(carry);
    }
    std::string s;
    for (auto it = d.rbegin(); it != d.rend(); ++it)
        s.push_back(char('0' + *it));
    return s;
}

// n * 2^-k written out exactly in decimal; for k < 0 an integer with no point.
std::string exact_binary(uint64_t n, int k)
{
    if (k < 0)
        return times_pow2(n, -k);
    const std::string digits = times_pow5(n, k);
    if (int(digits.size()) <= k)
        return "0." + std::string(size_t(k) - digits.size(), '0') + digits;
    return digits.substr(0, digits.size() - size_t(k)) + "." + digits.substr(digits.size() - size_t(k));
}

// The significant digits of a decimal text (all digits from the first that is not 0).
size_t significant_digits(const std::string &s)
{
    size_t n    = 0;
    bool   seen = false;
    for (char c : s)
        if (c >= '0' && c <= '9') {
            seen = seen || c != '0';
            if (seen)
                ++n;
        }
    return n;
}

// A positive decimal integer minus 1.
std::string minus_one(std::string s)
{
    size_t i = s.size();
    while (i > 0 && s[i - 1] == '0')
        s[--i] = '9';
    if (i > 0)
        --s[i - 1];
    if (s.size() > 1 && s[0] == '0')
        s.erase(0, 1);
    return s;
}

// `s` with zeros after it up to `n` significant digits.
std::string pad_to(std::string s, size_t n)
{
    const size_t have = significant_digits(s);
    if (have < n)
        s += std::string(n - have, '0');
    return s;
}

struct Group
{
    std::string              label;
    std::vector<std::string> texts;
};

std::vector<Group> number_groups()
{
    std::vector<Group> g;
    g.push_back({"plain", {"0", "1", "5", "42", "-7", "123456789", "0.5", "1.5", "-2.25", "3.14159", "100", "65535", "1000000", "0.1", "0.2", "0.3", "2.5", "1e0"}});
    g.push_back({"white space",
                 {" 5", "5 ", "  5  ", "\t5", "5\t", "\n5\n", "\r\n5\r\n", "\v5", "5\f", " \t\n\v\f\r5", " 0.5 ", "5 \xC2\xA0", "\xC2\xA0"
                                                                                                                           "5"}});
    g.push_back({"signs", {"+5", "-5", "+0", "-0", "+-5", "-+5", "--5", "++5", "- 5", "+ 5", "+", "-", "+.5", "-.5", "-0.0", "+0x10", "-0x10"}});
    g.push_back({"leading zeros",
                 {"007", "-007", "00", "000", "00.5", "0.0005", "0000000000000000000000000000000000000001", "-0000000000000000000000000000000000000001", "0005e0", "0e0",
                  "+007"}});
    g.push_back({"points", {".5", "5.", ".", "-.", "+.", "5.5.5", "..5", "5..", "0.", ".0", "5.e1", ".e1", "1.5."}});
    g.push_back({"exponents",
                 {"1e5", "1E5", "1e+5", "1e-5", "1e", "1e+", "1e-", "e5", ".e5", "1.e5", ".5e1", "1e05", "1e0000000000000000000000000005", "1ee5", "1e5e5", "1d5", "1E-0",
                  "5e", "1e5x", "1e 5", "1e+-5", "2e-1", "1p5"}});
    g.push_back({"hexadecimal",
                 {"0x10", "0X10", "0x", "0x1p-1", "0x1P+3", "0x.8p1", "0x1.8p1", "0xAbC", "0x1p", "0xg", "0x1e", "x10", "0x0", "0x00000000000001",
                  "0x1.fffffffffffffp1023", "0x1p-1074", "0x1p-1075", "0x1p1024", "0x1p-149", "0x1p-150", "0x1p128", "0x1.fffffep127", "0x 1", "0x-1", "0x1p+",
                  "0xp1", "0x1e5", "0x.p1", "0x1.p0", "0x10x"}});
    g.push_back({"infinity and nan", {"inf", "INF", "-inf", "+inf", "infinity", "Infinity", "nan", "NaN", "-nan", "nan(1)", "1.#INF", "1.#QNAN", "1.#IND"}});
    g.push_back({"not numbers",
                 {"", " ", "x", "5x", "5 x", "x5", "5,5", "0,5", "1,000", "5;", "five", "\xC3\xA9"
                                                                                        "5",
                  "5\xC3\xA9", "0.5x", "5%", "$5", lit("5\0"), lit("\0"
                                                                   "5"),
                  "true", "1/2", "\xD9\xA1"}});
    g.push_back({"two numbers", {"1 2", "1.5 2.5", "1e5 1", "1\t2", "1\n2", "0.5 0.6", "1 2 3", "-1 -1"}});
    g.push_back({"int's limits",
                 {"2147483647", "2147483648", "-2147483648", "-2147483649", "0002147483647", "00002147483648", "-0002147483648", "-00002147483649", "4294967295",
                  "4294967296", "99999999999999999999", "9223372036854775807", "9223372036854775808", "-4294967296"}});
    g.push_back({"size_t's limits",
                 {"18446744073709551615", "18446744073709551616", "-1", "-0", "-18446744073709551615", "-18446744073709551616", "000018446744073709551615",
                  "0000018446744073709551616", "36893488147419103231", "18446744073709551619", "99999999999999999999", "100000000000000000000", "+18446744073709551615",
                  "-9223372036854775808"}});
    g.push_back({"float's limits",
                 {"3.4028235e38", "3.4028236e38", "3.40282357e38", "3.4028237e38", "3.5e38", "-3.4028235e38", "1.17549435e-38", "1.1754942e-38", "1e-38", "1e-40",
                  "1.4e-45", "1.401298464324817e-45", "7e-46", "7.1e-46", "1e-45", "1e-46", "1e39", "-1e39", "1e38",
                  "340282356779733661637539395458142568448", "340282366920938463463374607431768211456"}});
    {
        Group d{"double's limits",
                {"1.7976931348623157e308", "1.7976931348623158e308", "1.7976931348623159e308", "-1.7976931348623157e308", "1.8e308", "2.2250738585072014e-308",
                 "2.2250738585072011e-308", "1e-310", "4.9e-324", "5e-324", "4.9406564584124654e-324", "2.4703282292062327e-324", "2.4703282292062328e-324",
                 "2.5e-324", "1e-324", "1e-400", "1e308", "1e309", "1e999", "1e-999", "1e1100", "1e-1100", "1e2147483648", "1e99999999999999999999",
                 "1e-99999999999999999999", "0e99999999999999999999", "-1e-400"}};
        // The top limit written out in full (309 digits): DBL_MAX = (2^53 - 1) * 2^971; the
        // rounding boundary past it, 2^1024 - 2^970 = (2^54 - 1) * 2^970 (ties to even, so it
        // overflows); and that boundary minus 1 (rounds down to DBL_MAX). Each also negative.
        const std::string dbl_max  = exact_binary((1ull << 53) - 1, -971);
        const std::string boundary = exact_binary((1ull << 54) - 1, -970);
        const std::string below    = minus_one(boundary);
        for (const std::string &s : {dbl_max, boundary, below})
            d.texts.push_back(s);
        for (const std::string &s : {dbl_max, boundary, below})
            d.texts.push_back("-" + s);
        g.push_back(d);
    }
    {
        // Fields longer than num_get keeps: 31 characters of an integer, 768 significant digits
        // of a floating-point number (the rest only scale it, and a dropped digit that is not 0
        // raises a last kept 0 or 5 by one), an exponent of more digits than a ptrdiff_t holds.
        std::vector<std::string> t;
        std::string              ints, ten = "1234567890";
        for (int i = 0; i < 30; ++i)
            ints += ten;
        t.push_back("1." + std::string(800, '0'));
        t.push_back(ints);                                        // 300 digits
        t.push_back(ints + ints + ints.substr(0, 170));           // 770 digits
        t.push_back("0." + std::string(300, '0') + "1");
        t.push_back("0." + std::string(780, '0') + "1");
        t.push_back("0." + std::string(1200, '0') + "1e1200");
        t.push_back(std::string(100, '0') + "5");
        t.push_back("-" + std::string(50, '0') + "5");
        t.push_back("1" + std::string(40, '0'));
        t.push_back("1" + std::string(900, '0') + "e-900");
        t.push_back("0.1" + std::string(900, '0') + "1");
        t.push_back("1e" + std::string(30, '9'));
        // 1 + 2^-53, half way between 1 and the next double: ties to even, and with a digit past
        // the 768th kept, up.
        const std::string half1 = exact_binary((1ull << 53) + 1, 53);
        t.push_back(half1);
        t.push_back(pad_to(half1, 768) + "1");
        t.push_back(pad_to(half1, 768) + "0");
        t.push_back(pad_to(half1, 900));
        // 1 + 2^-24 for float.
        const std::string halff = exact_binary((1ull << 24) + 1, 24);
        t.push_back(halff);
        t.push_back(pad_to(halff, 768) + "1");
        // 2^-1075, half way between 0 and the least denormal: ties to 0 (a range error), and with a
        // digit past the 768th, up.
        const std::string halfmin = exact_binary(1, 1075);
        t.push_back(halfmin);
        t.push_back(pad_to(halfmin, 768) + "1");
        // (2^53 - 3) * 2^-1075, half way between two denormals, has exactly 768 significant
        // digits ending in 5: ties to even (down); a 769th digit 1 raises the 5 to 6 (up); a
        // 769th digit 0 does not.
        const std::string half768 = exact_binary((1ull << 53) - 3, 1075);
        t.push_back(half768);
        t.push_back(half768 + "1");
        t.push_back(half768 + "0");
        t.push_back("-" + half768 + "1");
        g.push_back({"long fields", t});
    }
    return g;
}

// A text as the value of an attribute: &, ", < and NUL written as references, the rest as it is.
std::string attr_text(const std::string &s)
{
    std::string out;
    for (char c : s) {
        if (c == '&')
            out += "&amp;";
        else if (c == '"')
            out += "&quot;";
        else if (c == '<')
            out += "&lt;";
        else if (c == '\0')
            out += "&#0;";
        else
            out.push_back(c);
    }
    return out;
}

// <n> with a <v x="..."/> for each text, and each asked as int, size_t, float and double.
void put_numbers(Sets &sets, const std::string &label, const std::vector<std::string> &texts, bool committed)
{
    std::string        doc = "<n>\n";
    std::vector<Query> qs;
    for (size_t i = 0; i < texts.size(); ++i) {
        doc += " <v x=\"" + attr_text(texts[i]) + "\"/>\n";
        for (uint8_t k : {GET_INT, GET_SIZE, GET_FLOAT, GET_DOUBLE})
            qs.push_back(q({0, uint64_t(i)}, k, "<xmlattr>.x"));
    }
    doc += "</n>\n";
    add(sets, "PTGT", ptgt("numbers: " + label, doc, qs), committed);
}

void record_numbers(Sets &sets)
{
    for (const Group &g : number_groups())
        put_numbers(sets, g.label, g.texts, true);

    // As an element's text, as the height ranges' options hold theirs.
    {
        const std::string  doc = "<n><v>5</v><v> 0.1 </v><v>\n\t2.5e1\n</v><v></v><v>&lt;1</v><v>1<!--c-->2</v><v><![CDATA[3]]></v><v>4<w/></v></n>";
        std::vector<Query> qs;
        for (uint64_t i = 0; i < 8; ++i)
            for (uint8_t k : {GET_INT, GET_SIZE, GET_FLOAT, GET_DOUBLE, GET_STRING})
                qs.push_back(q({0, i}, k, ""));
        add(sets, "PTGT", ptgt("numbers: an element's text", doc, qs), true);
    }
    // Defaults: a number, a text no type takes, a missing attribute.
    {
        const std::string  doc = "<n><v x=\"5\"/><v x=\"x\"/><v/><v x=\"2147483648\"/><v x=\"1e39\"/></n>";
        std::vector<Query> qs;
        for (uint64_t i = 0; i < 5; ++i) {
            qs.push_back(q_int({0, i}, "<xmlattr>.x", -1));
            qs.push_back(q_size({0, i}, "<xmlattr>.x", 18446744073709551615ull));
            qs.push_back(q_float({0, i}, "<xmlattr>.x", -0.5f));
            qs.push_back(q_double({0, i}, "<xmlattr>.x", 1e300));
            qs.push_back(q_string({0, i}, "<xmlattr>.x", "none"));
        }
        add(sets, "PTGT", ptgt("numbers: defaults when the node is missing and when the conversion fails", doc, qs), true);
    }
}

// ---- (f) The 3MF reader ----------------------------------------------------------------------

// The fixture 3mf-2-numbers' two files (parity/oracle/probe/fixtures/model/3mf-2-numbers/model.3mf/
// Metadata/), as they are.
const char *const NUMBERS_CUT = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<objects>\n <object id=\"1\">\n"
                                "  <cut_id id=\" 5\" check_sum=\"+1\" connectors_cnt=\"2 \"/>\n  <connectors>\n"
                                "   <connector volume_id=\" 0\" type=\"+1\" r_tolerance=\".5\" h_tolerance=\"1e-1\"/>\n"
                                "   <connector volume_id=\"+3\" type=\"0\" r_tolerance=\"0\" h_tolerance=\"0\"/>\n  </connectors>\n"
                                " </object>\n <object id=\"0\"/>\n <object id=\"1\"/>\n</objects>\n";
const char *const NUMBERS_RANGES = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<objects>\n <object id=\" 1 \">\n"
                                   "  <range min_z=\" 0.5\" max_z=\"1 \">\n   <option opt_key=\"layer_height\">0.1</option>\n  </range>\n"
                                   "  <range min_z=\"+1\" max_z=\"1.5e0\">\n   <option opt_key=\"layer_height\">0.15</option>\n  </range>\n"
                                   "  <range min_z=\".2e1\" max_z=\"2.5E+0\">\n   <option opt_key=\"layer_height\">0.2</option>\n  </range>\n"
                                   "  <range min_z=\"-0\" max_z=\"0003.\">\n   <option opt_key=\"layer_height\">0.25</option>\n  </range>\n"
                                   " </object>\n <object id=\"x\">\n  <range min_z=\"5\" max_z=\"6\"/>\n </object>\n"
                                   " <object id=\"+1\">\n  <range min_z=\"7\" max_z=\"8\"/>\n </object>\n</objects>\n";

// The other fixtures whose cut information or height ranges read_xml reads, as they are.
const char *const XML_DECL = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";

// The reader's calls on the cut information (bbs_3mf.cpp:2571-2610), made on every node it could
// reach: get_child("objects") and its children; each child's id with -1 for a default and its
// children; a cut_id's three size_t; a connectors' children and each connector's two ints and
// two floats. A call that throws does not stop the rest.
std::vector<Query> walk_cut(const pt::ptree &tree)
{
    std::vector<Query> qs;
    qs.push_back(q({}, GET_CHILD, "objects"));
    const auto objects = tree.get_child_optional("objects");
    if (!objects)
        return qs;
    std::vector<uint64_t> at;
    locate(tree, &*objects, at);
    qs.push_back(q(at, CHILDREN));
    uint64_t i = 0;
    for (const auto &o : *objects) {
        std::vector<uint64_t> oat = at;
        oat.push_back(i++);
        qs.push_back(q_int(oat, "<xmlattr>.id", -1));
        qs.push_back(q(oat, CHILDREN));
        uint64_t j = 0;
        for (const auto &c : o.second) {
            std::vector<uint64_t> cat = oat;
            cat.push_back(j++);
            if (c.first == "cut_id")
                for (const char *a : {"<xmlattr>.id", "<xmlattr>.check_sum", "<xmlattr>.connectors_cnt"})
                    qs.push_back(q(cat, GET_SIZE, a));
            if (c.first == "connectors") {
                qs.push_back(q(cat, CHILDREN));
                uint64_t k = 0;
                for (const auto &n : c.second) {
                    std::vector<uint64_t> nat = cat;
                    nat.push_back(k++);
                    if (n.first != "connector")
                        continue;
                    qs.push_back(q(nat, GET_INT, "<xmlattr>.volume_id"));
                    qs.push_back(q(nat, GET_INT, "<xmlattr>.type"));
                    qs.push_back(q(nat, GET_FLOAT, "<xmlattr>.r_tolerance"));
                    qs.push_back(q(nat, GET_FLOAT, "<xmlattr>.h_tolerance"));
                }
            }
        }
    }
    return qs;
}

// The reader's calls on the height ranges (bbs_3mf.cpp:2899-2937), made on every node it could
// reach: get_child("objects") and its children; each child's id with -1 for a default and its
// children; a range's two doubles and its children; an option's opt_key and data().
std::vector<Query> walk_ranges(const pt::ptree &tree)
{
    std::vector<Query> qs;
    qs.push_back(q({}, GET_CHILD, "objects"));
    const auto objects = tree.get_child_optional("objects");
    if (!objects)
        return qs;
    std::vector<uint64_t> at;
    locate(tree, &*objects, at);
    qs.push_back(q(at, CHILDREN));
    uint64_t i = 0;
    for (const auto &o : *objects) {
        std::vector<uint64_t> oat = at;
        oat.push_back(i++);
        qs.push_back(q_int(oat, "<xmlattr>.id", -1));
        qs.push_back(q(oat, CHILDREN));
        uint64_t j = 0;
        for (const auto &r : o.second) {
            std::vector<uint64_t> rat = oat;
            rat.push_back(j++);
            if (r.first != "range")
                continue;
            qs.push_back(q(rat, GET_DOUBLE, "<xmlattr>.min_z"));
            qs.push_back(q(rat, GET_DOUBLE, "<xmlattr>.max_z"));
            qs.push_back(q(rat, CHILDREN));
            uint64_t k = 0;
            for (const auto &n : r.second) {
                std::vector<uint64_t> nat = rat;
                nat.push_back(k++);
                if (n.first != "option")
                    continue;
                qs.push_back(q(nat, GET_STRING, "<xmlattr>.opt_key"));
                qs.push_back(q(nat, DATA));
            }
        }
    }
    return qs;
}

// The 25 variants of 3mf-2-numbers (model.cpp, number_variants), in its order: each changes one
// number of one of the two files.
struct Variant
{
    bool        cut; // the cut information; else the height ranges
    const char *from, *to;
};
const Variant VARIANTS[] = {
    {false, "min_z=\" 0.5\"", "min_z=\"0x1p-1\""},
    {false, "min_z=\" 0.5\"", "min_z=\"inf\""},
    {false, "min_z=\" 0.5\"", "min_z=\"nan\""},
    {false, "min_z=\" 0.5\"", "min_z=\"1e999\""},
    {false, "min_z=\" 0.5\"", "min_z=\"1e-999\""},
    {false, "min_z=\" 0.5\"", "min_z=\"4.9e-324\""},
    {false, "min_z=\" 0.5\"", "min_z=\"\""},
    {false, "min_z=\" 0.5\"", "min_z=\"  \""},
    {false, "min_z=\" 0.5\"", "min_z=\"0,5\""},
    {false, "min_z=\" 0.5\"", "min_z=\"0.5x\""},
    {false, "min_z=\" 0.5\"", "min_z=\"- 0.5\""},
    {false, "min_z=\" 0.5\"", "min_z=\"--0.5\""},
    {false, "min_z=\" 0.5\"", "min_z=\"5e\""},
    {false, "min_z=\" 0.5\"", "min_z=\".\""},
    {false, "min_z=\" 0.5\"", "min_z=\"+.5\""},
    {false, "min_z=\" 0.5\"", "min_z=\"0.5 0.6\""},
    {false, "min_z=\" 0.5\"", "min_z=\"&#9;0.5&#10;\""},
    {false, "id=\" 1 \"", "id=\"2147483648\""},
    {false, "id=\" 1 \"", "id=\"01\""},
    {true, "check_sum=\"+1\"", "check_sum=\"-1\""},
    {true, "check_sum=\"+1\"", "check_sum=\"18446744073709551616\""},
    {true, "check_sum=\"+1\"", "check_sum=\"0x10\""},
    {true, "volume_id=\" 0\"", "volume_id=\"2147483648\""},
    {true, "r_tolerance=\".5\"", "r_tolerance=\"1e39\""},
    {true, "r_tolerance=\".5\"", "r_tolerance=\"1e-46\""},
};

void record_reader(Sets &sets)
{
    const std::string x = XML_DECL;
    add(sets, "PTGT", ptgt("3mf: 3mf-2-numbers' cut information", NUMBERS_CUT, {}, walk_cut), true);
    add(sets, "PTGT", ptgt("3mf: 3mf-2-numbers' height ranges", NUMBERS_RANGES, {}, walk_ranges), true);
    for (const Variant &v : VARIANTS) {
        std::string  doc = v.cut ? NUMBERS_CUT : NUMBERS_RANGES;
        const size_t at  = doc.find(v.from);
        if (at == std::string::npos || doc.find(v.from, at + 1) != std::string::npos)
            throw std::runtime_error(std::string("3mf-2-numbers has no single ") + v.from);
        doc.replace(at, std::strlen(v.from), v.to);
        add(sets, "PTGT", ptgt(std::string("3mf: 3mf-2-numbers with ") + v.to, doc, {}, v.cut ? walk_cut : walk_ranges), true);
    }
    // The other fixtures read_xml reads (cut information, then height ranges).
    add(sets, "PTGT",
        ptgt("3mf: 3mf-3-bambu's cut information",
             x + "<objects>\n <object id=\"1\">\n  <cut_id id=\"5\" check_sum=\"1\" connectors_cnt=\"0\"/>\n </object>\n</objects>\n", {}, walk_cut),
        true);
    add(sets, "PTGT",
        ptgt("3mf: 3mf-2-cut-bad-number's cut information",
             x + "<objects>\n <object id=\"1\">\n  <cut_id id=\"x5\" check_sum=\"1\" connectors_cnt=\"0\"/>\n </object>\n</objects>\n", {}, walk_cut),
        true);
    add(sets, "PTGT",
        ptgt("3mf: 3mf-2-cut-missing-attr's cut information",
             x + "<objects>\n <object id=\"1\">\n  <cut_id id=\"5\" connectors_cnt=\"0\"/>\n </object>\n</objects>\n", {}, walk_cut),
        true);
    add(sets, "PTGT",
        ptgt("3mf: 3mf-3-bambu's height ranges",
             x + "<objects>\n <object id=\"1\">\n  <range min_z=\"2\" max_z=\"4\">\n   <option opt_key=\"extruder\">2</option>\n"
                 "   <option opt_key=\"layer_height\">0.1</option>\n  </range>\n </object>\n</objects>\n",
             {}, walk_ranges),
        true);
    add(sets, "PTGT",
        ptgt("3mf: 3mf-2-ranges-bad-xml's height ranges",
             x + "<objects>\n <object id=\"1\">\n  <range min_z=\"2\" max_z=\"4\">\n   <option opt_key=\"layer_height\">0.1</option>\n"
                 " </object>\n</objects>\n",
             {}, walk_ranges),
        true);
    add(sets, "PTGT",
        ptgt("3mf: 3mf-2-ranges-bad-z's height ranges",
             x + "<objects>\n <object id=\"1\">\n  <range min_z=\"low\" max_z=\"4\">\n   <option opt_key=\"layer_height\">0.1</option>\n"
                 "  </range>\n </object>\n</objects>\n",
             {}, walk_ranges),
        true);
    add(sets, "PTGT", ptgt("3mf: 3mf-2-ranges-no-objects' height ranges", x + "<ranges>\n <object id=\"1\"/>\n</ranges>\n", {}, walk_ranges), true);
    add(sets, "PTGT",
        ptgt("3mf: 3mf-2-ranges-bad-option's height ranges",
             x + "<objects>\n <object id=\"1\">\n  <range min_z=\"2\" max_z=\"4\">\n   <option opt_key=\"layer_height\">thin</option>\n"
                 "  </range>\n </object>\n</objects>\n",
             {}, walk_ranges),
        true);
    // What the reader meets in a file it did not write: comments and attributes among the
    // objects, an object of another name, options with no opt_key.
    add(sets, "PTGT",
        ptgt("3mf: cut information with comments, attributes and other elements",
             "<objects n=\"2\"><!--c--><object id=\"3\"><cut_id id=\"1\" check_sum=\"2\" connectors_cnt=\"1\"/><connectors x=\"y\"><!--k-->"
             "<connector volume_id=\"1\" type=\"2\" r_tolerance=\"0.5\" h_tolerance=\"0.25\"/><plug/></connectors><cut_id/></object>"
             "<other id=\"4\"/>text</objects>",
             {}, walk_cut),
        true);
    add(sets, "PTGT",
        ptgt("3mf: height ranges with comments, attributes and options without a key",
             "<objects n=\"2\"><!--c--><object id=\"2\"><range min_z=\"1\" max_z=\"2\" k=\"v\"><option>0.1</option><option opt_key=\"\"/>"
             "<!--o--><option opt_key=\"a\" opt_key=\"b\">x<![CDATA[y]]></option></range><range/></object></objects>",
             {}, walk_ranges),
        true);
}

// ---- The full set only -----------------------------------------------------------------------

// `s` with one to three seeded changes: a byte of XML's own put in, taken out or written over, or a
// piece doubled.
std::string mutate(SplitMix &g, std::string s)
{
    static const std::string alphabet = lit("<>/=\"'&;#x!?-[]\n \t\r\0aZ9.\xC3\xA9");
    const int                n        = 1 + int(g.below(3));
    for (int k = 0; k < n; ++k) {
        const size_t at = size_t(g.below(s.size() + 1));
        switch (g.below(4)) {
        case 0: s.insert(at, 1, alphabet[size_t(g.below(alphabet.size()))]); break;
        case 1:
            if (at < s.size())
                s.erase(at, size_t(1 + g.below(3)));
            break;
        case 2:
            if (at < s.size())
                s[at] = alphabet[size_t(g.below(alphabet.size()))];
            break;
        default:
            if (at < s.size())
                s.insert(at, s.substr(at, size_t(1 + g.below(12))));
            break;
        }
    }
    return s;
}

// A seeded text from the pieces numbers are made of.
std::string gen_number(SplitMix &g)
{
    static const char *const parts[] = {"0",  "1",  "5", "9", "00", "12345", "-", "+",   ".",   "e",    "E",          "e-",         "e+",   "x",
                                        "0x", "p",  "p-", " ", "\t", "a",    "f", "inf", "nan", "1e308", "2147483647", "4294967296", "18446744073709551615",
                                        "1e-45", "7", "0.", ".5", "3.4028235e38", "2.2250738585072014e-308", ","};
    const size_t             n       = sizeof(parts) / sizeof(parts[0]);
    std::string              s;
    const int                k = 1 + int(g.below(6));
    for (int i = 0; i < k; ++i)
        s += parts[size_t(g.below(n))];
    return s;
}

// Cut information and height ranges like the reader's, with seeded numbers.
std::string gen_cut(SplitMix &g, int objects)
{
    std::string s = std::string(XML_DECL) + "<objects>\n";
    for (int i = 0; i < objects; ++i) {
        s += " <object id=\"" + std::to_string(int(g.below(40)) - 2) + "\">\n";
        s += "  <cut_id id=\"" + std::to_string(g.below(1000)) + "\" check_sum=\"" + std::to_string(g.next()) + "\" connectors_cnt=\"" +
             std::to_string(g.below(4)) + "\"/>\n  <connectors>\n";
        const int c = int(g.below(4));
        for (int k = 0; k < c; ++k) {
            char r[64], h[64];
            std::snprintf(r, sizeof r, "%.6g", double(g.below(100000)) / 997.0);
            std::snprintf(h, sizeof h, "%.9e", double(g.below(100000)) / 31.0);
            s += "   <connector volume_id=\"" + std::to_string(g.below(8)) + "\" type=\"" + std::to_string(g.below(3)) + "\" r_tolerance=\"" + r +
                 "\" h_tolerance=\"" + h + "\"/>\n";
        }
        s += "  </connectors>\n </object>\n";
    }
    return s + "</objects>\n";
}

std::string gen_ranges(SplitMix &g, int objects)
{
    std::string s = std::string(XML_DECL) + "<objects>\n";
    for (int i = 0; i < objects; ++i) {
        s += " <object id=\"" + std::to_string(int(g.below(40)) - 2) + "\">\n";
        const int r = 1 + int(g.below(6));
        for (int k = 0; k < r; ++k) {
            char lo[64], hi[64];
            std::snprintf(lo, sizeof lo, "%.17g", double(g.below(1000000)) / 1000.0);
            std::snprintf(hi, sizeof hi, "%.3f", double(g.below(1000000)) / 999.0);
            s += "  <range min_z=\"" + std::string(lo) + "\" max_z=\"" + hi + "\">\n";
            s += "   <option opt_key=\"layer_height\">" + std::to_string(double(1 + g.below(30)) / 100.0) + "</option>\n";
            if (g.below(2) == 0)
                s += "   <option opt_key=\"wall_loops\">" + std::to_string(g.below(9)) + "</option>\n";
            s += "  </range>\n";
        }
        s += " </object>\n";
    }
    return s + "</objects>\n";
}

void record_generated(Sets &sets)
{
    // Seeded mutations of every document of the tables (PTXR).
    SplitMix               g(0x50545245ull);
    std::vector<Doc>       docs = tree_docs();
    const std::vector<Doc> errs = error_docs();
    docs.insert(docs.end(), errs.begin(), errs.end());
    for (const Doc &d : docs) {
        if (d.text.size() > 2000)
            continue;
        for (int m = 0; m < 30; ++m)
            add(sets, "PTXR", ptxr({d.label + " (mutated)", mutate(g, d.text)}), false);
    }
    // Seeded number texts, 200 to a record.
    SplitMix n(0x5054474Eull);
    for (int r = 0; r < 10; ++r) {
        std::vector<std::string> t;
        for (int i = 0; i < 200; ++i)
            t.push_back(gen_number(n));
        put_numbers(sets, "seeded " + std::to_string(r), t, false);
    }
    // Larger cut information and height ranges, as the reader walks them.
    SplitMix w(0x50544733ull);
    for (int i = 0; i < 6; ++i) {
        const int objects = 1 + int(w.below(i < 3 ? 10 : 200));
        const std::string n_objects = " (" + std::to_string(objects) + " objects)";
        add(sets, "PTGT", ptgt("3mf: generated cut information" + n_objects, gen_cut(w, objects), {}, walk_cut), false);
        add(sets, "PTGT", ptgt("3mf: generated height ranges" + n_objects, gen_ranges(w, objects), {}, walk_ranges), false);
    }
}

bool run_ptree(const Options &options, std::string &error)
{
    Sets sets;
    head(sets, "ptree");
    try {
        for (const Doc &d : tree_docs())
            add(sets, "PTXR", ptxr(d), true);
        for (const Doc &d : error_docs())
            add(sets, "PTXR", ptxr(d), true);
        record_paths(sets);
        record_numbers(sets);
        record_reader(sets);
        record_generated(sets);
    } catch (const std::exception &e) {
        error = e.what();
        return false;
    }
    return save(sets, options, "ptree.sop1", error);
}

} // namespace

const Target &target()
{
    static const Target t{"ptree", false, run_ptree};
    return t;
}

} // namespace probe
