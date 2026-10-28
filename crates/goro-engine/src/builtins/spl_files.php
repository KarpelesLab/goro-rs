<?php
/* SPL file classes written in PHP (PHP manual: SplFileInfo,
 * SplFileObject, SplTempFileObject, DirectoryIterator,
 * FilesystemIterator, RecursiveDirectoryIterator, GlobIterator), on top of
 * the engine's file functions. Messages and layouts from php85 output and
 * the ext/spl .phpt expectations. `__*` properties are hidden state,
 * `__h_*` methods hidden helpers. */

class SplFileInfo implements Stringable {
    protected $__path = null;
    protected $__fileClass = 'SplFileObject';
    protected $__infoClass = 'SplFileInfo';
    public function __construct(string $filename) {
        $this->__h_setPath($filename);
    }
    protected function __h_setPath($p) {
        $p = (string) $p;
        $l = strlen($p);
        while ($l > 1 && $p[$l - 1] === '/') {
            $l--;
        }
        $this->__path = substr($p, 0, $l);
    }
    protected function __h_chk() {
        if ($this->__path === null) {
            throw new Error("Object not initialized");
        }
    }
    protected function __h_file() {
        return $this->__path;
    }
    protected function __h_stat($name, $fn) {
        $this->__h_chk();
        $p = $this->getPathname();
        $r = @$fn($p);
        if ($r === false) {
            throw new RuntimeException("SplFileInfo::$name(): stat failed for $p");
        }
        return $r;
    }
    public function getPath(): string {
        $this->__h_chk();
        $p = $this->getPathname();
        $i = strrpos($p, '/');
        if ($i === false) {
            return '';
        }
        return $i === 0 && strlen($p) > 1 ? '' : substr($p, 0, $i);
    }
    public function getFilename(): string {
        $this->__h_chk();
        $p = $this->getPathname();
        $i = strrpos($p, '/');
        if ($i === false || strlen($p) === 1) {
            return $p;
        }
        return substr($p, $i + 1);
    }
    public function getExtension(): string {
        $f = $this->getFilename();
        $i = strrpos($f, '.');
        return $i === false ? '' : substr($f, $i + 1);
    }
    public function getBasename(string $suffix = ""): string {
        $this->__h_chk();
        return basename($this->getFilename(), $suffix);
    }
    public function getPathname(): string {
        $this->__h_chk();
        return $this->__h_file();
    }
    public function getPerms(): int|false { return $this->__h_stat('getPerms', 'fileperms'); }
    public function getInode(): int|false { return $this->__h_stat('getInode', 'fileinode'); }
    public function getSize(): int|false { return $this->__h_stat('getSize', 'filesize'); }
    public function getOwner(): int|false { return $this->__h_stat('getOwner', 'fileowner'); }
    public function getGroup(): int|false { return $this->__h_stat('getGroup', 'filegroup'); }
    public function getATime(): int|false { return $this->__h_stat('getATime', 'fileatime'); }
    public function getMTime(): int|false { return $this->__h_stat('getMTime', 'filemtime'); }
    public function getCTime(): int|false { return $this->__h_stat('getCTime', 'filectime'); }
    public function getType(): string|false {
        $this->__h_chk();
        $p = $this->getPathname();
        $r = @filetype($p);
        if ($r === false) {
            throw new RuntimeException("SplFileInfo::getType(): Lstat failed for $p");
        }
        return $r;
    }
    public function isWritable(): bool { $this->__h_chk(); return is_writable($this->getPathname()); }
    public function isReadable(): bool { $this->__h_chk(); return is_readable($this->getPathname()); }
    public function isExecutable(): bool { $this->__h_chk(); return is_executable($this->getPathname()); }
    public function isFile(): bool { $this->__h_chk(); return is_file($this->getPathname()); }
    public function isDir(): bool { $this->__h_chk(); return is_dir($this->getPathname()); }
    public function isLink(): bool { $this->__h_chk(); return is_link($this->getPathname()); }
    public function getLinkTarget(): string|false {
        $this->__h_chk();
        $p = $this->getPathname();
        $r = @readlink($p);
        if ($r === false) {
            throw new RuntimeException("Unable to read link $p, error: " . (file_exists($p) ? "Invalid argument" : "No such file or directory"));
        }
        return $r;
    }
    public function getRealPath(): string|false {
        $this->__h_chk();
        $p = $this->getPathname();
        return realpath($p === '' ? '.' : $p);
    }
    public function getFileInfo(?string $class = null): SplFileInfo {
        $this->__h_chk();
        $c = $class ?? $this->__infoClass;
        return new $c($this->getPathname());
    }
    public function getPathInfo(?string $class = null): ?SplFileInfo {
        $this->__h_chk();
        $c = $class ?? $this->__infoClass;
        $p = $this->getPath();
        if ($p === '') {
            $p = $this->getPathname() === '' ? '' : '.';
            if ($p === '') {
                return null;
            }
        }
        return new $c($p);
    }
    public function openFile(string $mode = "r", bool $useIncludePath = false, $context = null): SplFileObject {
        $this->__h_chk();
        $c = $this->__fileClass;
        return new $c($this->getPathname(), $mode, $useIncludePath, $context);
    }
    public function setFileClass(string $class = SplFileObject::class): void {
        if (!is_a($class, 'SplFileObject', true)) {
            throw new TypeError('SplFileInfo::setFileClass(): Argument #1 ($class) must be a class name derived from SplFileObject, ' . $class . ' given');
        }
        $this->__fileClass = $class;
    }
    public function setInfoClass(string $class = SplFileInfo::class): void {
        if (!is_a($class, 'SplFileInfo', true)) {
            throw new TypeError('SplFileInfo::setInfoClass(): Argument #1 ($class) must be a class name derived from SplFileInfo, ' . $class . ' given');
        }
        $this->__infoClass = $class;
    }
    public function __toString(): string { return $this->getPathname(); }
    public function __debugInfo(): array {
        $out = get_mangled_object_vars($this);
        $out["\0SplFileInfo\0pathName"] = $this->__path === null ? '' : $this->getPathname();
        $out["\0SplFileInfo\0fileName"] = $this->__path === null ? '' : $this->getFilename();
        return $out;
    }
    final public function _bad_state_ex(): void {
        throw new LogicException("The parent constructor was not called: the object is in an invalid state");
    }
}

class SplFileObject extends SplFileInfo implements RecursiveIterator, SeekableIterator {
    const DROP_NEW_LINE = 1;
    const READ_AHEAD = 2;
    const SKIP_EMPTY = 4;
    const READ_CSV = 8;
    protected $__fp = null;
    protected $__mode = 'r';
    protected $__line = null;
    protected $__csv = null;
    protected $__num = 0;
    protected $__flags = 0;
    protected $__max = 0;
    protected $__sep = ',';
    protected $__enc = '"';
    protected $__esc = '\\';
    public function __construct(string $filename, string $mode = "r", bool $useIncludePath = false, $context = null) {
        if (is_dir($filename)) {
            throw new LogicException("Cannot use SplFileObject with directories");
        }
        error_clear_last();
        $fp = $context === null ? @fopen($filename, $mode, $useIncludePath) : @fopen($filename, $mode, $useIncludePath, $context);
        if ($fp === false) {
            $e = error_get_last();
            $m = $e ? $e['message'] : "Failed to open stream";
            if (str_starts_with($m, 'fopen(')) {
                $m = static::class . '::__construct(' . substr($m, 6);
            }
            throw new RuntimeException($m);
        }
        $this->__fp = $fp;
        $this->__mode = $mode;
        $this->__h_setPath($filename);
    }
    protected function __h_fchk() {
        if ($this->__fp === null) {
            throw new Error("Object not initialized");
        }
    }
    protected function __h_free() {
        $this->__line = null;
        $this->__csv = null;
    }
    /* Reads one line (spl_filesystem_file_read_ex): false at EOF. */
    protected function __h_read($silent, $lineAdd, $csv) {
        $this->__h_free();
        if (feof($this->__fp)) {
            if (!$silent) {
                throw new RuntimeException("Cannot read from file " . $this->getFilename());
            }
            return false;
        }
        $buf = $this->__max > 0 ? fgets($this->__fp, $this->__max + 1) : fgets($this->__fp);
        if ($buf === false) {
            $buf = '';
        } elseif (!$csv && ($this->__flags & 1)) {
            $l = strlen($buf);
            if ($l > 0 && $buf[$l - 1] === "\n") {
                $l--;
                if ($l > 0 && $buf[$l - 1] === "\r") {
                    $l--;
                }
                $buf = substr($buf, 0, $l);
            } elseif ($l > 0 && $buf[$l - 1] === "\r") {
                $buf = substr($buf, 0, $l - 1);
            }
        }
        $this->__line = $buf;
        $this->__num += $lineAdd;
        return true;
    }
    protected function __h_readCsv($sep, $enc, $esc, $silent) {
        do {
            $ok = $this->__h_readRaw($silent, true);
        } while ($ok && $this->__line === '' && ($this->__flags & 4));
        if (!$ok) {
            return false;
        }
        $buf = $this->__line;
        while ($enc !== '' && substr_count($buf, $enc) % 2 === 1 && !feof($this->__fp)) {
            $more = fgets($this->__fp);
            if ($more === false) {
                break;
            }
            $buf .= $more;
        }
        if (str_ends_with($buf, "\r\n")) {
            $buf = substr($buf, 0, -2);
        } elseif (str_ends_with($buf, "\n")) {
            $buf = substr($buf, 0, -1);
        }
        $this->__csv = $buf === '' ? [null] : str_getcsv($buf, $sep, $enc, $esc);
        return true;
    }
    protected function __h_readRaw($silent, $csv) {
        $add = ($this->__line !== null || $this->__csv !== null) ? 1 : 0;
        return $this->__h_read($silent, $add, $csv);
    }
    protected function __h_readLineEx($silent) {
        if (get_class($this) !== 'SplFileObject' && (new ReflectionMethod($this, 'getCurrentLine'))->getDeclaringClass()->getName() !== 'SplFileObject') {
            if (feof($this->__fp)) {
                if (!$silent) {
                    throw new RuntimeException("Cannot read from file " . $this->getFilename());
                }
                return false;
            }
            $v = $this->getCurrentLine();
            $this->__h_free();
            if (is_string($v)) {
                $this->__line = $v;
            } else {
                $this->__csv = $v;
            }
            return true;
        }
        if ($this->__flags & 8) {
            return $this->__h_readCsv($this->__sep, $this->__enc, $this->__esc, $silent);
        }
        return $this->__h_readRaw($silent, false);
    }
    protected function __h_empty() {
        if ($this->__line !== null && $this->__csv === null) {
            return $this->__line === '';
        }
        if ($this->__csv !== null) {
            if ($this->__flags & 8) {
                return count($this->__csv) === 1 && $this->__csv[0] === null;
            }
            return $this->__line === '';
        }
        return true;
    }
    protected function __h_readLine($silent) {
        $ok = $this->__h_readLineEx($silent);
        while (($this->__flags & 4) && $ok && $this->__h_empty()) {
            $this->__h_free();
            $ok = $this->__h_readLineEx($silent);
        }
        return $ok;
    }
    protected function __h_rewind() {
        if (!@rewind($this->__fp)) {
            throw new RuntimeException("Cannot rewind file " . $this->getFilename());
        }
        $this->__h_free();
        $this->__num = 0;
        if ($this->__flags & 2) {
            $this->__h_readLine(true);
        }
    }
    public function rewind(): void { $this->__h_fchk(); $this->__h_rewind(); }
    public function eof(): bool { $this->__h_fchk(); return feof($this->__fp); }
    public function valid(): bool {
        $this->__h_fchk();
        if ($this->__flags & 2) {
            return $this->__line !== null || $this->__csv !== null;
        }
        return !feof($this->__fp);
    }
    public function fgets(): string {
        $this->__h_fchk();
        $this->__h_read(false, 1, false);
        return $this->__line;
    }
    public function fread(int $length): string|false {
        $this->__h_fchk();
        if ($length <= 0) {
            throw new ValueError('SplFileObject::fread(): Argument #1 ($length) must be greater than 0');
        }
        return fread($this->__fp, $length);
    }
    public function fgetcsv(string $separator = ",", string $enclosure = "\"", string $escape = "\\"): array|false {
        $this->__h_fchk();
        $this->__h_csvArgs('fgetcsv', $separator, $enclosure, $escape);
        if (!$this->__h_readCsv($separator, $enclosure, $escape, true)) {
            return false;
        }
        return $this->__csv;
    }
    protected function __h_csvArgs($m, $s, $e, $esc) {
        if (strlen($s) !== 1) {
            throw new ValueError("SplFileObject::$m(): Argument #1 (\$separator) must be a single character");
        }
        if (strlen($e) !== 1) {
            throw new ValueError("SplFileObject::$m(): Argument #2 (\$enclosure) must be a single character");
        }
        if (strlen($esc) > 1) {
            throw new ValueError("SplFileObject::$m(): Argument #3 (\$escape) must be empty or a single character");
        }
    }
    public function fputcsv(array $fields, string $separator = ",", string $enclosure = "\"", string $escape = "\\", string $eol = "\n"): int|false {
        $this->__h_fchk();
        $this->__h_csvArgs('fputcsv', $separator, $enclosure, $escape);
        return fputcsv($this->__fp, $fields, $separator, $enclosure, $escape, $eol);
    }
    public function setCsvControl(string $separator = ",", string $enclosure = "\"", string $escape = "\\"): void {
        $this->__h_csvArgs('setCsvControl', $separator, $enclosure, $escape);
        $this->__sep = $separator;
        $this->__enc = $enclosure;
        $this->__esc = $escape;
    }
    public function getCsvControl(): array { return [$this->__sep, $this->__enc, $this->__esc]; }
    public function flock(int $operation, &$wouldBlock = null): bool { $this->__h_fchk(); return flock($this->__fp, $operation, $wouldBlock); }
    public function fflush(): bool { $this->__h_fchk(); return fflush($this->__fp); }
    public function ftell(): int|false { $this->__h_fchk(); return ftell($this->__fp); }
    public function fseek(int $offset, int $whence = SEEK_SET): int {
        $this->__h_fchk();
        $this->__h_free();
        return fseek($this->__fp, $offset, $whence);
    }
    public function fgetc(): string|false {
        $this->__h_fchk();
        $this->__h_free();
        $c = fgetc($this->__fp);
        if ($c === "\n") {
            $this->__num++;
        }
        return $c;
    }
    public function fpassthru(): int { $this->__h_fchk(); return fpassthru($this->__fp); }
    public function fscanf(string $format, mixed &...$vars): array|int|null {
        $this->__h_fchk();
        $this->__h_free();
        $line = fgets($this->__fp);
        if ($line === false) {
            return null;
        }
        $this->__num++;
        if (count($vars) === 0) {
            return sscanf($line, $format);
        }
        $r = sscanf($line, $format);
        $n = 0;
        foreach ($vars as $i => &$v) {
            if (is_array($r) && array_key_exists($i, $r) && $r[$i] !== null) {
                $v = $r[$i];
                $n++;
            }
        }
        return $n;
    }
    public function fwrite(string $data, int $length = 0): int|false {
        $this->__h_fchk();
        if (func_num_args() > 1) {
            if ($length < 0) {
                return 0;
            }
            $data = substr($data, 0, $length);
        }
        return fwrite($this->__fp, $data);
    }
    public function fstat(): array { $this->__h_fchk(); return fstat($this->__fp); }
    public function ftruncate(int $size): bool {
        $this->__h_fchk();
        if (!@ftruncate($this->__fp, $size)) {
            if (!str_contains($this->__mode, '+') && !str_contains($this->__mode, 'w') && !str_contains($this->__mode, 'a') && !str_contains($this->__mode, 'x') && !str_contains($this->__mode, 'c')) {
                throw new LogicException("Can't truncate file " . $this->getPathname());
            }
            return false;
        }
        return true;
    }
    public function current(): string|array|false {
        $this->__h_fchk();
        if ($this->__line === null && $this->__csv === null) {
            $this->__h_readLine(true);
        }
        if ($this->__line !== null && (!($this->__flags & 8) || $this->__csv === null)) {
            return $this->__line;
        }
        if ($this->__csv !== null) {
            return $this->__csv;
        }
        return false;
    }
    public function key(): int { $this->__h_fchk(); return $this->__num; }
    public function next(): void {
        $this->__h_fchk();
        $this->__h_free();
        if ($this->__flags & 2) {
            $this->__h_readLine(true);
        }
        $this->__num++;
    }
    public function setFlags(int $flags): void { $this->__flags = $flags; }
    public function getFlags(): int { return $this->__flags & 15; }
    public function setMaxLineLen(int $maxLength): void {
        if ($maxLength < 0) {
            throw new ValueError('SplFileObject::setMaxLineLen(): Argument #1 ($maxLength) must be greater than or equal to 0');
        }
        $this->__max = $maxLength;
    }
    public function getMaxLineLen(): int { return $this->__max; }
    public function hasChildren(): false { return false; }
    public function getChildren(): null { return null; }
    public function seek(int $line): void {
        $this->__h_fchk();
        if ($line < 0) {
            throw new ValueError('SplFileObject::seek(): Argument #1 ($line) must be greater than or equal to 0');
        }
        $this->__h_rewind();
        for ($i = 0; $i < $line; $i++) {
            if (!$this->__h_readLine(true)) {
                return;
            }
        }
        if ($line > 0 && !($this->__flags & 2)) {
            $this->__num++;
            $this->__h_free();
        }
    }
    public function getCurrentLine(): string { return $this->fgets(); }
    public function __toString(): string { return (string) $this->current(); }
    public function __debugInfo(): array {
        $out = parent::__debugInfo();
        $out["\0SplFileObject\0openMode"] = $this->__mode;
        $out["\0SplFileObject\0delimiter"] = $this->__sep;
        $out["\0SplFileObject\0enclosure"] = $this->__enc;
        return $out;
    }
}

class SplTempFileObject extends SplFileObject {
    public function __construct(int $maxMemory = 2 * 1024 * 1024) {
        if ($maxMemory < 0) {
            $name = 'php://memory';
        } elseif (func_num_args() > 0) {
            $name = 'php://temp/maxmemory:' . $maxMemory;
        } else {
            $name = 'php://temp';
        }
        $this->__fp = fopen($name, 'wb');
        $this->__mode = 'wb';
        $this->__path = $name;
    }
    public function getFilename(): string { return $this->getPathname(); }
    public function getPath(): string { return ''; }
}

class DirectoryIterator extends SplFileInfo implements SeekableIterator {
    protected $__dir = null;
    protected $__dh = null;
    protected $__entry = '';
    protected $__index = 0;
    protected $__dflags = 0;
    protected $__glob = false;
    protected $__globList = null;
    public function __construct(string $directory) {
        $this->__h_open($directory, 0, static::class);
    }
    protected function __h_open($directory, $flags, $cls) {
        if ($directory === '') {
            throw new ValueError($cls . '::__construct(): Argument #1 ($directory) cannot be empty');
        }
        $this->__dflags = $flags;
        if (str_starts_with($directory, 'glob://') || ($this instanceof GlobIterator)) {
            $pattern = str_starts_with($directory, 'glob://') ? substr($directory, 7) : $directory;
            $this->__glob = true;
            $this->__globList = glob($pattern) ?: [];
            $this->__dir = dirname($pattern);
            if ($this->__globList) {
                $this->__dir = dirname($this->__globList[0]);
            }
        } else {
            error_clear_last();
            $dh = @opendir($directory);
            if ($dh === false) {
                $e = error_get_last();
                $m = $e ? $e['message'] : 'Failed to open directory';
                if (str_starts_with($m, 'opendir(')) {
                    $m = $cls . '::__construct(' . substr($m, 8);
                }
                throw new UnexpectedValueException($m);
            }
            $this->__dh = $dh;
            $l = strlen($directory);
            while ($l > 1 && ($directory[$l - 1] === '/')) {
                $l--;
            }
            $this->__dir = substr($directory, 0, $l);
        }
        $this->__path = $this->__dir;
        $this->__index = 0;
        $this->__h_readEntry();
        if ($this->__dflags & 4096) {
            $this->__h_skipDots();
        }
    }
    protected function __h_chk() {
        if ($this->__dir === null) {
            throw new Error("Object not initialized");
        }
    }
    protected function __h_readEntry() {
        if ($this->__glob) {
            $this->__entry = $this->__globList[$this->__index] ?? '';
            if ($this->__entry !== '') {
                $this->__entry = basename($this->__entry);
            }
            return;
        }
        $e = readdir($this->__dh);
        $this->__entry = $e === false ? '' : $e;
    }
    protected function __h_skipDots() {
        while ($this->__entry === '.' || $this->__entry === '..') {
            $this->__h_readEntry();
        }
    }
    protected function __h_file() {
        if ($this->__entry === '') {
            return $this->__dir === null ? '' : ($this->__glob ? '' : '');
        }
        return $this->__dir . '/' . $this->__entry;
    }
    public function getFilename(): string { $this->__h_chk(); return $this->__entry; }
    public function getPath(): string { $this->__h_chk(); return $this->__dir; }
    public function getPathname(): string {
        $this->__h_chk();
        return $this->__entry === '' ? '' : $this->__dir . '/' . $this->__entry;
    }
    public function getExtension(): string {
        $f = $this->getFilename();
        $i = strrpos($f, '.');
        return $i === false ? '' : substr($f, $i + 1);
    }
    public function getBasename(string $suffix = ""): string {
        $this->__h_chk();
        return basename($this->__entry, $suffix);
    }
    public function isDot(): bool { $this->__h_chk(); return $this->__entry === '.' || $this->__entry === '..'; }
    public function rewind(): void {
        $this->__h_chk();
        $this->__index = 0;
        if (!$this->__glob && $this->__dh) {
            rewinddir($this->__dh);
        }
        $this->__h_readEntry();
        if ($this->__dflags & 4096) {
            $this->__h_skipDots();
        }
    }
    public function valid(): bool { $this->__h_chk(); return $this->__entry !== ''; }
    public function key(): mixed { $this->__h_chk(); return $this->__index; }
    public function current(): mixed { $this->__h_chk(); return $this; }
    public function next(): void {
        $this->__h_chk();
        $this->__index++;
        $this->__h_readEntry();
        if ($this->__dflags & 4096) {
            $this->__h_skipDots();
        }
    }
    public function seek(int $offset): void {
        $this->__h_chk();
        if ($this->__index > $offset) {
            $this->rewind();
        }
        while ($this->__index < $offset) {
            $this->next();
            if (!$this->valid()) {
                throw new OutOfBoundsException("Seek position $offset is out of range");
            }
        }
    }
    public function __toString(): string { return $this->getFilename(); }
    public function __debugInfo(): array {
        $out = get_mangled_object_vars($this);
        $out["\0SplFileInfo\0pathName"] = $this->__dir === null ? '' : $this->getPathname();
        if ($this->__entry !== '' && !$this->isDot() && false) {
            $out["\0SplFileInfo\0fileName"] = $this->getFilename();
        }
        $out["\0DirectoryIterator\0glob"] = $this->__glob;
        $out["\0RecursiveDirectoryIterator\0subPathName"] = '';
        return $out;
    }
}

class FilesystemIterator extends DirectoryIterator {
    const CURRENT_MODE_MASK = 240;
    const CURRENT_AS_PATHNAME = 32;
    const CURRENT_AS_FILEINFO = 0;
    const CURRENT_AS_SELF = 16;
    const KEY_MODE_MASK = 3840;
    const KEY_AS_PATHNAME = 0;
    const FOLLOW_SYMLINKS = 16384;
    const KEY_AS_FILENAME = 256;
    const NEW_CURRENT_AND_KEY = 256;
    const OTHER_MODE_MASK = 28672;
    const SKIP_DOTS = 4096;
    const UNIX_PATHS = 8192;
    public function __construct(string $directory, int $flags = FilesystemIterator::KEY_AS_PATHNAME | FilesystemIterator::CURRENT_AS_FILEINFO | FilesystemIterator::SKIP_DOTS) {
        $this->__h_open($directory, $flags, static::class);
    }
    public function rewind(): void { parent::rewind(); }
    public function key(): mixed {
        $this->__h_chk();
        if ($this->__dflags & 256) {
            return $this->getFilename();
        }
        return $this->getPathname();
    }
    public function current(): mixed {
        $this->__h_chk();
        if ($this->__dflags & 32) {
            return $this->getPathname();
        }
        if ($this->__dflags & 16) {
            return $this;
        }
        $c = $this->__infoClass;
        return new $c($this->getPathname());
    }
    public function getFlags(): int { return $this->__dflags & (3840 | 240 | 28672); }
    public function setFlags(int $flags): void {
        $this->__dflags = ($this->__dflags & ~(3840 | 240 | 28672)) | ($flags & (3840 | 240 | 28672));
    }
}

class RecursiveDirectoryIterator extends FilesystemIterator implements RecursiveIterator {
    protected $__sub = '';
    public function __construct(string $directory, int $flags = FilesystemIterator::KEY_AS_PATHNAME | FilesystemIterator::CURRENT_AS_FILEINFO) {
        $this->__h_open($directory, $flags, static::class);
    }
    public function hasChildren(bool $allowLinks = false): bool {
        $this->__h_chk();
        if ($this->isDot() || $this->__entry === '') {
            return false;
        }
        $p = $this->getPathname();
        if (!$allowLinks && !($this->__dflags & 16384) && is_link($p)) {
            return false;
        }
        return is_dir($p);
    }
    public function getChildren(): RecursiveDirectoryIterator {
        $this->__h_chk();
        $c = new static($this->getPathname(), $this->__dflags);
        $c->__sub = $this->__sub === '' ? $this->__entry : $this->__sub . '/' . $this->__entry;
        $c->__infoClass = $this->__infoClass;
        $c->__fileClass = $this->__fileClass;
        return $c;
    }
    public function getSubPath(): string { $this->__h_chk(); return $this->__sub; }
    public function getSubPathname(): string {
        $this->__h_chk();
        return $this->__sub === '' ? $this->__entry : $this->__sub . '/' . $this->__entry;
    }
    public function __debugInfo(): array {
        $out = parent::__debugInfo();
        $out["\0RecursiveDirectoryIterator\0subPathName"] = $this->__sub;
        return $out;
    }
}

class GlobIterator extends FilesystemIterator implements Countable {
    public function __construct(string $pattern, int $flags = FilesystemIterator::KEY_AS_PATHNAME | FilesystemIterator::CURRENT_AS_FILEINFO) {
        $this->__h_open($pattern, $flags, static::class);
    }
    public function count(): int { $this->__h_chk(); return count($this->__globList); }
}
