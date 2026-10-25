--TEST--
iconv_mime_encode(): ISO-2022-KR header and SO at word ends like glibc
--FILE--
<?php
// glibc's ISO-2022-KR writes the header / SO of a character that then has
// no room in the encoded word
$subjects = ["한국어 제목입니다", "Re: 회의 건 - 내일 10시", "a한b국c어d", "한국어한국어한국어한국어한국어한국어한국어한국어"];
foreach ($subjects as $s) {
    foreach (["B", "Q"] as $sc) {
        foreach ([28, 40, 41, 43, 51, 76] as $l) {
            $r = @iconv_mime_encode("Subject", $s, ["scheme" => $sc, "output-charset" => "ISO-2022-KR", "line-length" => $l]);
            echo "$sc $l ", $r === false ? "FALSE" : $r, "\n";
        }
    }
}
--EXPECT--
B 28 FALSE
B 40 FALSE
B 41 FALSE
B 43 Subject: =?ISO-2022-KR?B?GyQpQw5HUQ8=?=
 =?ISO-2022-KR?B?GyQpQw4xOT5uDyAOQSYP?=
 =?ISO-2022-KR?B?GyQpQw44cUBUNE80WQ8=?=
B 51 Subject: =?ISO-2022-KR?B?GyQpQw5HUTE5Pm4PIA4P?=
 =?ISO-2022-KR?B?GyQpQw5BJjhxQFQ0TzRZDw==?=
B 76 Subject: =?ISO-2022-KR?B?GyQpQw5HUTE5Pm4PIA5BJjhxQFQ0TzRZDw==?=
Q 28 FALSE
Q 40 Subject: =?ISO-2022-KR?Q?=1B$)C=0EGQ19?=
 =?ISO-2022-KR?Q?>n=0F=20=0EA&8q@T4O?=
 =?ISO-2022-KR?Q?=1B$)C=0E4Y=0F?=
Q 41 Subject: =?ISO-2022-KR?Q?=1B$)C=0EGQ=0F?=
 =?ISO-2022-KR?Q?19>n=0F=20=0EA&8q@T=0F?=
 =?ISO-2022-KR?Q?=1B$)C=0E4O4Y=0F?=
Q 43 Subject: =?ISO-2022-KR?Q?=1B$)C=0EGQ19=0F?=
 =?ISO-2022-KR?Q?>n=0F=20=0EA&8q@T4O4Y=0F?=
Q 51 Subject: =?ISO-2022-KR?Q?GQ19>n=0F=20=0EA&8q@T=0F?=
 =?ISO-2022-KR?Q?=1B$)C=0E4O4Y=0F?=
Q 76 Subject: =?ISO-2022-KR?Q?=1B$)C=0EGQ19>n=0F=20=0EA&8q@T4O4Y=0F?=
B 28 FALSE
B 40 Subject: =?ISO-2022-KR?B?GyQpQ1I=?=
 =?ISO-2022-KR?B?GyQpQ2U6IA5IOA8=?=
 =?ISO-2022-KR?B?GyQpQw5ARw8gDg8=?=
 =?ISO-2022-KR?B?GyQpQw4wRw8gLSA=?=
 =?ISO-2022-KR?B?GyQpQw4zO0BPDyA=?=
 =?ISO-2022-KR?B?GyQpQzEwDj1DDw==?=
B 41 Subject: =?ISO-2022-KR?B?GyQpQ1I=?=
 =?ISO-2022-KR?B?GyQpQ2U6IA5IOA8=?=
 =?ISO-2022-KR?B?GyQpQw5ARw8gDg8=?=
 =?ISO-2022-KR?B?GyQpQw4wRw8gLSA=?=
 =?ISO-2022-KR?B?GyQpQw4zO0BPDyA=?=
 =?ISO-2022-KR?B?GyQpQzEwDj1DDw==?=
B 43 Subject: =?ISO-2022-KR?B?GyQpQ1JlOiA=?=
 =?ISO-2022-KR?B?GyQpQw5IOEBHDyAOMEcP?=
 =?ISO-2022-KR?B?GyQpQyAtIA4zO0BPDyA=?=
 =?ISO-2022-KR?B?GyQpQzEwDj1DDw==?=
B 51 Subject: =?ISO-2022-KR?B?GyQpQ1JlOiAOSDhARw8=?=
 =?ISO-2022-KR?B?GyQpQyAOMEcPIC0gDjM7QE8PIDE=?=
 =?ISO-2022-KR?B?GyQpQzAOPUMP?=
B 76 Subject: =?ISO-2022-KR?B?GyQpQ1JlOiAOSDhARw8gDjBHDyAtIA4zO0BPDyAxMA4P?=
 =?ISO-2022-KR?B?GyQpQw49Qw8=?=
Q 28 FALSE
Q 40 Subject: =?ISO-2022-KR?Q?=1B$)CRe?=
 =?ISO-2022-KR?Q?=1B$)C:=20=0EH8@G?=
 =?ISO-2022-KR?Q?=0F=20=0E0G=0F=20-=20?=
 =?ISO-2022-KR?Q?=1B$)C=0E3;@O=0F=2010?=
 =?ISO-2022-KR?Q?=1B$)C=0E=3DC=0F?=
Q 41 Subject: =?ISO-2022-KR?Q?=1B$)CRe:=20?=
 =?ISO-2022-KR?Q?=1B$)C=0EH8@G=0F=20=0E?=
 =?ISO-2022-KR?Q?0G=0F=20-=20=0E3;@O?=
 =?ISO-2022-KR?Q?=1B$)C=2010=0E=3DC=0F?=
Q 43 Subject: =?ISO-2022-KR?Q?=1B$)CRe:=20=0E?=
 =?ISO-2022-KR?Q?=1B$)C=0EH8@G=0F=20=0E0G?=
 =?ISO-2022-KR?Q?=1B$)C=20-=20=0E3;@O?=
 =?ISO-2022-KR?Q?=1B$)C=2010=0E=3DC=0F?=
Q 51 Subject: =?ISO-2022-KR?Q?=1B$)CRe:=20=0EH8=0F?=
 =?ISO-2022-KR?Q?=1B$)C=0E@G=0F=20=0E0G=0F=20-=20?=
 =?ISO-2022-KR?Q?=1B$)C=0E3;@O=0F=2010=0E=3DC=0F?=
Q 76 Subject: =?ISO-2022-KR?Q?=1B$)CRe:=20=0EH8@G=0F=20=0E0G=0F=20-=20=0E3;@O?=
 =?ISO-2022-KR?Q?=1B$)C=2010=0E=3DC=0F?=
B 28 FALSE
B 40 Subject: =?ISO-2022-KR?B?GyQpQ2E=?=
 =?ISO-2022-KR?B?GyQpQw5HUQ9iDg8=?=
 =?ISO-2022-KR?B?GyQpQw4xOQ9jDg8=?=
 =?ISO-2022-KR?B?GyQpQw4+bg9k?=
B 41 Subject: =?ISO-2022-KR?B?GyQpQ2E=?=
 =?ISO-2022-KR?B?GyQpQw5HUQ9iDg8=?=
 =?ISO-2022-KR?B?GyQpQw4xOQ9jDg8=?=
 =?ISO-2022-KR?B?GyQpQw4+bg9k?=
B 43 Subject: =?ISO-2022-KR?B?GyQpQ2EOR1EP?=
 =?ISO-2022-KR?B?GyQpQ2IOMTkPYw4+bg8=?=
 =?ISO-2022-KR?B?GyQpQ2Q=?=
B 51 Subject: =?ISO-2022-KR?B?GyQpQ2EOR1EPYg4xOQ8=?=
 =?ISO-2022-KR?B?GyQpQ2MOPm4PZA==?=
B 76 Subject: =?ISO-2022-KR?B?GyQpQ2EOR1EPYg4xOQ9jDj5uD2Q=?=
Q 28 FALSE
Q 40 Subject: =?ISO-2022-KR?Q?=0Fa=0EGQ=0Fb?=
 =?ISO-2022-KR?Q?=1B$)C=0E19=0Fc=0E>n?=
 =?ISO-2022-KR?Q?=1B$)Cd?=
Q 41 Subject: =?ISO-2022-KR?Q?=1B$)Ca=0EGQ?=
 =?ISO-2022-KR?Q?=1B$)Cb=0E19=0Fc=0E>n?=
 =?ISO-2022-KR?Q?=1B$)Cd?=
Q 43 Subject: =?ISO-2022-KR?Q?=1B$)Ca=0EGQ=0Fb?=
 =?ISO-2022-KR?Q?=1B$)C=0E19=0Fc=0E>n=0Fd?=
Q 51 Subject: =?ISO-2022-KR?Q?=1B$)Ca=0EGQ=0Fb=0E=0F?=
 =?ISO-2022-KR?Q?=1B$)C=0E19=0Fc=0E>n=0Fd?=
Q 76 Subject: =?ISO-2022-KR?Q?=1B$)Ca=0EGQ=0Fb=0E19=0Fc=0E>n=0Fd?=
B 28 FALSE
B 40 FALSE
B 41 FALSE
B 43 Subject: =?ISO-2022-KR?B?GyQpQw5HUQ8=?=
 =?ISO-2022-KR?B?GyQpQw4xOT5uR1ExOQ8=?=
 =?ISO-2022-KR?B?GyQpQw4+bkdRMTk+bg8=?=
 =?ISO-2022-KR?B?GyQpQw5HUTE5Pm5HUQ8=?=
 =?ISO-2022-KR?B?GyQpQw4xOT5uR1ExOQ8=?=
 =?ISO-2022-KR?B?GyQpQw4+bkdRMTk+bg8=?=
 =?ISO-2022-KR?B?GyQpQw5HUTE5Pm4P?=
B 51 Subject: =?ISO-2022-KR?B?GyQpQw5HUTE5Pm5HUQ8=?=
 =?ISO-2022-KR?B?GyQpQw4xOT5uR1ExOT5uR1ExOQ8=?=
 =?ISO-2022-KR?B?GyQpQw4+bkdRMTk+bkdRMTk+bg8=?=
 =?ISO-2022-KR?B?GyQpQw5HUTE5Pm5HUTE5Pm4P?=
B 76 Subject: =?ISO-2022-KR?B?GyQpQw5HUTE5Pm5HUTE5Pm5HUTE5Pm5HUTE5Pm5HUQ8=?=
 =?ISO-2022-KR?B?GyQpQw4xOT5uR1ExOT5uR1ExOT5uR1ExOT5uDw==?=
Q 28 Subject: 
 =?ISO-2022-KR?Q?GQ19>n=0F?=
 =?ISO-2022-KR?Q?GQ19>n=0F?=
 =?ISO-2022-KR?Q?GQ19>n=0F?=
 =?ISO-2022-KR?Q?GQ19>n=0F?=
 =?ISO-2022-KR?Q?GQ19>n=0F?=
 =?ISO-2022-KR?Q?GQ19>n=0F?=
 =?ISO-2022-KR?Q?GQ19>n=0F?=
 =?ISO-2022-KR?Q?GQ19>n=0F?=
Q 40 Subject: =?ISO-2022-KR?Q?GQ19>nGQ19=0F?=
 =?ISO-2022-KR?Q?>nGQ19>nGQ19>nGQ19=0F?=
 =?ISO-2022-KR?Q?>nGQ19>nGQ19>nGQ19=0F?=
 =?ISO-2022-KR?Q?=1B$)C=0E>n=0F?=
Q 41 Subject: =?ISO-2022-KR?Q?=1B$)C=0EGQ19?=
 =?ISO-2022-KR?Q?=1B$)C=0E>nGQ19>nGQ19?=
 =?ISO-2022-KR?Q?=1B$)C=0E>nGQ19>nGQ19?=
 =?ISO-2022-KR?Q?=1B$)C=0E>nGQ19>nGQ19?=
 =?ISO-2022-KR?Q?=1B$)C=0E>nGQ19>n=0F?=
Q 43 Subject: =?ISO-2022-KR?Q?=1B$)C=0EGQ19>n?=
 =?ISO-2022-KR?Q?=1B$)C=0EGQ19>nGQ19>nGQ?=
 =?ISO-2022-KR?Q?=1B$)C=0E19>nGQ19>nGQ19?=
 =?ISO-2022-KR?Q?=1B$)C=0E>nGQ19>nGQ19>n?=
Q 51 Subject: =?ISO-2022-KR?Q?=1B$)C=0EGQ19>nGQ19>nGQ?=
 =?ISO-2022-KR?Q?=1B$)C=0E19>nGQ19>nGQ19>nGQ19>n?=
 =?ISO-2022-KR?Q?=1B$)C=0EGQ19>nGQ19>n=0F?=
Q 76 Subject: =?ISO-2022-KR?Q?GQ19>nGQ19>nGQ19>nGQ19>nGQ19>nGQ19>nGQ19>nGQ19=0F?=
 =?ISO-2022-KR?Q?=1B$)C=0E>n=0F?=
