--TEST--
mb_send_mail(): output written before the call precedes the mailer's
--INI--
sendmail_path=cat
--FILE--
<?php
echo "before\n";
var_dump(mb_send_mail("to@example.com", "日本語の件名", "本文です。\nSecond line", ["From" => "日本 <me@x>", "X-Foo" => "bar"]));
echo "between\n";
mb_language("en");
var_dump(mb_send_mail("a@b.c", "Ä subject", "Ä body"));
echo "after\n";
--EXPECT--
before
To: to@example.com
Subject: =?UTF-8?B?5pel5pys6Kqe44Gu5Lu25ZCN?=
From: 日本 <me@x>
X-Foo: bar
MIME-Version: 1.0
Content-Type: text/plain; charset=UTF-8
Content-Transfer-Encoding: BASE64

5pys5paH44Gn44GZ44CCClNlY29uZCBsaW5l
bool(true)
between
To: a@b.c
Subject: =?ISO-8859-1?Q?=C4=20subject?=
MIME-Version: 1.0
Content-Type: text/plain; charset=ISO-8859-1
Content-Transfer-Encoding: 8bit

� body
bool(true)
after
