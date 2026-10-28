<?php
// Fixture driver: DOMDocument::loadXML() + saveXML() on PHP 8.5.
// Run: php85 -d error_reporting=-1 -d display_errors=1 loadxml.php > loadxml.out
// Output, per case:
//   C <base64 input> <load options> <formatOutput 0|1> <save options> <preserveWhiteSpace 0|1>
//   R <1|0>                       loadXML result
//   E <level> <code> <line> <column> <base64 message>
//   O <base64 saveXML()>          when loaded
$cases = [];
function c($xml, $opts = 0, $fmt = 0, $save = 0, $pws = 1) { global $cases; $cases[] = [$xml, $opts, $fmt, $save, $pws]; }
// Well-formedness errors.
foreach ([" ", "<a><b></a>", "<a x=1/>", "<a>", "<a></b>", "<a/><b/>", "<a/>junk", "abc", "<?xml version='1.0'?>",
 "<a x='1' x='2'/>", "<a x='<'/>", "<a>&foo;</a>", "<a>&#0;</a>", "<a>&#xZZ;</a>", "<a>&#65</a>", "<a>&amp</a>",
 "<a>]]></a>", "<a><!-- -- --></a>", "<a><![CDATA[x</a>", "<a><?xml x?></a>", "<1a/>", "<a b/>", "<a b=>", "<a:b/>",
 "<a xmlns:p='u'><p:b/><q:c/></a>", "<a q:x='1'/>", "<a xmlns:p=''/>", "<a xmlns:xml='x'/>", "<?xml version='2.0'?><a/>",
 "<?xml version='1.0' encoding='bogus'?><a/>", "<?xml version='1.0' standalone='maybe'?><a/>", "<a>\x01</a>", "<a>\xff</a>",
 "<!DOCTYPE a [<!ENTITY e 'x'>]><a>&e;</a>", "<!DOCTYPE a [<!ENTITY e '<b>'>]><a>&e;</a>", "<!DOCTYPE a [<!ENTITY e '&e;'>]><a>&e;</a>",
 "<!DOCTYPE a [<!ELEMENT a (#PCDATA)>]><a><b/></a>", "<!DOCTYPE a [ <!ENTITY % p 'x'> %p; ]><a/>", "<!DOCTYPE a [<!ATTLIST a x CDATA 'd'>]><a/>",
 "<a>\n<b>\n</c>\n</a>", "<a\n  b='1'\n  c>", "<a>text", "<a><b/>", "<!-- c --><a/><!-- d -->", "<a>&lt;&gt;&amp;&apos;&quot;&#x41;&#66;</a>",
 "<a></a >", "<a></a b>", "< a/>", "<a/ >", "<a xmlns='http://x' xmlns='http://y'/>", "<a xmlns:p='a' p:x='1' xmlns:q='a' q:x='2'/>",
 "<?pi?><a/>", "<?xml-foo?><a/>", "<?XML x?><a/>", "<a><?pi x?></a>", "<a>\r\n\rb</a>", "<!DOCTYPE a SYSTEM 'nofile.dtd'><a/>",
 "<!DOCTYPE><a/>", "<!DOCTYPE a [<!ELEMENT a ANY]><a/>", "<a b='&#x0;'/>", "<a b='&e;'/>", "<a b=\"\x09\x0a\"/>",
 "\xef\xbb\xbf<a/>", "\xff\xfe<\0a\0/\0>\0", "<?xml version='1.0' encoding='ISO-8859-1'?><a>\xe9</a>",
 "<a>\u{e9}\u{e9}<b></a>", "<\u{e9}\u{e9} x=1/>", str_repeat("<a>", 300) . str_repeat("</a>", 300),
 "<a x='1'y='2'/>", "<a><!--x", "<a><?pi", "<a>&#x110000;</a>", "<a>&#xFFFE;</a>", "<a>\u{FFFF}</a>", "<a x='\x01'/>",
 "<!DOCTYPE a [<!ENTITY e SYSTEM 'x.xml'>]><a x='&e;'/>", "<!DOCTYPE a [<!ENTITY e SYSTEM 'x' NDATA n>]><a>&e;</a>",
 "<a xmlns:a=' '/>", "<a xmlns='d'/>", "<?xml version='1.0'?>\n<!DOCTYPE a>\n<a/>\n<!DOCTYPE b>", "<a/><?xml version='1.0'?>",
 "<a></a><a></a>", "</a>", "<a><b></b></a></a>", "<a b:c:d='1'/>", "<a:b:c/>", "<a>&#x41;&#65;&#97;x</a>",
 "<!DOCTYPE a [\n<!ENTITY e 'x'>\n<!ENTITY e 'y'>\n]><a>&e;</a>", "<!DOCTYPE a [<!ELEMENT a (b,c|d)>]><a/>", "<!DOCTYPE a [<!ATTLIST a x (a|b) #FOO>]><a/>",
 "<!DOCTYPE a [<!ENTITY % pe '<!ENTITY q \"v\">'> %pe; ]><a>&q;</a>", "<!DOCTYPE a [<!ENTITY e \"%pe;\">]><a/>",
 "<!DOCTYPE a PUBLIC 'x' ><a/>", "<!DOCTYPE a [<!NOTATION n>]><a/>", "<a xml:lang='en' xml:space='preserve'/>",
 "<a xmlns:x='u' x:b='1' b='2'/>", "<?xml version='1.0' encoding='UTF-16'?><a/>", "<?xml version=\"1.0\"encoding='UTF-8'?><a/>",
 "<?xml encoding='UTF-8'?><a/>", "<?xml version='1.0' ?><a/>", "<?xml version='1.1'?><a/>", "<?xml version='1.0' standalone='no'?><a/>",
] as $x) c($x);
// Options.
c(str_repeat("<a>", 300) . str_repeat("</a>", 300), LIBXML_PARSEHUGE);
c("<a>\n  <b> </b>\n  <c>x</c> <d/>\n</a>", LIBXML_NOBLANKS);
c("<a>text <b> </b> more <c/> </a>", LIBXML_NOBLANKS);
c("<a>\n  <b> </b>\n  <c>x</c>\n</a>", 0, 0, 0, 0);
c("<a xml:space='preserve'>\n  <b> </b>\n</a>", LIBXML_NOBLANKS);
c("<!DOCTYPE a [<!ELEMENT a (b)*><!ELEMENT b ANY>]><a>\n<b> </b>\n</a>", LIBXML_NOBLANKS);
c("<!DOCTYPE a [<!ELEMENT a (#PCDATA|b)*>]><a>\n<b/>\n</a>", LIBXML_NOBLANKS);
c("<a><b><c>x</c><d/></b><e>t<f/></e><!--c--><?p d?></a>", 0, 1);
c("<a>\n <b><c/></b>\n</a>", 0, 1);
c("<a>\n <b><c/></b>\n</a>", LIBXML_NOBLANKS, 1);
c("<a><e>t<f><g/></f></e></a>", 0, 1);
c("<a><![CDATA[x]]><b><c/></b></a>", 0, 1);
c("<!DOCTYPE a [<!ENTITY e 'x'>]><a>&e;<b><c/></b></a>", 0, 1);
c("<a><b/><c></c></a>", 0, 0, LIBXML_NOEMPTYTAG);
c("<a><![CDATA[x<y]]></a>");
c("<a><![CDATA[x<y]]>z</a>", LIBXML_NOCDATA);
c("<a><![CDATA[a]]>]]&gt;<![CDATA[b]]></a>");
c("<!DOCTYPE a [<!ENTITY e 'x<b>y</b>'>]><a>&e;</a>");
c("<!DOCTYPE a [<!ENTITY e 'x<b>y</b>'>]><a>&e;</a>", LIBXML_NOENT);
c("<!DOCTYPE a [<!ENTITY e 'v'>]><a x='1&e;2'/>");
c("<!DOCTYPE a [<!ENTITY e 'v'>]><a x='1&e;2'/>", LIBXML_NOENT);
c("<!DOCTYPE a [<!ENTITY e SYSTEM 'x.xml'>]><a>&e;</a>");
c("<!DOCTYPE a [<!ATTLIST a x CDATA 'd' y CDATA #FIXED 'f' z CDATA #IMPLIED>]><a/>", LIBXML_DTDATTR);
c("<!DOCTYPE a [<!ATTLIST a x NMTOKENS #IMPLIED>]><a x='  a   b  ' y='  a   b  '/>");
c("<?xml version='1.0' encoding='utf-8' standalone='yes'?><a/>");
c("<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>\xe9</a>");
c("<!DOCTYPE html PUBLIC '-//W3C//DTD XHTML 1.0 Strict//EN' 'http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd'><html><br/><p/></html>");
c("<!DOCTYPE a [\n<!ELEMENT a (b|c)*>\n<!ELEMENT b (#PCDATA|c)*>\n<!ELEMENT c EMPTY>\n<!ELEMENT d (b,c?,(a|b)+)>\n<!ATTLIST a id ID #IMPLIED t (x|y) 'x' n NOTATION (gif) #IMPLIED>\n<!NOTATION gif SYSTEM 'image/gif'>\n<!NOTATION png PUBLIC 'png'>\n<!ENTITY u SYSTEM 'u.bin' NDATA gif>\n<!ENTITY % pe 'abc'>\n<!ENTITY q \"a&#38;b'c\">\n<!-- comment -->\n<?pi data?>\n]>\n<a/>");
c("<!DOCTYPE a [<!ELEMENT a (#PCDATA)><!ELEMENT b (c)><!ELEMENT d (c+)><!ENTITY q 'say \"hi\"'><!ENTITY r \"it's\"><!NOTATION n PUBLIC 'p' 's'><!ENTITY x PUBLIC 'p' 's'><!ATTLIST a r CDATA #REQUIRED>]><a r='1'/>");
c("<!DOCTYPE a []><a/>");
c("<!DOCTYPE a [<!-- only -->]><a/>");
c("<a x='&quot;&lt;&gt;&amp;&#9;&#10;&#13;'>&lt;&gt;&amp;\"'&#13;&#xD;</a>");
c("<a xmlns:x='u'><b xmlns:x='u'><x:c xmlns:y='v'/></b></a>", LIBXML_NSCLEAN);
c("<a xmlns='d' xmlns:x='u'><x:b x:y='1'/></a>");
c("<a><b xml:id='i1'/></a>");
c("<?pi   data  ?><a><?q?></a>");
c("<!--a--><!--b--><r/>");
c("<a/>", 0, 0, LIBXML_NOXMLDECL);
c("<a x='a\r\nb'/>");
c("<a>\u{20ac}\u{1F600}</a>");
c("<?xml version='1.0' encoding='UTF-8'?><a x='\u{e9}'>\u{20ac}</a>");
c("  <a/>  ");
c("<a>x</a><!-- tail --><?tail?>");
c("<a>&#x1F600;</a>");
c("<?xml version='1.0' encoding='windows-1252'?><a>\x80\xe9</a>");
c("<a b='1' c=\"2\"   d = '3'/>");
c("<a><b>1</b><b>2</b></a>", 0, 1);
c("<?xml version='1.0'?><!DOCTYPE a><a/>", 0, 1);
c("<a>\n</a>", 0, 1);
c("<a><b/>text</a>", 0, 1);
c("<a><!--x--></a>", 0, 1);
foreach ($cases as [$xml, $opts, $fmt, $save, $pws]) {
  libxml_use_internal_errors(true); libxml_clear_errors();
  $d = new DOMDocument; $d->formatOutput = (bool)$fmt; $d->preserveWhiteSpace = (bool)$pws;
  echo "C ", base64_encode($xml), " $opts $fmt $save $pws\n";
  $r = $d->loadXML($xml, $opts);
  echo "R ", (int)$r, "\n";
  foreach (libxml_get_errors() as $e) echo "E {$e->level} {$e->code} {$e->line} {$e->column} ", base64_encode($e->message), "\n";
  if ($r) echo "O ", base64_encode($d->saveXML(null, $save)), "\n";
}
