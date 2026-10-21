//! mysqlnd statistics (mysqlnd_statistics.c): the collected counters, named
//! as `mysqlnd_stats_values_names` (index = `enum_mysqlnd_collected_stats`).
#![allow(dead_code)]

pub const STAT_BYTES_SENT: usize = 0;
pub const STAT_BYTES_RECEIVED: usize = 1;
pub const STAT_PACKETS_SENT: usize = 2;
pub const STAT_PACKETS_RECEIVED: usize = 3;
pub const STAT_PROTOCOL_OVERHEAD_IN: usize = 4;
pub const STAT_PROTOCOL_OVERHEAD_OUT: usize = 5;
pub const STAT_BYTES_RECEIVED_OK: usize = 6;
pub const STAT_BYTES_RECEIVED_EOF: usize = 7;
pub const STAT_BYTES_RECEIVED_RSET_HEADER: usize = 8;
pub const STAT_BYTES_RECEIVED_RSET_FIELD_META: usize = 9;
pub const STAT_BYTES_RECEIVED_RSET_ROW: usize = 10;
pub const STAT_BYTES_RECEIVED_PREPARE_RESPONSE: usize = 11;
pub const STAT_BYTES_RECEIVED_CHANGE_USER: usize = 12;
pub const STAT_PACKETS_SENT_CMD: usize = 13;
pub const STAT_PACKETS_RECEIVED_OK: usize = 14;
pub const STAT_PACKETS_RECEIVED_EOF: usize = 15;
pub const STAT_PACKETS_RECEIVED_RSET_HEADER: usize = 16;
pub const STAT_PACKETS_RECEIVED_RSET_FIELD_META: usize = 17;
pub const STAT_PACKETS_RECEIVED_RSET_ROW: usize = 18;
pub const STAT_PACKETS_RECEIVED_PREPARE_RESPONSE: usize = 19;
pub const STAT_PACKETS_RECEIVED_CHANGE_USER: usize = 20;
pub const STAT_RSET_QUERY: usize = 21;
pub const STAT_NON_RSET_QUERY: usize = 22;
pub const STAT_NO_INDEX_USED: usize = 23;
pub const STAT_BAD_INDEX_USED: usize = 24;
pub const STAT_QUERY_WAS_SLOW: usize = 25;
pub const STAT_BUFFERED_SETS: usize = 26;
pub const STAT_UNBUFFERED_SETS: usize = 27;
pub const STAT_PS_BUFFERED_SETS: usize = 28;
pub const STAT_PS_UNBUFFERED_SETS: usize = 29;
pub const STAT_FLUSHED_NORMAL_SETS: usize = 30;
pub const STAT_FLUSHED_PS_SETS: usize = 31;
pub const STAT_PS_PREPARED_NEVER_EXECUTED: usize = 32;
pub const STAT_PS_PREPARED_ONCE_USED: usize = 33;
pub const STAT_ROWS_FETCHED_FROM_SERVER_NORMAL: usize = 34;
pub const STAT_ROWS_FETCHED_FROM_SERVER_PS: usize = 35;
pub const STAT_ROWS_BUFFERED_FROM_CLIENT_NORMAL: usize = 36;
pub const STAT_ROWS_BUFFERED_FROM_CLIENT_PS: usize = 37;
pub const STAT_ROWS_FETCHED_FROM_CLIENT_NORMAL_BUF: usize = 38;
pub const STAT_ROWS_FETCHED_FROM_CLIENT_NORMAL_UNBUF: usize = 39;
pub const STAT_ROWS_FETCHED_FROM_CLIENT_PS_BUF: usize = 40;
pub const STAT_ROWS_FETCHED_FROM_CLIENT_PS_UNBUF: usize = 41;
pub const STAT_ROWS_FETCHED_FROM_CLIENT_PS_CURSOR: usize = 42;
pub const STAT_ROWS_AFFECTED_NORMAL: usize = 43;
pub const STAT_ROWS_AFFECTED_PS: usize = 44;
pub const STAT_ROWS_SKIPPED_NORMAL: usize = 45;
pub const STAT_ROWS_SKIPPED_PS: usize = 46;
pub const STAT_COPY_ON_WRITE_SAVED: usize = 47;
pub const STAT_COPY_ON_WRITE_PERFORMED: usize = 48;
pub const STAT_CMD_BUFFER_TOO_SMALL: usize = 49;
pub const STAT_CONNECT_SUCCESS: usize = 50;
pub const STAT_CONNECT_FAILURE: usize = 51;
pub const STAT_CONNECT_REUSED: usize = 52;
pub const STAT_RECONNECT: usize = 53;
pub const STAT_PCONNECT_SUCCESS: usize = 54;
pub const STAT_OPENED_CONNECTIONS: usize = 55;
pub const STAT_OPENED_PERSISTENT_CONNECTIONS: usize = 56;
pub const STAT_CLOSE_EXPLICIT: usize = 57;
pub const STAT_CLOSE_IMPLICIT: usize = 58;
pub const STAT_CLOSE_DISCONNECT: usize = 59;
pub const STAT_CLOSE_IN_MIDDLE: usize = 60;
pub const STAT_FREE_RESULT_EXPLICIT: usize = 61;
pub const STAT_FREE_RESULT_IMPLICIT: usize = 62;
pub const STAT_STMT_CLOSE_EXPLICIT: usize = 63;
pub const STAT_STMT_CLOSE_IMPLICIT: usize = 64;
pub const STAT_MEM_EMALLOC_COUNT: usize = 65;
pub const STAT_MEM_EMALLOC_AMOUNT: usize = 66;
pub const STAT_MEM_ECALLOC_COUNT: usize = 67;
pub const STAT_MEM_ECALLOC_AMOUNT: usize = 68;
pub const STAT_MEM_EREALLOC_COUNT: usize = 69;
pub const STAT_MEM_EREALLOC_AMOUNT: usize = 70;
pub const STAT_MEM_EFREE_COUNT: usize = 71;
pub const STAT_MEM_EFREE_AMOUNT: usize = 72;
pub const STAT_MEM_MALLOC_COUNT: usize = 73;
pub const STAT_MEM_MALLOC_AMOUNT: usize = 74;
pub const STAT_MEM_CALLOC_COUNT: usize = 75;
pub const STAT_MEM_CALLOC_AMOUNT: usize = 76;
pub const STAT_MEM_REALLOC_COUNT: usize = 77;
pub const STAT_MEM_REALLOC_AMOUNT: usize = 78;
pub const STAT_MEM_FREE_COUNT: usize = 79;
pub const STAT_MEM_FREE_AMOUNT: usize = 80;
pub const STAT_MEM_ESTRNDUP_COUNT: usize = 81;
pub const STAT_MEM_STRNDUP_COUNT: usize = 82;
pub const STAT_MEM_ESTRDUP_COUNT: usize = 83;
pub const STAT_MEM_STRDUP_COUNT: usize = 84;
pub const STAT_MEM_EDUP_COUNT: usize = 85;
pub const STAT_MEM_DUP_COUNT: usize = 86;
pub const STAT_TEXT_TYPE_FETCHED_NULL: usize = 87;
pub const STAT_TEXT_TYPE_FETCHED_BIT: usize = 88;
pub const STAT_TEXT_TYPE_FETCHED_INT8: usize = 89;
pub const STAT_TEXT_TYPE_FETCHED_INT16: usize = 90;
pub const STAT_TEXT_TYPE_FETCHED_INT24: usize = 91;
pub const STAT_TEXT_TYPE_FETCHED_INT32: usize = 92;
pub const STAT_TEXT_TYPE_FETCHED_INT64: usize = 93;
pub const STAT_TEXT_TYPE_FETCHED_DECIMAL: usize = 94;
pub const STAT_TEXT_TYPE_FETCHED_FLOAT: usize = 95;
pub const STAT_TEXT_TYPE_FETCHED_DOUBLE: usize = 96;
pub const STAT_TEXT_TYPE_FETCHED_DATE: usize = 97;
pub const STAT_TEXT_TYPE_FETCHED_YEAR: usize = 98;
pub const STAT_TEXT_TYPE_FETCHED_TIME: usize = 99;
pub const STAT_TEXT_TYPE_FETCHED_DATETIME: usize = 100;
pub const STAT_TEXT_TYPE_FETCHED_TIMESTAMP: usize = 101;
pub const STAT_TEXT_TYPE_FETCHED_STRING: usize = 102;
pub const STAT_TEXT_TYPE_FETCHED_JSON: usize = 103;
pub const STAT_TEXT_TYPE_FETCHED_BLOB: usize = 104;
pub const STAT_TEXT_TYPE_FETCHED_ENUM: usize = 105;
pub const STAT_TEXT_TYPE_FETCHED_SET: usize = 106;
pub const STAT_TEXT_TYPE_FETCHED_GEOMETRY: usize = 107;
pub const STAT_TEXT_TYPE_FETCHED_OTHER: usize = 108;
pub const STAT_BINARY_TYPE_FETCHED_NULL: usize = 109;
pub const STAT_BINARY_TYPE_FETCHED_BIT: usize = 110;
pub const STAT_BINARY_TYPE_FETCHED_INT8: usize = 111;
pub const STAT_BINARY_TYPE_FETCHED_INT16: usize = 112;
pub const STAT_BINARY_TYPE_FETCHED_INT24: usize = 113;
pub const STAT_BINARY_TYPE_FETCHED_INT32: usize = 114;
pub const STAT_BINARY_TYPE_FETCHED_INT64: usize = 115;
pub const STAT_BINARY_TYPE_FETCHED_DECIMAL: usize = 116;
pub const STAT_BINARY_TYPE_FETCHED_FLOAT: usize = 117;
pub const STAT_BINARY_TYPE_FETCHED_DOUBLE: usize = 118;
pub const STAT_BINARY_TYPE_FETCHED_DATE: usize = 119;
pub const STAT_BINARY_TYPE_FETCHED_YEAR: usize = 120;
pub const STAT_BINARY_TYPE_FETCHED_TIME: usize = 121;
pub const STAT_BINARY_TYPE_FETCHED_DATETIME: usize = 122;
pub const STAT_BINARY_TYPE_FETCHED_TIMESTAMP: usize = 123;
pub const STAT_BINARY_TYPE_FETCHED_STRING: usize = 124;
pub const STAT_BINARY_TYPE_FETCHED_BLOB: usize = 125;
pub const STAT_BINARY_TYPE_FETCHED_ENUM: usize = 126;
pub const STAT_BINARY_TYPE_FETCHED_SET: usize = 127;
pub const STAT_BINARY_TYPE_FETCHED_GEOMETRY: usize = 128;
pub const STAT_BINARY_TYPE_FETCHED_OTHER: usize = 129;
pub const STAT_INIT_COMMAND_EXECUTED_COUNT: usize = 130;
pub const STAT_INIT_COMMAND_FAILED_COUNT: usize = 131;
pub const STAT_COM_QUIT: usize = 132;
pub const STAT_COM_INIT_DB: usize = 133;
pub const STAT_COM_QUERY: usize = 134;
pub const STAT_COM_FIELD_LIST: usize = 135;
pub const STAT_COM_CREATE_DB: usize = 136;
pub const STAT_COM_DROP_DB: usize = 137;
pub const STAT_COM_REFRESH: usize = 138;
pub const STAT_COM_SHUTDOWN: usize = 139;
pub const STAT_COM_STATISTICS: usize = 140;
pub const STAT_COM_PROCESS_INFO: usize = 141;
pub const STAT_COM_CONNECT: usize = 142;
pub const STAT_COM_PROCESS_KILL: usize = 143;
pub const STAT_COM_DEBUG: usize = 144;
pub const STAT_COM_PING: usize = 145;
pub const STAT_COM_TIME: usize = 146;
pub const STAT_COM_DELAYED_INSERT: usize = 147;
pub const STAT_COM_CHANGE_USER: usize = 148;
pub const STAT_COM_BINLOG_DUMP: usize = 149;
pub const STAT_COM_TABLE_DUMP: usize = 150;
pub const STAT_COM_CONNECT_OUT: usize = 151;
pub const STAT_COM_REGISTER_SLAVE: usize = 152;
pub const STAT_COM_STMT_PREPARE: usize = 153;
pub const STAT_COM_STMT_EXECUTE: usize = 154;
pub const STAT_COM_STMT_SEND_LONG_DATA: usize = 155;
pub const STAT_COM_STMT_CLOSE: usize = 156;
pub const STAT_COM_STMT_RESET: usize = 157;
pub const STAT_COM_SET_OPTION: usize = 158;
pub const STAT_COM_STMT_FETCH: usize = 159;
pub const STAT_COM_DAEMON: usize = 160;
pub const STAT_BYTES_RECEIVED_PURE_DATA_TEXT: usize = 161;
pub const STAT_BYTES_RECEIVED_PURE_DATA_PS: usize = 162;
pub const STAT_LAST: usize = 163;

pub static NAMES: [&str; STAT_LAST] = [
    "bytes_sent",
    "bytes_received",
    "packets_sent",
    "packets_received",
    "protocol_overhead_in",
    "protocol_overhead_out",
    "bytes_received_ok_packet",
    "bytes_received_eof_packet",
    "bytes_received_rset_header_packet",
    "bytes_received_rset_field_meta_packet",
    "bytes_received_rset_row_packet",
    "bytes_received_prepare_response_packet",
    "bytes_received_change_user_packet",
    "packets_sent_command",
    "packets_received_ok",
    "packets_received_eof",
    "packets_received_rset_header",
    "packets_received_rset_field_meta",
    "packets_received_rset_row",
    "packets_received_prepare_response",
    "packets_received_change_user",
    "result_set_queries",
    "non_result_set_queries",
    "no_index_used",
    "bad_index_used",
    "slow_queries",
    "buffered_sets",
    "unbuffered_sets",
    "ps_buffered_sets",
    "ps_unbuffered_sets",
    "flushed_normal_sets",
    "flushed_ps_sets",
    "ps_prepared_never_executed",
    "ps_prepared_once_executed",
    "rows_fetched_from_server_normal",
    "rows_fetched_from_server_ps",
    "rows_buffered_from_client_normal",
    "rows_buffered_from_client_ps",
    "rows_fetched_from_client_normal_buffered",
    "rows_fetched_from_client_normal_unbuffered",
    "rows_fetched_from_client_ps_buffered",
    "rows_fetched_from_client_ps_unbuffered",
    "rows_fetched_from_client_ps_cursor",
    "rows_affected_normal",
    "rows_affected_ps",
    "rows_skipped_normal",
    "rows_skipped_ps",
    "copy_on_write_saved",
    "copy_on_write_performed",
    "command_buffer_too_small",
    "connect_success",
    "connect_failure",
    "connection_reused",
    "reconnect",
    "pconnect_success",
    "active_connections",
    "active_persistent_connections",
    "explicit_close",
    "implicit_close",
    "disconnect_close",
    "in_middle_of_command_close",
    "explicit_free_result",
    "implicit_free_result",
    "explicit_stmt_close",
    "implicit_stmt_close",
    "mem_emalloc_count",
    "mem_emalloc_amount",
    "mem_ecalloc_count",
    "mem_ecalloc_amount",
    "mem_erealloc_count",
    "mem_erealloc_amount",
    "mem_efree_count",
    "mem_efree_amount",
    "mem_malloc_count",
    "mem_malloc_amount",
    "mem_calloc_count",
    "mem_calloc_amount",
    "mem_realloc_count",
    "mem_realloc_amount",
    "mem_free_count",
    "mem_free_amount",
    "mem_estrndup_count",
    "mem_strndup_count",
    "mem_estrdup_count",
    "mem_strdup_count",
    "mem_edupl_count",
    "mem_dupl_count",
    "proto_text_fetched_null",
    "proto_text_fetched_bit",
    "proto_text_fetched_tinyint",
    "proto_text_fetched_short",
    "proto_text_fetched_int24",
    "proto_text_fetched_int",
    "proto_text_fetched_bigint",
    "proto_text_fetched_decimal",
    "proto_text_fetched_float",
    "proto_text_fetched_double",
    "proto_text_fetched_date",
    "proto_text_fetched_year",
    "proto_text_fetched_time",
    "proto_text_fetched_datetime",
    "proto_text_fetched_timestamp",
    "proto_text_fetched_string",
    "proto_text_fetched_blob",
    "proto_text_fetched_enum",
    "proto_text_fetched_set",
    "proto_text_fetched_geometry",
    "proto_text_fetched_other",
    "proto_binary_fetched_null",
    "proto_binary_fetched_bit",
    "proto_binary_fetched_tinyint",
    "proto_binary_fetched_short",
    "proto_binary_fetched_int24",
    "proto_binary_fetched_int",
    "proto_binary_fetched_bigint",
    "proto_binary_fetched_decimal",
    "proto_binary_fetched_float",
    "proto_binary_fetched_double",
    "proto_binary_fetched_date",
    "proto_binary_fetched_year",
    "proto_binary_fetched_time",
    "proto_binary_fetched_datetime",
    "proto_binary_fetched_timestamp",
    "proto_binary_fetched_string",
    "proto_binary_fetched_json",
    "proto_binary_fetched_blob",
    "proto_binary_fetched_enum",
    "proto_binary_fetched_set",
    "proto_binary_fetched_geometry",
    "proto_binary_fetched_other",
    "init_command_executed_count",
    "init_command_failed_count",
    "com_quit",
    "com_init_db",
    "com_query",
    "com_field_list",
    "com_create_db",
    "com_drop_db",
    "com_refresh",
    "com_shutdown",
    "com_statistics",
    "com_process_info",
    "com_connect",
    "com_process_kill",
    "com_debug",
    "com_ping",
    "com_time",
    "com_delayed_insert",
    "com_change_user",
    "com_binlog_dump",
    "com_table_dump",
    "com_connect_out",
    "com_register_slave",
    "com_stmt_prepare",
    "com_stmt_execute",
    "com_stmt_send_long_data",
    "com_stmt_close",
    "com_stmt_reset",
    "com_stmt_set_option",
    "com_stmt_fetch",
    "com_deamon",
    "bytes_received_real_data_normal",
    "bytes_received_real_data_ps",
];

use std::cell::RefCell;

/// MYSQLND_STATS
#[derive(Clone, Debug)]
pub struct Stats {
    pub values: Vec<u64>,
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            values: vec![0; STAT_LAST],
        }
    }
}

thread_local! {
    /// mysqlnd_global_stats
    static GLOBAL: RefCell<Stats> = RefCell::new(Stats::default());
}

fn enabled() -> bool {
    super::globals().collect_statistics
}

impl Stats {
    fn add(&mut self, s: usize, v: u64) {
        if s < STAT_LAST {
            self.values[s] = self.values[s].wrapping_add(v);
        }
    }

    /// MYSQLND_INC_CONN_STATISTIC_W_VALUE: global and connection counters.
    pub fn inc_v(&mut self, s: usize, v: u64) {
        if enabled() {
            GLOBAL.with(|g| g.borrow_mut().add(s, v));
            self.add(s, v);
        }
    }

    /// MYSQLND_INC_CONN_STATISTIC
    pub fn inc(&mut self, s: usize) {
        self.inc_v(s, 1);
    }

    /// MYSQLND_INC_CONN_STATISTIC_W_VALUE2
    pub fn inc2(&mut self, s1: usize, v1: u64, s2: usize, v2: u64) {
        self.inc_v(s1, v1);
        self.inc_v(s2, v2);
    }

    /// MYSQLND_INC_CONN_STATISTIC_W_VALUE3
    pub fn inc3(&mut self, s1: usize, v1: u64, s2: usize, v2: u64, s3: usize, v3: u64) {
        self.inc_v(s1, v1);
        self.inc_v(s2, v2);
        self.inc_v(s3, v3);
    }
}

/// MYSQLND_INC_GLOBAL_STATISTIC
pub fn global_inc(s: usize) {
    if enabled() {
        GLOBAL.with(|g| g.borrow_mut().add(s, 1));
    }
}

/// MYSQLND_INC_GLOBAL_STATISTIC_W_VALUE2
pub fn global_inc2(s1: usize, v1: u64, s2: usize, v2: u64) {
    if enabled() {
        GLOBAL.with(|g| {
            let mut g = g.borrow_mut();
            g.add(s1, v1);
            g.add(s2, v2);
        });
    }
}

/// MYSQLND_DEC_GLOBAL_STATISTIC
pub fn global_dec(s: usize) {
    if enabled() {
        GLOBAL.with(|g| g.borrow_mut().add(s, u64::MAX));
    }
}

/// Snapshot of mysqlnd_global_stats.
pub fn global_values() -> Vec<u64> {
    GLOBAL.with(|g| g.borrow().values.clone())
}
