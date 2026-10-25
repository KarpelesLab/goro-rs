--TEST--
Malformed internal subset markup declarations always make progress
--FILE--
<?php
// a malformed markup declaration in the internal subset used to loop forever
libxml_use_internal_errors(true);
foreach (['<!DOCTYPE r [<!ELEMENT r ANY <!AT', '<!DOCTYPE r [<!-x comment -->]><r/>', '<!DOCTYPE r [<!-', '<!DOCTYPE r [<!AT x>]><r/>', '<!DOCTYPE r [<!ELEMEN x>]><r/>', '<!DOCTYPE r [<!NOTA x>]><r/>', '<!DOCTYPE r [<!ENTIT x>]><r/>', '<!DOCTYPE r [<!Q>]><r/>', '<!DOCTYPE r [<?>]><r/>'] as $s) {
    foreach ([0, LIBXML_RECOVER] as $o) {
        $d = new DOMDocument;
        var_dump($d->loadXML($s, $o));
        foreach (libxml_get_errors() as $e) echo $e->level, " ", $e->code, " ", $e->line, ":", $e->column, " ", $e->message;
        libxml_clear_errors();
        echo $d->saveXML();
    }
    $p = xml_parser_create();
    var_dump(xml_parse($p, $s, true), xml_get_error_code($p));
    $r = XMLReader::fromString($s);
    while (@$r->read());
}
--EXPECT--
bool(false)
3 73 1:30 expected '>'
3 118 1:32 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 73 1:30 expected '>'
3 118 1:32 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
int(0)
int(118)
bool(false)
3 73 1:30 expected '>'
3 118 1:32 Content error in the internal subset
3 73 1:30 expected '>'
3 118 1:32 Content error in the internal subset
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
int(0)
int(118)
bool(false)
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
int(0)
int(118)
bool(false)
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
int(0)
int(118)
bool(false)
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
int(0)
int(118)
bool(false)
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
int(0)
int(118)
bool(false)
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
int(0)
int(118)
bool(false)
3 118 1:16 Content error in the internal subset
3 118 1:16 Content error in the internal subset
3 118 1:14 Content error in the internal subset
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 118 1:14 Content error in the internal subset
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
int(0)
int(118)
bool(false)
3 118 1:14 Content error in the internal subset
3 118 1:16 Content error in the internal subset
3 118 1:14 Content error in the internal subset
3 118 1:16 Content error in the internal subset
3 46 1:16 xmlParsePI : no target name
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 46 1:16 xmlParsePI : no target name
3 118 1:16 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
int(0)
int(118)
