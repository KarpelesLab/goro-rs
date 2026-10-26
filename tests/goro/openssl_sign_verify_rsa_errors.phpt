--TEST--
openssl_sign()/openssl_verify() and RSA encryption: error queue per provider, digest restrictions, PSS checks, PKCS#1 implicit rejection
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
function t($label, $f) { try { $r = $f(); echo "$label: ", is_string($r) ? bin2hex($r) : var_export($r, true); } catch (Throwable $e) { echo "$label: ", get_class($e), ": ", $e->getMessage(); } echo errs(), "\n"; }
$rsa = openssl_pkey_get_private(<<<PEM
-----BEGIN RSA PRIVATE KEY-----
MIICXAIBAAKBgQDLXp6PkCtbpV+P1gwFQWH6Ez0U83uEmS8IGnpeI8Fk8rY/vHOZ
zZZaxRCw+loyc342qCDIQheMOCNm5Fkevz06q757/oooiLR3yryYGKiKG1IZIipl
mtsC95oKrzUSKk60wuI1mbgpMUP5LKi/Tvxes5PmkUtXfimz2qgkeUcPpQIDAQAB
AoGBAMcP/dp+fsI9FFYBaVC3mASlUjOwxKWdH3kqGb8N9p4uKRAoEWtp3hNJM7ZX
x3P8sn0jgrsiXlRFGvn65/T9shp8hj+CdJKg2jKCs7S58v60TLfSvOQSIYsw9Qm9
Bsx4hKfz+d52ptuJRbv8tDxsYP3D/KjQfpX1OysiP/WBfeg9AkEA+AGT0goqjWOM
YgFtZGrefIegF31XSCQTaLIml6/2JwF+oBKjJUQFar2Rwn6qUwrsGtSPMM0Iz8ry
9uvUbs8PPwJBANHsuTVWzLf8TJNGc+xIlhvzKFkF0nJIWx4ozhlMNDQMMF/3FRSo
zvHIgUnpG9Vwa2GtjTDnD8jHtzTauAZmjBsCQCGDVQ5VAVsJ0LaNqtKe/mGlkiSa
c2j0Nws2x7BHvuOWeB35ZsJqZrD93OyDYVDHcRBPGOpnSoGJ0zs6swImSNECQHSH
0BgH4wSPDYMDrP4RHSLOzCr+zF+cQthvFll8r83kpkXfRth9DMOy5fI9cLH/Adzr
FmF7Iov2MYEpmNYUvtkCQHfW0ntkVY9xS2/VTs57F5tUkfNG2hG74pJM6vSfTNWn
R/oI5m2sDtRWQ88LCYJMEmIZhN00Ys4xOSoTs+SUakY=
-----END RSA PRIVATE KEY-----
PEM);
$rsapub = openssl_pkey_get_public(openssl_pkey_get_details($rsa)['key']);
$ec = openssl_pkey_new(["ec" => ["curve_name" => "prime256v1", "d" => str_repeat("\x05", 32)]]);
$ecpub = openssl_pkey_get_public(openssl_pkey_get_details($ec)['key']);
$d = "data";
openssl_sign($d, $sig, $rsa, "sha256");
openssl_sign($d, $pss, $rsa, "sha256", OPENSSL_PKCS1_PSS_PADDING);
openssl_sign($d, $ecsig, $ec, "sha256");
errs();
t("rsa ok", fn() => openssl_verify($d, $sig, $rsapub, "sha256"));
t("rsa mismatch", fn() => openssl_verify("x", $sig, $rsapub, "sha256"));
t("rsa short", fn() => openssl_verify($d, "garbage", $rsapub, "sha256"));
t("rsa padding", fn() => openssl_verify($d, str_repeat("z", 128), $rsapub, "sha256"));
t("pss mismatch", fn() => openssl_verify("x", $pss, $rsapub, "sha256", OPENSSL_PKCS1_PSS_PADDING));
t("pss of pkcs1", fn() => openssl_verify($d, $sig, $rsapub, "sha256", OPENSSL_PKCS1_PSS_PADDING));
t("pss short", fn() => openssl_verify($d, "x", $rsapub, "sha256", OPENSSL_PKCS1_PSS_PADDING));
t("ec mismatch", fn() => openssl_verify("x", $ecsig, $ecpub, "sha256"));
t("ec garbage", fn() => openssl_verify($d, "garbage", $ecpub, "sha256"));
t("ec trailing", fn() => openssl_verify($d, $ecsig . "x", $ecpub, "sha256"));
t("ec r=0", fn() => openssl_verify($d, "\x30\x06\x02\x01\x00\x02\x01\x01", $ecpub, "sha256"));
t("rsa shake", fn() => openssl_verify($d, $sig, $rsapub, "shake128"));
t("ec shake", fn() => openssl_sign($d, $s, $ec, "shake256"));
t("unknown md", fn() => openssl_sign($d, $s, $rsa, "nope"));
t("sha512 pkcs1 512-bit", fn() => openssl_sign($d, $s, $rsa, "sha512"));
// RSA encryption
t("implicit rejection", function() use ($rsa) { $r = openssl_private_decrypt(str_repeat("z", 128), $out, $rsa); return $r ? $out : $r; });
t("implicit rejection short", function() use ($rsa) { $r = openssl_private_decrypt("z", $out, $rsa); return $r ? $out : $r; });
t("private_encrypt empty + public_decrypt", function() use ($rsa, $rsapub) { openssl_private_encrypt("", $c, $rsa); return openssl_public_decrypt($c, $out, $rsapub); });
t("public_decrypt garbage", fn() => openssl_public_decrypt(str_repeat("z", 128), $out, $rsapub));
t("private_encrypt too long", fn() => openssl_private_encrypt(str_repeat("a", 60), $c, $rsa));
t("public_encrypt padding 99", fn() => openssl_public_encrypt("a", $c, $rsapub, 99));
t("private_encrypt padding 99", fn() => openssl_private_encrypt("a", $c, $rsa, 99));
t("private_encrypt ec", fn() => openssl_private_encrypt("a", $c, $ec));
t("oaep unknown md", fn() => openssl_public_encrypt("a", $c, $rsapub, OPENSSL_PKCS1_OAEP_PADDING, "nope"));
?>
--EXPECTF--
rsa ok: 1
rsa mismatch: 0 [error:02000068:rsa routines::bad signature | error:1C880004:Provider routines::RSA lib]
rsa short: 0 [error:02000077:rsa routines::wrong signature length | error:1C880004:Provider routines::RSA lib]
rsa padding: 0 [error:0200008A:rsa routines::invalid padding | error:02000072:rsa routines::padding check failed | error:1C880004:Provider routines::RSA lib]
pss mismatch: 0 [error:02000068:rsa routines::bad signature | error:1C880004:Provider routines::RSA lib]
pss of pkcs1: 0 [error:02000086:rsa routines::last octet invalid | error:1C880004:Provider routines::RSA lib]
pss short: 0 [error:02000085:rsa routines::first octet invalid | error:1C880004:Provider routines::RSA lib]
ec mismatch: 0 [error:030000EA:digital envelope routines::provider signature failure]
ec garbage: -1
ec trailing: -1
ec r=0: 0 [error:0800009C:elliptic curve routines::bad signature]
rsa shake: -1 [error:1C8000AE:Provider routines::digest not allowed]
ec shake: false [error:1C80007A:Provider routines::invalid digest]

Warning: openssl_sign(): Unknown digest algorithm in %s on line 42
unknown md: false [error:0308010C:digital envelope routines::unsupported]
sha512 pkcs1 512-bit: true
implicit rejection: 021fac0220cf664a7fbe4617da3d717167e5086e529f3993e5d3d73847b9f20dcfb8c8ec8f4c3701143eaef481ef2ce70088b02a84bb82ceaa7bb1d71d12707cc837e3de9ca3620adb1c7505a7b10f41216e5c4bb64b59cf569b85a752eb6007eff88bbb941b8f514a00f2e68330f7
implicit rejection short: 164d802dbdf31c051f86367808833514ebf9d652ffe42c9b4348df442adc976a81a3351d0aab27ecd7ad2c5146594f410823cd5ea7d7ac357b2d7cd7d589b9005b98f68e87df10489e4069232d86d9fa2b44ed894cd139e773eaeb1b8ea8f4cf927923793b4b71f44203dcd64939c63eed6356
private_encrypt empty + public_decrypt: false [error:1C880004:Provider routines::RSA lib | error:030000EA:digital envelope routines::provider signature failure]
public_decrypt garbage: false [error:0200008A:rsa routines::invalid padding | error:02000072:rsa routines::padding check failed | error:1C880004:Provider routines::RSA lib | error:030000EA:digital envelope routines::provider signature failure]
private_encrypt too long: true
public_encrypt padding 99: false [error:02000076:rsa routines::unknown padding type]
private_encrypt padding 99: false [error:1C8000A5:Provider routines::illegal or unsupported padding mode]
private_encrypt ec: false

Warning: openssl_public_encrypt(): Unknown digest algorithm: nope in %s on line 53
oaep unknown md: false [error:0308010C:digital envelope routines::unsupported]
