--TEST--
Comments: libxml2's ASCII fast path for '--' errors
--FILE--
<?php libxml_use_internal_errors(true); foreach (["<r><!--- x --></r>", "<r><!--a---b--></r>", "<r><!--a----b--></r>", "<r><!------></r>", "<r><!-- a --->", "<r><!--a--></r>", "<r><!---></r>", "<r><!--a\r--b--></r>", "<r><!--\x01--></r>", "<r><!--a--", "<r><!--a-", "<r><!--abc"] as $s) { $d = new DOMDocument; $d->loadXML($s, LIBXML_RECOVER); foreach (libxml_get_errors() as $e) echo $e->code, " ", $e->line, ":", $e->column, " ", json_encode($e->message), "\n"; libxml_clear_errors(); echo $d->saveXML(); }
 libxml_use_internal_errors(true); foreach (["<r><!-- a -- b --></r>", "<r><!--a--b--></r>", "<r><!-- x\n -- b --></r>", "<r><!--\xc3\xa9--b--></r>", "<r><!-- abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz -- y --></r>"] as $s) { $d = new DOMDocument; $d->loadXML($s); foreach (libxml_get_errors() as $e) echo $e->code, " ", $e->line, ":", $e->column, " ", $e->message; libxml_clear_errors(); }
--EXPECT--
<?xml version="1.0"?>
<r><!--- x --></r>
80 1:9 "Double hyphen within comment: <!--a\n"
<?xml version="1.0"?>
<r><!--a---b--></r>
80 1:9 "Double hyphen within comment: <!--a\n"
80 1:11 "Double hyphen within comment: <!--a--\n"
<?xml version="1.0"?>
<r><!--a----b--></r>
80 1:8 "Double hyphen within comment\n"
<?xml version="1.0"?>
<r><!------></r>
80 1:11 "Double hyphen within comment: <!-- a \n"
45 1:15 "Comment not terminated\n"
<?xml version="1.0"?>
<r/>
<?xml version="1.0"?>
<r><!--a--></r>
45 1:14 "Comment not terminated\n"
<?xml version="1.0"?>
<r/>
80 1:12 "Comment must not contain '--' (double-hyphen)\n"
<?xml version="1.0"?>
<r><!--a
--b--></r>
9 1:8 "xmlParseComment: invalid xmlChar value 1\n"
9 1:8 "PCDATA invalid Char value 1\n"
<?xml version="1.0"?>
<r>--&gt;</r>
80 1:9 "Double hyphen within comment: <!--a\n"
45 1:11 "Comment not terminated\n"
<?xml version="1.0"?>
<r/>
45 1:10 "Comment not terminated\n"
<?xml version="1.0"?>
<r/>
45 1:11 "Comment not terminated\n"
<?xml version="1.0"?>
<r/>
80 1:11 Double hyphen within comment: <!-- a 
80 1:9 Double hyphen within comment: <!--a
80 2:2 Double hyphen within comment: <!-- x
 
80 1:11 Comment must not contain '--' (double-hyphen)
80 1:88 Double hyphen within comment: <!-- abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvw
