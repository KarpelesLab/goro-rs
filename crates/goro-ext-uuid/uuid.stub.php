<?php

/**
 * Signatures of PECL uuid 1.3 as built against util-linux libuuid >= 2.41
 * (UUID_TYPE_DCE_TIME_V6/V7/VENDOR available, uuid_time64()).
 *
 * Read by tools/gen_stub_sigs.php (stub_sigs.rs: arginfo and Reflection).
 * The constant values are the ones of <uuid/uuid.h>; goro-ext-uuid
 * registers them.
 */

/**
 * @var int
 * @cvalue UUID_VARIANT_NCS
 */
const UUID_VARIANT_NCS = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_VARIANT_DCE
 */
const UUID_VARIANT_DCE = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_VARIANT_MICROSOFT
 */
const UUID_VARIANT_MICROSOFT = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_VARIANT_OTHER
 */
const UUID_VARIANT_OTHER = UNKNOWN;

/** @var int */
const UUID_TYPE_DEFAULT = 0;

/**
 * @var int
 * @cvalue UUID_TYPE_DCE
 * @deprecated
 */
const UUID_TYPE_DCE = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_TYPE_NAME
 * @deprecated
 */
const UUID_TYPE_NAME = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_TYPE_TIME
 */
const UUID_TYPE_TIME = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_TYPE_DCE_TIME_V6
 */
const UUID_TYPE_TIME_V6 = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_TYPE_DCE_TIME_V7
 */
const UUID_TYPE_TIME_V7 = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_TYPE_DCE_VENDOR
 */
const UUID_TYPE_VENDOR = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_TYPE_DCE_SECURITY
 */
const UUID_TYPE_SECURITY = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_TYPE_DCE_MD5
 */
const UUID_TYPE_MD5 = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_TYPE_DCE_RANDOM
 */
const UUID_TYPE_RANDOM = UNKNOWN;

/**
 * @var int
 * @cvalue UUID_TYPE_DCE_SHA1
 */
const UUID_TYPE_SHA1 = UNKNOWN;

/** @var int */
const UUID_TYPE_NULL = -1;

/** @var int */
const UUID_TYPE_INVALID = -42;

function uuid_create(int $uuid_type = UUID_TYPE_DEFAULT): string {}

function uuid_is_valid(string $uuid): bool {}

function uuid_compare(string $uuid1, string $uuid2): int {}

function uuid_is_null(string $uuid): bool {}

function uuid_generate_md5(string $uuid_ns, string $name): string {}

function uuid_generate_sha1(string $uuid_ns, string $name): string {}

function uuid_type(string $uuid): int {}

function uuid_variant(string $uuid): int {}

function uuid_time(string $uuid): int {}

function uuid_mac(string $uuid): string {}

function uuid_parse(string $uuid): string {}

function uuid_unparse(string $uuid): string {}
