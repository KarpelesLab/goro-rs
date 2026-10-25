--TEST--
ob_iconv_handler: fails once output was sent
--FILE--
<?php
@ini_set("iconv.output_encoding", "ISO-8859-1");
ob_start("ob_iconv_handler");
echo "caf\u{e9}\n";
ob_end_flush();
// output was sent: the handler fails and the output passes through
ob_start("ob_iconv_handler");
echo "caf\u{e9}\n";
ob_end_flush();
--EXPECT--
café
cafÃ©
