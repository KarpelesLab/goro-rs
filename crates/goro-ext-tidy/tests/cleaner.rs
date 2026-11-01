//! The cleaner's behaviour on inputs of the ext/tidy tests' kind.

use goro_ext_tidy::{Config, Document, find_option};

fn config(opts: &[(&str, &str)]) -> Config {
    let mut c = Config::new();
    c.set_bool("tidy-mark", false);
    for (n, v) in opts {
        let i = find_option(n).expect("known option");
        assert!(c.set_parsed(i, v), "{n}: {v}");
    }
    c
}

fn tidy(input: &str, opts: &[(&str, &str)], repair: bool) -> String {
    let mut d = Document::new(config(opts));
    d.parse(input.as_bytes());
    if repair {
        d.clean_repair();
    }
    String::from_utf8(d.output()).expect("utf-8")
}

fn body_only(input: &str) -> String {
    tidy(input, &[("show-body-only", "yes")], false)
}

#[test]
fn skeleton_is_always_there() {
    assert_eq!(
        tidy("<HTML></HTML>", &[], false),
        "<html>\n<head>\n<title></title>\n</head>\n<body>\n</body>\n</html>"
    );
}

#[test]
fn repair_adds_the_doctype() {
    assert!(tidy("", &[], true).starts_with("<!DOCTYPE html>\n<html>"));
    assert!(!tidy("", &[("doctype", "omit")], true).contains("DOCTYPE"));
}

#[test]
fn messages_follow_test_006() {
    let mut d = Document::new(config(&[]));
    d.parse(b"<HTML><asd asdf></HTML>");
    assert_eq!(
        d.error_buffer(),
        "line 1 column 1 - Warning: missing <!DOCTYPE> declaration\n\
         line 1 column 7 - Error: <asd> is not recognized!\n\
         line 1 column 7 - Warning: discarding unexpected <asd>\n\
         line 1 column 17 - Warning: discarding unexpected </html>\n\
         line 1 column 7 - Warning: inserting missing 'title' element\n"
    );
    assert_eq!(d.status(), 2);
}

#[test]
fn inline_elements_reopen_around_blocks() {
    assert_eq!(
        body_only("<b>a<p>b</p>c</b>"),
        "<b>a</b>\n<p><b>b</b></p>\n<b>c</b>"
    );
}

#[test]
fn implied_structure_is_inserted() {
    assert_eq!(
        body_only("<ul><li>a<li>b</ul>"),
        "<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n"
    );
    assert_eq!(
        body_only("<table><td>a<td>b</table>"),
        "<table>\n<tr>\n<td>a</td>\n<td>b</td>\n</tr>\n</table>\n"
    );
    assert_eq!(
        body_only("<select><option>a<option>b</select>"),
        "<select>\n<option>a</option>\n<option>b</option>\n</select>\n"
    );
}

#[test]
fn whitespace_collapses_outside_pre() {
    assert_eq!(
        body_only("<p>  a   b <b> c </b> d </p>"),
        "<p>a b <b>c</b> d</p>\n"
    );
    assert_eq!(body_only("<pre>  a\n  b </pre>"), "<pre>  a\n  b </pre>\n");
}

#[test]
fn entities_are_decoded_and_escaped() {
    assert_eq!(
        body_only("<p>&eacute; &foo; a&b &nbsp;</p>"),
        "<p>\u{e9} &amp;foo; a&amp;b &nbsp;</p>\n"
    );
    assert_eq!(
        tidy(
            "<p>&eacute;</p>",
            &[("show-body-only", "yes"), ("char-encoding", "ascii")],
            false
        ),
        "<p>&eacute;</p>\n"
    );
}

#[test]
fn lines_wrap_at_spaces() {
    let out = tidy(
        "<p>aaaa bbbb cccc dddd</p>",
        &[("show-body-only", "yes"), ("wrap", "10")],
        false,
    );
    assert!(out.lines().all(|l| l.len() <= 10), "{out}");
    assert_eq!(out.split_whitespace().count(), 4);
}

#[test]
fn indent_follows_nesting() {
    assert_eq!(
        tidy(
            "<div><p>a</p></div>",
            &[("show-body-only", "yes"), ("indent", "yes")],
            false
        ),
        "<div>\n  <p>a</p>\n</div>\n"
    );
}

#[test]
fn xhtml_output_closes_void_elements() {
    let out = tidy("<p>a<br>b</p>", &[("output-xhtml", "yes")], true);
    assert!(out.contains("<p>a<br />\nb</p>"), "{out}");
    assert!(
        out.contains("<html xmlns=\"http://www.w3.org/1999/xhtml\">"),
        "{out}"
    );
    assert!(out.starts_with("<!DOCTYPE html PUBLIC"), "{out}");
}

#[test]
fn xml_input_has_no_html_skeleton() {
    assert_eq!(
        tidy("<a x='1'><b>t</b><c/></a>", &[("input-xml", "yes")], false),
        "<a x=\"1\">\n<b>t</b>\n<c />\n</a>\n"
    );
}

#[test]
fn deep_nesting_stays_bounded() {
    let out = tidy(
        &"<div>".repeat(100_000),
        &[("show-body-only", "yes")],
        false,
    );
    assert!(out.matches("<div>").count() < 1000);
}

#[test]
fn config_files_count_their_errors() {
    let mut c = Config::new();
    c.load_text("clean: yes\n# comment\nindent:@\nbogus: 1\n");
    assert!(c.bool("clean"));
    assert_eq!(c.errors, 2);
}

#[test]
fn tag_ids_match_the_tests() {
    for (name, id) in [
        ("b", 8),
        ("body", 16),
        ("head", 46),
        ("html", 48),
        ("i", 49),
    ] {
        assert_eq!(goro_ext_tidy::tags::lookup(name).map(|t| t.id), Some(id));
    }
    for (i, t) in goro_ext_tidy::TAGS.iter().enumerate() {
        assert_eq!(t.id, i as i64 + 1, "{}", t.name);
    }
}
