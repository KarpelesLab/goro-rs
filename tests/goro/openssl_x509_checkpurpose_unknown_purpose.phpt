--TEST--
openssl_x509_checkpurpose(): an unknown purpose id is reported but the chain is still verified
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
$ca = "-----BEGIN CERTIFICATE-----
MIIC4jCCAkugAwIBAgIIEjRWeJCrze8wDQYJKoZIhvcNAQELBQAwgYAxCzAJBgNV
BAYTAkZSMQ4wDAYDVQQIDAVQYXJpczEOMAwGA1UEBwwFUGFyaXMxETAPBgNVBAoM
CFRlc3QgT3JnMQ0wCwYDVQQLDARVbml0MRAwDgYDVQQDDAdUZXN0IENBMR0wGwYJ
KoZIhvcNAQkBFg5jYUBleGFtcGxlLmNvbTAeFw0yNjEwMjcxNTQ2NDJaFw0zNjEw
MjQxNTQ2NDJaMIGAMQswCQYDVQQGEwJGUjEOMAwGA1UECAwFUGFyaXMxDjAMBgNV
BAcMBVBhcmlzMREwDwYDVQQKDAhUZXN0IE9yZzENMAsGA1UECwwEVW5pdDEQMA4G
A1UEAwwHVGVzdCBDQTEdMBsGCSqGSIb3DQEJARYOY2FAZXhhbXBsZS5jb20wgZ8w
DQYJKoZIhvcNAQEBBQADgY0AMIGJAoGBAKiEw2Wes3ZCqhTefhFnxn84MCqLcRGX
e1mHg8YGO/u6ZjYz6nyhOkimbU5Ua6YvnPhoVENnWIfGcJaaOtjL3IOIyi4Uc4Aw
uoVFLkVGPl5bqXk9ljjzIiAiiQUnLC1DZy+/qKTE1pc2U+LP1tw4lqtbmU110gGN
wdaZZpvhqHYHAgMBAAGjYzBhMB8GA1UdIwQYMBaAFCNceaLRwgdNrUJ2FtiEuNlx
Z135MA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgEGMB0GA1UdDgQWBBQj
XHmi0cIHTa1CdhbYhLjZcWdd+TANBgkqhkiG9w0BAQsFAAOBgQBt21jRoA2Lxu6n
XvHioJxnzriq85wD019PN52bBiFU5sQeC1lJWsX9thJVez8AIHSwGl1jGKW+VMg4
6Q2r6QSELuPuWIAm3Oz3dQnLCx5NN/WH3k9AYdUBy4mQjQ4dP4aqzyGB5S2xMlHx
Y/X77czvaaToU03jRqU8HRioy7zRiA==
-----END CERTIFICATE-----";
$leaf = "-----BEGIN CERTIFICATE-----
MIIDkDCCAvmgAwIBAgICAP8wDQYJKoZIhvcNAQELBQAwgYAxCzAJBgNVBAYTAkZS
MQ4wDAYDVQQIDAVQYXJpczEOMAwGA1UEBwwFUGFyaXMxETAPBgNVBAoMCFRlc3Qg
T3JnMQ0wCwYDVQQLDARVbml0MRAwDgYDVQQDDAdUZXN0IENBMR0wGwYJKoZIhvcN
AQkBFg5jYUBleGFtcGxlLmNvbTAeFw0yNjEwMjcxNTQ2NDJaFw0yNzEwMjcxNTQ2
NDJaMDsxGTAXBgNVBAMMEGxlYWYuZXhhbXBsZS5jb20xETAPBgNVBAoMCExlYWYg
SW5jMQswCQYDVQQFEwI0MjBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABMqXx9bx
kgQNgvQ2MFI3vCmwju40Y4Xy1LYbapwAbLIrWfys1tOZPAKeIBpDeh14hXy2gvTG
mMkb7I1h0KWaecqjggGiMIIBnjBdBgNVHREEVjBUghBsZWFmLmV4YW1wbGUuY29t
ghIqLmxlYWYuZXhhbXBsZS5jb22HBAoAAAGHEAAAAAAAAAAAAAAAAAAAAAGBBWFA
Yi5jhg1odHRwczovL3gueS96MB0GA1UdJQQWMBQGCCsGAQUFBwMBBggrBgEFBQcD
AjALBgNVHQ8EBAMCA4gwCQYDVR0TBAIwADAdBgNVHQ4EFgQUZExDdVSqHMensOXS
bbEfnCS2gS8wHwYDVR0jBBgwFoAUI1x5otHCB02tQnYW2IS42XFnXfkwGgYDVR0g
BBMwETAFBgMqAwQwCAYGZ4EMAQIBMC4GA1UdHwQnMCUwI6AhoB+GHWh0dHA6Ly9j
cmwuZXhhbXBsZS5jb20vY2EuY3JsMF0GCCsGAQUFBwEBBFEwTzAjBggrBgEFBQcw
AYYXaHR0cDovL29jc3AuZXhhbXBsZS5jb20wKAYIKwYBBQUHMAKGHGh0dHA6Ly9j
YS5leGFtcGxlLmNvbS9jYS5jcnQwGwYJYIZIAYb4QgENBA4WDFRlc3QgY29tbWVu
dDANBgkqhkiG9w0BAQsFAAOBgQAJKtcaBavggxv+dfyg6T3hVnUs9Aoaui7e9jlG
BMlEGjWjDleook/vsPI76TiT1lKMmsaCP3dA01UWNPnol0RcHgvsTDWHv71lopj6
YauHMl3MavCXIxXSnI41dzi0EgauJbnReAbRv6px7Dhti7n8cNB+BtLW7YdOO5Wi
i+XIdA==
-----END CERTIFICATE-----";
$dir = sys_get_temp_dir() . "/goro_purpose_" . getmypid();
@mkdir($dir);
file_put_contents("$dir/ca.pem", $ca);
// errors from loading the (absent) default verify paths are environment specific
$clean = function () { $o = []; while ($e = openssl_error_string()) if (str_contains($e, 'purpose')) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; };
foreach ([0, 1, 2, 6, 10, 11, 99] as $p) {
  echo "leaf $p: ", var_export(openssl_x509_checkpurpose($leaf, $p, ["$dir/ca.pem"]), true), $clean(), "\n";
  echo "ca $p: ", var_export(openssl_x509_checkpurpose($ca, $p, ["$dir/ca.pem"]), true), $clean(), "\n";
}
unlink("$dir/ca.pem");
rmdir($dir);
?>
--EXPECT--
leaf 0: true
ca 0: true
leaf 1: true
ca 1: false
leaf 2: true
ca 2: false
leaf 6: false
ca 6: true
leaf 10: false
ca 10: false
leaf 11: true [error:05800079:x509 certificate routines::unknown purpose id]
ca 11: true [error:05800079:x509 certificate routines::unknown purpose id]
leaf 99: true [error:05800079:x509 certificate routines::unknown purpose id]
ca 99: true [error:05800079:x509 certificate routines::unknown purpose id]
