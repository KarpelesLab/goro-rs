--TEST--
openssl_pkey_derive(): mismatching domain parameters and unsupported key type pairs report the provider errors
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
$p1024 = hex2bin("ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a637ed6b0bff5cb6f406b7edee386bfb5a899fa5ae9f24117c4b1fe649286651ece65381ffffffffffffffff");
$p768 = hex2bin("ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a63a3620ffffffffffffffff");
$d1 = openssl_pkey_new(["dh" => ["p" => $p1024, "g" => "\x02", "priv_key" => str_repeat("\x03", 32)]]);
$d2 = openssl_pkey_new(["dh" => ["p" => $p768, "g" => "\x02", "priv_key" => str_repeat("\x05", 32)]]);
$a = openssl_pkey_new(["ec" => ["curve_name" => "prime256v1", "d" => str_repeat("\x11", 32)]]);
$b = openssl_pkey_new(["ec" => ["curve_name" => "secp384r1", "d" => str_repeat("\x22", 48)]]);
$x = openssl_pkey_new(["x25519" => ["priv_key" => str_repeat("\x01", 32)]]);
errs();
echo "dh different p: ", var_export(openssl_pkey_derive(openssl_pkey_get_details($d2)['key'], $d1), true), errs(), "\n";
echo "ec different curve: ", var_export(openssl_pkey_derive(openssl_pkey_get_details($b)['key'], $a), true), errs(), "\n";
echo "ec vs dh: ", var_export(openssl_pkey_derive(openssl_pkey_get_details($d2)['key'], $a), true), errs(), "\n";
echo "ec vs x25519: ", var_export(openssl_pkey_derive(openssl_pkey_get_details($x)['key'], $a), true), errs(), "\n";
?>
--EXPECT--
dh different p: false [error:1C8000CB:Provider routines::mismatching domain parameters]
ec different curve: false [error:1C8000CB:Provider routines::mismatching domain parameters]
ec vs dh: false [error:03000096:digital envelope routines::operation not supported for this keytype]
ec vs x25519: false [error:03000096:digital envelope routines::operation not supported for this keytype]
