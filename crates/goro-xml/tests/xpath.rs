//! XPath 1.0 behaviour (expected values observed on PHP 8.5 with
//! DOMXPath::evaluate on the same documents; see
//! tests/fixtures/xpath/probe.php).

use goro_xml::tree::{Arena, DocKind, NodeData, NodeId, NsDecl, QName};
use goro_xml::xpath::{self, Context, FunctionHandler, Value, XPathError, XPathErrorKind};

/// `<root xmlns:a="urn:a"><x n="1">1</x><x n="2">2.5</x><y>abc</y><a:z/></root>`
fn doc1(a: &mut Arena) -> NodeId {
    let d = a.new_document(DocKind::Xml);
    let root = a.new_element(d, QName::local("root"));
    a.element_mut(root).unwrap().ns_decls.push(NsDecl {
        prefix: Some("a".into()),
        uri: "urn:a".into(),
    });
    a.raw_append(d, root);
    for (n, t) in [("1", "1"), ("2", "2.5")] {
        let x = a.new_element(d, QName::local("x"));
        a.set_attribute(x, "n", n);
        let tx = a.new_text(d, t);
        a.raw_append(x, tx);
        a.raw_append(root, x);
    }
    let y = a.new_element(d, QName::local("y"));
    let tx = a.new_text(d, "abc");
    a.raw_append(y, tx);
    a.raw_append(root, y);
    let z = a.new_element(
        d,
        QName {
            prefix: Some("a".into()),
            local: "z".into(),
            ns: Some("urn:a".into()),
        },
    );
    a.raw_append(root, z);
    d
}

fn eval(a: &mut Arena, ctx: NodeId, e: &str) -> Result<Value, XPathError> {
    let x = xpath::compile(e)?;
    let mut c = Context::new();
    xpath::evaluate(a, &x, ctx, &mut c)
}

fn s(a: &mut Arena, d: NodeId, e: &str) -> String {
    let v = eval(a, d, e).unwrap_or_else(|err| panic!("{e}: {err}"));
    match v {
        Value::NodeSet(n) => format!("nodes({})", n.len()),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => format!("num:{}", xpath::number_to_string(n)),
        Value::String(s) => format!("'{s}'"),
    }
}

#[test]
fn php85_values() {
    let mut a = Arena::new();
    let d = doc1(&mut a);
    let cases: &[(&str, &str)] = &[
        ("string(1 div 3)", "'0.333333333333333'"),
        ("string(1 div 0)", "'Infinity'"),
        ("string(-1 div 0)", "'-Infinity'"),
        ("string(0 div 0)", "'NaN'"),
        ("string(0 * -1)", "'0'"),
        ("string(0.1 + 0.2)", "'0.3'"),
        ("string(1e21)", "'1e+21'"),
        (
            "string(123456789012345678901234567890)",
            "'1.23456789012346e+29'",
        ),
        ("string(0.000001)", "'1e-06'"),
        ("string(2.5 * 4)", "'10'"),
        ("number('  12  ')", "num:12"),
        ("number('1e3')", "num:1000"),
        ("number('+5')", "num:NaN"),
        ("round(2.5)", "num:3"),
        ("round(-2.5)", "num:-2"),
        ("string(round(-0.4))", "'0'"),
        ("floor(-1.5)", "num:-2"),
        ("substring('12345', 1.5, 2.6)", "'234'"),
        ("substring('12345', 0, 3)", "'12'"),
        ("substring('12345', 0 div 0, 3)", "''"),
        ("substring('12345', 1, 0 div 0)", "''"),
        ("substring('12345', -42, 1 div 0)", "'12345'"),
        ("substring('12345', -1 div 0, 1 div 0)", "''"),
        ("substring-before('1999/04/01','/')", "'1999'"),
        ("substring-after('1999/04/01','19')", "'99/04/01'"),
        ("translate('bar','abc','ABC')", "'BAr'"),
        ("translate('--aaa--','abc-','ABC')", "'AAA'"),
        ("normalize-space('  a  b   c ')", "'a b c'"),
        ("concat('a', 1, true())", "'a1true'"),
        ("string-length('h\u{e9}llo')", "num:5"),
        ("count(//x)", "num:2"),
        ("sum(//x)", "num:3.5"),
        ("sum(//y)", "num:NaN"),
        ("string(//x)", "'1'"),
        ("string(//x/@n)", "'1'"),
        ("boolean('0')", "true"),
        ("'1' = 1", "true"),
        ("//x = 1", "true"),
        ("//x = '2.5'", "true"),
        ("//x != 1", "true"),
        ("//x < //x", "true"),
        ("//x > 2", "true"),
        ("local-name(//a:z)", "'z'"),
        ("name(//a:z)", "'a:z'"),
        ("namespace-uri(//a:z)", "'urn:a'"),
        ("1 = 1 = 1", "true"),
        ("- - 1", "num:1"),
        ("-3 mod 2", "num:-1"),
        ("3 mod -2", "num:1"),
        ("5.5 mod 2", "num:1.5"),
        ("10 div 4 div 5", "num:0.5"),
        ("//x[1]", "nodes(1)"),
        ("//x[last()]", "nodes(1)"),
        ("//x[position() = 2]/@n", "nodes(1)"),
        ("count(//x[@n > 1])", "num:1"),
        ("count(//*)", "num:5"),
        ("count(//node())", "num:8"),
        ("count(//text())", "num:3"),
        ("count(/)", "num:1"),
        ("count(//@*)", "num:2"),
        ("count(//namespace::*)", "num:10"),
        ("count(/descendant-or-self::node())", "num:9"),
        ("count(//x/following-sibling::*)", "num:3"),
        ("count(//y/preceding::*)", "num:2"),
        ("count(//x/ancestor::*)", "num:1"),
        ("count(//x/..)", "num:1"),
        ("string(//x[2]/preceding-sibling::x)", "'1'"),
        ("name(//*[2])", "'x'"),
        ("//*[1.5]", "nodes(0)"),
        ("//*['a']", "nodes(5)"),
        ("1.", "num:1"),
        ("1e5", "num:100000"),
        ("/..", "nodes(0)"),
        ("name(//namespace::*[1])", "'xml'"),
        ("name(/*/namespace::*[2])", "'a'"),
    ];
    let mut bad = Vec::new();
    for (e, want) in cases {
        let got = s(&mut a, d, e);
        if got != *want {
            bad.push(format!("{e}: got {got}, want {want}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn php85_errors() {
    let mut a = Arena::new();
    let d = doc1(&mut a);
    let cases: &[(&str, i32, &str)] = &[
        ("//[", 1207, "Invalid expression"),
        ("1+", 1207, "Invalid expression"),
        ("-", 1207, "Invalid expression"),
        ("foo()", 1209, "Unregistered function: foo"),
        ("unknown:foo()", 1219, "Undefined namespace prefix: unknown"),
        ("x:y", 1219, "Undefined namespace prefix: x"),
        ("//@", 1207, "Invalid expression"),
        ("1 1", 1207, "Invalid expression"),
        ("\"abc", 1202, "Unfinished literal"),
        ("//x]", 1207, "Invalid expression"),
        ("()", 1207, "Invalid expression"),
        ("$var", 1205, "Undefined variable: var"),
        ("count()", 1212, "Invalid number of arguments"),
        ("substring('a')", 1212, "Invalid number of arguments"),
        ("foo::x", 1207, "Invalid expression"),
        ("processing-instruction(1)", 1203, "Start of literal"),
        ("node(1)", 1208, "Missing closing curly brace"),
        ("..[1]", 1207, "Invalid expression"),
        ("..5", 1207, "Invalid expression"),
        ("1 e5", 1207, "Invalid expression"),
        ("//x | 1", 1211, "Invalid type"),
        ("count(1)", 1211, "Invalid type"),
        ("sum('a')", 1211, "Invalid type"),
        ("name(1)", 1211, "Invalid type"),
        ("'a' | 'b'", 1211, "Invalid type"),
        ("!x", 1207, "Invalid expression"),
        ("mod 1", 1207, "Invalid expression"),
    ];
    for (e, code, msg) in cases {
        match eval(&mut a, d, e) {
            Ok(v) => panic!("{e}: expected an error, got {v:?}"),
            Err(err) => {
                assert_eq!((err.code(), err.message.as_str()), (*code, *msg), "{e}");
            }
        }
    }
}

struct Php;

impl FunctionHandler for Php {
    fn has_function(&self, ns: &str, local: &str) -> bool {
        ns == "http://php.net/xpath" && matches!(local, "function" | "functionString")
    }
    fn call(
        &mut self,
        arena: &mut Arena,
        _ns: &str,
        local: &str,
        args: Vec<Value>,
    ) -> Result<Value, XPathError> {
        let name = args[0].to_string_value(arena);
        if name == "fail" {
            return Err(XPathError::callback("No callbacks were registered", None));
        }
        assert_eq!(name, "strrev");
        let s = if local == "functionString" {
            args[1].to_string_value(arena)
        } else {
            match &args[1] {
                Value::NodeSet(_) => args[1].to_string_value(arena),
                v => v.to_string_value(arena),
            }
        };
        Ok(Value::String(s.chars().rev().collect()))
    }
}

#[test]
fn extension_functions() {
    // DOMXPath_evaluate_node_set_to_string.phpt
    let mut a = Arena::new();
    let d = doc1(&mut a);
    let mut php = Php;
    let mut c = Context::new();
    c.register_namespace("php", "http://php.net/xpath");
    c.functions = Some(&mut php);
    let e = xpath::compile("php:functionString('strrev', //y)").unwrap();
    assert_eq!(
        xpath::evaluate(&mut a, &e, d, &mut c).unwrap(),
        Value::String("cba".into())
    );
    let e = xpath::compile("php:functionString('strrev', //namespace::*)").unwrap();
    assert_eq!(
        xpath::evaluate(&mut a, &e, d, &mut c).unwrap(),
        Value::String("ecapseman/8991/LMX/gro.3w.www//:ptth".into())
    );
    // Namespace nodes are reported for freeing.
    assert!(!c.namespace_nodes.is_empty());
    for &n in &c.namespace_nodes {
        assert!(matches!(a.data(n), NodeData::Namespace(_)));
    }
    let e = xpath::compile("php:function('fail', 1)").unwrap();
    let err = xpath::evaluate(&mut a, &e, d, &mut c).unwrap_err();
    assert_eq!(err.kind, XPathErrorKind::Callback);
    assert_eq!(err.message, "No callbacks were registered");
    let e = xpath::compile("php:other(1)").unwrap();
    let err = xpath::evaluate(&mut a, &e, d, &mut c).unwrap_err();
    assert_eq!(err.code(), 1209);
}

#[test]
fn prefixes_in_scope() {
    // modern/xml/xpath_query_context_in_scope_ns.phpt-like: registerNodeNS.
    let mut a = Arena::new();
    let d = doc1(&mut a);
    assert_eq!(s(&mut a, d, "count(//a:z)"), "num:1");
    let e = xpath::compile("count(//a:z)").unwrap();
    let mut c = Context::new();
    c.register_node_ns = false;
    let err = xpath::evaluate(&mut a, &e, d, &mut c).unwrap_err();
    assert_eq!(err.message, "Undefined namespace prefix: a");
    let mut c = Context::new();
    c.register_node_ns = false;
    c.register_namespace("b", "urn:a");
    let e = xpath::compile("count(//b:z) + count(//b:*)").unwrap();
    assert_eq!(
        xpath::evaluate(&mut a, &e, d, &mut c).unwrap(),
        Value::Number(2.0)
    );
}

#[test]
fn html_namespace() {
    // HTMLDocument_fromString_DOM_HTML_NO_DEFAULT_NS.phpt
    let mut a = Arena::new();
    let r = goro_xml::html5::parse_document(&mut a, b"<p>x</p>", &Default::default());
    assert_eq!(s(&mut a, r.doc, "count(//p)"), "num:0");
    assert_eq!(s(&mut a, r.doc, "count(//*[name()='p'])"), "num:1");
    let e = xpath::compile("count(//x:p)").unwrap();
    let mut c = Context::new();
    c.register_namespace("x", "http://www.w3.org/1999/xhtml");
    assert_eq!(
        xpath::evaluate(&mut a, &e, r.doc, &mut c).unwrap(),
        Value::Number(1.0)
    );
    // Modern documents have only the xml namespace node per element.
    assert_eq!(s(&mut a, r.doc, "count(//namespace::*)"), "num:4");
}
