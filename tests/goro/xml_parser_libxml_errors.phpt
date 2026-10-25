--TEST--
xml_parse() errors reach libxml_get_last_error() and the internal error list
--FILE--
<?php libxml_use_internal_errors(true); $p = xml_parser_create(); xml_parse($p, "<a>\n</b>", true); xml_parse($p, "<more/>", true); print_r(libxml_get_errors()); libxml_clear_errors(); $p = xml_parser_create_ns(); xml_parse($p, "<a xmlns:p=\"\"><q:b/>&x;<!DOCTYPE><a a=\"1\" a=\"2\"/>", true); print_r(libxml_get_errors()); libxml_clear_errors(); $p = xml_parser_create(); xml_parse($p, "<a>", false); xml_parse($p, "</a><b/>", false);xml_parse($p, "", true); print_r(libxml_get_errors()); libxml_use_internal_errors(false); $p = xml_parser_create(); xml_parse($p, "<a></b>", true); echo "done\n";
 $p = xml_parser_create(); xml_parse($p, "<a>\n</b>", true); var_dump(libxml_get_last_error()); libxml_clear_errors(); $p = xml_parser_create(); xml_parse($p, "<a>\n</a>", true); var_dump(libxml_get_last_error()); $p = xml_parser_create(); xml_parse($p, "<a>&x;</a>", true); var_dump(libxml_get_last_error());
--EXPECT--
Array
(
    [0] => LibXMLError Object
        (
            [level] => 3
            [code] => 76
            [column] => 5
            [message] => Opening and ending tag mismatch: a line 0 and b

            [file] => 
            [line] => 2
        )

)
Array
(
    [0] => LibXMLError Object
        (
            [level] => 2
            [code] => 200
            [column] => 14
            [message] => xmlns:p: Empty XML namespace is not allowed

            [file] => 
            [line] => 1
        )

    [1] => LibXMLError Object
        (
            [level] => 2
            [code] => 201
            [column] => 19
            [message] => Namespace prefix q on b is not defined

            [file] => 
            [line] => 1
        )

    [2] => LibXMLError Object
        (
            [level] => 3
            [code] => 26
            [column] => 24
            [message] => Entity 'x' not defined

            [file] => 
            [line] => 1
        )

)
Array
(
    [0] => LibXMLError Object
        (
            [level] => 3
            [code] => 5
            [column] => 8
            [message] => Extra content at the end of the document

            [file] => 
            [line] => 1
        )

)
done
object(LibXMLError)#2 (6) {
  ["level"]=>
  int(3)
  ["code"]=>
  int(76)
  ["column"]=>
  int(5)
  ["message"]=>
  string(48) "Opening and ending tag mismatch: a line 0 and b
"
  ["file"]=>
  string(0) ""
  ["line"]=>
  int(2)
}
bool(false)
object(LibXMLError)#2 (6) {
  ["level"]=>
  int(3)
  ["code"]=>
  int(26)
  ["column"]=>
  int(7)
  ["message"]=>
  string(23) "Entity 'x' not defined
"
  ["file"]=>
  string(0) ""
  ["line"]=>
  int(1)
}
