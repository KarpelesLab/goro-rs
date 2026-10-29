<?php
class PharFileInfo extends SplFileInfo {
    public function __construct(string $filename) {
        $this->__h_pharInit($filename);
        parent::__construct($filename);
    }
    public function __destruct() {}
    protected function __h_pharInit($filename) {}
    public function chmod(int $perms): void {}
    public function compress(int $compression): true {}
    public function decompress(): true {}
    public function delMetadata(): bool {}
    public function getCRC32(): int {}
    public function getCompressedSize(): int {}
    public function getContent(): string {}
    public function getMetadata(array $unserializeOptions = []): mixed {}
    public function getPharFlags(): int {}
    public function hasMetadata(): bool {}
    public function isCRCChecked(): bool {}
    public function isCompressed(?int $compression = null): bool {}
    public function setMetadata(mixed $metadata): void {}
}
