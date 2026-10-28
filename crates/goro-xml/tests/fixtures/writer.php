<?php
// Fixture driver: XMLWriter output on PHP 8.5. Each case is a list of
// [method, args...]; output: "W <base64 json of ops>" then "O <base64 output>"
// and "R <json of return values>".
$cases = [
 [['openMemory'], ['startDocument', '1.0', 'UTF-8'], ['startElement', 'root'], ['writeAttribute', 'a', '1&"<>'], ['text', 'x<&>"\''], ['endElement'], ['endDocument']],
 [['openMemory'], ['setIndent', true], ['startDocument'], ['startElement', 'root'], ['startElement', 'a'], ['text', 't'], ['endElement'], ['startElement', 'b'], ['startElement', 'c'], ['endElement'], ['endElement'], ['writeElement', 'd', 'v'], ['writeComment', 'cm'], ['endElement'], ['endDocument']],
 [['openMemory'], ['setIndent', true], ['setIndentString', "\t"], ['startElement', 'r'], ['startElement', 'a'], ['endElement'], ['text', 'mix'], ['startElement', 'b'], ['endElement'], ['endElement']],
 [['openMemory'], ['startElement', 'r'], ['fullEndElement']],
 [['openMemory'], ['startElement', 'r'], ['startElement', 'e'], ['fullEndElement'], ['endElement']],
 [['openMemory'], ['startElementNs', 'p', 'r', 'urn:p'], ['writeAttributeNs', 'q', 'a', 'urn:q', 'v'], ['startElementNs', 'p', 'c', null], ['endElement'], ['startElementNs', null, 'd', 'urn:d'], ['endElement'], ['endElement']],
 [['openMemory'], ['startElement', 'r'], ['startAttribute', 'a'], ['text', 'x'], ['text', 'y'], ['endAttribute'], ['startAttributeNs', 'p', 'b', 'urn:p'], ['text', 'z'], ['endAttribute'], ['endElement']],
 [['openMemory'], ['startElement', 'r'], ['startCdata'], ['text', 'a]]>b'], ['endCdata'], ['writeCdata', 'c'], ['endElement']],
 [['openMemory'], ['startPi', 'php'], ['text', 'echo 1;'], ['endPi'], ['writePi', 'x', 'y'], ['startElement', 'r'], ['writePi', 'z', ''], ['endElement']],
 [['openMemory'], ['startDtd', 'html', '-//W3C//DTD XHTML 1.0 Strict//EN', 'http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd'], ['endDtd'], ['startElement', 'html'], ['endElement']],
 [['openMemory'], ['startDtd', 'r'], ['writeDtdElement', 'r', '(#PCDATA)'], ['writeDtdAttlist', 'r', 'a CDATA #IMPLIED'], ['writeDtdEntity', 'e', 'val'], ['startDtdEntity', 'pe', true], ['text', 'pv'], ['endDtdEntity'], ['endDtd'], ['startElement', 'r'], ['endElement']],
 [['openMemory'], ['setIndent', true], ['startDtd', 'r'], ['writeDtdElement', 'r', '(#PCDATA)'], ['writeDtdAttlist', 'r', 'a CDATA #IMPLIED'], ['endDtd'], ['startElement', 'r'], ['endElement']],
 [['openMemory'], ['writeDtd', 'r', null, 'r.dtd', '<!ELEMENT r ANY>'], ['writeElement', 'r']],
 [['openMemory'], ['startElement', 'r'], ['writeRaw', '<raw&>'], ['endElement']],
 [['openMemory'], ['startDocument', '1.0', 'ISO-8859-1', 'yes'], ['writeElement', 'r', "\u{e9}\u{20ac}"], ['endDocument']],
 [['openMemory'], ['endElement']],
 [['openMemory'], ['text', 'loose']],
 [['openMemory'], ['startElement', 'r'], ['writeAttribute', 'a', 'b'], ['text', 'x'], ['writeAttribute', 'c', 'd'], ['endElement']],
 [['openMemory'], ['startElement', '1bad']],
 [['openMemory'], ['startElement', 'r'], ['flush'], ['text', 't'], ['flush', false], ['endElement'], ['flush']],
 [['openMemory'], ['startDocument'], ['startElement', 'a'], ['startElement', 'b']],
 [['openMemory'], ['setIndent', true], ['startElement', 'a'], ['startComment'], ['text', 'c'], ['endComment'], ['writeElement', 'b'], ['endElement']],
 [['openMemory'], ['setIndent', true], ['startElement', 'a'], ['writeAttribute', 'x', '1'], ['startElement', 'b'], ['writeElement', 'c', 'x'], ['endElement'], ['endElement']],
 [['openMemory'], ['startElement', 'a'], ['text', "t\n\r\t&#x;"], ['writeAttribute', 'x', "a\nb\tc\rd"], ['endElement']],
 [['openMemory'], ['writeElementNs', 'p', 'e', 'urn:p', 'content'], ['writeElementNs', null, 'f', null]],
 [['openMemory'], ['writeElement', 'e', null], ['writeElement', 'f', '']],
 [['openMemory'], ['setIndent', true], ['writeElement', 'a'], ['writeElement', 'b'], ['startPi', 'x'], ['endPi']],
 [['openMemory'], ['startDocument', '1.0'], ['setIndent', true], ['startElement', 'r'], ['writeElement', 'a', 'b'], ['endElement'], ['endDocument']],
 [['openMemory'], ['startElement', 'r'], ['startElement', 's'], ['endDocument']],
 [['openMemory'], ['startAttribute', 'x']],
 [['openMemory'], ['startElement', 'r'], ['writeAttributeNs', 'xmlns', 'p', null, 'urn:x'], ['startElementNs', 'p', 'c', null], ['endElement'], ['endElement']],
 [['openMemory'], ['startElementNs', 'p', 'r', 'urn:p'], ['writeAttributeNs', 'p', 'a', 'urn:p', 'v'], ['endElement']],
 [['openMemory'], ['startElementNs', 'p', 'r', 'urn:p'], ['writeAttributeNs', 'p', 'a', 'urn:other', 'v'], ['endElement']],
];
foreach ($cases as $ops) {
  $w = new XMLWriter;
  $rets = [];
  echo "W ", base64_encode(json_encode($ops)), "\n";
  foreach ($ops as $op) {
    $m = array_shift($op);
    try { $r = @$w->$m(...$op); } catch (Throwable $e) { $r = 'EXC ' . get_class($e) . ': ' . $e->getMessage(); }
    $rets[] = is_string($r) ? base64_encode($r) : $r;
  }
  echo "R ", json_encode($rets), "\n";
  echo "O ", base64_encode($w->outputMemory()), "\n";
}
