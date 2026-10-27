--TEST--
openssl keys keep their encoding: compressed EC points, multi-prime RSA, X9.42 DH (DHX)
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
$cfg = ['config' => __DIR__ . '/openssl_key_forms.cnf'];
file_put_contents($cfg['config'], "HOME = .\n[ req ]\ndistinguished_name = dn\n[ dn ]\n");
// compressed EC points keep their form; multi-prime RSA keeps otherPrimeInfos;
// X9.42 DH (dhpublicnumber) keys stay DHX
$keys = [
  'ec compressed public' => "-----BEGIN PUBLIC KEY-----
MDkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDIgACypfH1vGSBA2C9DYwUje8KbCO7jRj
hfLUthtqnABssis=
-----END PUBLIC KEY-----",
  'ec compressed private' => "-----BEGIN EC PRIVATE KEY-----
MFcCAQEEIDJPJsfYRKaKUIuXAiLWeXpKpFbpI1HKx3mWQDUaLWIeoAoGCCqGSM49
AwEHoSQDIgACypfH1vGSBA2C9DYwUje8KbCO7jRjhfLUthtqnABssis=
-----END EC PRIVATE KEY-----",
  'rsa 3 primes' => "-----BEGIN PRIVATE KEY-----
MIICmAIBADANBgkqhkiG9w0BAQEFAASCAoIwggJ+AgEBAoGBAJn2SuyW8ddFBE8I
yoj0EHvjmAqyNCwlwh3pQd3aI1fys9qSWM4Sh7c9RSEOGESl5H1zz99Lg63zUk2J
A8HLGCXO5Ag8opWlmmVPZAi0QiR2dGT3b2/9T9kPyvc1tEX3V0MYMF6mFier4fOT
ntbq0AGYwSY8svmnQeZ2gc+KZWOfAgMBAAECgYEAj3CZSWcwFcFlTEUCfSn2GZZn
Vr/EP8VF3xwsk4biGtQxoFdsiUFWU++vZPgOT7zeBpvE1ar1ABcRrNfdC+4dPPqV
d4Dlmso7PkjcTm//yQtCAGol6HcWEOnk0dBikC2LhlNVuPjtkvmhfgQNhvKhWxMG
dmQMKvlDr5s6twLJ3cECKzJYgXqkVkjK2h6ts+5+xw73QLXmJQfPLDa1uHmEYVbZ
GA+NV5VBeNxd4OcCKxi3AglSFnx/9ii64nm0PwpxlGx6REfhcCBZBRUwxsVGLDDL
ElEzyGjPy/UCKwHvlUr59rdl1W1IkJqjFVRl/X8BEntyY+jWo/lINdX5EwvGC5AZ
rGQM2BECKw0bw/C9oT+XfzBLLyPt/iHD7D08/EpDv/zoOlHq9QJdHcMjciuvk3RE
AqECKwTSzcnt0nJQ5plxZQT6EXXPjYBTv9RtqcHpiJy0T/UF3jqD5RrCa1t4XEMw
gYowgYcCKx+tGblVfrpQI2MUk9JpsvG4iWumqMFGn1HST0Ovndmo9Tfn4qtHVgId
kcUCKwZvAx2Iht/SrQ1PvaJsA4NjBs5NoFL5RBy7SYwm3Dm8Pmond/tv1sFBPj0C
KxIrIttWgaxTWPgUKdTGrconbLNe1GqFfiDTT9mNpzVm1mKI5Au6JKXZJWU=
-----END PRIVATE KEY-----",
  'dhx private' => "-----BEGIN PRIVATE KEY-----
MIIBSgIBADCCASsGByqGSM4+AgEwggEeAoGBAI47s9ESohTx3gAV38TrNhdt/QmF
V4IfVHyckq43eZwuqecz/Hpd+PuIkIupaXNPrFlryZO2XEziiV55fKEnWoLPvLZl
YsR4TQkoJnK6+bdjhoGk+7Hd54FsYMOXgkGuEqQXCV7zGnSM5PHNDF+GpUrrk6xZ
yvX2LqteFpuUtTCxAoGAFzKmsj/6zM6LhMT4SbQJ7w5xUNcx37/aDyEy09UTHNYO
+pV0dLRIfT+voEab2R9tRJJj1l8ybSRfpHL8x1fWrq7ZU6yUx0ohSjK92m0GH0y0
R/F2OOPB5N1epHlT6ya5JGDrpfojXOWHjKS3zmu3vQIIKUdR0yWxjONgsmyZaC8C
FQDTzMpoH+r+JBM8RAZTNWJY7th6FQQWAhQoQmifuStjsR3Jy4z5u5vm8G9V4A==
-----END PRIVATE KEY-----",
  'dhx public' => "-----BEGIN PUBLIC KEY-----
MIIBtjCCASsGByqGSM4+AgEwggEeAoGBAI47s9ESohTx3gAV38TrNhdt/QmFV4If
VHyckq43eZwuqecz/Hpd+PuIkIupaXNPrFlryZO2XEziiV55fKEnWoLPvLZlYsR4
TQkoJnK6+bdjhoGk+7Hd54FsYMOXgkGuEqQXCV7zGnSM5PHNDF+GpUrrk6xZyvX2
LqteFpuUtTCxAoGAFzKmsj/6zM6LhMT4SbQJ7w5xUNcx37/aDyEy09UTHNYO+pV0
dLRIfT+voEab2R9tRJJj1l8ybSRfpHL8x1fWrq7ZU6yUx0ohSjK92m0GH0y0R/F2
OOPB5N1epHlT6ya5JGDrpfojXOWHjKS3zmu3vQIIKUdR0yWxjONgsmyZaC8CFQDT
zMpoH+r+JBM8RAZTNWJY7th6FQOBhAACgYAuAJ6URmpMxXCkqaZqv+SbOkgqnUcI
D8Vf6gwO5IN3QT8PQZ2wznevWxuelZDLhWUrLTOJaX9rPtk29/MzvuXN55IiNbx3
MvZM3RwxsAD6t13sHbsiq7ux0A94VKIcNmySah5bJOmzqADSJrnxoZqz4EC7a68O
u2/h+oNgjWu9Dw==
-----END PUBLIC KEY-----",
];
foreach ($keys as $n => $pem) {
  $k = str_contains($n, 'public') ? openssl_pkey_get_public($pem) : openssl_pkey_get_private($pem);
  $d = openssl_pkey_get_details($k);
  echo "== $n: bits=", $d['bits'], " type=", $d['type'], " arrays=", implode(",", array_diff(array_keys($d), ['bits', 'key', 'type'])), "\n", $d['key'];
  if (!str_contains($n, 'public')) {
    var_dump(openssl_pkey_export($k, $out, null, $cfg));
    echo $out;
    $back = openssl_pkey_get_private($out);
    echo "reload: ", openssl_pkey_get_details($back)['key'] === $d['key'] ? "same" : "different", "\n";
  }
  errs();
}
$a = openssl_pkey_get_private($keys['dhx private']);
echo "dhx derive self: ", strlen(openssl_pkey_derive(openssl_pkey_get_public($keys['dhx public']), $a)), errs(), "\n";
unlink($cfg['config']);
?>
--EXPECT--
== ec compressed public: bits=256 type=3 arrays=ec
-----BEGIN PUBLIC KEY-----
MDkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDIgACypfH1vGSBA2C9DYwUje8KbCO7jRj
hfLUthtqnABssis=
-----END PUBLIC KEY-----
== ec compressed private: bits=256 type=3 arrays=ec
-----BEGIN PUBLIC KEY-----
MDkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDIgACypfH1vGSBA2C9DYwUje8KbCO7jRj
hfLUthtqnABssis=
-----END PUBLIC KEY-----
bool(true)
-----BEGIN PRIVATE KEY-----
MGcCAQAwEwYHKoZIzj0CAQYIKoZIzj0DAQcETTBLAgEBBCAyTybH2ESmilCLlwIi
1nl6SqRW6SNRysd5lkA1Gi1iHqEkAyIAAsqXx9bxkgQNgvQ2MFI3vCmwju40Y4Xy
1LYbapwAbLIr
-----END PRIVATE KEY-----
reload: same
== rsa 3 primes: bits=1024 type=0 arrays=rsa
-----BEGIN PUBLIC KEY-----
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQCZ9krslvHXRQRPCMqI9BB745gK
sjQsJcId6UHd2iNX8rPakljOEoe3PUUhDhhEpeR9c8/fS4Ot81JNiQPByxglzuQI
PKKVpZplT2QItEIkdnRk929v/U/ZD8r3NbRF91dDGDBephYnq+Hzk57W6tABmMEm
PLL5p0HmdoHPimVjnwIDAQAB
-----END PUBLIC KEY-----
bool(true)
-----BEGIN PRIVATE KEY-----
MIICmAIBADANBgkqhkiG9w0BAQEFAASCAoIwggJ+AgEBAoGBAJn2SuyW8ddFBE8I
yoj0EHvjmAqyNCwlwh3pQd3aI1fys9qSWM4Sh7c9RSEOGESl5H1zz99Lg63zUk2J
A8HLGCXO5Ag8opWlmmVPZAi0QiR2dGT3b2/9T9kPyvc1tEX3V0MYMF6mFier4fOT
ntbq0AGYwSY8svmnQeZ2gc+KZWOfAgMBAAECgYEAj3CZSWcwFcFlTEUCfSn2GZZn
Vr/EP8VF3xwsk4biGtQxoFdsiUFWU++vZPgOT7zeBpvE1ar1ABcRrNfdC+4dPPqV
d4Dlmso7PkjcTm//yQtCAGol6HcWEOnk0dBikC2LhlNVuPjtkvmhfgQNhvKhWxMG
dmQMKvlDr5s6twLJ3cECKzJYgXqkVkjK2h6ts+5+xw73QLXmJQfPLDa1uHmEYVbZ
GA+NV5VBeNxd4OcCKxi3AglSFnx/9ii64nm0PwpxlGx6REfhcCBZBRUwxsVGLDDL
ElEzyGjPy/UCKwHvlUr59rdl1W1IkJqjFVRl/X8BEntyY+jWo/lINdX5EwvGC5AZ
rGQM2BECKw0bw/C9oT+XfzBLLyPt/iHD7D08/EpDv/zoOlHq9QJdHcMjciuvk3RE
AqECKwTSzcnt0nJQ5plxZQT6EXXPjYBTv9RtqcHpiJy0T/UF3jqD5RrCa1t4XEMw
gYowgYcCKx+tGblVfrpQI2MUk9JpsvG4iWumqMFGn1HST0Ovndmo9Tfn4qtHVgId
kcUCKwZvAx2Iht/SrQ1PvaJsA4NjBs5NoFL5RBy7SYwm3Dm8Pmond/tv1sFBPj0C
KxIrIttWgaxTWPgUKdTGrconbLNe1GqFfiDTT9mNpzVm1mKI5Au6JKXZJWU=
-----END PRIVATE KEY-----
reload: same
== dhx private: bits=1024 type=-1 arrays=
-----BEGIN PUBLIC KEY-----
MIIBtjCCASsGByqGSM4+AgEwggEeAoGBAI47s9ESohTx3gAV38TrNhdt/QmFV4If
VHyckq43eZwuqecz/Hpd+PuIkIupaXNPrFlryZO2XEziiV55fKEnWoLPvLZlYsR4
TQkoJnK6+bdjhoGk+7Hd54FsYMOXgkGuEqQXCV7zGnSM5PHNDF+GpUrrk6xZyvX2
LqteFpuUtTCxAoGAFzKmsj/6zM6LhMT4SbQJ7w5xUNcx37/aDyEy09UTHNYO+pV0
dLRIfT+voEab2R9tRJJj1l8ybSRfpHL8x1fWrq7ZU6yUx0ohSjK92m0GH0y0R/F2
OOPB5N1epHlT6ya5JGDrpfojXOWHjKS3zmu3vQIIKUdR0yWxjONgsmyZaC8CFQDT
zMpoH+r+JBM8RAZTNWJY7th6FQOBhAACgYAuAJ6URmpMxXCkqaZqv+SbOkgqnUcI
D8Vf6gwO5IN3QT8PQZ2wznevWxuelZDLhWUrLTOJaX9rPtk29/MzvuXN55IiNbx3
MvZM3RwxsAD6t13sHbsiq7ux0A94VKIcNmySah5bJOmzqADSJrnxoZqz4EC7a68O
u2/h+oNgjWu9Dw==
-----END PUBLIC KEY-----
bool(true)
-----BEGIN PRIVATE KEY-----
MIIBSgIBADCCASsGByqGSM4+AgEwggEeAoGBAI47s9ESohTx3gAV38TrNhdt/QmF
V4IfVHyckq43eZwuqecz/Hpd+PuIkIupaXNPrFlryZO2XEziiV55fKEnWoLPvLZl
YsR4TQkoJnK6+bdjhoGk+7Hd54FsYMOXgkGuEqQXCV7zGnSM5PHNDF+GpUrrk6xZ
yvX2LqteFpuUtTCxAoGAFzKmsj/6zM6LhMT4SbQJ7w5xUNcx37/aDyEy09UTHNYO
+pV0dLRIfT+voEab2R9tRJJj1l8ybSRfpHL8x1fWrq7ZU6yUx0ohSjK92m0GH0y0
R/F2OOPB5N1epHlT6ya5JGDrpfojXOWHjKS3zmu3vQIIKUdR0yWxjONgsmyZaC8C
FQDTzMpoH+r+JBM8RAZTNWJY7th6FQQWAhQoQmifuStjsR3Jy4z5u5vm8G9V4A==
-----END PRIVATE KEY-----
reload: same
== dhx public: bits=1024 type=-1 arrays=
-----BEGIN PUBLIC KEY-----
MIIBtjCCASsGByqGSM4+AgEwggEeAoGBAI47s9ESohTx3gAV38TrNhdt/QmFV4If
VHyckq43eZwuqecz/Hpd+PuIkIupaXNPrFlryZO2XEziiV55fKEnWoLPvLZlYsR4
TQkoJnK6+bdjhoGk+7Hd54FsYMOXgkGuEqQXCV7zGnSM5PHNDF+GpUrrk6xZyvX2
LqteFpuUtTCxAoGAFzKmsj/6zM6LhMT4SbQJ7w5xUNcx37/aDyEy09UTHNYO+pV0
dLRIfT+voEab2R9tRJJj1l8ybSRfpHL8x1fWrq7ZU6yUx0ohSjK92m0GH0y0R/F2
OOPB5N1epHlT6ya5JGDrpfojXOWHjKS3zmu3vQIIKUdR0yWxjONgsmyZaC8CFQDT
zMpoH+r+JBM8RAZTNWJY7th6FQOBhAACgYAuAJ6URmpMxXCkqaZqv+SbOkgqnUcI
D8Vf6gwO5IN3QT8PQZ2wznevWxuelZDLhWUrLTOJaX9rPtk29/MzvuXN55IiNbx3
MvZM3RwxsAD6t13sHbsiq7ux0A94VKIcNmySah5bJOmzqADSJrnxoZqz4EC7a68O
u2/h+oNgjWu9Dw==
-----END PUBLIC KEY-----
dhx derive self: 128
