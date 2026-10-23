<?php

/**
 * Signatures of PECL mailparse 3.2 (its arginfo.h: untyped parameters, no
 * return types), read by tools/gen_stub_sigs.php for stub_sigs.rs.
 */

/** @var int */
const MAILPARSE_EXTRACT_OUTPUT = 0;
/** @var int */
const MAILPARSE_EXTRACT_STREAM = 1;
/** @var int */
const MAILPARSE_EXTRACT_RETURN = 2;

function mailparse_msg_parse_file($filename) {}

function mailparse_msg_get_part($fp, $data) {}

function mailparse_msg_get_structure($fp) {}

function mailparse_msg_get_part_data($fp) {}

function mailparse_msg_extract_part($fp, $msgbody, $callback = UNKNOWN) {}

function mailparse_msg_extract_part_file($fp, $filename, $callback = UNKNOWN) {}

function mailparse_msg_extract_whole_part_file($fp, $filename, $callback = UNKNOWN) {}

function mailparse_msg_create() {}

function mailparse_msg_free($fp) {}

function mailparse_msg_parse($fp, $data) {}

function mailparse_rfc822_parse_addresses($addresses) {}

function mailparse_determine_best_xfer_encoding($fp) {}

function mailparse_stream_encode($source_fp, $dest_fp, $encoding) {}

function mailparse_uudecode_all($fp) {}

function mailparse_test($header) {}

class mimemessage
{
    public $data = null;

    public function __construct($mode, $source) {}

    public function get_child($item_to_find) {}

    public function get_child_count() {}

    public function get_parent() {}

    public function extract_headers($mode = UNKNOWN, $arg = UNKNOWN) {}

    public function extract_body($mode = UNKNOWN, $arg = UNKNOWN) {}

    public function enum_uue() {}

    public function extract_uue($index, $mode = UNKNOWN, $arg = UNKNOWN) {}

    public function remove() {}

    public function add_child() {}
}
