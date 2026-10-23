--TEST--
strip_tags(): php_strip_tags_ex() state machine
--FILE--
<?php
$cases = [
  ["ab<", null], ["<b>x</b><", "<b>"], ["<b", "<b>"], ["<a href='x>y'>z</a>", "<a>"],
  ["<a title='<b>'>x</a>", "<a>"], ["<b '>'>y", "<b>"], ['<b"<">y', "<b>"], ["<<a href='x>y'><p>z", "<a>"],
  ["a < b > c", null], ["<?php echo 1; ?>x", null], ["<?xml version='1.0'?>x", null], ["<!-- c -->x", null],
  ["<!DOCTYPE html>x", null], ["<!DOCTYPE html>x", "<!doctype>"], ["<br/>x<br />", "<br>"], ["</b>x", "<b>"],
  ["<B>x</B>", "<b>"], ["<b>x</b>", ["b"]], ["<b>x</b><i>y</i>", ["b", "i"]], ["<b>x</b>", []], ["<b>x</b>", ""],
  ["a\0b<b\0>c", "<b>"], ["<a\"b>c", null], ["'<b>'x", null], ["\"x\"<b>y", "<b>"], ["<b x=\"a\\\"b\">c", "<b>"],
  ["<? if (a) { ?>x", null], ["<?php ('>') ?>y", null], ["<scr<script>ipt>x", null], ["< b>x", null],
  ["<b\n>x", "<b>"], ["<\nb>x", "<b>"], ["x>y", null], ["<p>a<<b>>c</p>", "<p>"], ["<p >x</p >", "<p>"],
  ["<!-- a -- > b -->x", null], ["<!x>y", null], ["<b>x</b>", "<b\0><i>"], ["<i>x</i>", "<b\0><i>"],
];
foreach ($cases as [$s, $a]) {
  echo json_encode(strip_tags($s, $a)), "\n";
}
--EXPECT--
"ab"
"<b>x<\/b>"
""
"<a href='xy'>z<\/a>"
"<a title='b'>x<\/a>"
"<b ''>y"
"y"
"<a href='xy'>z"
"a < b > c"
"x"
"x"
"x"
"x"
"x"
"<br\/>x<br \/>"
"<\/b>x"
"<B>x<\/B>"
"<b>x<\/b>"
"<b>x<\/b><i>y<\/i>"
"x"
"x"
"ab<b>c"
""
"''x"
"\"x\"<b>y"
""
"x"
"y"
"x"
"< b>x"
"<b\n>x"
"<\nb>x"
"x>y"
"<p>ac<\/p>"
"<p >x<\/p >"
"x"
"y"
"x"
"x"
