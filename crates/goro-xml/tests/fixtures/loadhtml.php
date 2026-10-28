<?php
// Fixture driver: DOMDocument::loadHTML() (libxml HTML parser) on PHP 8.5.
//   C <base64 input> <options>
//   R <1|0>
//   E <level> <code> <line> <column> <base64 message>
//   H <base64 saveHTML()>
//   X <base64 saveXML()>
$cases = [];
function c($h, $o = 0) { global $cases; $cases[] = [$h, $o]; }
foreach ([
 '<p>hello</p><p>world</p>', 't', '<p>foo</p>', '<!DOCTYPE html><p>hello</p>', '<!DOCTYPE HTML><html><p>hi</p> test <p>hi2</p></html>',
 '<html><body><p>Test<br></p></body></html>', '<p id=x>foo</p>', '<span title="y">x</span><span title="z">x</span>',
 "<p>foo\0bar</p>", "<foo>tes\x00t</foo>", '<html id="htmlelement"><body id="x">before<div id="y">hello</div>after</body></html>',
 '<p align="center">foo</p>', '<a href="https://php.net">hello</a>', '<tag-throw></tag-throw>', '<!DOCTYPE>',
 '<ul><li>a<li>b</ul>', '<p>a<p>b<div>c</div>', '<table><tr><td>1<td>2<tr><td>3</table>', '<b><i>x</b>y</i>',
 '<input type=checkbox checked disabled>', '<select><option selected>a<option>b</select>', '<br/><hr><img src="x.png">',
 '<script>if (a < b && c > d) { x = "</p>"; }</script>', '<style>p > a { color: red }</style>', '<textarea><b>x</b></textarea>',
 '<title>a &amp; b</title>', '&amp; &lt; &gt; &quot; &nbsp; &copy; &eacute; &#233; &#xe9; &bogus; &amp', '<p title="&amp;&quot;&lt;">x</p>',
 '<!-- comment --><p>x</p><!-- trailing -->', '<?php echo 1; ?><p>x</p>', '<head><title>T</title></head><body>B</body>',
 '<meta charset="utf-8"><p>é</p>', '<meta http-equiv="Content-Type" content="text/html; charset=iso-8859-1"><p>' . "\xe9" . '</p>',
 "<p>\xc3\xa9</p>", '<p>é</p>', '<div><p>a</div>b', '</p>', '<p></b></p>', '<unknown attr="1">x</unknown>', '<P CLASS="A">X</P>',
 '<a href="x?a=1&b=2">l</a>', '<a href="x y">l</a>', '<img alt="">', '<option value=1 selected>x</option>',
 "<p>line1\nline2</p>\n<p>b</p>", '<html><head></head><body></body></html>', '<html>', '<body><p>x</body>', '<frameset><frame src="a"></frameset>',
 '<p>a</p><!DOCTYPE html>', '<svg><circle r="1"/></svg>', '<p>x<pre>\nfoo</pre>', '<dl><dt>a<dd>b<dt>c</dl>', '<h1>a<h2>b</h2>',
 '<a><a>x</a></a>', '<form><input name=a></form>', '<p class="a b" data-x="1" aria-label="l">x</p>', "<p>  spaced   text  </p>",
 '<table><caption>c</caption><thead><tr><th>h</th></tr></thead><tbody><tr><td>d</td></tr></tbody></table>', '<a name=top></a>',
 '<div id="a"><div id="b"></div></div>', '<button disabled>b</button>', '<p>a<br>b</p>', '<!DOCTYPE html PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd"><p>x</p>',
 '<p>x</p></body></html><p>after</p>', '<noscript><p>n</p></noscript>', '<iframe src="x"></iframe>', '<p><span>a</span> <span>b</span></p>',
 '<x:y xmlns:x="u">z</x:y>', '<p a=1 a=2>x</p>', '<p =x>y</p>', '<p a="1"b="2">y</p>', '<p>&#0;&#128;&#x110000;</p>', '<p>a&b c&d;e</p>',
] as $h) c($h);
foreach (['<p foo>x</p>', '<input checked="checked"><input checked=""><option selected="foo">a</option><input CHECKED>',
  '<p>&#x1F600; &#x2603; &#169; &#8364;</p>', '<meta charset="utf-8"><p>é ☃ €</p>',
  '<a href="x é/ü?q=ä&r=1#frag" name="a b" src="c d">l</a><img src="a b"><form action="a b"></form>',
  '<?pi data?><p>x</p>', '<p>a<b><div>c</div></b></p>', '<table><td>x</td></table>', '<p>x</p><p>y',
  '<html><body></body></html><!-- after -->', '<html lang="en"><html id="x"><body class="b"><body id="y">t',
  "<title>x</title>\n<p>y</p>", "  \n<p>y</p>\n  ", '<br></br>', '<p>x</P>', '<SCRIPT>a</SCRIPT>',
  '<p title=\'a"b\'>x</p>', '<script></script><style></style>', "<p>x\n<b>y</i>\n</p>\n<div>\n</span></div>"] as $h) c($h);
c('<p>hello</p><p>world</p>', LIBXML_HTML_NOIMPLIED | LIBXML_HTML_NODEFDTD);
c('t', LIBXML_HTML_NOIMPLIED | LIBXML_HTML_NODEFDTD);
c('<p>hello</p>', LIBXML_HTML_NODEFDTD);
c('<p>hello</p>', LIBXML_HTML_NOIMPLIED);
c('<div>a</div>text', LIBXML_HTML_NOIMPLIED);
c("<p>\n  <b> x </b>\n</p>", LIBXML_NOBLANKS);
c('<p>x</p>', LIBXML_NOERROR);
c('<html><body><p>a</p>   <p>b</p>  </body></html>');
foreach ($cases as [$h, $o]) {
  libxml_use_internal_errors(true); libxml_clear_errors();
  $d = new DOMDocument;
  echo "C ", base64_encode($h), " $o\n";
  try { $r = $d->loadHTML($h, $o); } catch (Throwable $e) { echo "R EXC\n"; continue; }
  echo "R ", (int)$r, "\n";
  foreach (libxml_get_errors() as $e) echo "E {$e->level} {$e->code} {$e->line} {$e->column} ", base64_encode($e->message), "\n";
  if ($r) {
    echo "H ", base64_encode($d->saveHTML()), "\n";
    echo "X ", base64_encode($d->saveXML()), "\n";
  }
}
