--TEST--
openssl_spki_new(): signing errors for keys/digests the providers refuse
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
$keys = [
  'ec' => "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgMk8mx9hEpopQi5cC
ItZ5ekqkVukjUcrHeZZANRotYh6hRANCAATKl8fW8ZIEDYL0NjBSN7wpsI7uNGOF
8tS2G2qcAGyyK1n8rNbTmTwCniAaQ3odeIV8toL0xpjJG+yNYdClmnnK
-----END PRIVATE KEY-----",
  'dsa' => "-----BEGIN PRIVATE KEY-----
MIIBWgIBADCCATMGByqGSM44BAEwggEmAoGBAIqdCNrK7agQWR6aE80gVqjb02w/
WdY9qJeyn0PU4qRsu1VlBu1rGv0VcoVORyURonBJQnfsTx75TjBSoJOCc0/yklrl
QshMFsIPhWcPQ7Bu71D2wBpQ8djElHjr2KF+XUeFDjhpMO3csTIQG48/rgysLpN4
rQO2OunsHTpTpEU5Ah0A8LhV4aICwaXmmYoSFOEYuu9potu6vm1TjpZlBQKBgCog
mczNlcF6gQ5ojb6y7diDt7fmbzMV7YzPMg0Ze1wIeWrV0Pr9j7F7YpF8VMkm6M6z
1TPIN3lWELfGDjblw7ODFLncYrIGOVaALjBIFRMNVAPBfirx4plal44/l9s3CC2v
O/htRw57rHAaKozDqGgv7rQgW9r9JLjzcQBeOIw2BB4CHGVhqwZILkLaOfkacSHi
mdxEH11YOMdVzaC92F4=
-----END PRIVATE KEY-----",
  'x25519' => "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VuBCIEIDAbr1/eb+xWo/m6HR36yaMhADS/ihxNbB0tlJZ7Enpq
-----END PRIVATE KEY-----",
  'dh' => "-----BEGIN PRIVATE KEY-----
MIGcAgEAMFMGCSqGSIb3DQEDATBGAkEA5hgVovVu4uB/ZjtPTTqZepWaVno/+RBm
nubLlNkj8ZkTaJMG3oti8MZCgj1MBZ6SpWFBljb1ZFhFnefURZRXXwIBAgRCAkA7
dtGAPLThsLxA7ds5QI9RXfAZm/wOySljLg4CESsKjhlGDNb7PCurAhTKKyASaxaN
oDRX6JfqUyDzASBmA8WW
-----END PRIVATE KEY-----",
  'ed25519' => "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEII6t0unMenHqGrqYAtQceX/dl4qIMD726wG+u/wkfq/d
-----END PRIVATE KEY-----",
];
foreach ($keys as $n => $pem) {
  $k = openssl_pkey_get_private($pem);
  foreach ([OPENSSL_ALGO_SHA256, OPENSSL_ALGO_MD5] as $a) {
    $s = @openssl_spki_new($k, "challenge", $a);
    echo "$n/$a: ", $s === false ? "false" : (openssl_spki_verify(preg_replace("/^SPKAC=/", "", $s)) ? "verified" : "unverified"), errs(), "\n";
  }
}
?>
--EXPECT--
ec/7: verified
ec/2: false [error:078C0102:common libcrypto routines::passed a null parameter]
dsa/7: verified
dsa/2: false [error:1C8000AE:Provider routines::digest not allowed]
x25519/7: false [error:03000096:digital envelope routines::operation not supported for this keytype]
x25519/2: false [error:03000096:digital envelope routines::operation not supported for this keytype]
dh/7: false [error:03000096:digital envelope routines::operation not supported for this keytype]
dh/2: false [error:03000096:digital envelope routines::operation not supported for this keytype]
ed25519/7: false [error:1C80007A:Provider routines::invalid digest]
ed25519/2: false [error:1C80007A:Provider routines::invalid digest]
