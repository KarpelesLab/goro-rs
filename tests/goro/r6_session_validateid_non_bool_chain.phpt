--TEST--
session: a non-bool validateId() leaves create_sid() unrun and chains the errors
--FILE--
<?php
ob_start();
ini_set('session.use_cookies', 0);
ini_set('session.cache_limiter', '');
ini_set('session.use_strict_mode', 1);
class H implements SessionHandlerInterface, SessionIdInterface, SessionUpdateTimestampHandlerInterface {
    public $valid;
    function __construct($v) { $this->valid = $v; }
    function open($p, $n): bool { return true; }
    function close(): bool { return true; }
    function read($id): string|false { echo "read $id\n"; return ''; }
    function write($id, $d): bool { return true; }
    function destroy($id): bool { return true; }
    function gc($m): int|false { return 0; }
    function create_sid(): string { echo "create_sid\n"; return 'newid'; }
    #[ReturnTypeWillChange] function validateId($id) { echo "validateId $id\n"; return $this->valid; }
    function updateTimestamp($id, $d): bool { return true; }
}
// A validateId() that does not return a bool throws a TypeError; the
// following create_sid() call then runs nothing (an exception is pending)
// and its NULL gives "Session id must be a string", chained to it.
foreach ([null, 'x', 0, false] as $v) {
    session_set_save_handler(new H($v));
    session_id('given');
    try {
        var_dump(session_start());
    } catch (Throwable $e) {
        do {
            echo get_class($e), ": ", $e->getMessage(), "\n";
        } while ($e = $e->getPrevious());
    }
    var_dump(session_id(), session_status());
    @session_write_close();
}
--EXPECTF--
validateId given
Error: Session id must be a string
TypeError: Session callback must have a return value of type bool, null returned
string(0) ""
int(1)
validateId given
Error: Session id must be a string
TypeError: Session callback must have a return value of type bool, string returned
string(0) ""
int(1)
validateId given

Deprecated: session_start(): Session callback must have a return value of type bool, int returned in %s on line 26
read given
bool(true)
string(5) "given"
int(2)
validateId given
create_sid
read newid
bool(true)
string(5) "newid"
int(2)
