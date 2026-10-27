--TEST--
openssl_pkey_new() EC key checks leave their errors; '+' in name values is escaped
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) if (!str_contains($e, 'such file') && !str_contains($e, 'configuration file') && !str_contains($e, 'Cannot open file')) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
$cnf = sys_get_temp_dir() . "/goro_ecinit_" . getmypid() . ".cnf";
file_put_contents($cnf, "HOME = .\n[ req ]\ndistinguished_name = dn\n[ dn ]\n");
$cfg = ['config' => $cnf];
$ec = openssl_pkey_get_details(openssl_pkey_get_private("-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgMk8mx9hEpopQi5cC
ItZ5ekqkVukjUcrHeZZANRotYh6hRANCAATKl8fW8ZIEDYL0NjBSN7wpsI7uNGOF
8tS2G2qcAGyyK1n8rNbTmTwCniAaQ3odeIV8toL0xpjJG+yNYdClmnnK
-----END PRIVATE KEY-----"))['ec'];
// EVP_PKEY_check() then EVP_PKEY_public_check_quick() leave their errors
$k = openssl_pkey_new($cfg + ['ec' => ['curve_name' => 'prime256v1', 'x' => $ec['x'], 'y' => $ec['y']]]);
echo "public x/y: ", openssl_pkey_get_details($k)['type'], errs(), "\n";
$k = openssl_pkey_new($cfg + ['ec' => ['curve_name' => 'prime256v1', 'd' => $ec['d']]]);
echo "private d: ", openssl_pkey_get_details($k)['type'], errs(), "\n";
$k = openssl_pkey_new($cfg + ['ec' => ['curve_name' => 'prime256v1']]);
echo "generated: ", isset(openssl_pkey_get_details($k)['ec']['d']) ? "private" : "?", errs(), "\n";
// '/' and '+' inside values are escaped in X509_NAME_oneline()
$key = openssl_pkey_get_private("-----BEGIN PRIVATE KEY-----
MIICdgIBADANBgkqhkiG9w0BAQEFAASCAmAwggJcAgEAAoGBAKiEw2Wes3ZCqhTe
fhFnxn84MCqLcRGXe1mHg8YGO/u6ZjYz6nyhOkimbU5Ua6YvnPhoVENnWIfGcJaa
OtjL3IOIyi4Uc4AwuoVFLkVGPl5bqXk9ljjzIiAiiQUnLC1DZy+/qKTE1pc2U+LP
1tw4lqtbmU110gGNwdaZZpvhqHYHAgMBAAECgYBTlj7BWf2ULC7m2RFZGstXcQxb
Hf1Ah420c8ZF4/O+wc9PFWM63FHnlT1ke/VFYhqPEFizckB0YFaaLPgBbuT6PtzD
M/e2rig2uvowb9DMc0KJiQWWHBLl8/LpS6XtElraKTU9K5dT5d6s3/Xji2nV9G9W
JRPoVuZSlazW0YjYGQJBAN0Y/AOMCpEWwB/iLkcY0jtm2fsgZaeIgqpwUBy6Myh3
YMdVaDF19Adk1QFwjbRcqVBnVKwsfI2lG9BjyYxCqOsCQQDDHvO8WTZGurAaATmD
hP42xo6xkCCqCirq0uo9zkLWrslawSPrcTX/6AP3D7DkNvkedKmdYSbCVW9xY04U
+yBVAkAyUjFpjUIKa6cmmo/11oxiwLH0GheI3eT/bbiOEUL7ftAkDig8uZ/k3uor
/bYCyuQuTJZU6Xsfseh2sX950OIpAkACIDIzmR04RAxrHBxOTIKggiVH1fQlJhs1
l7qplbewsxLKhFQp2n/mG9b/OBKLSfYI2OiIKGjNsTSXnadaHypJAkEA0aOEAY1w
5UzCVeAw5yo/lF2cCTeDoaTdMiACUywZsZnRB2gG6yAiumQPWrBoR8O3+8t+As2b
AnR3SEKhEVgvGg==
-----END PRIVATE KEY-----");
$csr = openssl_csr_new(['CN' => 'x', 'O' => 'a+b,c;d<e>f"g\\h/i#j=k ', 'OU' => ' lead#'], $key, $cfg);
$c = openssl_csr_sign($csr, null, $key, 1, $cfg);
echo openssl_x509_parse($c)['name'], "\n";
openssl_x509_export($c, $t, false); preg_match('/Subject: (.*)/', $t, $m); echo $m[1], "\n";
unlink($cnf);
?>
--EXPECT--
public x/y: 3 [error:080C0102:elliptic curve routines::passed a null parameter]
private d: 3
generated: private [error:0300009A:digital envelope routines::no key set | error:0300009A:digital envelope routines::no key set]
/CN=x/O=a\+b,c;d<e>f"g\h\/i#j=k /OU= lead#
CN=x, O=a\+b,c;d<e>f"g\h\/i#j=k , OU= lead#
