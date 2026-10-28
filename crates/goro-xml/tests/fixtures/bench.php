<?php
// The PHP side of tests/bench.rs: same documents, best of 5 runs.
function document($n) {
  $s = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<catalog xmlns:x=\"urn:x\">\n";
  for ($i = 0; $i < $n; $i++) {
    $s .= sprintf("  <item id=\"i%d\" x:kind=\"k%d\" price=\"%d.99\">\n    <name>Item number %d &amp; friends</name>\n    <desc>Some descriptive text for item %d, with &lt;markup&gt; and caf\u{e9}.</desc>\n    <tags><tag>a</tag><tag>b%d</tag><x:tag/></tags>\n    <!-- comment %d -->\n  </item>\n",
      $i, $i % 7, $i % 100, $i, $i, $i % 13, $i);
  }
  return $s . "</catalog>\n";
}
function html_document($n) {
  $s = "<!DOCTYPE html><html><head><title>Bench</title></head><body>\n";
  for ($i = 0; $i < $n; $i++) {
    $s .= sprintf("<div class=\"row r%d\" id=\"d%d\"><p>Paragraph %d with <b>bold</b> and <a href=\"/x?a=%d&amp;b=2\">a link</a>.<br>Line two &copy; 2026</p><ul><li>one<li>two</ul></div>\n", $i % 5, $i, $i, $i);
  }
  return $s . "</body></html>\n";
}
$x = document(20000);
$bp = $bs = INF;
for ($k = 0; $k < 5; $k++) {
  $d = new DOMDocument;
  $t = hrtime(true); $d->loadXML($x); $p = (hrtime(true) - $t) / 1e6;
  $t = hrtime(true); $o = $d->saveXML(); $s = (hrtime(true) - $t) / 1e6;
  $bp = min($bp, $p); $bs = min($bs, $s);
}
printf("XML %d bytes: parse %.1f ms, saveXML %.1f ms (output %d bytes)\n", strlen($x), $bp, $bs, strlen($o));
$h = html_document(20000);
$bp = $bs = INF;
for ($k = 0; $k < 5; $k++) {
  $d = new DOMDocument;
  $t = hrtime(true); @$d->loadHTML($h); $p = (hrtime(true) - $t) / 1e6;
  $t = hrtime(true); $o = $d->saveHTML(); $s = (hrtime(true) - $t) / 1e6;
  $bp = min($bp, $p); $bs = min($bs, $s);
}
printf("HTML %d bytes: loadHTML %.1f ms, saveHTML %.1f ms (output %d bytes)\n", strlen($h), $bp, $bs, strlen($o));
$bp = INF;
for ($k = 0; $k < 5; $k++) {
  $t = hrtime(true); $d = Dom\HTMLDocument::createFromString($h, LIBXML_NOERROR); $p = (hrtime(true) - $t) / 1e6;
  $bp = min($bp, $p);
}
printf("HTML5 %d bytes: Dom\\HTMLDocument::createFromString %.1f ms\n", strlen($h), $bp);
