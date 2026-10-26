--TEST--
XMLWriter::toStream(): the buffered output reaches the stream as soon as the writer object is freed
--FILE--
<?php
$h = fopen('php://memory', 'w+');
$w = XMLWriter::toStream($h);
$w->startElement('a');
$w->writeElement('b', 'c');
var_dump($w->flush());
$w->writeComment('more');
$w->endElement();
unset($w);
rewind($h);
var_dump(stream_get_contents($h));
function f($h) { $w = XMLWriter::toStream($h); $w->writeElement('in-function'); }
f($h);
rewind($h);
var_dump(stream_get_contents($h));
$w = XMLWriter::toStream($h);
$w->writeElement('reassigned');
$w = null;
rewind($h);
var_dump(stream_get_contents($h));
$w = XMLWriter::toStream($h);
$w->writeElement('at-shutdown');
echo "end\n";
?>
--EXPECT--
int(11)
string(26) "<a><b>c</b><!--more--></a>"
string(40) "<a><b>c</b><!--more--></a><in-function/>"
string(53) "<a><b>c</b><!--more--></a><in-function/><reassigned/>"
end
