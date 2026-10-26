--TEST--
XMLWriter::startDocument() with an empty encoding writes encoding="" (UTF-8 output)
--FILE--
<?php
foreach (['', 'utf-8', 'latin1', 'bogus'] as $e) {
    $w = new XMLWriter;
    $w->openMemory();
    var_dump(@$w->startDocument('1.0', $e, ''));
    $w->writeElement('r', "\xc3\xa9");
    $w->endDocument();
    var_dump(bin2hex($w->outputMemory()));
}
$w = xmlwriter_open_memory();
xmlwriter_start_document($w, null, '', 'yes');
xmlwriter_write_element($w, 'r');
xmlwriter_end_document($w);
var_dump(xmlwriter_output_memory($w));
?>
--EXPECT--
bool(true)
string(116) "3c3f786d6c2076657273696f6e3d22312e302220656e636f64696e673d2222207374616e64616c6f6e653d22223f3e0a3c723ec3a93c2f723e0a"
bool(true)
string(126) "3c3f786d6c2076657273696f6e3d22312e302220656e636f64696e673d225554462d3822207374616e64616c6f6e653d22223f3e0a3c723ec3a93c2f723e0a"
bool(true)
string(126) "3c3f786d6c2076657273696f6e3d22312e302220656e636f64696e673d226c6174696e3122207374616e64616c6f6e653d22223f3e0a3c723ee93c2f723e0a"
bool(false)
string(20) "3c723ec3a93c2f723e0a"
string(56) "<?xml version="1.0" encoding="" standalone="yes"?>
<r/>
"
