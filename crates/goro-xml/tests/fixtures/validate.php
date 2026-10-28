<?php
// Fixture driver: DOMDocument::validate() and LIBXML_DTDVALID on PHP 8.5.
//   C <base64 input> <mode: validate|dtdvalid>
//   R <result>
//   E <level> <code> <line> <column> <base64 message>
$cases = [
 "<!DOCTYPE a [<!ELEMENT a (b,c)><!ELEMENT b EMPTY><!ELEMENT c (#PCDATA)>]><a><b/><c>x</c></a>",
 "<!DOCTYPE a [<!ELEMENT a (b,c)><!ELEMENT b EMPTY><!ELEMENT c (#PCDATA)>]><a><c>x</c><b/></a>",
 "<!DOCTYPE a [<!ELEMENT a (b,c)><!ELEMENT b EMPTY><!ELEMENT c (#PCDATA)>]><a><b/></a>",
 "<!DOCTYPE a [<!ELEMENT a (b)*><!ELEMENT b EMPTY>]><a><b>x</b></a>",
 "<!DOCTYPE a [<!ELEMENT a (#PCDATA)>]><a><b/></a>",
 "<!DOCTYPE a [<!ELEMENT a (#PCDATA|b)*><!ELEMENT b EMPTY>]><a>t<b/><c/></a>",
 "<!DOCTYPE a [<!ELEMENT a ANY>]><a><zz/></a>",
 "<!DOCTYPE x [<!ELEMENT a ANY>]><a/>",
 "<a/>",
 "<!DOCTYPE a [<!ELEMENT a EMPTY><!ATTLIST a r CDATA #REQUIRED>]><a/>",
 "<!DOCTYPE a [<!ELEMENT a EMPTY><!ATTLIST a f CDATA #FIXED 'v'>]><a f='w'/>",
 "<!DOCTYPE a [<!ELEMENT a EMPTY><!ATTLIST a e (x|y) #IMPLIED>]><a e='z'/>",
 "<!DOCTYPE a [<!ELEMENT a EMPTY>]><a q='1'/>",
 "<!DOCTYPE a [<!ELEMENT a (b)*><!ELEMENT b EMPTY><!ATTLIST b id ID #IMPLIED>]><a><b id='x'/><b id='x'/></a>",
 "<!DOCTYPE a [<!ELEMENT a (b)*><!ELEMENT b EMPTY><!ATTLIST b r IDREF #IMPLIED>]><a><b r='nope'/></a>",
 "<!DOCTYPE a [<!ELEMENT a EMPTY><!ATTLIST a n NMTOKEN #IMPLIED>]><a n='a b'/>",
 "<!DOCTYPE a [<!ELEMENT a EMPTY><!ATTLIST a i ID #IMPLIED>]><a i='1x'/>",
 "<!DOCTYPE a [<!ELEMENT a (b?,c+)><!ELEMENT b EMPTY><!ELEMENT c EMPTY>]><a><c/><c/></a>",
 "<!DOCTYPE a [<!ELEMENT a (b|c)+><!ELEMENT b EMPTY><!ELEMENT c EMPTY>]><a></a>",
 "<!DOCTYPE a [<!ELEMENT a (b)><!ELEMENT b EMPTY>]><a> <b/> </a>",
 "<!DOCTYPE a [<!ELEMENT a (b)><!ELEMENT b EMPTY>]><a>x<b/></a>",
 "<!DOCTYPE a [<!ELEMENT a EMPTY><!ATTLIST a e ENTITY #IMPLIED><!NOTATION n SYSTEM 'x'><!ENTITY u SYSTEM 'u' NDATA n>]><a e='u'/>",
 "<!DOCTYPE a [<!ELEMENT a EMPTY><!ATTLIST a e ENTITY #IMPLIED>]><a e='nope'/>",
 "<!DOCTYPE a [<!ELEMENT a (b)><!ELEMENT a (c)><!ELEMENT b EMPTY>]><a><b/></a>",
 "<!DOCTYPE a [<!ELEMENT a (#PCDATA)>]><a><!-- c --><?p?>x</a>",
 "<!DOCTYPE a [<!ELEMENT a EMPTY>]><a><!-- c --></a>",
];
foreach ($cases as $x) {
  foreach (['validate', 'dtdvalid'] as $mode) {
    libxml_use_internal_errors(true); libxml_clear_errors();
    $d = new DOMDocument;
    echo "C ", base64_encode($x), " $mode\n";
    if ($mode == 'validate') { $d->loadXML($x); libxml_clear_errors(); $r = $d->validate(); }
    else { $r = $d->loadXML($x, LIBXML_DTDVALID); }
    echo "R ", (int)$r, "\n";
    foreach (libxml_get_errors() as $e) echo "E {$e->level} {$e->code} {$e->line} {$e->column} ", base64_encode($e->message), "\n";
  }
}
