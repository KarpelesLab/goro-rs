--TEST--
'<' in an entity referenced from an attribute: every '<' reported, once per entity unless substituted
--FILE--
<?php
libxml_use_internal_errors(true);
$cases = [
 '<!DOCTYPE a [<!ENTITY e "<b>x</b>">]><a x="&e;"/>',
 '<!DOCTYPE a [<!ENTITY e "<b>x</b>">]><a x="&e;" y="&e;"><c z="&e;"/></a>',
 '<!DOCTYPE a [<!ENTITY e "<b>x</b>"><!ENTITY f "-&e;-">]><a x="&f;"/>',
 '<!DOCTYPE a [<!ENTITY e "<b>x</b>"><!ENTITY f "-&e;-">]><a x="&f;" y="&f;"/>',
 '<!DOCTYPE a [<!ENTITY e "<b>x</b>">]><a>&e;<c z="&e;"/></a>',
 '<!DOCTYPE a [<!ENTITY e "&#60;">]><a x="&e;"/>',
 '<!DOCTYPE a [<!ENTITY e "&#38;#60;">]><a x="&e;"/>',
];
foreach ($cases as $x) {
  echo $x, "\n";
  foreach ([0, LIBXML_NOENT, LIBXML_NOENT|1] as $o) {
    $d = new DOMDocument; $r = $d->loadXML($x, $o);
    foreach (libxml_get_errors() as $e) echo "  $o L{$e->level} C{$e->code} {$e->line}:{$e->column} ", trim($e->message), "\n";
    libxml_clear_errors();
    if ($r) echo "  ", json_encode($d->saveXML($d->documentElement)), "\n";
  }
  $p = xml_parser_create(); xml_parse($p, $x, true); echo "  xml ", xml_get_error_code($p), "\n";
  libxml_clear_errors();
}
?>
--EXPECT--
<!DOCTYPE a [<!ENTITY e "<b>x</b>">]><a x="&e;"/>
  0 L3 C38 1:46 '<' in entity 'e' is not allowed in attributes values
  0 L3 C38 1:46 '<' in entity 'e' is not allowed in attributes values
  2 L3 C38 1:46 '<' in entity 'e' is not allowed in attributes values
  3 L3 C38 1:46 '<' in entity 'e' is not allowed in attributes values
  "<a x=\"\"\/>"
  xml 38
<!DOCTYPE a [<!ENTITY e "<b>x</b>">]><a x="&e;" y="&e;"><c z="&e;"/></a>
  0 L3 C38 1:46 '<' in entity 'e' is not allowed in attributes values
  0 L3 C38 1:46 '<' in entity 'e' is not allowed in attributes values
  2 L3 C38 1:46 '<' in entity 'e' is not allowed in attributes values
  2 L3 C38 1:54 '<' in entity 'e' is not allowed in attributes values
  2 L3 C38 1:65 '<' in entity 'e' is not allowed in attributes values
  3 L3 C38 1:46 '<' in entity 'e' is not allowed in attributes values
  3 L3 C38 1:54 '<' in entity 'e' is not allowed in attributes values
  3 L3 C38 1:65 '<' in entity 'e' is not allowed in attributes values
  "<a x=\"\" y=\"\"><c z=\"\"\/><\/a>"
  xml 38
<!DOCTYPE a [<!ENTITY e "<b>x</b>"><!ENTITY f "-&e;-">]><a x="&f;"/>
  0 L3 C38 1:64 '<' in entity 'e' is not allowed in attributes values
  0 L3 C38 1:64 '<' in entity 'e' is not allowed in attributes values
  2 L3 C38 1:64 '<' in entity 'e' is not allowed in attributes values
  3 L3 C38 1:64 '<' in entity 'e' is not allowed in attributes values
  "<a x=\"--\"\/>"
  xml 38
<!DOCTYPE a [<!ENTITY e "<b>x</b>"><!ENTITY f "-&e;-">]><a x="&f;" y="&f;"/>
  0 L3 C38 1:64 '<' in entity 'e' is not allowed in attributes values
  0 L3 C38 1:64 '<' in entity 'e' is not allowed in attributes values
  2 L3 C38 1:64 '<' in entity 'e' is not allowed in attributes values
  2 L3 C38 1:72 '<' in entity 'e' is not allowed in attributes values
  3 L3 C38 1:64 '<' in entity 'e' is not allowed in attributes values
  3 L3 C38 1:72 '<' in entity 'e' is not allowed in attributes values
  "<a x=\"--\" y=\"--\"\/>"
  xml 38
<!DOCTYPE a [<!ENTITY e "<b>x</b>">]><a>&e;<c z="&e;"/></a>
  0 L3 C38 1:52 '<' in entity 'e' is not allowed in attributes values
  0 L3 C38 1:52 '<' in entity 'e' is not allowed in attributes values
  2 L3 C38 1:52 '<' in entity 'e' is not allowed in attributes values
  3 L3 C38 1:52 '<' in entity 'e' is not allowed in attributes values
  "<a><b>x<\/b><c z=\"\"\/><\/a>"
  xml 38
<!DOCTYPE a [<!ENTITY e "&#60;">]><a x="&e;"/>
  0 L3 C38 1:43 '<' in entity 'e' is not allowed in attributes values
  2 L3 C38 1:43 '<' in entity 'e' is not allowed in attributes values
  3 L3 C38 1:43 '<' in entity 'e' is not allowed in attributes values
  "<a x=\"\"\/>"
  xml 38
<!DOCTYPE a [<!ENTITY e "&#38;#60;">]><a x="&e;"/>
  "<a x=\"&e;\"\/>"
  "<a x=\"&lt;\"\/>"
  "<a x=\"&lt;\"\/>"
  xml 0
