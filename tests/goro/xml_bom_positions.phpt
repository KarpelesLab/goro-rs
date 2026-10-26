--TEST--
Byte order marks: the UTF-8 BOM takes no column, the UTF-16 BOM counts as consumed bytes
--FILE--
<?php
$docs = [
    "\xef\xbb\xbf<a><b/></c>",
    "\xff\xfe" . mb_convert_encoding("<a>h\xc3\xa9<b x='1'/></c>", 'UTF-16LE', 'UTF-8'),
    "\xfe\xff" . mb_convert_encoding("<a>h\xc3\xa9<b x='1'/></a>", 'UTF-16BE', 'UTF-8'),
];
foreach ($docs as $doc) {
    foreach ([1, 3, 1000] as $chunk) {
        $p = xml_parser_create();
        $out = [];
        xml_set_element_handler($p, function ($p, $n, $a) use (&$out) { $out[] = "S$n@" . xml_get_current_byte_index($p); }, function ($p, $n) use (&$out) { $out[] = "E$n@" . xml_get_current_byte_index($p); });
        $parts = str_split($doc, $chunk);
        foreach ($parts as $i => $c) { $r = xml_parse($p, $c, $i == count($parts) - 1); if (!$r) break; }
        echo "chunk=$chunk r=$r ", xml_error_string(xml_get_error_code($p)), " ", xml_get_current_line_number($p), ":", xml_get_current_column_number($p), "@", xml_get_current_byte_index($p), " ", implode(" ", $out), "\n";
    }
}
libxml_use_internal_errors(true);
$d = new DOMDocument;
$d->loadXML("\xef\xbb\xbf<a>\n<b/></c>");
$d->loadXML("\xef\xbb\xbf<a><b/></c>");
foreach (libxml_get_errors() as $e) echo $e->line, ":", $e->column, " ", $e->message;
?>
--EXPECT--
chunk=1 r=0 Mismatched tag 1:12@14 SA@5 SB@8 EB@10
chunk=3 r=0 Mismatched tag 1:12@14 SA@5 SB@8 EB@10
chunk=1000 r=0 Mismatched tag 1:12@14 SA@5 SB@8 EB@10
chunk=1 r=0 Mismatched tag 1:20@22 SA@4 SB@16 EB@18
chunk=3 r=0 Mismatched tag 1:20@22 SA@4 SB@16 EB@18
chunk=1000 r=0 Mismatched tag 1:20@22 SA@4 SB@16 EB@18
chunk=1 r=1 No error 1:20@22 SA@4 SB@16 EB@18 EA@22
chunk=3 r=1 No error 1:20@22 SA@4 SB@16 EB@18 EA@22
chunk=1000 r=1 No error 1:20@22 SA@4 SB@16 EB@18 EA@22
2:9 Opening and ending tag mismatch: a line 1 and c
1:12 Opening and ending tag mismatch: a line 1 and c
