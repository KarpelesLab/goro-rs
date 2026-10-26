--TEST--
Entity values: raw scan, then libxml2 2.13 validation/expansion at the closing quote; partial values on non-fatal errors
--FILE--
<?php
libxml_use_internal_errors(true);
$vals = ["x\r\ny", "x\ry", "a\x01b", "a\xe9b", "a\xc3\xa9b", "&#3>;#60;", "&#38;#60;", "&#xZZ;", "a&b;c", "a&;b", "a&b", "&#0;", "a\0b", "&#65;&#x42;", "%pe;", "\xe2\x82\xac\xe2\x82\xac x"];
foreach ($vals as $v) {
  $x = "<!DOCTYPE a [<!ENTITY e \"$v\">]><a>&e;</a>";
  echo json_encode(bin2hex($v)), "\n";
  foreach ([0, LIBXML_NOENT] as $o) {
    $d = new DOMDocument; $r = $d->loadXML($x, $o);
    foreach (libxml_get_errors() as $e) echo "  $o L{$e->level} C{$e->code} {$e->line}:{$e->column} ", trim($e->message), "\n";
    libxml_clear_errors();
    if ($r) echo "  $o ", bin2hex($d->saveXML($d->documentElement)), " ", bin2hex($d->doctype->entities->getNamedItem('e')?->textContent ?? 'NONE'), "\n";
  }
}
libxml_use_internal_errors(true);
$docs = [
 '<!DOCTYPE a [<!ENTITY % pe "x"><!ENTITY e "%pe;">]><a>&e;</a>',
 '<!DOCTYPE a [<!ENTITY e "a%pe">]><a>&e;</a>',
 '<!DOCTYPE a [<!ENTITY e "a%;">]><a>&e;</a>',
 '<!DOCTYPE a [<!ENTITY e "a%pe;b%pe2;c">]><a>&e;</a>',
 '<!DOCTYPE a [<!ENTITY e "a&#65;%pe;&#3x;">]><a>&e;</a>',
 '<!DOCTYPE a [<!ENTITY e "&#65">]><a>&e;</a>',
 '<!DOCTYPE a [<!ENTITY e "&#1114112;">]><a>&e;</a>',
 '<!DOCTYPE a [<!ENTITY e "&#xD800;">]><a>&e;</a>',
 '<!DOCTYPE a [<!ENTITY e "a&b;&#x41;">]><a>&e;</a>',
 "<!DOCTYPE a [<!ENTITY e 'it\"s'>]><a>&e;</a>",
 "<!DOCTYPE a [<!ENTITY e \"x\ty\nz\">]><a>&e;</a>",
];
foreach ($docs as $x) {
  echo json_encode($x), "\n";
  foreach ([0, LIBXML_NOENT] as $o) {
    $d = new DOMDocument; $r = $d->loadXML($x, $o);
    foreach (libxml_get_errors() as $e) echo "  $o L{$e->level} C{$e->code} {$e->line}:{$e->column} ", trim($e->message), "\n";
    libxml_clear_errors();
    if ($r) echo "  $o ", json_encode($d->saveXML($d->documentElement)), "\n";
  }
}
?>
--EXPECT--
"780d0a79"
  0 3c613e26653b3c2f613e 780a79
  2 3c613e780a793c2f613e 780a79
"780d79"
  0 3c613e26653b3c2f613e 780a79
  2 3c613e780a793c2f613e 780a79
"610162"
  0 L3 C9 1:28 invalid character in entity value
  0 L3 C26 1:38 Entity 'e' not defined
  2 L3 C9 1:28 invalid character in entity value
  2 L3 C26 1:38 Entity 'e' not defined
"61e962"
  0 L3 C81 1:28 Invalid bytes in character encoding
  0 L3 C26 1:38 Entity 'e' not defined
  2 L3 C81 1:28 Invalid bytes in character encoding
  2 L3 C26 1:38 Entity 'e' not defined
"61c3a962"
  0 3c613e26653b3c2f613e 61c3a962
  2 3c613e61c3a9623c2f613e 61c3a962
"2623333e3b2336303b"
  0 L3 C7 1:34 CharRef: invalid decimal value
  0 L3 C9 1:34 xmlParseStringCharRef: invalid xmlChar value 0
  0 L3 C26 1:44 Entity 'e' not defined
  2 L3 C7 1:34 CharRef: invalid decimal value
  2 L3 C9 1:34 xmlParseStringCharRef: invalid xmlChar value 0
  2 L3 C26 1:44 Entity 'e' not defined
"262333383b2336303b"
  0 3c613e26653b3c2f613e 3c
  2 3c613e266c743b3c2f613e 3c
"2623785a5a3b"
  0 L3 C6 1:31 CharRef: invalid hexadecimal value
  0 L3 C9 1:31 xmlParseStringCharRef: invalid xmlChar value 0
  0 L3 C26 1:41 Entity 'e' not defined
  2 L3 C6 1:31 CharRef: invalid hexadecimal value
  2 L3 C9 1:31 xmlParseStringCharRef: invalid xmlChar value 0
  2 L3 C26 1:41 Entity 'e' not defined
"6126623b63"
  0 L3 C26 1:40 Entity 'b' not defined
  2 L3 C26 1:40 Entity 'b' not defined
"61263b62"
  0 L3 C87 1:29 EntityValue: '&' forbidden except for entities references
  0 L3 C26 1:39 Entity 'e' not defined
  2 L3 C87 1:29 EntityValue: '&' forbidden except for entities references
  2 L3 C26 1:39 Entity 'e' not defined
"612662"
  0 L3 C87 1:28 EntityValue: '&' forbidden except for entities references
  0 L3 C26 1:38 Entity 'e' not defined
  2 L3 C87 1:28 EntityValue: '&' forbidden except for entities references
  2 L3 C26 1:38 Entity 'e' not defined
"2623303b"
  0 L3 C9 1:29 xmlParseStringCharRef: invalid xmlChar value 0
  0 L3 C26 1:39 Entity 'e' not defined
  2 L3 C9 1:29 xmlParseStringCharRef: invalid xmlChar value 0
  2 L3 C26 1:39 Entity 'e' not defined
"610062"
  0 L3 C9 1:26 invalid character in entity value
  0 L3 C37 1:26 xmlParseEntityDecl: entity e not terminated
  2 L3 C9 1:26 invalid character in entity value
  2 L3 C37 1:26 xmlParseEntityDecl: entity e not terminated
"262336353b26237834323b"
  0 3c613e26653b3c2f613e 4142
  2 3c613e41423c2f613e 4142
"2570653b"
  0 L1 C27 1:29 Entity 'pe' not defined
  0 3c613e26653b3c2f613e 
  2 L2 C27 1:29 Entity 'pe' not defined
  2 3c612f3e 
"e282ace282ac2078"
  0 3c613e26653b3c2f613e e282ace282ac2078
  2 3c613ee282ace282ac20783c2f613e e282ace282ac2078
"<!DOCTYPE a [<!ENTITY % pe \"x\"><!ENTITY e \"%pe;\">]><a>&e;<\/a>"
  0 L3 C88 1:46 PEReferences forbidden in internal subset
  0 L1 C27 1:56 Entity 'e' not defined
  2 L3 C88 1:46 PEReferences forbidden in internal subset
  2 L2 C27 1:56 Entity 'e' not defined
"<!DOCTYPE a [<!ENTITY e \"a%pe\">]><a>&e;<\/a>"
  0 L3 C23 1:29 EntityRef: expecting ';'
  0 L3 C26 1:39 Entity 'e' not defined
  2 L3 C23 1:29 EntityRef: expecting ';'
  2 L3 C26 1:39 Entity 'e' not defined
"<!DOCTYPE a [<!ENTITY e \"a%;\">]><a>&e;<\/a>"
  0 L3 C68 1:28 xmlParseStringPEReference: no name
  0 L3 C26 1:38 Entity 'e' not defined
  2 L3 C68 1:28 xmlParseStringPEReference: no name
  2 L3 C26 1:38 Entity 'e' not defined
"<!DOCTYPE a [<!ENTITY e \"a%pe;b%pe2;c\">]><a>&e;<\/a>"
  0 L1 C27 1:37 Entity 'pe' not defined
  0 "<a>&e;<\/a>"
  2 L2 C27 1:37 Entity 'pe' not defined
  2 "<a>a<\/a>"
"<!DOCTYPE a [<!ENTITY e \"a&#65;%pe;&#3x;\">]><a>&e;<\/a>"
  0 L1 C27 1:40 Entity 'pe' not defined
  0 "<a>&e;<\/a>"
  2 L2 C27 1:40 Entity 'pe' not defined
  2 "<a>aA<\/a>"
"<!DOCTYPE a [<!ENTITY e \"&#65\">]><a>&e;<\/a>"
  0 L3 C7 1:29 CharRef: invalid decimal value
  0 L3 C9 1:29 xmlParseStringCharRef: invalid xmlChar value 0
  0 L3 C26 1:39 Entity 'e' not defined
  2 L3 C7 1:29 CharRef: invalid decimal value
  2 L3 C9 1:29 xmlParseStringCharRef: invalid xmlChar value 0
  2 L3 C26 1:39 Entity 'e' not defined
"<!DOCTYPE a [<!ENTITY e \"&#1114112;\">]><a>&e;<\/a>"
  0 L3 C9 1:35 xmlParseStringCharRef: character reference out of bounds
  0 L3 C26 1:45 Entity 'e' not defined
  2 L3 C9 1:35 xmlParseStringCharRef: character reference out of bounds
  2 L3 C26 1:45 Entity 'e' not defined
"<!DOCTYPE a [<!ENTITY e \"&#xD800;\">]><a>&e;<\/a>"
  0 L3 C9 1:33 xmlParseStringCharRef: invalid xmlChar value 55296
  0 L3 C26 1:43 Entity 'e' not defined
  2 L3 C9 1:33 xmlParseStringCharRef: invalid xmlChar value 55296
  2 L3 C26 1:43 Entity 'e' not defined
"<!DOCTYPE a [<!ENTITY e \"a&b;&#x41;\">]><a>&e;<\/a>"
  0 L3 C26 1:45 Entity 'b' not defined
  2 L3 C26 1:45 Entity 'b' not defined
"<!DOCTYPE a [<!ENTITY e 'it\"s'>]><a>&e;<\/a>"
  0 "<a>&e;<\/a>"
  2 "<a>it\"s<\/a>"
"<!DOCTYPE a [<!ENTITY e \"x\ty\nz\">]><a>&e;<\/a>"
  0 "<a>&e;<\/a>"
  2 "<a>x\ty\nz<\/a>"
