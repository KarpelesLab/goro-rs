--TEST--
Text before a misplaced ']]>' is kept in recover mode
--FILE--
<?php libxml_use_internal_errors(true); foreach (["<r><![CDAT x]]></r>", "<r><![CDATA x]]></r>", "<r><![ x</r>", "<r><!DOCTYPE x></r>", "<r><!x></r>", "<r><!--x--></r>"] as $s) { $d = new DOMDocument; $d->loadXML($s, LIBXML_RECOVER); foreach (libxml_get_errors() as $e) echo $e->code, " ", $e->line, ":", $e->column, " ", json_encode($e->message), "\n"; libxml_clear_errors(); echo $d->saveXML(); }
--EXPECT--
68 1:5 "StartTag: invalid element name\n"
62 1:13 "Sequence ']]>' not allowed in content\n"
<?xml version="1.0"?>
<r>![CDAT x]]&gt;</r>
68 1:5 "StartTag: invalid element name\n"
62 1:14 "Sequence ']]>' not allowed in content\n"
<?xml version="1.0"?>
<r>![CDATA x]]&gt;</r>
68 1:5 "StartTag: invalid element name\n"
<?xml version="1.0"?>
<r>![ x</r>
68 1:5 "StartTag: invalid element name\n"
<?xml version="1.0"?>
<r>!DOCTYPE x&gt;</r>
68 1:5 "StartTag: invalid element name\n"
<?xml version="1.0"?>
<r>!x&gt;</r>
<?xml version="1.0"?>
<r><!--x--></r>
