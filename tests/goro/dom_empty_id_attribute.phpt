--TEST--
An empty ID attribute value is reported as already defined and not registered
--FILE--
<?php
libxml_use_internal_errors(true);
foreach (['<p id="">x</p>', '<p id="a">x</p><p id="a">y</p>', '<p id="">x</p><p id="">y</p>', '<p id>x</p>'] as $h) {
    $d = new DOMDocument;
    $d->loadHTML($h);
    foreach (libxml_get_errors() as $e) echo "  L{$e->level} C{$e->code} ", json_encode($e->message), "\n";
    libxml_clear_errors();
    echo $h, " => ", $d->getElementById('') ? 'found' : 'none', "\n";
}
foreach (['<a xml:id=""/>', '<!DOCTYPE a [<!ATTLIST a i ID #IMPLIED>]><a i=""/>', '<a><b xml:id="x"/><b xml:id="x"/></a>'] as $x) {
    $d = new DOMDocument;
    $d->loadXML($x);
    foreach (libxml_get_errors() as $e) echo "  L{$e->level} C{$e->code} {$e->line}:{$e->column} ", json_encode($e->message), "\n";
    libxml_clear_errors();
    echo $x, " => ", $d->getElementById('') ? 'found' : 'none', "\n";
}
?>
--EXPECT--
  L2 C513 "ID  already defined\n"
<p id="">x</p> => none
  L2 C513 "ID a already defined\n"
<p id="a">x</p><p id="a">y</p> => none
  L2 C513 "ID  already defined\n"
  L2 C513 "ID  already defined\n"
<p id="">x</p><p id="">y</p> => none
<p id>x</p> => none
  L2 C539 1:13 "xml:id : attribute value  is not an NCName\n"
  L2 C513 1:13 "ID  already defined\n"
<a xml:id=""/> => none
  L2 C513 1:49 "ID  already defined\n"
<!DOCTYPE a [<!ATTLIST a i ID #IMPLIED>]><a i=""/> => none
  L2 C513 1:32 "ID x already defined\n"
<a><b xml:id="x"/><b xml:id="x"/></a> => none
