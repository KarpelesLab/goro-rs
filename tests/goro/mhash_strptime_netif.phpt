--TEST--
mhash*() emulation, strptime(), net_get_interfaces()
--FILE--
<?php
var_dump(bin2hex(mhash(MHASH_MD5, 'abc')));
var_dump(bin2hex(@mhash(MHASH_SHA1, 'abc', 'key')));
var_dump(@mhash_count(), @mhash_get_hash_name(MHASH_SHA256), @mhash_get_block_size(MHASH_SHA256));
var_dump(@mhash_get_hash_name(4), @mhash_get_block_size(999), @mhash(26, 'x'));
var_dump(bin2hex(@mhash_keygen_s2k(MHASH_MD5, 'pw', 'saltsalt', 20)));
try { @mhash_keygen_s2k(MHASH_MD5, 'pw', 's', 0); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
var_dump(strptime('2024-03-05 10:11:12 rest', '%Y-%m-%d %H:%M:%S'));
var_dump(@strptime('garbage', '%Y'));
$ifs = net_get_interfaces();
var_dump(is_array($ifs));
foreach ($ifs as $name => $if) {
    var_dump(is_string($name), is_bool($if['up']), is_array($if['unicast']), is_int($if['unicast'][0]['flags']));
    break;
}
--EXPECTF--

Deprecated: Constant MHASH_MD5 is deprecated since 8.5, as the mhash*() functions were deprecated in %s on line 2

Deprecated: Function mhash() is deprecated since 8.1 in %s on line 2
string(32) "900150983cd24fb0d6963f7d28e17f72"
string(40) "4fd0b215276ef12f2b3e4c8ecac2811498b656fc"
int(41)
string(6) "SHA256"
int(32)
bool(false)
bool(false)
bool(false)
string(40) "1f9f93a0f5d6305e0b33ef635322861aabf31b8a"
mhash_keygen_s2k(): Argument #4 ($length) must be a greater than 0

Deprecated: Function strptime() is deprecated since 8.2, use date_parse_from_format() (for locale-independent parsing), or IntlDateFormatter::parse() (for locale-dependent parsing) instead in %s on line 8
array(9) {
  ["tm_sec"]=>
  int(12)
  ["tm_min"]=>
  int(11)
  ["tm_hour"]=>
  int(10)
  ["tm_mday"]=>
  int(5)
  ["tm_mon"]=>
  int(2)
  ["tm_year"]=>
  int(124)
  ["tm_wday"]=>
  int(2)
  ["tm_yday"]=>
  int(64)
  ["unparsed"]=>
  string(5) " rest"
}
bool(false)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
