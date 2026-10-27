--TEST--
openssl_csr_export() text lists a new request's attributes in insertion order
--EXTENSIONS--
openssl
--FILE--
<?php
// X509_REQ_print() lists a new request's attributes in the order they were
// added (extra_attributes first, then the config's attributes section)
$cnf = sys_get_temp_dir() . "/goro_csrattr_" . getmypid() . ".cnf";
file_put_contents($cnf, "HOME = .\n[ req ]\ndefault_bits = 2048\ndistinguished_name = dn\nattributes = attrs\n[ dn ]\n[ attrs ]\nchallengePassword = secret123\nunstructuredName = Some Name\n");
$k = openssl_pkey_new(['config' => $cnf, 'private_key_type' => OPENSSL_KEYTYPE_EC, 'curve_name' => 'prime256v1']);
foreach ([[], ['unstructuredAddress' => 'addr'], ['unstructuredName' => 'uname', 'challengePassword' => 'pw1234']] as $attrs) {
  $csr = openssl_csr_new(['CN' => 'attr'], $k, ['config' => $cnf], $attrs);
  openssl_csr_export($csr, $t, false);
  preg_match('/Attributes:\n(.*?)\n\s*Requested Extensions/s', $t, $m);
  echo json_encode($attrs), "\n", $m[1], "\n";
  // re-read from DER: the SET OF order
  openssl_csr_export($csr, $pem);
  openssl_csr_export($pem, $t2, false);
  preg_match('/Attributes:\n(.*?)\n\s*Requested Extensions/s', $t2, $m2);
  echo "reread:\n", $m2[1], "\n";
}
unlink($cnf);
?>
--EXPECT--
[]
            challengePassword        :secret123
            unstructuredName         :Some Name
reread:
            unstructuredName         :Some Name
            challengePassword        :secret123
{"unstructuredAddress":"addr"}
            unstructuredAddress      :addr
            challengePassword        :secret123
            unstructuredName         :Some Name
reread:
            unstructuredAddress      :addr
            unstructuredName         :Some Name
            challengePassword        :secret123
{"unstructuredName":"uname","challengePassword":"pw1234"}
            unstructuredName         :uname
            challengePassword        :pw1234
reread:
            unstructuredName         :uname
            challengePassword        :pw1234
