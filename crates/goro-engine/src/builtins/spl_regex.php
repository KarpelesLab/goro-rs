<?php
/* RegexIterator and RecursiveRegexIterator (PHP manual), on preg_*. */

class RegexIterator extends FilterIterator {
    const USE_KEY = 1;
    const INVERT_MATCH = 2;
    const MATCH = 0;
    const GET_MATCH = 1;
    const ALL_MATCHES = 2;
    const SPLIT = 3;
    const REPLACE = 4;
    public ?string $replacement = null;
    protected $__rx = '';
    protected $__mode = 0;
    protected $__rflags = 0;
    protected $__pflags = 0;
    public function __construct(Iterator $iterator, string $pattern, int $mode = RegexIterator::MATCH, int $flags = 0, int $pregFlags = 0) {
        if ($mode < 0 || $mode > 4) {
            throw new ValueError(static::class . '::__construct(): Argument #3 ($mode) must be RegexIterator::MATCH, RegexIterator::GET_MATCH, RegexIterator::ALL_MATCHES, RegexIterator::SPLIT, or RegexIterator::REPLACE');
        }
        parent::__construct($iterator);
        $this->__rx = $pattern;
        $this->__mode = $mode;
        $this->__rflags = $flags;
        $this->__pflags = $pregFlags;
    }
    public function accept(): bool {
        $this->__h_chk();
        if (!$this->__valid || is_array($this->__cur)) {
            return false;
        }
        $subject = ($this->__rflags & 1) ? (string) $this->__key : (string) $this->__cur;
        switch ($this->__mode) {
            case 0:
                $r = preg_match($this->__rx, $subject) > 0;
                break;
            case 1:
                $m = [];
                $r = preg_match($this->__rx, $subject, $m, $this->__pflags) > 0;
                if ($r) {
                    $this->__cur = $m;
                }
                break;
            case 2:
                $m = [];
                preg_match_all($this->__rx, $subject, $m, $this->__pflags ?: PREG_PATTERN_ORDER);
                $r = isset($m[0]) && count($m[0]) > 0;
                $this->__cur = $m;
                break;
            case 3:
                $m = preg_split($this->__rx, $subject, -1, $this->__pflags);
                $r = is_array($m) && count($m) > 1;
                if ($r) {
                    $this->__cur = $m;
                }
                break;
            default:
                $res = preg_replace($this->__rx, (string) $this->replacement, $subject);
                $r = $res !== null;
                if ($r) {
                    if ($this->__rflags & 1) {
                        $this->__key = $res;
                    } else {
                        $this->__cur = $res;
                    }
                }
                break;
        }
        if ($this->__rflags & 2) {
            return !$r;
        }
        return $r;
    }
    public function getMode(): int { $this->__h_chk(); return $this->__mode; }
    public function setMode(int $mode): void {
        $this->__h_chk();
        if ($mode < 0 || $mode > 4) {
            throw new ValueError(static::class . '::setMode(): Argument #1 ($mode) must be RegexIterator::MATCH, RegexIterator::GET_MATCH, RegexIterator::ALL_MATCHES, RegexIterator::SPLIT, or RegexIterator::REPLACE');
        }
        $this->__mode = $mode;
    }
    public function getFlags(): int { $this->__h_chk(); return $this->__rflags; }
    public function setFlags(int $flags): void { $this->__h_chk(); $this->__rflags = $flags; }
    public function getPregFlags(): int { $this->__h_chk(); return $this->__pflags; }
    public function setPregFlags(int $pregFlags): void { $this->__h_chk(); $this->__pflags = $pregFlags; }
    public function getRegex(): string { $this->__h_chk(); return $this->__rx; }
}

class RecursiveRegexIterator extends RegexIterator implements RecursiveIterator {
    public function __construct(RecursiveIterator $iterator, string $pattern, int $mode = RecursiveRegexIterator::MATCH, int $flags = 0, int $pregFlags = 0) {
        parent::__construct($iterator, $pattern, $mode, $flags, $pregFlags);
    }
    public function accept(): bool {
        if (!$this->__valid) {
            return false;
        }
        if (is_array($this->__cur)) {
            return count($this->__cur) > 0;
        }
        return parent::accept();
    }
    public function hasChildren(): bool { $this->__h_chk(); return $this->__it->hasChildren(); }
    public function getChildren(): RecursiveRegexIterator {
        $this->__h_chk();
        return new static($this->__it->getChildren(), $this->__rx, $this->__mode, $this->__rflags, $this->__pflags);
    }
}
