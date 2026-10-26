--TEST--
password_hash() argon2 as ext/standard's libargon2 provider: any thread count, ASCII salt, its errors and constants
--FILE--
<?php
var_dump(PASSWORD_ARGON2_PROVIDER, PASSWORD_ARGON2_DEFAULT_THREADS);
$h = password_hash('pw', PASSWORD_ARGON2ID, ['memory_cost' => 64, 'time_cost' => 2, 'threads' => 2]);
var_dump(substr($h, 0, 28), password_verify('pw', $h), password_verify('px', $h));
// the salt is 16 characters of the crypt alphabet
$salt = base64_decode(explode('$', $h)[4]);
var_dump(strlen($salt), (bool)preg_match('~^[./A-Za-z0-9]{16}$~', $salt));
var_dump(password_get_info($h)['options']);
var_dump(password_needs_rehash($h, PASSWORD_ARGON2ID, ['memory_cost' => 64, 'time_cost' => 2, 'threads' => 2]), password_needs_rehash($h, PASSWORD_ARGON2ID));
// verifies a hash made by PHP 8.5 with p=4
var_dump(password_verify('pw', '$argon2i$v=19$m=32,t=1,p=4$OGp4YjJFWHRPTDZxTVV6SQ$okkaWwSsWbla54yxzzPXz4QRUN8AIj57LfYz4LY5lEU'));
foreach ([['threads' => 0], ['threads' => -1], ['memory_cost' => 7], ['time_cost' => 0], ['memory_cost' => 8, 'threads' => 2]] as $o) {
    try { password_hash('pw', PASSWORD_ARGON2I, $o); } catch (ValueError $e) { echo json_encode($o), ': ', $e->getMessage(), "\n"; }
}
echo implode(',', array_slice(array_keys(get_defined_constants(true)['standard']), array_search('PASSWORD_BCRYPT_DEFAULT_COST', array_keys(get_defined_constants(true)['standard'])), 7)), "\n";
?>
--EXPECT--
string(8) "standard"
int(1)
string(28) "$argon2id$v=19$m=64,t=2,p=2$"
bool(true)
bool(false)
int(16)
bool(true)
array(3) {
  ["memory_cost"]=>
  int(64)
  ["time_cost"]=>
  int(2)
  ["threads"]=>
  int(2)
}
bool(false)
bool(true)
bool(true)
{"threads":0}: Invalid number of threads
{"threads":-1}: Invalid number of threads
{"memory_cost":7}: Memory cost is outside of allowed memory range
{"time_cost":0}: Time cost is outside of allowed time range
{"memory_cost":8,"threads":2}: Memory cost is too small
PASSWORD_BCRYPT_DEFAULT_COST,PASSWORD_ARGON2I,PASSWORD_ARGON2ID,PASSWORD_ARGON2_PROVIDER,PASSWORD_ARGON2_DEFAULT_MEMORY_COST,PASSWORD_ARGON2_DEFAULT_TIME_COST,PASSWORD_ARGON2_DEFAULT_THREADS
