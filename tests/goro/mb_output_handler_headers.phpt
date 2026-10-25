--TEST--
mb_output_handler(): no Content-Type from inside a handler, header warning once output was sent
--FILE--
<?php
mb_http_output("SJIS");
ob_start("mb_output_handler");
echo "日本語テキスト\n";
ob_end_flush();
echo "\n";
ob_start("mb_output_handler", 4);
echo "日本語";
echo "テキスト";
ob_end_flush();
echo "\n";
mb_http_output("pass");
ob_start("mb_output_handler");
echo "日本語\n";
ob_end_flush();
var_dump(mb_output_handler("日本", PHP_OUTPUT_HANDLER_START | PHP_OUTPUT_HANDLER_END));
mb_http_output("EUC-JP");
var_dump(bin2hex(mb_output_handler("日本", PHP_OUTPUT_HANDLER_START | PHP_OUTPUT_HANDLER_END)));
var_dump(bin2hex(mb_output_handler("日\xe6", PHP_OUTPUT_HANDLER_START)));
var_dump(bin2hex(mb_output_handler("\x9c本", PHP_OUTPUT_HANDLER_END)));
var_dump(headers_list());
ini_set("iconv.output_encoding", "ISO-8859-1//TRANSLIT");
ob_start("ob_iconv_handler");
echo "café\n";
ob_end_flush();
var_dump(headers_list());
ob_start("ob_iconv_handler");
echo "café 2\n";
ob_end_flush();
ob_start("mb_output_handler");
echo "x\n";
ob_end_flush();
ob_start("ob_iconv_handler");
ob_start("mb_output_handler");
echo "y\n";
ob_end_flush();
ob_end_flush();
print_r(ob_list_handlers());
--EXPECTF--
���{��e�L�X�g

���{��e�L�X�g
日本語
string(6) "日本"

Warning: Cannot modify header information - headers already sent by (output started at %s:5) in %s on line 18
string(8) "c6fccbdc"

Warning: Cannot modify header information - headers already sent by (output started at %s:5) in %s on line 19
string(6) "c6fc3f"
string(6) "3fcbdc"
array(0) {
}

Deprecated: ini_set(): Use of iconv.output_encoding is deprecated in %s on line 22
café
array(0) {
}
café 2
x
y
Array
(
)
