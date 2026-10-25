--TEST--
ini_set() of an entry no extension registers fails
--FILE--
<?php
// Entries no extension registers (or removed ones) cannot be set.
foreach (['session.hash_function', 'session.entropy_length', 'session.hash_bits_per_character', 'no.such.entry'] as $k) {
    var_dump(ini_set($k, '1'), ini_get($k));
}
var_dump(ini_get_all('session', false)['session.hash_function'] ?? 'absent');
var_dump(ini_set('session.name', 'X'), ini_get('session.name'));
--EXPECTF--
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
string(6) "absent"

Warning: ini_set(): Session ini settings cannot be changed after headers have already been sent (sent from %s on line 4) in %s on line 7
bool(false)
string(9) "PHPSESSID"
