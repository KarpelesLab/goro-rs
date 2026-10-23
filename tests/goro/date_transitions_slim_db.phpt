--TEST--
getTransitions() uses PHP's slim timezone database
--FILE--
<?php
foreach (['Europe/London', 'America/Matamoros', 'Europe/Kyiv', 'Europe/Riga', 'Europe/Jersey', 'America/Nuuk', 'America/Tijuana'] as $id) {
    echo $id, " ", count((new DateTimeZone($id))->getTransitions(-2**31, 2**31)), "\n";
}
foreach ((new DateTimeZone('Europe/London'))->getTransitions(814000000, 830000000) as $t) {
    echo $t['ts'], " ", $t['time'], " ", $t['offset'], " ", $t['abbr'], "\n";
}
--EXPECT--
Europe/London 243
America/Matamoros 89
Europe/Kyiv 122
Europe/Riga 127
Europe/Jersey 242
America/Nuuk 118
America/Tijuana 189
814000000 1995-10-18T07:06:40+00:00 3600 BST
814323600 1995-10-22T01:00:00+00:00 0 GMT
820454400 1996-01-01T00:00:00+00:00 0 GMT
828234000 1996-03-31T01:00:00+00:00 3600 BST
