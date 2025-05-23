pub const SQLITE_MAGIC: &str = "SQLite format 3";

/* BTree page types */
pub const INTERIOR_INDEX_BTREE_PAGE: u8 = 2;
pub const INTERIOR_TABLE_BTREE_PAGE: u8 = 5;
pub const LEAF_INDEX_BTREE_PAGE: u8 = 10;
pub const LEAF_TABLE_BTREE_PAGE: u8 = 13;

/* Known lengths */
pub const FILE_HEADER_LEN: usize = 100;
pub const LEAF_BTREE_HEADER_LEN: usize = 8;
pub const INTERIOR_BTREE_HEADER_LEN: usize = 12;

pub const WAL_FILE_HEADER_LEN: usize = 32;
pub const WAL_FRAME_HEADER_LEN: usize = 24;
