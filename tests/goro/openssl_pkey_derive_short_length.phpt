--TEST--
openssl_pkey_derive(): a short key_length fails for X25519/DH (only ECDH truncates); EdDSA keys cannot derive
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
$x = openssl_pkey_get_private("-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VuBCIEIDAbr1/eb+xWo/m6HR36yaMhADS/ihxNbB0tlJZ7Enpq
-----END PRIVATE KEY-----");
$xpub = openssl_pkey_get_public("-----BEGIN PUBLIC KEY-----
MCowBQYDK2VuAyEABU1fRj9DFEQ+Urlq+DOF1dd0l+Gjldu39f+Pe8y1w0g=
-----END PUBLIC KEY-----");
$dh = openssl_pkey_get_private("-----BEGIN PRIVATE KEY-----
MIGcAgEAMFMGCSqGSIb3DQEDATBGAkEA5hgVovVu4uB/ZjtPTTqZepWaVno/+RBm
nubLlNkj8ZkTaJMG3oti8MZCgj1MBZ6SpWFBljb1ZFhFnefURZRXXwIBAgRCAkA7
dtGAPLThsLxA7ds5QI9RXfAZm/wOySljLg4CESsKjhlGDNb7PCurAhTKKyASaxaN
oDRX6JfqUyDzASBmA8WW
-----END PRIVATE KEY-----");
$dhpub = openssl_pkey_get_public("-----BEGIN PUBLIC KEY-----
MIGaMFMGCSqGSIb3DQEDATBGAkEA5hgVovVu4uB/ZjtPTTqZepWaVno/+RBmnubL
lNkj8ZkTaJMG3oti8MZCgj1MBZ6SpWFBljb1ZFhFnefURZRXXwIBAgNDAAJAYBHz
0qKR3Bmvj7K2KLw6n/cIsk44d4PPuO733sIIaVvDwsBbe5jewGOpf99SZ3HzICF9
t4DkUe+OKC4WlNGKDw==
-----END PUBLIC KEY-----");
$ed = openssl_pkey_get_private("-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEII6t0unMenHqGrqYAtQceX/dl4qIMD726wG+u/wkfq/d
-----END PRIVATE KEY-----");
$edpub = openssl_pkey_get_public("-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEASlPNBaYYLDnOzndgDFgxExFp+ntIAs8JtvqKHZVQ0Ig=
-----END PUBLIC KEY-----");
echo "x25519: ", strlen(openssl_pkey_derive($xpub, $x)), errs(), "\n";
echo "x25519 len 8: ", var_export(@openssl_pkey_derive($xpub, $x, 8), true), errs(), "\n";
echo "x25519 len 100: ", strlen(@openssl_pkey_derive($xpub, $x, 100)), errs(), "\n";
echo "dh: ", strlen(openssl_pkey_derive($dhpub, $dh)), errs(), "\n";
echo "dh len 8: ", var_export(@openssl_pkey_derive($dhpub, $dh, 8), true), errs(), "\n";
echo "ed25519: ", var_export(openssl_pkey_derive($edpub, $ed), true), errs(), "\n";
?>
--EXPECT--
x25519: 32
x25519 len 8: false [error:1C80006A:Provider routines::output buffer too small]
x25519 len 100: 32
dh: 64
dh len 8: false [error:1C80006A:Provider routines::output buffer too small]
ed25519: false [error:03000096:digital envelope routines::operation not supported for this keytype]
