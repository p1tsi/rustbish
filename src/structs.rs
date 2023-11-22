use log::debug;
use base64::{engine::general_purpose, Engine as _};
use serde::{Serialize, Deserialize};
use crate::utils::{
    read_varint,
    read_encoded_string
};
use std::str::from_utf8;

use crate::constants::*;


pub static mut OVERFLOW_PAGES   : Vec<u32> = vec![];
pub static mut FREEPAGES        : Vec<u32> = vec![];
pub static mut PAGE_SIZE        : usize = 0;
pub static mut RESERVED_SPACE   : usize = 0;


// If I am looking for an overflow page in wal frames,
// but it is not present after current position,
// it is possible to look backwards (or get unchanged page from main db)
fn overflow_frame_offset_by_page_num(bytearray: &[u8], overflow_page_num: u32, current_offset: usize) -> usize {
    debug!("LOOKING FOR PAGE {} from offset 0x{:02x?}", overflow_page_num, current_offset);
        
    let filelen: usize = bytearray.len();
    let jump: usize;
    unsafe{
        jump = PAGE_SIZE + WAL_FRAME_HEADER_LEN;
    }


    let mut cur: usize = WAL_FILE_HEADER_LEN;
    while cur < current_offset {
        cur += jump;
    }

    let current_page_offset = cur - jump;

    debug!("Next frame at 0x{:02x?}", cur);
    let mut page_num = u32::from_be_bytes([
        bytearray[cur],
        bytearray[cur + 1],
        bytearray[cur + 2],
        bytearray[cur + 3],
    ]);
    
    cur += jump;
    while page_num != overflow_page_num && cur < filelen {
        page_num = u32::from_be_bytes([
            bytearray[cur],
            bytearray[cur + 1],
            bytearray[cur + 2],
            bytearray[cur + 3],
        ]);
        cur += jump;
        //debug!("Frame with page {}", page_num);
    }

    if page_num == overflow_page_num{
        debug!("Page offset: 0x{:02x?}", cur);
        return cur;
    }

    debug!("Search backwards...");
    cur = current_page_offset - jump;
    page_num = u32::from_be_bytes([
        bytearray[cur],
        bytearray[cur + 1],
        bytearray[cur + 2],
        bytearray[cur + 3],
    ]);

    while page_num != overflow_page_num && cur > 32{
        page_num = u32::from_be_bytes([
            bytearray[cur],
            bytearray[cur + 1],
            bytearray[cur + 2],
            bytearray[cur + 3],
        ]);
        cur -= jump;
    }
    
    debug!("Page not found. Getting original page from main db");    

    0
}


pub struct OverflowPage<'a> {
    offset: usize,
    next_page: u32,
    content: &'a[u8],
    cur: usize,         // Moving pointer inside the content of the page
    end: usize,         // Fixed pointer to the end of the page
}

impl OverflowPage<'_> {
    fn new(
        bytearray: &[u8], 
        page_offset: usize, 
        start_content: Option<usize>, 
    ) -> OverflowPage {

        //debug!("PAGE NUM: {}", page_num);
        //let offset: usize = (page_num - 1) as usize * page_size as usize;
        debug!("OVERFLOW PAGE OFFSET: 0x{:02x?}", page_offset);

        let cur: usize = match start_content {
            // In case there has already been an overflow
            //   And there are further values to take after the overflow
            Some(value) => value - (page_offset + 4),
            // In case we have to read blob/string
            //   from the beginning of the overflow page 
            //   (ie: start of the page + 4 bytes)
            None => 0
        };
        unsafe{
            OverflowPage {
                offset: page_offset, 
                next_page: u32::from_be_bytes([
                    bytearray[page_offset],
                    bytearray[page_offset + 1],
                    bytearray[page_offset + 2],
                    bytearray[page_offset + 3],
                ]),
                content: &bytearray[page_offset + 4 .. page_offset + PAGE_SIZE as usize],
                cur,   // Points to a location inside 'content' from which the string/blob starts or continues
                end: PAGE_SIZE as usize - 4 - RESERVED_SPACE // Points to the end of 'content'
            }

        }
    }

    fn read_string(&mut self, bytes_to_read: usize) -> String {
        debug!("READ STRING OF LEN {} FROM OVERFLOW PAGE.", bytes_to_read);
        debug!("Start: 0x{:02x?}, End: 0x{:02x?}", self.cur, self.end);
        let t: &[u8];
        if self.cur + bytes_to_read <= self.end {
            t = &self.content[self.cur .. self.cur + bytes_to_read];
            self.cur += bytes_to_read as usize;
        }
        else {
            t = &self.content[self.cur .. self.end];
            self.cur = self.end;
        }

        let final_str: String = read_encoded_string(t);

        final_str
    }

    fn read_bytes(&mut self, bytes_to_read: usize) -> Vec<u8> {
        debug!("READ {} BYTES FROM OVERFLOW PAGE", bytes_to_read);
        debug!("Start: 0x{:02x?}, End: 0x{:02x?}", self.cur, self.end);
        let final_bytearray: Vec<u8>;
        if self.cur + bytes_to_read <= self.end {
            final_bytearray = (&self.content[self.cur .. self.cur + bytes_to_read]).to_vec();
            self.cur += bytes_to_read as usize;
        }
        else {
            final_bytearray = (&self.content[self.cur .. self.end]).to_vec();
            self.cur = self.end;
        }

        debug!("FINAL BYTEARRAY LEN: {}", final_bytearray.len());

        final_bytearray
    }
}

type Row = Vec<String>;

/// Representation of a cell contained in both table and index b-tree leaf pages
#[derive(Serialize, Deserialize, Clone)]
pub struct LeafCell {
    rowid: Option<u32>,
    data: Row,
}

impl LeafCell {

    /// Parses a region of raw bytes of the file and returns a cell with id and data
    pub fn new(
        bytearray: &[u8], 
        offset: usize, 
        page_type: u8, 
        bytes_size_in_cell: Option<u32>, 
    ) -> LeafCell {

        let mut cell_offset: usize = offset;
        let (l, len_bytes_num): (i64, usize) = read_varint(&bytearray[cell_offset ..]);
        debug!("cell length: {} ({})", l, len_bytes_num);

        let mut rowid: Option<u32> = None;
        cell_offset += len_bytes_num;

        if page_type == LEAF_TABLE_BTREE_PAGE || page_type == 0 {
            let (rid, rid_len): (i64, usize) = read_varint(&bytearray[cell_offset ..]);
            rowid = Some(rid as u32);
            cell_offset += rid_len;
        }

        let (header_len, header_len_bytes_count): (i64, usize) = read_varint(&bytearray[cell_offset ..]);
        debug!("Header length: {}", header_len);
        cell_offset += header_len_bytes_count;
        let mut start_content: usize = cell_offset + header_len as usize - header_len_bytes_count;  // header contains header length itself
        let header: &[u8] = &bytearray[cell_offset .. start_content];
        
        debug!("Start Content: 0x{:02x?}", start_content);


        let cell_size: u32 = match bytes_size_in_cell {
            Some(c) => c,
            None => unsafe { PAGE_SIZE as u32}       // in case there is no overflow, put 'cell_size' to page_size (or random high value)
        };

        // Start cell parsing (with overflow pages if present)...
        let mut cell_record: Vec<String> = vec![];

        // Keeps track of cell's size in case there is an overflow 
        let mut cur_size: u32 =  header_len as u32;
        
        let mut overflow: bool = false;

        let mut overflow_page_num: u32 = 0;
        let mut i: usize = 0;
        while i < header.len() {
            let (item_value, item_size): (i64, usize) = read_varint(&header[i .. ]);
            
            match item_value {
                0 => cell_record.push(String::from("NULL")),
                1 => {
                    cell_record.push(
                        bytearray[start_content].to_string()
                    );
                    start_content += 1;
                    cur_size += 1;
                },
                2 => {
                    cell_record.push(
                        i32::from_be_bytes([
                            0,
                            0,
                            bytearray[start_content],
                            bytearray[start_content + 1]
                        ]).to_string()
                    );
                    start_content += 2;
                    cur_size += 2;
                },
                3 => {
                    cell_record.push(
                        i32::from_be_bytes([
                            0,
                            bytearray[start_content],
                            bytearray[start_content + 1],
                            bytearray[start_content + 2]
                        ]).to_string()
                    );
                    start_content += 3;
                    cur_size += 3;
                },
                4 => {
                    cell_record.push(
                        i32::from_be_bytes(
                            bytearray[start_content .. start_content + 4]
                            .try_into()
                            .expect("error")
                        ).to_string()
                    );
                    start_content += 4;
                    cur_size += 4;
                },
                5 => {
                    cell_record.push(
                        i64::from_be_bytes([
                            0,
                            0,
                            bytearray[start_content],
                            bytearray[start_content + 1],
                            bytearray[start_content + 2],
                            bytearray[start_content + 3],
                            bytearray[start_content + 4],
                            bytearray[start_content + 5],
                        ]).to_string()
                    );
                    start_content += 6;
                    cur_size += 6;
                },
                6 => {
                    cell_record.push(
                        i64::from_be_bytes(
                            bytearray[start_content .. start_content + 8]
                            .try_into()
                            .expect("error")
                        ).to_string()
                    );
                    start_content += 8;
                    cur_size += 8;
                },
                7 => {
                    cell_record.push(
                        f64::from_be_bytes(
                            bytearray[start_content .. start_content + 8]
                            .try_into()
                            .expect("error")
                        ).to_string()
                    );
                    start_content += 8;
                    cur_size += 8;
                },
                8 => cell_record.push(String::from("0")),
                9 => cell_record.push(String::from("1")),
                _ => {
                    if item_value >= 12 && item_value % 2 == 0 {
                        let blob_size: usize = ((item_value - 12) / 2) as usize;
                        debug!("Blob size: {}", blob_size);
                        
                        if blob_size == 0 {
                            cell_record.push(String::new());
                        }
                        else if overflow {
                            debug!("GET blob AFTER OVERFLOW");
                            debug!("CUR OVERFLOW PAGE NUM: {}", overflow_page_num);
                            debug!("START CONTENT: 0x{:02x?}", start_content);
                            let mut of: usize;
                            unsafe {
                                of = (overflow_page_num - 1) as usize * PAGE_SIZE as usize
                            }
                            let mut of_page: OverflowPage = OverflowPage::new(
                                bytearray, 
                                of,
                                //page_size, 
                                Some(start_content),
                                //page_reserved_space
                            );
                            let mut bytes: Vec<u8> = of_page.read_bytes(blob_size);
                            let mut remaing_bytes_to_read: usize = blob_size - bytes.len();
                            
                            debug!("Remainin bytes to read: {}", remaing_bytes_to_read);

                            while remaing_bytes_to_read > 0 {
                                overflow_page_num = of_page.next_page;
                                debug!("OF PAGE next page: {}", overflow_page_num);

                                unsafe{
                                    OVERFLOW_PAGES.push(overflow_page_num);
                                    of = (overflow_page_num - 1) as usize * PAGE_SIZE as usize
                                }

                                of_page = OverflowPage::new(
                                    bytearray, 
                                    of, 
                                    None,
                                );
                                let mut overflowed_bytes: Vec<u8> = of_page.read_bytes(remaing_bytes_to_read as usize);
                                remaing_bytes_to_read -= overflowed_bytes.len();
                                bytes.append(overflowed_bytes.as_mut());

                                debug!("REmaining bytes to read: {}", remaing_bytes_to_read);
                            }
                            start_content = of_page.offset + 4 + of_page.cur;
                            cell_record.push(general_purpose::STANDARD.encode(bytes));
                        }
                        else if blob_size as u32 + cur_size < cell_size {
                            cell_record.push(general_purpose::STANDARD.encode(&bytearray[start_content .. start_content + blob_size]));      
                            start_content += blob_size;
                            cur_size += blob_size as u32;
                        }
                        else {                          
                            let mut blob: Vec<u8> = (&bytearray[start_content .. start_content + (cell_size - cur_size) as usize]).to_vec();
                            debug!("read Bytes in cell: {}", blob.len());
                            start_content += (cell_size - cur_size) as usize;
                            debug!("OF PAGE OFFSET: 0x{:02x?}", start_content);
                            overflow_page_num = u32::from_be_bytes([
                                bytearray[start_content],
                                bytearray[start_content + 1],
                                bytearray[start_content + 2],
                                bytearray[start_content + 3],
                            ]);                    
                            debug!("CUR OVERFLOW PAGE: {}", overflow_page_num);
                            let mut of: usize;
                            
                            unsafe{
                                OVERFLOW_PAGES.push(overflow_page_num);
                                of = (overflow_page_num - 1) as usize * PAGE_SIZE as usize;
                            }

                            let mut remaining_bytes_to_read: usize = blob_size - blob.len();

                            while remaining_bytes_to_read > 0 {
                                debug!("REMAINING BYTES TO READ: {}", remaining_bytes_to_read);
                                let mut of_page: OverflowPage = OverflowPage::new(
                                    bytearray, 
                                    of,
                                    None,
                                );
                                let mut overflowed_bytes: Vec<u8> = of_page.read_bytes(remaining_bytes_to_read);
                                remaining_bytes_to_read -= overflowed_bytes.len();
                                blob.append(overflowed_bytes.as_mut());
                                
                                debug!("blob length: {}", blob.len());

                                // If the content of the blob is greater than the capacity of the current overflow page...
                                if remaining_bytes_to_read > 0 {
                                    overflow_page_num = of_page.next_page;
                                    unsafe{
                                        OVERFLOW_PAGES.push(overflow_page_num);
                                        of = (overflow_page_num - 1) as usize * PAGE_SIZE as usize;
                                    }
                                    
                                }
                                else {
                                    // Update the pointer to the eventual next data in the cell (now inside the overflow page)
                                    start_content = of_page.offset + 4 + of_page.cur;
                                }
                            }        
                            cell_record.push(general_purpose::STANDARD.encode(blob));
                            overflow = true;
                        }
                        debug!("BLOB DONE!");
                        //debug!("CELL_REC: {:?}", cell_record);
                    }
                    else if item_value >= 13 && item_value % 2 == 1 {
                        let string_size: usize = ((item_value - 13) / 2) as usize;

                        if string_size == 0 {
                            cell_record.push(String::new());
                        }
                        else if overflow {
                            debug!("GET String AFTER OVERFLOW");
                            debug!("CUR OVERFLOW PAGE NUM: {}", overflow_page_num);
                            debug!("START CONTENT: 0x{:02x?}", start_content);

                            let mut of: usize;
                            unsafe {
                                of = (overflow_page_num - 1) as usize * PAGE_SIZE as usize
                            }

                            let mut of_page: OverflowPage = OverflowPage::new(
                                bytearray, 
                                of, 
                                Some(start_content),
                            );
                            let mut string: String = of_page.read_string(string_size);
                            let mut remaing_chars_to_read: usize = string_size - string.len();
                            
                            debug!("Remainin bytes to read: {}", remaing_chars_to_read);

                            while remaing_chars_to_read > 0 {
                                overflow_page_num = of_page.next_page;
                                debug!("OF PAGE next page: {}", overflow_page_num);

                                unsafe{
                                    OVERFLOW_PAGES.push(overflow_page_num);
                                    of = (overflow_page_num - 1) as usize * PAGE_SIZE as usize;
                                }

                                of_page = OverflowPage::new(
                                    bytearray, 
                                    of, 
                                    None,
                                );
                                let overflowed_string: String = of_page.read_string(remaing_chars_to_read as usize);
                                string = format!("{}{}", string, overflowed_string);

                                debug!("READ STRING CHARS LEN: {}", overflowed_string.len());

                                remaing_chars_to_read -= overflowed_string.len();
                                debug!("REmaining chars to read: {}", remaing_chars_to_read);
                            }
                            start_content = of_page.offset + 4 + of_page.cur;
                            cell_record.push(string.to_string());
                        }
                        else if string_size as u32 + cur_size < cell_size {
                            if string_size == 1 {
                                let a: char = bytearray[start_content] as char;
                                cell_record.push(a.to_string());
                                start_content += 1;
                                cur_size += 1;
                            }
                            else {
                                let t: &[u8] = &bytearray[start_content .. start_content + string_size];
                                let string: String = read_encoded_string(t);
                                cell_record.push(string);
                                start_content += string_size;
                                cur_size += string_size as u32;
                            }
                        }
                        else {
                            debug!("START OF STRING 0x{:02x?} - END OF STRING: 0x{:02x?}", start_content, start_content + (cell_size) as usize);
                            let t: &[u8] = &bytearray[start_content .. start_content + (cell_size - cur_size) as usize];
                            let mut string: String = read_encoded_string(t);
                            
                            debug!("STRING FROM CELL: {}", string);

                            start_content += (cell_size - cur_size) as usize;

                            debug!("OF PAGE NUM OFFSET: 0x{:02x?}", start_content);
                            
                            overflow_page_num = u32::from_be_bytes([
                                bytearray[start_content],
                                bytearray[start_content + 1],
                                bytearray[start_content + 2],
                                bytearray[start_content + 3],
                            ]);

                            debug!("READ CUR OVERFLOW PAGE NUM: {}", overflow_page_num);
                            let mut of: usize;
                            unsafe{
                                OVERFLOW_PAGES.push(overflow_page_num);
                                of = (overflow_page_num - 1) as usize * PAGE_SIZE as usize;
                            }

                            let mut remaining_chars_to_read: usize = string_size - (cell_size - cur_size) as usize;
                            while remaining_chars_to_read > 0 {
                                debug!("Remaining {} chars to read from page {}", remaining_chars_to_read, overflow_page_num);
                                let mut of_page: OverflowPage = OverflowPage::new(
                                    bytearray, 
                                    of, 
                                    None,
                                );
                                let overflowed_string: String = of_page.read_string(remaining_chars_to_read as usize);
                                string = format!("{}{}", string, overflowed_string);

                                remaining_chars_to_read -= overflowed_string.len();
                                
                                // If we have to read remaining bytes from next overflow page...
                                if remaining_chars_to_read > 0 {
                                    overflow_page_num = of_page.next_page;

                                    unsafe{
                                        OVERFLOW_PAGES.push(overflow_page_num);
                                        of = (overflow_page_num - 1) as usize * PAGE_SIZE as usize;
                                    }
                                }
                                // else if string is totally read (and could be other columns value to read...)
                                else{
                                    start_content = of_page.offset + 4 + of_page.cur;
                                }
                            }
                            debug!("CUR OVERFLOW PAGE NUM: {}", overflow_page_num);
                            cell_record.push(string.to_string());
                            overflow = true;
                        }
                        debug!("String done ({})", string_size);
                    }
                    else {
                        debug!("TODO: {}", item_value);
                        cell_record.push(String::from("TODO"));
                    }
                }
            };

            i += item_size;
        }
        debug!("{} -{:?}", rowid.unwrap(), cell_record);

        LeafCell {
            rowid,
            data: cell_record
        }
    }

    pub fn new_wal(
        bytearray: &[u8], 
        offset: usize, 
        page_type: u8, 
        bytes_size_in_cell: Option<u32>, 
    ) -> LeafCell {
        let mut cell_offset: usize = offset;
        let (l, len_bytes_num): (i64, usize) = read_varint(&bytearray[cell_offset ..]);
        debug!("cell length: {} ({})", l, len_bytes_num);

        let mut rowid: Option<u32> = None;
        cell_offset += len_bytes_num;

        if page_type == LEAF_TABLE_BTREE_PAGE || page_type == 0 {
            let (rid, rid_len): (i64, usize) = read_varint(&bytearray[cell_offset ..]);
            rowid = Some(rid as u32);
            cell_offset += rid_len;
        }

        let (header_len, header_len_bytes_count): (i64, usize) = read_varint(&bytearray[cell_offset ..]);
        debug!("Header length: {}", header_len);
        cell_offset += header_len_bytes_count;
        let mut start_content: usize = cell_offset + header_len as usize - header_len_bytes_count;  // header contains header length itself
        let header: &[u8] = &bytearray[cell_offset .. start_content];
        
        debug!("Start Content: 0x{:02x?}", start_content);

        let cell_size: u32 = match bytes_size_in_cell {
            Some(c) => c,
            None => 4096       // in case there is no overflow, put 'cell_size' to page_size (or random high value)
        };

        // Start cell parsing (with overflow pages if present)...
        let mut cell_record: Vec<String> = vec![];

        // Keeps track of cell's size in case there is an overflow 
        let mut cur_size: u32 =  header_len as u32;
        
        let mut overflow: bool = false;

        let mut overflow_page_num: u32 = 0;
        let mut overflow_frame_offset: usize = 0;
        let mut i: usize = 0;
        while i < header.len() {
            let (item_value, item_size): (i64, usize) = read_varint(&header[i .. ]);
            
            match item_value {
                0 => cell_record.push(String::from("NULL")),
                1 => {
                    cell_record.push(
                        bytearray[start_content].to_string()
                    );
                    start_content += 1;
                    cur_size += 1;
                },
                2 => {
                    cell_record.push(
                        i32::from_be_bytes([
                            0,
                            0,
                            bytearray[start_content],
                            bytearray[start_content + 1]
                        ]).to_string()
                    );
                    start_content += 2;
                    cur_size += 2;
                },
                3 => {
                    cell_record.push(
                        i32::from_be_bytes([
                            0,
                            bytearray[start_content],
                            bytearray[start_content + 1],
                            bytearray[start_content + 2]
                        ]).to_string()
                    );
                    start_content += 3;
                    cur_size += 3;
                },
                4 => {
                    cell_record.push(
                        i32::from_be_bytes(
                            bytearray[start_content .. start_content + 4]
                            .try_into()
                            .expect("error")
                        ).to_string()
                    );
                    start_content += 4;
                    cur_size += 4;
                },
                5 => {
                    cell_record.push(
                        i64::from_be_bytes([
                            0,
                            0,
                            bytearray[start_content],
                            bytearray[start_content + 1],
                            bytearray[start_content + 2],
                            bytearray[start_content + 3],
                            bytearray[start_content + 4],
                            bytearray[start_content + 5],
                        ]).to_string()
                    );
                    start_content += 6;
                    cur_size += 6;
                },
                6 => {
                    cell_record.push(
                        i64::from_be_bytes(
                            bytearray[start_content .. start_content + 8]
                            .try_into()
                            .expect("error")
                        ).to_string()
                    );
                    start_content += 8;
                    cur_size += 8;
                },
                7 => {
                    cell_record.push(
                        f64::from_be_bytes(
                            bytearray[start_content .. start_content + 8]
                            .try_into()
                            .expect("error")
                        ).to_string()
                    );
                    start_content += 8;
                    cur_size += 8;
                },
                8 => cell_record.push(String::from("0")),
                9 => cell_record.push(String::from("1")),
                _ => {
                    if item_value >= 12 && item_value % 2 == 0 {
                        let blob_size: usize = ((item_value - 12) / 2) as usize;
                        debug!("Blob size: {}", blob_size);
                        if blob_size == 0 {
                            cell_record.push(String::new());
                        }
                        else if overflow {
                            debug!("GET blob AFTER OVERFLOW");
                            debug!("CUR OVERFLOW PAGE NUM: {}", overflow_page_num);
                            debug!("START CONTENT: 0x{:02x?}", start_content);
                            let mut of_page: OverflowPage = OverflowPage::new(
                                bytearray, 
                                overflow_frame_offset, 
                                Some(start_content),
                            );
                            let mut bytes: Vec<u8> = of_page.read_bytes(blob_size);
                            let mut remaing_bytes_to_read: usize = blob_size - bytes.len();
                            
                            debug!("Remainin bytes to read: {}", remaing_bytes_to_read);

                            while remaing_bytes_to_read > 0 {
                                overflow_page_num = of_page.next_page;
                                debug!("OF PAGE next page: {}", overflow_page_num);
                                overflow_frame_offset = overflow_frame_offset_by_page_num(&bytearray, overflow_page_num, overflow_frame_offset);

                                unsafe{
                                    OVERFLOW_PAGES.push(overflow_page_num);
                                }

                                of_page = OverflowPage::new(
                                    bytearray, 
                                    overflow_frame_offset, 
                                    None,
                                );
                                let mut overflowed_bytes: Vec<u8> = of_page.read_bytes(remaing_bytes_to_read as usize);
                                remaing_bytes_to_read -= overflowed_bytes.len();
                                bytes.append(overflowed_bytes.as_mut());

                                debug!("REmaining bytes to read: {}", remaing_bytes_to_read);
                            }
                            start_content = of_page.offset + 4 + of_page.cur;
                            cell_record.push(general_purpose::STANDARD.encode(bytes));
                        }
                        else if blob_size as u32 + cur_size < cell_size {
                            cell_record.push(general_purpose::STANDARD.encode(&bytearray[start_content .. start_content + blob_size]));      
                            start_content += blob_size;
                            cur_size += blob_size as u32;
                        }
                        else {                          
                            let mut blob: Vec<u8> = (&bytearray[start_content .. start_content + (cell_size - cur_size) as usize]).to_vec();
                            debug!("Read bytes in cell: {}", blob.len());
                            start_content += (cell_size - cur_size) as usize;
                            debug!("OF PAGE OFFSET: 0x{:02x?}", start_content);
                            overflow_page_num = u32::from_be_bytes([
                                bytearray[start_content],
                                bytearray[start_content + 1],
                                bytearray[start_content + 2],
                                bytearray[start_content + 3],
                            ]);                    

                            unsafe{
                                OVERFLOW_PAGES.push(overflow_page_num);
                            }

                            debug!("CUR OVERFLOW PAGE: {}", overflow_page_num);

                            overflow_frame_offset = overflow_frame_offset_by_page_num(&bytearray, overflow_page_num, start_content);
                            
                            let mut remaining_bytes_to_read: usize;
                            if overflow_frame_offset == 0 {
                                remaining_bytes_to_read = 0;
                            }
                            else{
                                remaining_bytes_to_read = blob_size - blob.len();
                            }

                            while remaining_bytes_to_read > 0 {
                                debug!("REMAINING BYTES TO READ: {}", remaining_bytes_to_read);
                                let mut of_page: OverflowPage = OverflowPage::new(
                                    bytearray, 
                                    overflow_frame_offset + WAL_FRAME_HEADER_LEN,
                                    None,
                                );
                                let mut overflowed_bytes: Vec<u8> = of_page.read_bytes(remaining_bytes_to_read);
                                remaining_bytes_to_read -= overflowed_bytes.len();
                                blob.append(overflowed_bytes.as_mut());
                                
                                debug!("blob length: {}", blob.len());

                                // If the content of the blob is greater than the capacity of the current overflow page...
                                if remaining_bytes_to_read > 0 {
                                    overflow_page_num = of_page.next_page;
                                    unsafe{
                                        OVERFLOW_PAGES.push(overflow_page_num);
                                    }

                                    overflow_frame_offset = overflow_frame_offset_by_page_num(
                                        &bytearray,
                                        overflow_page_num,
                                        overflow_frame_offset
                                    );

                                }
                                else {
                                    // Update the pointer to the eventual next data in the cell (now inside the overflow page)
                                    start_content = of_page.offset + 4 + of_page.cur;
                                }
                            }        
                            cell_record.push(general_purpose::STANDARD.encode(blob));
                            overflow = true;
                        }
                        debug!("BLOB DONE!");
                    }
                    else if item_value >= 13 && item_value % 2 == 1 {
                        let string_size: usize = ((item_value - 13) / 2) as usize;
        
                        if string_size == 0 {
                            cell_record.push(String::new());
                        }
                        else if overflow {
                            debug!("GET String AFTER OVERFLOW");
                            debug!("CUR OVERFLOW PAGE NUM: {}", overflow_page_num);
                            debug!("START CONTENT: 0x{:02x?}", start_content);
                            let mut of_page: OverflowPage = OverflowPage::new(
                                bytearray, 
                                overflow_frame_offset, 
                                Some(start_content),
                            );
                            let mut string: String = of_page.read_string(string_size);
                            let mut remaing_chars_to_read: usize = string_size - string.len();
                            
                            debug!("Remainin bytes to read: {}", remaing_chars_to_read);

                            while remaing_chars_to_read > 0 {
                                overflow_page_num = of_page.next_page;
                                debug!("OF PAGE next page: {}", overflow_page_num);
                                overflow_frame_offset = overflow_frame_offset_by_page_num(&bytearray, overflow_page_num, overflow_frame_offset);

                                unsafe{
                                    OVERFLOW_PAGES.push(overflow_page_num);
                                }

                                of_page = OverflowPage::new(
                                    bytearray, 
                                    overflow_frame_offset, 
                                    None,
                                );
                                let overflowed_string: String = of_page.read_string(remaing_chars_to_read as usize);
                                string = format!("{}{}", string, overflowed_string);

                                debug!("READ STRING CHARS LEN: {}", overflowed_string.len());

                                remaing_chars_to_read -= overflowed_string.len();
                                debug!("REmaining chars to read: {}", remaing_chars_to_read);
                            }
                            start_content = of_page.offset + 4 + of_page.cur;
                            cell_record.push(string.to_string());
                        }
                        else if string_size as u32 + cur_size < cell_size {
                            if string_size == 1 {
                                let a: char = bytearray[start_content] as char;
                                cell_record.push(a.to_string());
                                start_content += 1;
                                cur_size += 1;
                            }
                            else {
                                let t: &[u8] = &bytearray[start_content .. start_content + string_size];
                                let string: String = read_encoded_string(t);
                                cell_record.push(string);
                                start_content += string_size;
                                cur_size += string_size as u32;
                            }
                        }
                        else {
                            debug!("START OF STRING 0x{:02x?} - END OF STRING: 0x{:02x?}", start_content, start_content + (cell_size) as usize);
                            let t: &[u8] = &bytearray[start_content .. start_content + (cell_size - cur_size) as usize];
                            let mut string: String = read_encoded_string(t);
                            
                            debug!("STRING FROM CELL: {}", string);

                            start_content += (cell_size - cur_size) as usize;

                            debug!("OF PAGE NUM OFFSET: 0x{:02x?}", start_content);
                            
                            overflow_page_num = u32::from_be_bytes([
                                bytearray[start_content],
                                bytearray[start_content + 1],
                                bytearray[start_content + 2],
                                bytearray[start_content + 3],
                            ]);

                            debug!("READ CUR OVERFLOW PAGE NUM: {}", overflow_page_num);
                            unsafe{
                                OVERFLOW_PAGES.push(overflow_page_num);
                            }

                            overflow_frame_offset = overflow_frame_offset_by_page_num(&bytearray, overflow_page_num, start_content);

                            let mut remaining_chars_to_read: usize;
                            if overflow_frame_offset == 0 {
                                remaining_chars_to_read = 0;
                            }
                            else {
                                remaining_chars_to_read = string_size - (cell_size - cur_size) as usize;
                            }

                            while remaining_chars_to_read > 0 {
                                debug!("Remaining {} chars to read from page {}", remaining_chars_to_read, overflow_page_num);
                                let mut of_page: OverflowPage = OverflowPage::new(
                                    bytearray, 
                                    overflow_frame_offset + 24, 
                                    None,
                                );
                                let overflowed_string: String = of_page.read_string(remaining_chars_to_read as usize);
                                string = format!("{}{}", string, overflowed_string);

                                remaining_chars_to_read -= overflowed_string.len();
                                
                                // If we have to read remaining bytes from next overflow page...
                                if remaining_chars_to_read > 0 {
                                    overflow_page_num = of_page.next_page;

                                    unsafe{
                                        OVERFLOW_PAGES.push(overflow_page_num);
                                    }
                                }
                                // else if string is totally read (and could be other columns value to read...)
                                else{
                                    start_content = of_page.offset + 4 + of_page.cur;
                                }
                            }
                            debug!("CUR OVERFLOW PAGE NUM: {}", overflow_page_num);
                            cell_record.push(string.to_string());
                            overflow = true;
                        }
                        debug!("STRING DONE!");                        
                    }
                    else {
                        debug!("TODO: {}", item_value);
                        cell_record.push(String::from("TODO"));
                    }
                }
            };

            i += item_size;
        }
        debug!("{} -{:?}", rowid.unwrap(), cell_record);

        LeafCell {
            rowid,
            data: cell_record
        }
    }

    pub fn rowid(&self) -> Option<u32> {
        self.rowid
    }

    pub fn data(&self) -> Vec<String> {
        self.data.clone()
    }

    pub fn to_csv(&self) -> String {
        let mut csv_string = String::from(format!("{};", self.rowid.unwrap()));
        self.data
        .iter()
        .for_each(
            |item| 
                csv_string.push_str(
                    format!("{};", item).as_str()
            )
        );
        
        csv_string
    }

}

impl TryFrom<&Cell> for LeafCell {
    type Error = Cell;
    
    fn try_from(other: &Cell) -> Result<Self, Self::Error> {
        match other {
            Cell::LC(c) => Ok(c.to_owned()),
            a => Err(a.to_owned()),
        }
    }
}

impl std::fmt::Debug for LeafCell {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let res: std::fmt::Result;
        
        match self.rowid{
            Some(r) => res = writeln!(f, "\t\tRECORD:\t\t{} - {:?}", r, self.data),
            None => res = writeln!(f, "\t\tRECORD:\t\t # - {:?}", self.data),
        }

        res
    }
}


/// Representation of a cell contained in a table b-tree interior page
#[derive(Clone)]
pub struct InteriorTableCell {
    /// Page number of the following table in the tree
    left_pointer: u32,
    /// Max value for rowid in following page
    key: u32,
}

impl InteriorTableCell {

    /// Parses a region of raw bytes of the file and returns a cell in an table b-tree interior page
    pub fn new(bytearray: &[u8], offset: usize) -> InteriorTableCell {
        let (key, _) = read_varint(&bytearray[offset + 4 .. ]);
        InteriorTableCell {
            left_pointer: u32::from_be_bytes(bytearray[offset .. offset + 4].try_into().expect("error")),
            key: key as u32,
        }
    }

    pub fn left_pointer(&self) -> u32 {
        self.left_pointer
    }
}

impl TryFrom<&Cell> for InteriorTableCell {
    type Error = Cell;
    
    fn try_from(other: &Cell) -> Result<Self, Self::Error> {
        match other {
            Cell::ITC(c) => Ok(c.to_owned()),
            a => Err(a.to_owned()),
        }
    }
}

impl std::fmt::Debug for InteriorTableCell {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result =  writeln!(f, "\t\tNEXT PAGE:\t\t{:?}", self.left_pointer);

        res = writeln!(f, "\t\tMAX KEY:\t\t\t{:?}", self.key);
        res = writeln!(f, "");

        res
    }
}

#[derive(Clone, Debug)]
pub enum Cell {
    LC(LeafCell),
    ITC(InteriorTableCell),
}


/// Representation of the header of a page
#[derive(Clone)]
pub struct PageHeader {
    page_type                   : u8,
    first_freeblock_offset      : u32,
    cell_count                  : u32,
    cell_content_offset         : u32,      // if 0 -> 65536
    fragmented_free_bytes       : u32,      // within the cell content area
    rightmost_ptr               : Option<u32>,
}

impl PageHeader {

    /// Parses a region of raw bytes of the file and returns the header of a page
    pub fn new(bytearray: &[u8], page_offset: usize) -> PageHeader {
        let pt: u8 = bytearray[page_offset];
        let mut rightmost_ptr: Option<u32> = None;
        if pt == INTERIOR_INDEX_BTREE_PAGE || pt == INTERIOR_TABLE_BTREE_PAGE {
            // GET RIGHT-MOST POINTER
            rightmost_ptr = Some(u32::from_be_bytes([
                bytearray[page_offset + 8],
                bytearray[page_offset + 9],
                bytearray[page_offset + 10],
                bytearray[page_offset + 11],
            ]));
        }

        PageHeader {
            page_type: pt,
            first_freeblock_offset: u32::from_be_bytes([0, 0, bytearray[page_offset + 1], bytearray[page_offset + 2]]),
            cell_count: u32::from_be_bytes([0, 0, bytearray[page_offset + 3], bytearray[page_offset + 4]]),
            cell_content_offset: u32::from_be_bytes([0, 0, bytearray[page_offset + 5], bytearray[page_offset + 6]]),
            fragmented_free_bytes:  u32::from_be_bytes([0, 0, 0, bytearray[page_offset + 7]]),
            rightmost_ptr,
        }
    }

    pub fn page_type(&self) -> u8 {
        self.page_type
    }

    pub fn cell_count(&self) -> u32 {
        self.cell_count
    }

    pub fn rightmost_ptr(&self) -> Option<u32> {
        self.rightmost_ptr
    }

}

impl std::fmt::Debug for PageHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result = writeln!(f, "\tPAGE_TYPE:\t\t\t{:?}", self.page_type);
        res = writeln!(f, "\tFREEBLOCK_START:\t0x{:04x?}", self.first_freeblock_offset);
        res = writeln!(f, "\tCELL_COUNT:\t\t\t{:?}", self.cell_count);
        res = writeln!(f, "\tCONTENT_START:\t\t0x{:04x?}", self.cell_content_offset);
        res = writeln!(f, "\tFRAGMENTED_BYTES:\t{:?}", self.fragmented_free_bytes);
        if !self.rightmost_ptr.is_none() {
            res = writeln!(f, "\tFIRST OFLOW PAGE NUM:\t{:?}", self.rightmost_ptr.unwrap());
        }

        res
    }
}


/// Representation of a page, composed of a header and a group of cells
#[derive(Clone)]
pub struct Page {
    number: u32,
    offset: usize,
    header: PageHeader,
    live_cells: Vec<Cell>,
    deleted_cells: Vec<Cell>,
    deleted_cells_count: u32
}

impl Page {
    ///TODO:
    /// If a page contains no cells (which is only possible for a root page of a table that contains no rows)
    ///  then the offset to the cell content area will equal the page size minus the bytes of reserved space. 
    /// If the database uses a 65536-byte page size and the reserved space is zero (the usual value for reserved space) 
    /// then the cell content offset of an empty page wants to be 65536. However, that integer is too large to be 
    /// stored in a 2-byte unsigned integer, so a value of 0 is used in its place.

    /// Parses a region of raw bytes of the file and returns a page
    pub fn new(
        bytearray: &[u8], 
        page_offset: usize, 
        page_num: u32, 
        is_wal: bool
    ) -> Page {     

        debug!("Page: {} (0x{:02x?})", page_num + 1, page_offset);

        let mut live_cells: Vec<Cell> = vec![];
        let mut deleted_cells: Vec<Cell> = vec![];
        let mut deleted_cells_count: u32 = 0;

        let header: PageHeader;
        let mut first_page: bool = false;
        let tag = match from_utf8(&bytearray[page_offset .. page_offset + 15]) {
            Ok(v) => v,
            Err(_) => "ERROR",
        };

        if tag == SQLITE_MAGIC {
            header = PageHeader::new(&bytearray, page_offset + FILE_HEADER_LEN);
            first_page = true;
        }
        else if page_num == 0 && !is_wal {
            header = PageHeader::new(&bytearray, FILE_HEADER_LEN);
        }
        else {
            header = PageHeader::new(&bytearray, page_offset);
        }

        debug!("page header: {:?}", header);
        
        /*if bytearray[page_offset] == 0 {
            // FREEPAGE

            let freepage_header: FreePageHeader = FreePageHeader::new(bytearray, page_offset);
            trace!("FREEPAGE HEADER: {:?}", freepage_header);
            let cell_array_offset: usize = page_offset + freepage_header.count as usize * 4 + 8; // +8 -> | 0x00 * 4 + 4 bytes of 'count' |
            let cell_array: Vec<usize> = Self::get_cell_array(bytearray, cell_array_offset);
            header = PageHeader::Free(freepage_header);

            /*for &cell_offset in cell_array.iter() {
                let mut cell_address: usize = cell_offset;
                if page_num > 0 {
                    cell_address += page_offset;
                }
                let cell: LeafCell = Self::parse_leaf_cell(
                    bytearray,
                    0,
                    cell_address,
                    page_size,
                    reserved_space
                );                
                deleted_cells.push(Cell::LC(cell));
            }*/
        }
        else {*/
        
        //debug!("CELL_CONTENT_OFFSET: 0x{:02x?} - LAST BYTE: 0x{:02x?}", std_header.cell_content_offset, page_size as usize - reserved_space);

        // Check if is valid page or all cells have been deleted
        /*if std_header.cell_count == 0 && 
            std_header.cell_content_offset as usize == page_size as usize - reserved_space {
            
            // In this case there are not valid cells (they have been deleted)
            trace!("NO LIVING CELLS IN THIS PAGE!");

            // Check if there are cells in cells array
            let cell_array: Vec<usize> = Self::get_cell_array(
                bytearray, 
                page_offset + 8, 
            );
        }
        else{*/
            // In this case there is at least 1 cell valid (not deleted)
            // and should be checked 'freeblock_start' value of header


        // If the page is a leaf in a index or table b-tree...
        if header.page_type == LEAF_TABLE_BTREE_PAGE /*|| header.page_type == 10*/ {
            debug!("Page type: leaf table page");
            let cell_array: Vec<usize>;
            
            if page_num == 0 && (!is_wal || first_page) {
                cell_array = Self::get_cell_array(
                    bytearray, 
                    page_offset + FILE_HEADER_LEN + LEAF_BTREE_HEADER_LEN,
                );
            }
            else{
                cell_array = Self::get_cell_array(
                    bytearray, 
                    page_offset + LEAF_BTREE_HEADER_LEN,
                );
            }
            debug!("CELL COUNT: {}; CELL ARRAY LEN: {}", header.cell_count, cell_array.len());
            
            // LIVE CELLS
            for &cell_offset in cell_array.iter().take(header.cell_count as usize) {
                let cell_address: usize = cell_offset + page_offset;
                debug!("cell address: 0x{:02x?}", cell_address);
                let cell: LeafCell = Self::parse_leaf_cell(
                    bytearray,
                    header.page_type,
                    cell_address,
                    //page_size,
                    //reserved_space,
                    is_wal
                );     
                live_cells.push(Cell::LC(cell));
            }
            
            // DELETED CELLS
            if cell_array.len() - header.cell_count as usize > 0 {
                debug!("DELETED {} ROWS", cell_array.len() - header.cell_count as usize);

                deleted_cells_count = cell_array.len() as u32 - header.cell_count;

                /*for &cell_offset in cell_array[std_header.cell_count as usize ..].iter(){
                    let mut cell_address: usize = cell_offset;
                    if page_num > 0 {
                        cell_address += page_offset;
                    }
                    let cell: LeafCell = Self::parse_leaf_cell(
                        bytearray,
                        std_header.page_type,
                        cell_address,
                        page_size,
                        reserved_space
                    );     
                    deleted_cells.push(Cell::LC(cell));
                }*/
                
            }
        }
        // else if is an internal table b-tree page
        else if header.page_type == INTERIOR_TABLE_BTREE_PAGE {
            debug!("Page type: internal table page");
            let cell_array: Vec<usize>;
            if page_num == 0 {
                cell_array = Self::get_cell_array(
                    bytearray, 
                    page_offset + FILE_HEADER_LEN + INTERIOR_BTREE_HEADER_LEN,
                );
            }
            else{
                cell_array = Self::get_cell_array(
                    bytearray, 
                    page_offset + INTERIOR_BTREE_HEADER_LEN,
                );
            }

            for &cell_offset in cell_array.iter().take(header.cell_count as usize) { 
                let cell_address: usize = cell_offset + page_offset;
                debug!("cell offset: 0x{:02x?}", cell_address);
                let cell: InteriorTableCell = InteriorTableCell::new(&bytearray, cell_address);
                live_cells.push(Cell::ITC(cell));
            }
        }
        /*else if header.page_type == 2 {
            trace!("** INTERIOR INDEX B-TREE PAGE (0x{:02x?}) **", page_offset);
            let mut cells: Vec<T> = vec![];
        }*/
        
        debug!("{:*<20}", "");

        Page {
            number: page_num + 1,
            offset: page_offset,
            header,
            live_cells,
            deleted_cells,
            deleted_cells_count
        }
    } 

    fn get_cell_array(bytearray: &[u8], array_offset: usize) -> Vec<usize> {
        debug!("ARRAY OFFSET: 0x{:02x?}", array_offset);
        let mut cell_array: Vec<usize> = vec![];
        let mut n: usize = 0;
        let mut cell: usize = u32::from_be_bytes([
            0, 
            0, 
            bytearray[array_offset + 2*n],
            bytearray[array_offset + 1 + 2*n]
        ]) as usize;

        while cell > 0 {
            cell_array.push(cell);
            n += 1;
            cell = u32::from_be_bytes([
                0, 
                0, 
                bytearray[array_offset + 2*n], 
                bytearray[array_offset + 1 + 2*n]
            ]) as usize;
        }

        cell_array
    }

    fn parse_leaf_cell(
        bytearray: &[u8], 
        page_type: u8, 
        cell_address: usize,
        //page_size: u32,
        //reserved_space: usize,
        is_wal: bool
    ) -> LeafCell {
        
        let cell: LeafCell;
        let (cell_size, _) = read_varint(&bytearray[cell_address..]);
        
        debug!("CELL SIZE: {}", cell_size);
        let usable_page_size: u32;
        unsafe{
            usable_page_size = PAGE_SIZE as u32 - RESERVED_SPACE as u32;

        }
        debug!("Usable page size: {}", usable_page_size);
        
        // Check if cell contains whole data or there is an overflow
        if cell_size as u32 <= usable_page_size - 35 {
            cell = LeafCell::new(
                &bytearray, 
                cell_address, 
                page_type, 
                None,
            );
        }
        else {
            let m: u32 = ((usable_page_size - 12) * 32 / 255) - 23;
            let k: u32 = m + ((cell_size as u32 - m) % (usable_page_size - 4));
            if k <= usable_page_size - 35 {
                debug!("K = {}; M = {}", k, m);
                if is_wal {
                    cell = LeafCell::new_wal(
                        &bytearray, 
                        cell_address, 
                        page_type, 
                        Some(k),
                    );
                }
                else{
                    cell = LeafCell::new(
                        &bytearray, 
                        cell_address, 
                        page_type, 
                        Some(k),
                    );
                }
            }
            else {
                if is_wal {
                    cell = LeafCell::new_wal(
                        &bytearray, 
                        cell_address, 
                        page_type, 
                        Some(m),
                    );
                }
                else{
                    cell = LeafCell::new(
                        &bytearray, 
                        cell_address, 
                        page_type, 
                        Some(m),
                    );
                }
            }
        }        
        
        cell
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn header(&self) -> PageHeader {
        self.header.clone()
    }

    pub fn live_cells(&self) -> Vec<Cell> {
        self.live_cells.clone()
    }

    pub fn is_internal_table_page(&self) -> bool {
        self.header.page_type == INTERIOR_TABLE_BTREE_PAGE
    }


    /*pub fn is_leaf_page(&self) -> bool {
        match self.header.clone() {
            PageHeader::Free(_) => false,
            PageHeader::Standard(h) => {
                h.page_type == 13 || h.page_type == 10
            }
        }
        
    }

    pub fn is_table_leaf_page(&self) -> bool {
        self.header.page_type == 13
    }

    pub fn is_index_leaf_page(&self) -> bool {
        match self.header.clone() {
            PageHeader::Free(_) => false,
            PageHeader::Standard(h) => {
                h.page_type == 10
            }
        }
    }

    pub fn is_table_page(&self) -> bool {
        match self.header.clone() {
            PageHeader::Free(_) => false,
            PageHeader::Standard(h) => {
                h.page_type == 13 || h.page_type == 5
            }
        }
    }

    pub fn is_index_page(&self) -> bool {
        match self.header.clone() {
            PageHeader::Free(_) => false,
            PageHeader::Standard(h) => {
                h.page_type == 2 || h.page_type == 10
            }
        }
    }

    pub fn is_internal_index_page(&self) -> bool {
        match self.header.clone() {
            PageHeader::Free(_) => false,
            PageHeader::Standard(h) => {
                h.page_type == 2
            }
        }
    }

    pub fn contains_rowid(&self, rowid: Option<u32>) -> bool {
        let mut res: bool = false;
        for cell in self.live_cells.iter(){  
            if LeafCell::try_from(cell).unwrap().rowid == rowid {
                res = true;
                break;
            }
        }

        res
    }*/

    pub fn get_all_rowids(&self) -> Vec<u32> {
        let mut rowids: Vec<u32> = vec![];

        for cell in self.live_cells.iter(){
            rowids.push(
                LeafCell::try_from(cell).unwrap().rowid.unwrap()
            )
        }

        rowids
    }

    pub fn get_cell_by_rowid(&self, rowid: u32) -> Option<LeafCell> {
        for cell in self.live_cells.iter(){
            let leafcell = LeafCell::try_from(cell).unwrap();
            if leafcell.rowid.unwrap() == rowid {
                return Some(leafcell);
            }
        }

        None
    }

}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result = writeln!(f, "PAGE {} (0x{:02x?})", self.number, self.offset);
        res = write!(f, "{:?}", self.header);
        res = writeln!(f, "\tDELETED ROWS COUNT:\t{}\n", self.deleted_cells_count);
        if self.header.page_type == LEAF_TABLE_BTREE_PAGE || self.header.page_type == INTERIOR_TABLE_BTREE_PAGE {

            for (i, cell) in self.live_cells.iter().enumerate(){
                res = writeln!(f, "\tCELL {}", i);
                match cell {
                    Cell::LC(c) => res = writeln!(f, "{:?}", c),
                    Cell::ITC(c) => res = writeln!(f, "{:?}", c),
                }
            }
            
            if self.deleted_cells.len() > 0 {
                res = writeln!(f, ">> DELETED CELLS");
                for (i, cell) in self.deleted_cells.iter().enumerate(){
                    res = writeln!(f, "\tCELL {}", i);
                    match cell {
                        Cell::LC(c) => res = writeln!(f, "{:?}", c),
                        Cell::ITC(c) => res = writeln!(f, "{:?}", c),
                    }
                }
            }
        }

        res
    }
}

