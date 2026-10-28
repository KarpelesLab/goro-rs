//! phpcredits() text (as the reference PHP 8.5 CLI prints it).

/// (CREDITS_* flag, section text without the blank line before it)
pub const SECTIONS: &[(i64, &str)] = &[
    (
        1,
        r#"PHP Group
Thies C. Arntzen, Stig Bakken, Shane Caraveo, Andi Gutmans, Rasmus Lerdorf, Sam Ruby, Sascha Schumann, Zeev Suraski, Jim Winstead, Andrei Zmievski
"#,
    ),
    (
        2,
        r#"Language Design & Concept
Andi Gutmans, Rasmus Lerdorf, Zeev Suraski, Marcus Boerger
"#,
    ),
    (
        2,
        r#"                               PHP Authors                               
Contribution => Authors
Zend Scripting Language Engine => Andi Gutmans, Zeev Suraski, Stanislav Malyshev, Marcus Boerger, Dmitry Stogov, Xinchen Hui, Nikita Popov
Extension Module API => Andi Gutmans, Zeev Suraski, Andrei Zmievski
UNIX Build and Modularization => Stig Bakken, Sascha Schumann, Jani Taskinen, Peter Kokot
Windows Support => Shane Caraveo, Zeev Suraski, Wez Furlong, Pierre-Alain Joye, Anatol Belski, Kalle Sommer Nielsen
Server API (SAPI) Abstraction Layer => Andi Gutmans, Shane Caraveo, Zeev Suraski
Streams Abstraction Layer => Wez Furlong, Sara Golemon
PHP Data Objects Layer => Wez Furlong, Marcus Boerger, Sterling Hughes, George Schlossnagle, Ilia Alshanetsky
Output Handler => Zeev Suraski, Thies C. Arntzen, Marcus Boerger, Michael Wallner
Consistent 64 bit support => Anthony Ferrara, Anatol Belski
"#,
    ),
    (
        4,
        r#"                               SAPI Modules                               
Contribution => Authors
Apache 2 Handler => Ian Holsman, Justin Erenkrantz (based on Apache 2 Filter code)
CGI / FastCGI => Rasmus Lerdorf, Stig Bakken, Shane Caraveo, Dmitry Stogov
CLI => Edin Kadribasic, Marcus Boerger, Johannes Schlueter, Moriyoshi Koizumi, Xinchen Hui
Embed => Edin Kadribasic
FastCGI Process Manager => Andrei Nigmatulin, dreamcat4, Antony Dovgal, Jerome Loyet
litespeed => George Wang
phpdbg => Felipe Pena, Joe Watkins, Bob Weinand
"#,
    ),
    (
        8,
        r#"                              Module Authors                              
Module => Authors
BC Math => Andi Gutmans
Bzip2 => Sterling Hughes
Calendar => Shane Caraveo, Colin Viebrock, Hartmut Holzgraefe, Wez Furlong
COM and .Net => Wez Furlong
ctype => Hartmut Holzgraefe
cURL => Sterling Hughes
Date/Time Support => Derick Rethans
DB-LIB (MS SQL, Sybase) => Wez Furlong, Frank M. Kromann, Adam Baratz
DBA => Sascha Schumann, Marcus Boerger
DOM => Christian Stocker, Rob Richards, Marcus Boerger, Niels Dossche
enchant => Pierre-Alain Joye, Ilia Alshanetsky
EXIF => Rasmus Lerdorf, Marcus Boerger
FFI => Dmitry Stogov
fileinfo => Ilia Alshanetsky, Pierre Alain Joye, Scott MacVicar, Derick Rethans, Anatol Belski
Firebird driver for PDO => Ard Biesheuvel
FTP => Stefan Esser, Andrew Skalski
GD imaging => Rasmus Lerdorf, Stig Bakken, Jim Winstead, Jouni Ahto, Ilia Alshanetsky, Pierre-Alain Joye, Marcus Boerger, Mark Randall
GetText => Alex Plotnick
GNU GMP support => Stanislav Malyshev
Iconv => Rui Hirokawa, Stig Bakken, Moriyoshi Koizumi
Input Filter => Rasmus Lerdorf, Derick Rethans, Pierre-Alain Joye, Ilia Alshanetsky
Internationalization => Ed Batutis, Vladimir Iordanov, Dmitry Lakhtyuk, Stanislav Malyshev, Vadim Savchuk, Kirti Velankar
JSON => Jakub Zelenka, Omar Kilani, Scott MacVicar
LDAP => Amitay Isaacs, Eric Warnke, Rasmus Lerdorf, Gerrit Thomson, Stig Venaas
LIBXML => Christian Stocker, Rob Richards, Marcus Boerger, Wez Furlong, Shane Caraveo
Multibyte String Functions => Tsukada Takuya, Rui Hirokawa
MySQL driver for PDO => George Schlossnagle, Wez Furlong, Ilia Alshanetsky, Johannes Schlueter
MySQLi => Zak Greant, Georg Richter, Andrey Hristov, Ulf Wendel
MySQLnd => Andrey Hristov, Ulf Wendel, Georg Richter, Johannes Schlüter
ODBC driver for PDO => Wez Furlong
ODBC => Stig Bakken, Andreas Karajannis, Frank M. Kromann, Daniel R. Kalowsky
Opcache => Andi Gutmans, Zeev Suraski, Stanislav Malyshev, Dmitry Stogov, Xinchen Hui
OpenSSL => Stig Venaas, Wez Furlong, Sascha Kettler, Scott MacVicar, Eliot Lear
pcntl => Jason Greene, Arnaud Le Blanc
Perl Compatible Regexps => Andrei Zmievski
PHP Archive => Gregory Beaver, Marcus Boerger
PHP Data Objects => Wez Furlong, Marcus Boerger, Sterling Hughes, George Schlossnagle, Ilia Alshanetsky
PHP hash => Sara Golemon, Rasmus Lerdorf, Stefan Esser, Michael Wallner, Scott MacVicar
Posix => Kristian Koehntopp
PostgreSQL driver for PDO => Edin Kadribasic, Ilia Alshanetsky
PostgreSQL => Jouni Ahto, Zeev Suraski, Yasuo Ohgaki, Chris Kings-Lynne
random => Go Kudo, Tim Düsterhus, Guilliam Xavier, Christoph M. Becker, Jakub Zelenka, Bob Weinand, Máté Kocsis, and Original RNG implementators
Readline => Thies C. Arntzen
Reflection => Marcus Boerger, Timm Friebe, George Schlossnagle, Andrei Zmievski, Johannes Schlueter
Sessions => Sascha Schumann, Andrei Zmievski
Shared Memory Operations => Slava Poliakov, Ilia Alshanetsky
SimpleXML => Sterling Hughes, Marcus Boerger, Rob Richards
SNMP => Rasmus Lerdorf, Harrie Hazewinkel, Mike Jackson, Steven Lawrance, Johann Hanne, Boris Lytochkin
SOAP => Brad Lafountain, Shane Caraveo, Dmitry Stogov
Sockets => Chris Vandomelen, Sterling Hughes, Daniel Beulshausen, Jason Greene
Sodium => Frank Denis
SPL => Marcus Boerger, Etienne Kneuss
SQLite 3.x driver for PDO => Wez Furlong
SQLite3 => Scott MacVicar, Ilia Alshanetsky, Brad Dewar
System V Message based IPC => Wez Furlong
System V Semaphores => Tom May
System V Shared Memory => Christian Cartus
tidy => John Coggeshall, Ilia Alshanetsky
tokenizer => Andrei Zmievski, Johannes Schlueter
uri => Máté Kocsis, Tim Düsterhus, Ignace Nyamagana Butera, Arnaud Le Blanc, Dennis Snell, Niels Dossche, Nicolas Grekas
XML => Stig Bakken, Thies C. Arntzen, Sterling Hughes
XMLReader => Rob Richards
XMLWriter => Rob Richards, Pierre-Alain Joye
XSL => Christian Stocker, Rob Richards
Zip => Pierre-Alain Joye, Remi Collet
Zlib => Rasmus Lerdorf, Stefan Roehrich, Zeev Suraski, Jade Nicoletti, Michael Wallner
"#,
    ),
    (
        16,
        r#"                            PHP Documentation                            
Authors => Mehdi Achour, Friedhelm Betz, Antony Dovgal, Nuno Lopes, Hannes Magnusson, Philip Olson, Georg Richter, Damien Seguy, Jakub Vrana, Adam Harvey
Editor => Peter Cowburn
User Note Maintainers => Daniel P. Brown, Thiago Henrique Pojda
Other Contributors => Previously active authors, editors and other contributors are listed in the manual.
"#,
    ),
    (
        64,
        r#"PHP Quality Assurance Team
Ilia Alshanetsky, Joerg Behrens, Antony Dovgal, Stefan Esser, Moriyoshi Koizumi, Magnus Maatta, Sebastian Nohn, Derick Rethans, Melvyn Sopacua, Pierre-Alain Joye, Dmitry Stogov, Felipe Pena, David Soria Parra, Stanislav Malyshev, Julien Pauli, Stephen Zarkos, Anatol Belski, Remi Collet, Ferenc Kovacs
"#,
    ),
    (
        128,
        r#"                     Websites and Infrastructure team                     
PHP Websites Team => Rasmus Lerdorf, Hannes Magnusson, Philip Olson, Lukas Kahwe Smith, Pierre-Alain Joye, Kalle Sommer Nielsen, Peter Cowburn, Adam Harvey, Ferenc Kovacs, Levi Morrison
Event Maintainers => Damien Seguy, Daniel P. Brown
Network Infrastructure => Daniel P. Brown
Windows Infrastructure => Alex Schoenmaker
"#,
    ),
];

pub const LICENSE: &str = r#"This program is free software; you can redistribute it and/or modify
it under the terms of the PHP License as published by the PHP Group
and included in the distribution in the file:  LICENSE

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

If you did not receive a copy of the PHP license, or have any
questions about PHP licensing, please contact license@php.net.
"#;
