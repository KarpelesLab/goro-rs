//! Doc comments and Reflection start/end lines, as PHP 8.5 reports them
//! through getDocComment() / getStartLine() / getEndLine() (values
//! observed on PHP 8.5 for this source).

use goro_syntax::ast::*;
use goro_syntax::parse;

#[test]
fn doc_comments_and_lines() {
    let src = b"<?php
/** doc f */
#[A]
function
f(
)
{
}
#[A]
/** doc g */
function g() {}
/** doc h1 */
/** doc h2 */
function h() {}
/** doc i */
$x = 1;
function i() {}
/** doc C */
#[A] final
class C {
    /** doc p */
    public
    $p,
    /** doc q */
    $q;
    /** doc m */
    #[A]
    public static
    function m() {}
    public /** doc n */ function n() {}
    /** doc K */
    const K = 1, /** doc L */ L = 2;
}
function a1() { /** inner */ } function b1() {}
class E3 { /** pd */ } function k3(){}
/** a4 */ echo 1; /** b4 */ echo 2; function z4(){}
class P6 { function __construct(/** promo */ public $x, public $y) {} }
/** cl8 */ class C8 { public $p; function m() {} }
if (true) { /** if9 */ } function w9() {}
";
    let arena = goro_syntax::Arena::new();
    let p = parse(&arena, src).unwrap();
    let s = |d: Option<&[u8]>| {
        d.map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_else(|| "false".into())
    };
    let n = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let mut funcs = Vec::new();
    let mut classes = Vec::new();
    for st in p.stmts {
        match &st.kind {
            StmtKind::Function(f) => funcs.push(format!(
                "{} {}-{} {}",
                n(f.name.name),
                f.span.line,
                f.span.end_line,
                s(f.doc_comment)
            )),
            StmtKind::Class(c) => {
                classes.push(format!(
                    "{} {}-{} {}",
                    n(c.name.unwrap().name),
                    c.span.line,
                    c.span.end_line,
                    s(c.doc_comment)
                ));
                for m in c.members {
                    match m {
                        ClassMember::Method(f) => {
                            classes.push(format!(
                                "  {} {}-{} {}",
                                n(f.name.name),
                                f.span.line,
                                f.span.end_line,
                                s(f.doc_comment)
                            ));
                            for prm in f.params {
                                classes.push(format!(
                                    "  param {} {}",
                                    n(prm.name.name),
                                    s(prm.doc_comment)
                                ));
                            }
                        }
                        ClassMember::Property(pd) => {
                            for it in pd.props {
                                classes.push(format!(
                                    "  ${} {}",
                                    n(it.name.name),
                                    s(it.doc_comment)
                                ));
                            }
                        }
                        ClassMember::Const(cd) => {
                            for it in cd.items {
                                classes.push(format!(
                                    "  {} {}",
                                    n(it.name.name),
                                    s(it.doc_comment)
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        funcs,
        [
            "f 4-8 /** doc f */",
            "g 11-11 /** doc g */",
            "h 14-14 /** doc h2 */",
            "i 17-17 /** doc i */",
            "a1 34-34 false",
            "b1 34-34 false",
            "k3 35-35 false",
            "z4 36-36 /** b4 */",
            "w9 39-39 false",
        ]
    );
    assert_eq!(
        classes,
        [
            "C 20-33 /** doc C */",
            "  $p /** doc p */",
            "  $q /** doc q */",
            "  m 29-29 /** doc m */",
            "  n 30-30 /** doc n */",
            "  K /** doc K */",
            "  L /** doc L */",
            "E3 35-35 false",
            "P6 37-37 false",
            "  __construct 37-37 false",
            "  param x /** promo */",
            "  param y false",
            "C8 38-38 /** cl8 */",
            "  $p false",
            "  m 38-38 false",
        ]
    );
}
