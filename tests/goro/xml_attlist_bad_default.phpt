--TEST--
A missing attribute default value does not end the ATTLIST
--FILE--
<?php
// a missing default value does not end the ATTLIST declaration
libxml_use_internal_errors(true);
foreach (['<!DOCTYPE r [<!ATTLIST r a CDATA #IMPLD>]><r/>', '<!DOCTYPE r [<!ATTLIST r a CDATA #FIXED >]><r/>', '<!DOCTYPE r [<!ATTLIST r a CDATA #FIXED b CDATA "x">]><r/>', '<!DOCTYPE r [<!ATTLIST r a CDATA x>]><r/>', '<!DOCTYPE r [<!ATTLIST r a CDATA #FIXED"v">]><r/>'] as $s) {
    foreach ([0, LIBXML_RECOVER] as $o) {
        $d = new DOMDocument;
        var_dump($d->loadXML($s, $o));
        foreach (libxml_get_errors() as $e) echo $e->level, " ", $e->code, " ", $e->line, ":", $e->column, " ", $e->message;
        libxml_clear_errors();
        echo $d->saveXML();
    }
}
--EXPECT--
bool(false)
3 39 1:34 AttValue: " or ' expected
3 39 1:34 Attribute default value declaration error
3 65 1:34 Space required after the attribute default value
3 118 1:34 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 39 1:34 AttValue: " or ' expected
3 39 1:34 Attribute default value declaration error
3 65 1:34 Space required after the attribute default value
3 118 1:34 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
bool(false)
3 39 1:41 AttValue: " or ' expected
3 39 1:41 Attribute default value declaration error
<?xml version="1.0"?>
bool(true)
3 39 1:41 AttValue: " or ' expected
3 39 1:41 Attribute default value declaration error
<?xml version="1.0"?>
<!DOCTYPE r [
<!ATTLIST r a CDATA #FIXED>
]>
<r/>
bool(false)
3 39 1:41 AttValue: " or ' expected
3 39 1:41 Attribute default value declaration error
3 65 1:41 Space required after the attribute default value
3 118 1:41 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 39 1:41 AttValue: " or ' expected
3 39 1:41 Attribute default value declaration error
3 65 1:41 Space required after the attribute default value
3 118 1:41 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
bool(false)
3 39 1:34 AttValue: " or ' expected
3 39 1:34 Attribute default value declaration error
3 65 1:34 Space required after the attribute default value
3 118 1:34 Content error in the internal subset
<?xml version="1.0"?>
bool(true)
3 39 1:34 AttValue: " or ' expected
3 39 1:34 Attribute default value declaration error
3 65 1:34 Space required after the attribute default value
3 118 1:34 Content error in the internal subset
<?xml version="1.0"?>
<!DOCTYPE r>
bool(false)
3 65 1:40 Space required after '#FIXED'
<?xml version="1.0"?>
bool(true)
3 65 1:40 Space required after '#FIXED'
<?xml version="1.0"?>
<!DOCTYPE r [
<!ATTLIST r a CDATA #FIXED "v">
]>
<r/>
