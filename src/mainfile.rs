use log::{info, trace, debug, warn};
use std::collections::HashMap;
use std::str::from_utf8;

use crate::structs::{
    Page,
    Cell,
    LeafCell,
    OVERFLOW_PAGES,
    FREEPAGES,
    PAGE_SIZE,
    RESERVED_SPACE
};

use crate::utils::{
    read_varint,
    STRING_ENCODING
};


#[derive(Clone)]
pub struct FreeListTrunkPageHeader {
    next_page_num: u32,
    count: u32,         // If first freepage, 'count' is the number of following freepages
    next_freepages: Vec<u32>
}

impl FreeListTrunkPageHeader {
    pub fn new(bytearray: &[u8], page_offset: usize) -> FreeListTrunkPageHeader {
        trace!("PAGE OFFSET: 0x{:02x?}", page_offset);
        let next_page_num: u32 = u32::from_be_bytes([
            bytearray[page_offset], 
            bytearray[page_offset + 1],
            bytearray[page_offset + 2], 
            bytearray[page_offset + 3]
        ]);

        debug!("Next freelist trunk page: {}", next_page_num);

        let count_offset: usize = page_offset + 4;
        let count: u32 = u32::from_be_bytes([
            bytearray[count_offset], 
            bytearray[count_offset + 1],
            bytearray[count_offset + 2], 
            bytearray[count_offset + 3]
        ]);
        debug!("Free pages array count: {}", count);
        
        let next_freepages: Vec<u32> = match count {
            0 => vec![],
            n => {
                let array_offset: usize = count_offset + 4;
                let mut next_pages: Vec<u32> = vec![];
                for i in 0 .. n as usize {
                    next_pages.push(
                        u32::from_be_bytes([
                            bytearray[array_offset + 4 * i], 
                            bytearray[array_offset + 1 + 4 * i],
                            bytearray[array_offset + 2 + 4 * i], 
                            bytearray[array_offset + 3 + 4 * i]
                        ])
                    )
                }
                next_pages
            }
        }; 

        debug!("Array len: {}", next_freepages.len());

        FreeListTrunkPageHeader {
            next_page_num,
            count,
            next_freepages
        }
    }
}

impl std::fmt::Debug for FreeListTrunkPageHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result = writeln!(f, "\tNEXT FREELIST TRUNK PAGE:\t{}", self.next_page_num);
        res = writeln!(f, "\tCOUNT:\t\t{}", self.count);
        res = writeln!(f, "\tFREE PAGES:\t{:?}", self.next_freepages);

        res
    }
}

#[derive(Clone)]
struct FreeListTrunkPage {
    number: u32,
    pub header: FreeListTrunkPageHeader,
    cells: Vec<Cell>
}

impl FreeListTrunkPage {
    fn new(
        bytearray: &[u8], 
        page_num: u32, 
        page_offset: usize, 
        page_size: u32, 
        reserved_space: usize
    ) -> FreeListTrunkPage {

        debug!("Page offset: 0x{:02x?}", page_offset);
        let header: FreeListTrunkPageHeader = FreeListTrunkPageHeader::new(bytearray, page_offset);

        debug!("FREEPAGE HEADER:\n{:?}", header);
        let cell_array_offset: usize = page_offset + header.count as usize * 4 + 8; // +8 -> | 0x00 * 4 + 4 bytes of 'count' |

        let cell_array: Vec<usize> = Self::get_cell_array(bytearray, cell_array_offset);
        let mut deleted_cells: Vec<Cell> = vec![];

        /*for &cell_offset in cell_array.iter() {
            //debug!("cell_offset: 0x{:02x?}", cell_offset);
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

        FreeListTrunkPage { 
            number: page_num, 
            header, 
            cells: deleted_cells
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
            //debug!("CELL OFFSET: 0x{:02x?}", cell);
            cell_array.push(cell);
            n += 1;
            cell = u32::from_be_bytes([
                0, 
                0, 
                bytearray[array_offset + 2*n], 
                bytearray[array_offset + 1 + 2*n]
            ]) as usize;
        }

        //debug!("CELL ARRAY: {:?}", cell_array);
        cell_array
    }

    fn parse_leaf_cell(
        bytearray: &[u8], 
        page_type: u8, 
        cell_address: usize,
        page_size: u32,
        reserved_space: usize
    ) -> LeafCell {
        
        let cell: LeafCell;
        let (cell_size, _) = read_varint(&bytearray[cell_address..]);
        
        debug!("CELL SIZE: {}", cell_size);
        //debug!("Cell offset: 0x{:02x?}", cell_address);
        
        let usable_page_size: u32 = page_size - reserved_space as u32;
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
                cell = LeafCell::new(
                    &bytearray, 
                    cell_address, 
                    page_type, 
                    Some(k),
                );
            }
            else {
                cell = LeafCell::new(
                    &bytearray, 
                    cell_address, 
                    page_type, 
                    Some(m),
                );
            }
        }        
        
        cell
    }

}

impl std::fmt::Debug for FreeListTrunkPage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result = writeln!(f, "FREE PAGE {}", self.number);
        
        res = writeln!(f, "\t{:?}", self.header);
        for (i, cell) in self.cells.iter().enumerate(){
            res = writeln!(f, "\tCELL {}", i);
            match cell {
                Cell::LC(c) => res = writeln!(f, "{:?}", c),
                Cell::ITC(c) => res = writeln!(f, "{:?}", c),
            }
        }
        
        res
    }
}


pub struct TableInfo {
    pub root_page: String,
    pub sql: String
}

/// Representation of the header of an sqlite file contained in the first 100 bytes of the file
#[derive(Clone)]
pub struct FileHeader {
    magic                       : String,
    page_size                   : u32,
    format_write                : u8,
    format_read                 : u8,
    reserved_space              : u32,
    max_embed_payload_fraction  : u32,
    min_embed_payload_fraction  : u32,
    file_change_ctr             : u32,
    page_count                  : u32,
    first_freelist_trunk_page   : u32,
    freelist_page_count         : u32,
    schema_cookie               : u32,
    schema_format_number        : u32,
    page_cache_size             : u32,
    largest_rootbtree_page_num  : u32,
    text_encodig                : u32,
    user_version                : u32,
    is_incremental_vacuum_mode  : u32,
    app_id                      : u32,
    version                     : u32,
}

impl FileHeader {

    /// Parses the first 100 bytes of the file
    pub fn new(bytearray: &[u8]) -> Result<FileHeader, &'static str> {
        info!("Parsing file header...");
        let s: &[u8] = &bytearray[0..15];
        let magic = match from_utf8(s) {
            Ok(v) => v,
            Err(_) => "ERROR",
        };

        if magic != "SQLite format 3" {
            return Err("NOT AN SQLITE FILE");
        }


        let mut page_size: u32 = u32::from_be_bytes([0, 0, bytearray[16], bytearray[17]]);
        if page_size == 1 {
            page_size = 65536;
        }

        let text_encoding: u32 = u32::from_be_bytes([bytearray[56], bytearray[57], bytearray[58], bytearray[59]]);
        
        STRING_ENCODING.with(|encoding| {
            *encoding.borrow_mut() = text_encoding;
        });

        Ok(FileHeader {
            magic: magic.to_string(),
            page_size,
            format_write: bytearray[18],
            format_read: bytearray[19],
            reserved_space: u32::from_be_bytes([0, 0, 0, bytearray[20]]),
            max_embed_payload_fraction: u32::from_be_bytes([0, 0, 0, bytearray[21]]),
            min_embed_payload_fraction: u32::from_be_bytes([0, 0, 0, bytearray[22]]),
            file_change_ctr: u32::from_be_bytes([bytearray[24], bytearray[25], bytearray[26], bytearray[27]]),
            page_count: u32::from_be_bytes([bytearray[28], bytearray[29], bytearray[30], bytearray[31]]),
            first_freelist_trunk_page: u32::from_be_bytes([bytearray[32], bytearray[33], bytearray[34], bytearray[35]]),
            freelist_page_count: u32::from_be_bytes([bytearray[36], bytearray[37], bytearray[38], bytearray[39]]),
            schema_cookie: u32::from_be_bytes([bytearray[40], bytearray[41], bytearray[42], bytearray[43]]),
            schema_format_number: u32::from_be_bytes([bytearray[44], bytearray[45], bytearray[46], bytearray[47]]),
            page_cache_size: u32::from_be_bytes([bytearray[48], bytearray[49], bytearray[50], bytearray[51]]),
            largest_rootbtree_page_num: u32::from_be_bytes([bytearray[52], bytearray[53], bytearray[54], bytearray[55]]),
            text_encodig: u32::from_be_bytes([bytearray[56], bytearray[57], bytearray[58], bytearray[59]]),
            user_version: u32::from_be_bytes([bytearray[60], bytearray[61], bytearray[62], bytearray[63]]),
            is_incremental_vacuum_mode: u32::from_be_bytes([bytearray[64], bytearray[65], bytearray[66], bytearray[67]]),
            app_id: u32::from_be_bytes([bytearray[68], bytearray[69], bytearray[70], bytearray[71]]),
            version: u32::from_be_bytes([bytearray[96], bytearray[97], bytearray[98], bytearray[99]]),
        })
    }

}

impl std::fmt::Debug for FileHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result = writeln!(f, "FILE HEADER");

        res = writeln!(f, "\tMAGIC:\t\t\t\t\t\t{}", self.magic);
        res = writeln!(f, "\tPAGE SIZE:\t\t\t\t\t{:?}", self.page_size);
        res = writeln!(f, "\tFORMAT WRITE:\t\t\t\t{:?}\t(2 = WAL)", self.format_write);
        res = writeln!(f, "\tFORMAT READ:\t\t\t\t{:?}\t(2 = WAL)", self.format_read);
        res = writeln!(f, "\tRESERVED SPACE:\t\t\t\t{:?}", self.reserved_space);
        res = writeln!(f, "\tMAX EMBED PAYLOAD FRACTION:\t{:?}", self.max_embed_payload_fraction);
        res = writeln!(f, "\tMIN EMBED PAYLOAD FRACTION:\t{:?}", self.min_embed_payload_fraction);
        res = writeln!(f, "\tFILE CHANGE COUNTER:\t\t{:?}", self.file_change_ctr);
        res = writeln!(f, "\tPAGE COUNT:\t\t\t\t\t{:?}", self.page_count);
        res = writeln!(f, "\tFIRST FREELIST TRUNK PAGE NUM:\t{:?}", self.first_freelist_trunk_page);
        res = writeln!(f, "\tFREELIST PAGES COUNT:\t\t{:?}", self.freelist_page_count);
        res = writeln!(f, "\tSCHEMA COOKIE:\t\t\t\t{:?}\t(Incremented each time the db schema changes)", self.schema_cookie);
        res = writeln!(f, "\tSCHEMA FORMAT NUMBER:\t\t{:?}", self.schema_format_number);
        res = writeln!(f, "\tPAGE CACHE SIZE:\t\t\t{:?}", self.page_cache_size);
        res = writeln!(f, "\tLARGEST ROOT B-TREE PAGE NUM:\t{:?}", self.largest_rootbtree_page_num);
        res = writeln!(f, "\tTEXT ENCODING:\t\t\t\t{:?}\t(1 = UTF8, 2 = UTF16le; 3 = UTF16be)", self.text_encodig);
        res = writeln!(f, "\tUSER VERSION:\t\t\t\t{:?}", self.user_version);
        res = writeln!(f, "\tAUTO VACUUM MODE:\t\t\t{:?}\t(0 = DISABLED; 1 = AUTO/FULL; 2 = INCREMENTAL)", self.is_incremental_vacuum_mode);
        res = writeln!(f, "\tAPPLICATION ID:\t\t\t\t{:?}", self.app_id);
        res = writeln!(f, "\tVERSION:\t\t\t\t\t{:?}", self.version);
        res = writeln!(f, "");

        res
    }
}


/// Representation of a sqlite file composed of an header and a group of pages
#[derive(Clone)]
pub struct MainFile {
    header: FileHeader,
    pages: Vec<Page>,
    freepages: Vec<FreeListTrunkPage> 
}

impl MainFile {

    /// Parses the whole raw bytes of the file and creates a DatabaseFile struct
    pub fn new(bytearray: &[u8]) -> Result<MainFile, &'static str> {
        info!("Parsing main database file...");
        let header: FileHeader = match FileHeader::new(&bytearray){
            Ok(h) => h,
            Err(e) => return Err(e) 
        };

        unsafe {
            PAGE_SIZE = header.page_size as usize;
            RESERVED_SPACE = header.reserved_space as usize;
        }

        debug!("{:?}", header);
        info!("Pages: {}", header.page_count);

        let mut pages: Vec<Page> = vec![];
        let mut freepages: Vec<FreeListTrunkPage> = vec![];
        
        /* Create an array with page num of free pages */
        if header.freelist_page_count > 0 {
            
            unsafe {
                FREEPAGES.push(header.first_freelist_trunk_page);
            }
            
            debug!("First free page: {}", header.first_freelist_trunk_page);
            let mut first_freepage: FreeListTrunkPage = FreeListTrunkPage::new(
                &bytearray,
                header.first_freelist_trunk_page - 1 ,
                ((header.first_freelist_trunk_page - 1) * header.page_size) as usize,
                header.page_size,
                header.reserved_space as usize
            );
            
            unsafe {
                FREEPAGES.append(first_freepage.header.next_freepages.as_mut());
            }

            let mut n = first_freepage.header.next_page_num;
            while n != 0 {
                debug!("NEXT FREE TRUNK PAGE: {}", n);
                first_freepage = FreeListTrunkPage::new(
                    &bytearray,
                    n - 1 ,
                    ((n - 1) * header.page_size) as usize,
                    header.page_size,
                    header.reserved_space as usize
                );
                unsafe {
                    FREEPAGES.append(first_freepage.header.next_freepages.as_mut());
                }

                n = first_freepage.header.next_page_num;

                debug!("N: {}", n);
            }

            unsafe {
                debug!("FINAL ({}){:?}", FREEPAGES.len(), FREEPAGES);
            }
        }

        for page_num in 0..header.page_count {

            unsafe{
                /* If page is a free page, do not parse it now */
                if FREEPAGES.contains(&(page_num + 1)){
                    warn!("Page {} is a free page. Let's go to the next one", page_num + 1);
                    continue;
                }
                
                /* If page is an overflow page, its content is taken when parsing leaf table pages' cells */
                if OVERFLOW_PAGES.contains(&(page_num + 1)){
                    warn!("Page {} is an overflow page. Let's go to the next one", page_num + 1);
                    continue;
                }
            }

            let parsed_page: Page = Page::new(
                &bytearray, 
                header.page_size as usize * page_num as usize, 
                page_num,
                false
            );
            pages.push(parsed_page);
        }

        info!("Done!");

        //info!("Free pages count: {}", header.freelist_page_count);
        // Parse freepages

        /*match first_freepage {
            Some(ffp) => {
                debug!("First free page: {:?}", ffp);
                freepages.push(ffp.clone());

                let next_freepages_array = ffp.header.next_freepages;

                for &next_free_page_num in next_freepages_array.iter() {
                    let succ_free_page: Page = Page::new(
                        &bytearray,
                        ((next_free_page_num - 1)  * header.page_size) as usize,
                        next_free_page_num - 1,
                        header.page_size,
                        header.reserved_space as usize,
                        false
                    );
                    /*freepages.push(
                        succ_free_page
                    );*/
                }
            }
            None => ()
        };*/

        Ok(MainFile {
            header,
            pages,
            freepages
        })
    }

    pub fn get_page_by_number(&self, number: u32) -> Option<Page> {
        for page in self.pages.iter(){
            if page.number() == number {
                return Some((*page).clone());
            }
        }

        None
    }

    /// Returns a mapping between table name and sql creation query plus root page num
    /// e.g.: "properties" -> ("CREATE TABLE properties(name TEXT, class TEXT NOT NULL)", "5")
    pub fn get_tables_info(&self) -> HashMap<String, TableInfo> {
        let mut tables_info: HashMap<String, TableInfo> = HashMap::new();

        let first_page: Page = self.get_page_by_number(1).unwrap();
        for cell in first_page.live_cells().iter() {
            match cell {
                Cell::LC(c) => {
                    if c.data()[0] == "table" {
                        tables_info.insert(
                            c.data()[1].to_string(),
                            TableInfo {
                                root_page: c.data()[3].to_string(),
                                sql: c.data()[4].to_string()
                            }
                        );
                    }
                },
                Cell::ITC(c) => {
                    for cell in self.get_page_by_number(c.left_pointer()).unwrap().live_cells().iter() {
                        match cell {
                            Cell::LC(c) => {
                                if c.data()[0] == "table" {
                                    tables_info.insert(
                                        c.data()[1].to_string(),
                                        TableInfo {
                                            root_page: c.data()[3].to_string(),
                                            sql: c.data()[4].to_string()
                                        }
                                    );
                                }
                            }
                            _ => (),
                        }
                    }
                    if first_page.header().rightmost_ptr().is_some() {
                        for cell in self.get_page_by_number(first_page.header().rightmost_ptr().unwrap()).unwrap().live_cells().iter() {
                            match cell {
                                Cell::LC(c) => {
                                    if c.data()[0] == "table" {
                                        tables_info.insert(
                                            c.data()[1].to_string(),
                                            TableInfo {
                                                root_page: c.data()[3].to_string(),
                                                sql: c.data()[4].to_string()
                                            }
                                        );
                                    }
                                }
                                _ => (),
                            }
                        }
                    }                    
                }
            }
        }
        
        tables_info
    }

    pub fn get_triggers(&self) -> Vec<String> {
        let mut triggers: Vec<String> = vec![];

        let first_page: Page = self.get_page_by_number(1).unwrap();

        for  cell in first_page.live_cells().iter() {
            match cell {
                Cell::LC(c) => {
                    if c.data()[0] == "trigger" {
                        triggers.push(
                            c.data()[4].to_string()
                        );
                    }
                },
                Cell::ITC(c) => {
                    for cell in self.get_page_by_number(c.left_pointer()).unwrap().live_cells().iter() {
                        match cell {
                            Cell::LC(c) => {
                                if c.data()[0] == "trigger" {
                                    triggers.push(
                                        c.data()[4].to_string()
                                    );
                                }
                            }
                            _ => (),
                        }
                    }
                    if first_page.header().rightmost_ptr().is_some() {
                        for cell in self.get_page_by_number(first_page.header().rightmost_ptr().unwrap()).unwrap().live_cells().iter() {
                            match cell {
                                Cell::LC(c) => {
                                    if c.data()[0] == "trigger" {
                                        triggers.push(
                                            c.data()[4].to_string()
                                        );
                                    }
                                }
                                _ => (),
                            }
                        }
                    }                    
                }
            }
        }

        triggers
    }

}

impl std::fmt::Debug for MainFile {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result = writeln!(f, "{:?}", self.header);
        
        for page in self.pages.iter(){
            res = writeln!(f, "{:?}", page);
        }

        for freepage in self.freepages.iter(){
            res = writeln!(f, "{:?}", freepage);
        }

        res
    }
}