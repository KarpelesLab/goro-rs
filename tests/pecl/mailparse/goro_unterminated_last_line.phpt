--TEST--
goro: the last line of a message without a line break is parsed
--DESCRIPTION--
Upstream mailparse keeps a final line without a line break pending forever,
so it is missing from the part data and from extracted sections (and a header
on the last line of a message without body is never processed). goro processes
the pending data as the end of the message when the message is inspected, and
undoes that when more data is parsed, so incremental parsing gives the same
result as parsing the whole message.
--SKIPIF--
<?php if (!extension_loaded("mailparse")) print "skip"; ?>
--FILE--
<?php
function info($m) {
    foreach (mailparse_msg_get_structure($m) as $s) {
        $d = mailparse_msg_get_part_data(mailparse_msg_get_part($m, $s));
        printf("%s: %s %d-%d body %d-%d lines %d/%d %s\n", $s, json_encode($d["headers"]),
            $d["starting-pos"], $d["ending-pos"], $d["starting-pos-body"], $d["ending-pos-body"],
            $d["line-count"], $d["body-line-count"], $d["content-type"]);
    }
}

echo "-- body\n";
$msg = "Subject: hi\nTo: a@b\n\nline 1\nlast line";
$m = mailparse_msg_create();
mailparse_msg_parse($m, $msg);
info($m);
var_dump(mailparse_msg_extract_part($m, $msg, null));
var_dump(mailparse_msg_extract_whole_part_file($m, fopen("data://text/plain," . rawurlencode($msg), "r"), null));

echo "-- headers only\n";
$m = mailparse_msg_create();
mailparse_msg_parse($m, "Subject: hi\nContent-Type: text/html");
info($m);

echo "-- multipart\n";
$msg = "Mime-Version: 1.0\nContent-Type: multipart/mixed; boundary=b\n\n--b\n"
    . "Content-Transfer-Encoding: base64\n\naGVsbG8=\n--b\n\nsecond\n--b--";
$m = mailparse_msg_create();
mailparse_msg_parse($m, $msg);
info($m);
var_dump(mailparse_msg_extract_part(mailparse_msg_get_part($m, "1.1"), $msg, null));

echo "-- incremental\n";
$m = mailparse_msg_create();
mailparse_msg_parse($m, "Subject: a\nX: 1");
info($m);
mailparse_msg_parse($m, "23\n\nbo");
info($m);
mailparse_msg_parse($m, "dy\n");
info($m);
$whole = mailparse_msg_create();
mailparse_msg_parse($whole, "Subject: a\nX: 123\n\nbody\n");
var_dump(mailparse_msg_get_part_data($m) == mailparse_msg_get_part_data($whole));

echo "-- mimemessage\n";
$mm = new MimeMessage("var", "Subject: x\n\nfirst\nlast");
var_dump($mm->extract_body(MAILPARSE_EXTRACT_RETURN), $mm->data["body-line-count"]);
?>
--EXPECT--
-- body
1: {"subject":"hi","to":"a@b"} 0-37 body 21-37 lines 5/2 text/plain
string(16) "line 1
last line"
string(37) "Subject: hi
To: a@b

line 1
last line"
-- headers only
1: {"subject":"hi","content-type":"text\/html"} 0-35 body 0-35 lines 2/0 text/html
-- multipart
1: {"mime-version":"1.0","content-type":"multipart\/mixed; boundary=b"} 0-126 body 61-126 lines 11/8 multipart/mixed
1.1: {"content-transfer-encoding":"base64"} 65-108 body 100-108 lines 2/0 text/plain
1.2: [] 113-120 body 114-120 lines 1/0 text/plain
string(5) "hello"
-- incremental
1: {"subject":"a","x":"1"} 0-15 body 0-15 lines 2/0 text/plain; (error)
1: {"subject":"a","x":"123"} 0-21 body 19-21 lines 4/1 text/plain
1: {"subject":"a","x":"123"} 0-24 body 19-24 lines 4/1 text/plain
bool(true)
-- mimemessage
string(10) "first
last"
int(2)
