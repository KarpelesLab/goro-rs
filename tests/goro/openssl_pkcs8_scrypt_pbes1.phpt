--TEST--
PKCS#8 keys encrypted with PBES2/scrypt and PBES1
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
// openssl pkcs8 -topk8 -scrypt (PBES2 with id-scrypt) and -v1 PBE-SHA1-3DES
foreach (['scrypt' => "-----BEGIN ENCRYPTED PRIVATE KEY-----
MIIC3TBXBgkqhkiG9w0BBQ0wSjApBgkrBgEEAdpHBAswHAQQO5mU9GIirYUKJB7c
pQlm9wICQAACAQgCAQEwHQYJYIZIAWUDBAEqBBAQzr57xTyzM25eNjC2cMIhBIIC
gDm1yCYmsdtl9QZSos8k5SD5C0nokYKieCSECe3CsAJ0loT4fdDhVVBKQXN9rRdc
yLoN5riUWpSbI29rJGApZvmPYatWs4pIo3u2h8ZSklaGwNk12dkoaQKMkCWT/Cbq
aYQXtpDDd+7WQGhgCdAnVIGAOFkTaOApWmVPGUfXK0c6GHnxLC5E3+Xt89KeF5bd
0Ktuxt3E2ZijOOHqBt8GZ3qHWdXKR3wivwHW6cbrYRH0wamHQ9LeMI2X+rM9whDU
vcHWVp40gq8saOpzpA2F+bZEICc2Et6y7jTqMrmgMxher6ctTVtS8GVy4GnOG3QN
Vw6icb0+f7qfcXZtkLXE1yDP6cNLqFiO5yScvQiST9SqcgnDzkqGRW+q/DrJpEmi
QthrMykLiAbg5H5Z00wSCObpBPvH4nWGgrVCUaSdyQHbl3l49V2kNzQHwDWj8S19
q+kwMStyX+SMg+t14rjU67tlFgoeJ3nMbdFjj/vigBciB1R5vEv+DmahbDJDV/00
n1IwfKlcLOpzO7cjqzGBT5Ta0IOsJBUpx5MuRhm8tKNekiHZjkrPRFTtcdSViVQK
JtPUnJC361Yokk1Kj26DC2LYbArNYyBFRO7V9VXLiD8WuiiLLZlj0GNoQc/KcTD1
RMhY+6X7U6nrMHdo93aOFm9l9xWhnYx+xg8MId0DZwNf5pzJR5pNgK9iLhodACaF
ARh6IPwIe5Dv52XyD2YEFU4LGAhKXHMzzoQE3HzI3DsKxv+VJGwIrVZeL5RJTTfo
MC6gE0aTxZ6zre2s92+cuBRFh2kdUJn/LRgKMwyUXDR8XyLUK0T+2IsUZyfU+F4q
hgnahdcVsVvKK/UlGFExUx0=
-----END ENCRYPTED PRIVATE KEY-----", 'pbe-sha1-3des' => "-----BEGIN ENCRYPTED PRIVATE KEY-----
MIICojAcBgoqhkiG9w0BDAEDMA4ECLdG5miiqNJeAgIIAASCAoAyDpyHxBVCpEEb
iMOw34DM3DT1onvhbb6HwybHJWnuHy0tFJBbiLfdomw93anlZdvSjZJT1Y3gnbL0
FHxzHuhK5Bwtav3hwC8EjMoEYCeedkZNdn5TYN73UUBSPRVnJH15oddomQfrBixO
jhfyXjyq7Tig1taUdHboj8iYmWf/vfDnLK8lNOLGVYmYXRSMTmRQ+lww405J/iMH
AYUmjGY3kAV35t9aYD1Gc/IFqwrHx5nLAB5Zx9tmztROSH1+w9QeYw8gEIMNnYOf
zB5zQqAvIsHFqrGYgtSwbkTppj7bmQMgBNQ7uJz08T27GzPLUpPjHRHCNO+np3mZ
CiQIv6yJ+KLgnIsZU+BVEGaeHtf3Imw0rQeQJsXL+5CE4BZmMGBYHVF0GPZFJb5i
erObnY4rDzG+S3lCXbDKLD6aKhnV9ZKO7VKujyVfxF4KqC7xVVjCth6hXb3FU9c/
B++91Z3xkT5MOz9GOQQ0qEzzqrHlcgKjxUDIgW8z5d1cWhd1vZHhRAFCEb1DXtor
M3UHU/2ECzJ1C881k0oc1psNbYSTHQHSus40yGFAGafi+/AbSi2CfDmotNml4NYv
xnO3aFgsr0GN8yvRgz++Nk17aYCIZ2QZyWwVCdW9JO8ag846vwQZgeGemYFK97Uf
ng0ei5Rq1lWiPnb3Axwud8a/f15c4ek4b2Ak0kfRI3B8DFglNoZ9Tvy7aMB0vDeu
5AB3w6h3IqhgLzdPrmkiEy613YFm3FrzNVUdWyqpgaXERg4UVmeuifUA77Ef424m
MH62+3T4mPEgjFBmzPC5fS+LCpXuHp+keh1JCqFt2KoiTAhZdyFmT4Gc8BdPP/tf
y+DTcuTM
-----END ENCRYPTED PRIVATE KEY-----", 'des3' => "-----BEGIN ENCRYPTED PRIVATE KEY-----
MIIC3DBWBgkqhkiG9w0BBQ0wSTAxBgkqhkiG9w0BBQwwJAQQ1US+HWmz4AJtFrkt
GtmAdAICCAAwDAYIKoZIhvcNAgkFADAUBggqhkiG9w0DBwQIBDFTKocqAFAEggKA
Jb7p555KIPcrmthNnjoZMjEr0l61SamM7qhXycguvTTfI63v5GMBfEbAFLyTM4Vb
DnUJTI56haEpe+1nUlcBn2/KCEYvp7APUtidXIwZCU2gTMrK8tqAUYL5iysBJI6E
bsOCVK2owSqDySQ8wHdGIoGt8aFdEfAWubF5QRlq3Hz0s21JAD21BB72ZV6w3nHU
8VjKeqj6KNqyipBPNpXQMGVKzEO4ZumgKMtj8Nk1YfW+V5CPcCq0erIcuNakPisb
VC+GOKa3m/cuuhXijxeV3Q1dNRWUG62B0m+UcLuXWQziXl6kCw9J/Eg+Hfn4Yusm
1eyj5T3Loi8jROe9S2IO8gmcB4lAES9swQTnNoQ7SSOPWf+ag4/YlxVI29l2GlQE
n5QqWr2ugIBhaHU6rSAsYxcA8Btw0OEDWL7j7eCP/PVqTLpqm7im2x7pC0O+UrA7
ok8QE+3G2culn+wsmw1sBts5PDDiuBKvO2C5e2/anDICCWU3PYsGJC11p+lNwNfG
SBNP8WgODe+4dVcco1Wi8CPkzMPBDbkDDx3UV+UAtcImHVx2cvf2uJ9HZ/uE0beG
VsZC84XGbFAg2qZTkIL6swIj1GpkJ/z+QISo2Ni0Vyn0Fcr0gODVmYwvdGJn67aa
1HCWfncML8NyM44w3uCFCEqnY7G6mctpIWcb33OztDXI0FedopS3qwk/8PWckMAD
igPPoz9WFg2TFgZMEKBw4kGeRArGEymF17DqHug7h6y2aeyU7r5C+jKijsfilmbO
UNYOQ7YsJIj2yS/YsXJpE65MAgWHeT1U0cdbC86AWXa+LvKmlwJqm92SaW3P23lw
te8WoftjdpYKGH23y/1Z+A==
-----END ENCRYPTED PRIVATE KEY-----"] as $n => $pem) {
  $k = openssl_pkey_get_private($pem, "secret");
  echo "$n: ", $k ? openssl_pkey_get_details($k)['bits'] : "false", errs(), "\n";
  echo "$n wrong: ", var_export(openssl_pkey_get_private($pem, "wrong"), true), errs(), "\n";
}
?>
--EXPECT--
scrypt: 1024
scrypt wrong: false [error:1C800064:Provider routines::bad decrypt | error:11800074:PKCS12 routines::pkcs12 cipherfinal error | error:1C800064:Provider routines::bad decrypt | error:11800074:PKCS12 routines::pkcs12 cipherfinal error]
pbe-sha1-3des: 1024
pbe-sha1-3des wrong: false [error:1C800064:Provider routines::bad decrypt | error:11800074:PKCS12 routines::pkcs12 cipherfinal error | error:1C800064:Provider routines::bad decrypt | error:11800074:PKCS12 routines::pkcs12 cipherfinal error]
des3: 1024
des3 wrong: false [error:1C800064:Provider routines::bad decrypt | error:11800074:PKCS12 routines::pkcs12 cipherfinal error | error:1C800064:Provider routines::bad decrypt | error:11800074:PKCS12 routines::pkcs12 cipherfinal error]
