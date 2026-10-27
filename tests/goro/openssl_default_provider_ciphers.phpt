--TEST--
openssl ciphers of the default provider: CBC-CTS, CBC-HMAC outside TLS, GCM-SIV/SIV, inverse key wraps, DES3-WRAP, SM4 GCM/CCM/XTS, OID names
--EXTENSIONS--
openssl
--FILE--
<?php
function errs() { $o = []; while ($e = openssl_error_string()) $o[] = $e; return $o ? " [" . implode(" | ", $o) . "]" : ""; }
$all = openssl_get_cipher_methods(true);
foreach (['aes-128-cbc-cts', 'camellia-256-cbc-cts', 'aes-128-cbc-hmac-sha1', 'aes-256-cbc-hmac-sha256', 'aes-128-gcm-siv', 'aes-256-gcm-siv', 'aes-128-siv', 'aes-256-siv', 'aes-128-wrap-inv', 'aes192-wrap-pad-inv', 'sm4-gcm', 'sm4-ccm', 'sm4-xts', 'des3-wrap', 'id-smime-alg-cms3deswrap', '1.2.410.200046.1.1.2', '0.3.4401.5.3.1.9.1', '1.2.392.200011.61.1.1.1.2', '1.3.14.3.2.17', '1.2.156.10197.1.104.8', '2.16.840.1.101.3.4.1.2'] as $c) {
  $kl = openssl_cipher_key_length($c); $ivl = openssl_cipher_iv_length($c);
  echo "== $c key=$kl iv=$ivl listed=", var_export(in_array(strtolower($c), $all), true), "\n";
  $k = substr(str_repeat("0123456789abcdefFEDCBA9876543210", 4), 0, $kl);
  $iv = substr(str_repeat("fedcba9876543210", 4), 0, $ivl);
  foreach (["", "a", str_repeat("b", 15), str_repeat("c", 16), str_repeat("d", 17), str_repeat("e", 24), str_repeat("h", 33)] as $m) {
    $tag = null;
    $e = openssl_encrypt($m, $c, $k, OPENSSL_RAW_DATA, $iv, $tag, "aad");
    echo strlen($m), " ";
    if (str_contains($c, "3des") || str_contains($c, "des3")) {
      echo $e === false ? "false" : "len " . strlen($e);  // random IV
    } else {
      echo $e === false ? "false" : bin2hex($e), " tag=", $tag === null ? "null" : bin2hex($tag);
    }
    if ($e !== false) { $d = openssl_decrypt($e, $c, $k, OPENSSL_RAW_DATA, $iv, $tag, "aad"); echo " dec=", $d === $m ? "same" : var_export($d, true); }
    echo errs(), "\n";
  }
}
?>
--EXPECT--
== aes-128-cbc-cts key=16 iv=16 listed=true
0 false tag=null
1 false tag=null
15 false tag=null
16 f8a50144db771461a99f70e8cf659375 tag=null dec=same
17 4c2141aa1b00d2efab03a261d669c2400b tag=null dec=same
24 d23fc0d2e0d9173a78613a2deaf1c892d5de892d43e55729 tag=null dec=same
33 72ec2e384afe95dbdd244eabb0f4b94256182aad4faeeb5b19ce404d07463e1b80 tag=null dec=same
== camellia-256-cbc-cts key=32 iv=16 listed=true
0 false tag=null
1 false tag=null
15 false tag=null
16 15fd1efcc32528dc2ce06f0fd38d6a9e tag=null dec=same
17 62e7571a5dc70bcc781c1b820d97aac5b9 tag=null dec=same
24 52f73b970b73ff3f6dc3511844e131f6ba8dd6575d6e9059 tag=null dec=same
33 5488ee3185863c6d2932d672c5bc08c9007fe1fc0f804825d9a3c630524a8863e7 tag=null dec=same
== aes-128-cbc-hmac-sha1 key=16 iv=16 listed=true
0  tag=null dec=same
1 false tag=null [error:1C800066:Provider routines::cipher operation failed]
15 false tag=null [error:1C800066:Provider routines::cipher operation failed]
16 f8a50144db771461a99f70e8cf659375 tag=null dec=same
17 false tag=null [error:1C800066:Provider routines::cipher operation failed]
24 false tag=null [error:1C800066:Provider routines::cipher operation failed]
33 false tag=null [error:1C800066:Provider routines::cipher operation failed]
== aes-256-cbc-hmac-sha256 key=32 iv=16 listed=true
0  tag=null dec=same
1 false tag=null [error:1C800066:Provider routines::cipher operation failed]
15 false tag=null [error:1C800066:Provider routines::cipher operation failed]
16 3036f975ef278a74f421bb63991aca83 tag=null dec=same
17 false tag=null [error:1C800066:Provider routines::cipher operation failed]
24 false tag=null [error:1C800066:Provider routines::cipher operation failed]
33 false tag=null [error:1C800066:Provider routines::cipher operation failed]
== aes-128-gcm-siv key=16 iv=12 listed=true
0  tag=null dec=false
1 51 tag=null dec=false
15 7afff499ea0ef648264b87711aa6e2 tag=null dec=false
16 77ab00a9a0ddf71530bd413abd8a0d35 tag=null dec=false
17 79607d1eeaed51d16a986e1e3c6aa26a6b tag=null dec=false
24 ea4889f70be58df14810c580d9066f3158570b05d7d89a7d tag=null dec=false
33 6c76ff3de50da8e4e18afc17a3f10d94761afeb2d8995167b7f25129570c76aad1 tag=null dec=false
== aes-256-gcm-siv key=32 iv=12 listed=true
0  tag=null dec=false
1 23 tag=null dec=false
15 47d68dd1b24a7879dcaf58ceb78c8b tag=null dec=false
16 9b37a01f42714cdd4b8d26c92659414b tag=null dec=false
17 14ed512b7f57379a877f636dd1f603fbba tag=null dec=false
24 b3d65a207a5413613dd555bb3f75b525f6f2d007f69723b5 tag=null dec=false
33 b6fe4a9f5270cdaece183d2487dde8d03b2264a00701fc00d63b4bb7192676d631 tag=null dec=false
== aes-128-siv key=32 iv=0 listed=true
0  tag=null dec=false
1 eb tag=null dec=false
15 0abe065a54dead04f7b1551429c6a5 tag=null dec=false
16 28181c49baf8569ff2bba43a1e2f6cd6 tag=null dec=false
17 b572376d655920fec141105597b733f2a8 tag=null dec=false
24 9afc2d79b0496f82a040d77b6af7c6332c04300e0c5cfdbe tag=null dec=false
33 c8f587841303fcf96c76188ad2e0b6aa8e6bb2611e972e83108e5843b39d18b660 tag=null dec=false
== aes-256-siv key=64 iv=0 listed=true
0  tag=null dec=false
1 25 tag=null dec=false
15 2fa284b2af4b95db754fb68d8e54ca tag=null dec=false
16 f7e008e363eea36f9d70be92fb04959a tag=null dec=false
17 c7d3d759a07d055fe87d28c3bc55ced331 tag=null dec=false
24 d0d855329a968edff91ddcdee25a911d8e17b0877c73eef5 tag=null dec=false
33 abb54ac1ab3f5a2126ecd35615e6b93ac991a161827e2dae93c66fc61b85549bb2 tag=null dec=false
== aes-128-wrap-inv key=16 iv=8 listed=true
0  tag=null dec=same
1 false tag=null [error:1C8000E6:Provider routines::invalid input length | error:030000BD:digital envelope routines::update error]
15 false tag=null [error:1C8000E6:Provider routines::invalid input length | error:030000BD:digital envelope routines::update error]
16 803b6a64aa7ac84066f4405f7f02cef807e2f85b3baa1b10 tag=null dec=same
17 false tag=null [error:1C8000E6:Provider routines::invalid input length | error:030000BD:digital envelope routines::update error]
24 e9691fae12364dd1e370bdb372a2c8ce332e3a4a5c05d28856fa73db24e44912 tag=null dec=same
33 false tag=null [error:1C8000E6:Provider routines::invalid input length | error:030000BD:digital envelope routines::update error]
== aes192-wrap-pad-inv key=24 iv=4 listed=true
0  tag=null dec=same
1 f620195688c3f046b08b4c914193c20b tag=null dec=same
15 319ca922debc854170e19c14beb783f508c6ea9757a990ba tag=null dec=same
16 b817a21336fed12d6c9f6c8518a94b464cd5ac96e652f2df tag=null dec=same
17 b8acd2c2b4148d6b7d0a9da7ce5a7a8b6f9a7a67c902499470366f4c3baf6391 tag=null dec=same
24 a69e5c8a6aae842317e5bd1aa1425d0f36d7fd7b3248c0ed7612ae1a60742e26 tag=null dec=same
33 6ceb921aa26db6f006c1f2e0ca907e5b7e3c8ac6e9fe4e6e7a28dae73fe1464f310f852262cd6dbcfe61237ff355b705 tag=null dec=same
== sm4-gcm key=16 iv=12 listed=true
0  tag=f70b4d6987bcabd26c539c13be35968b dec=same
1 82 tag=bb65af7e3e1710e650f51add55bce094 dec=same
15 81031d7398a0df4bfd64e20407d782 tag=484bcf905ed3a0dc0cf2b7300069567b dec=same
16 80021c7299a1de4afc65e30506d683e3 tag=5195edab28eb77f9d99c34f4a5b5233c dec=same
17 87051b759ea6d94dfb62e40201d184e4c8 tag=959de301f089481362ea4df89838c844 dec=same
24 86041a749fa7d84cfa63e50300d085e5c9ba2bd89abdeae1 tag=c9d086ac0a3431e18810df40b541f3cf dec=same
33 8b09177992aad541f76ee80e0ddd88e8c4b726d597b0e7ecc357f0e8579664ece7 tag=a22f4d9c5a67340d4c40625ae29696ba dec=same
== sm4-ccm key=16 iv=12 listed=true
0  tag=e143a614d560547e14c4aa8c3c747eed dec=same
1 3f tag=089c59a1ea9fed15febea86d0c0b9ff5 dec=same
15 3c586ee97e018805aabffe273e18c3 tag=2ccc94ded2866198f3535f3107eb196b dec=same
16 3d596fe87f008904abbeff263f19c231 tag=56aafc6ec55202b07217f3e57915a995 dec=same
17 3a5e68ef78078e03acb9f821381ec536e5 tag=597affa8b5c7f069c4eb01eeec33f3e4 dec=same
24 3b5f69ee79068f02adb8f920391fc437e4e53545a8e7c2e6 tag=8d6456e5b1ec15f4dc1a232714f3ca8f dec=same
33 365264e3740b820fa0b5f42d3412c93ae9e83848a5eacfeb15a92f55a047c4d96f tag=a89f506a044d40480f4d4349a381e732 dec=same
== sm4-xts key=32 iv=16 listed=true
0 false tag=null [error:1C800066:Provider routines::cipher operation failed]
1 false tag=null [error:1C800066:Provider routines::cipher operation failed]
15 false tag=null [error:1C800066:Provider routines::cipher operation failed]
16 eb8bb8635bd3a838e68fb4e7604f90ff tag=null dec=same
17 9903f179e6f5ae7276b2a285ec2caff0ec tag=null dec=same
24 d799a64dc4e6385e46fe5ca291d6331fb6b4849bec9805eb tag=null dec=same
33 c844888a51df436f6c43f6377752f53622965fa555a80e988a32401677a0f7d5fa tag=null dec=same
== des3-wrap key=24 iv=0 listed=true
0 len 0 dec=same
1 false [error:1C800066:Provider routines::cipher operation failed]
15 false [error:1C800066:Provider routines::cipher operation failed]
16 len 32 dec=same
17 false [error:1C800066:Provider routines::cipher operation failed]
24 len 40 dec=same
33 false [error:1C800066:Provider routines::cipher operation failed]
== id-smime-alg-cms3deswrap key=24 iv=0 listed=true
0 len 0 dec=same
1 false [error:1C800066:Provider routines::cipher operation failed]
15 false [error:1C800066:Provider routines::cipher operation failed]
16 len 32 dec=same
17 false [error:1C800066:Provider routines::cipher operation failed]
24 len 40 dec=same
33 false [error:1C800066:Provider routines::cipher operation failed]
== 1.2.410.200046.1.1.2 key=16 iv=16 listed=true
0 d35c4ff2ae7d09b460c755f2d8f6d97d tag=null dec=same
1 fff9fd72b4d228af350915e9939b8f26 tag=null dec=same
15 d33a4ae30a3060a0e31929623644d9a2 tag=null dec=same
16 cbe559c773f98bda9f98c0d941e76ffba8ff23360fae81b4f9dbd9d4120c3b57 tag=null dec=same
17 ace1bcb455f774c8c20883cf3563ae96c2d5b54aa78e2ca17e2f0927382462a7 tag=null dec=same
24 df3ab6011ae4259fd0f3e817c7d9ff6f736bdc8d9ce8d4d77f32fec74c4c9cc2 tag=null dec=same
33 837028f0ffccc356851891978a206bc0ade68c0e46ab2ec5ec5a36eb17385d2d1f9654802b7cf511a05a0de0dc4e5e08 tag=null dec=same
== 0.3.4401.5.3.1.9.1 key=16 iv=0 listed=true
0 9ecebe83cf963ad30b2028da012d0ed8 tag=null dec=same
1 5c61745fd7148d6f716f2286f4773363 tag=null dec=same
15 7d3544d2930a975f68a9eb14241136d1 tag=null dec=same
16 d794e235c67f0153ef737e607eed1ee59ecebe83cf963ad30b2028da012d0ed8 tag=null dec=same
17 8596a2a4e8df0e613c102bfd3fb286193097af970aed7f26f6e7383a39f22ba8 tag=null dec=same
24 7e7286b6f2a103f3ca74e129021db84d95476152e96e1dce32b9752773ff4262 tag=null dec=same
33 bccaf82165833f87de0a31dec5bc66ddbccaf82165833f87de0a31dec5bc66dd00ab03e3a321a8c29c74aa81301dcd85 tag=null dec=same
== 1.2.392.200011.61.1.1.1.2 key=16 iv=16 listed=true
0 28aa27e967f33fb66dce07bcc297dce1 tag=null dec=same
1 34f1bba63dd96e5ad7b4e976e9fedbaf tag=null dec=same
15 1d3b6cf96bc98c00ebda173d09ce32cd tag=null dec=same
16 88ec7bacdda3b5c97c6bf56398477422d875c1bb97a5d93d779a8665e7867f2e tag=null dec=same
17 5214959f9211c8afc82685ab85cfebf1e9f52199f94e5cb3d14e7fd822867dd1 tag=null dec=same
24 619258b3d78ee3e704bf125698b51cfc994306fecc74e1bec97afef903218b54 tag=null dec=same
33 10da92c26e9290e57239211f9e6d256707c8651a3f2ccf6e934391ffa776e8704ff198dd6c93b6f1336ee0fba954d220 tag=null dec=same
== 1.3.14.3.2.17 key=16 iv=0 listed=true
0 fa5916059a76b93b tag=null dec=same
1 5afc32e39e1b831e tag=null dec=same
15 e5de0ae48e334fa50fe6e5d558ca9a1c tag=null dec=same
16 643dee73b8e5d17a643dee73b8e5d17afa5916059a76b93b tag=null dec=same
17 a288c5a6df330bd4a288c5a6df330bd4110ec8c16937f7e6 tag=null dec=same
24 535f07a6b24045a4535f07a6b24045a4535f07a6b24045a4fa5916059a76b93b tag=null dec=same
33 bbbec3c28df87604bbbec3c28df87604bbbec3c28df87604bbbec3c28df87604bd22aa27be9e1c48 tag=null dec=same
== 1.2.156.10197.1.104.8 key=16 iv=12 listed=true
0  tag=f70b4d6987bcabd26c539c13be35968b dec=same
1 82 tag=bb65af7e3e1710e650f51add55bce094 dec=same
15 81031d7398a0df4bfd64e20407d782 tag=484bcf905ed3a0dc0cf2b7300069567b dec=same
16 80021c7299a1de4afc65e30506d683e3 tag=5195edab28eb77f9d99c34f4a5b5233c dec=same
17 87051b759ea6d94dfb62e40201d184e4c8 tag=959de301f089481362ea4df89838c844 dec=same
24 86041a749fa7d84cfa63e50300d085e5c9ba2bd89abdeae1 tag=c9d086ac0a3431e18810df40b541f3cf dec=same
33 8b09177992aad541f76ee80e0ddd88e8c4b726d597b0e7ecc357f0e8579664ece7 tag=a22f4d9c5a67340d4c40625ae29696ba dec=same
== 2.16.840.1.101.3.4.1.2 key=16 iv=16 listed=true
0 3c492cedd954cc383f3d3ee93e728484 tag=null dec=same
1 30aef88f86574fe3493174858fa4e2d5 tag=null dec=same
15 2f797be90786543b6028e894c35063b2 tag=null dec=same
16 f8a50144db771461a99f70e8cf6593755f3fd7739793913e26610db5aa190da9 tag=null dec=same
17 4ca5032728bb50626cdc77ab9c77a57eda087c3227ba1d07780b3be363c20644 tag=null dec=same
24 d23fc0d2e0d9173afeb03be9cabaf320b1b312c1de21b8a3da17116c38b3b30c tag=null dec=same
33 72ec2e384afe95dbdd244eabb0f4b94256166af0641ec3dbf4f2f04530cdbe1f4a543d6e41f8f2882efa9d4dc73f2464 tag=null dec=same
