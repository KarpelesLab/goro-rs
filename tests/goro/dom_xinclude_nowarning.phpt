--TEST--
DOMDocument::xinclude(LIBXML_NOWARNING) drops the I/O warning of a missing include (with a fallback, or the "no fallback" error alone)
--EXTENSIONS--
dom
--FILE--
<?php
chdir(__DIR__);
$x1 = '<?xml version="1.0"?>
<r xmlns:xi="http://www.w3.org/2001/XInclude"><x><xi:include href="dom_xinclude_nowarning_missing.xml"><xi:fallback><M/></xi:fallback></xi:include></x></r>';
$x2 = '<?xml version="1.0"?>
<r xmlns:xi="http://www.w3.org/2001/XInclude"><x><xi:include href="dom_xinclude_nowarning_missing.xml"/></x></r>';
$x3 = '<?xml version="1.0"?>
<r xmlns:xi="http://www.w3.org/2001/XInclude"><x><xi:include parse="text" href="dom_xinclude_nowarning_missing.txt"><xi:fallback>F</xi:fallback></xi:include></x></r>';
$d = new DOMDocument();
foreach ([$x1, $x2, $x3] as $x) {
    $d->loadXML($x, LIBXML_NONET);
    var_dump($d->xinclude(LIBXML_NOWARNING | LIBXML_NONET));
    echo $d->saveXML($d->documentElement), "\n";
}
echo "-- warnings on\n";
$d->loadXML($x1);
var_dump($d->xinclude());
?>
--EXPECTF--
int(1)
<r xmlns:xi="http://www.w3.org/2001/XInclude"><x><M/></x></r>

Warning: DOMDocument::xinclude(): could not load %sdom_xinclude_nowarning_missing.xml, and no fallback was found in %s on line %d
int(-1)
<r xmlns:xi="http://www.w3.org/2001/XInclude"><x/></r>
int(1)
<r xmlns:xi="http://www.w3.org/2001/XInclude"><x>F</x></r>
-- warnings on

Warning: DOMDocument::xinclude(): I/O warning : failed to load "%sdom_xinclude_nowarning_missing.xml": No such file or directory in %s on line %d
int(1)
