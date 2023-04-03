use log::{info, warn, debug};
use crate::structs::{
    Page,
    FREEPAGES,
    OVERFLOW_PAGES
};


/// Representation of the header of a wal frame
#[derive(Clone)]
pub struct WALFrameHeader {
    page_num: u32,
    page_count_after_commit: u32,   // For commit records, the size of the database file in pages after the commit. For all other records, zero. 
    salt1: u32,                     // Should be the same as the one in WALFileHeader
    salt2: u32,                     // Should be the same as the one in WALFileHeader  
    checksum1: u32,                        
    checksum2: u32           
}

impl WALFrameHeader {

    /// Parses first 32 bytes of the wal file
    fn new(wal_bytearray: &[u8], page_ptr: usize) -> WALFrameHeader {
        
        WALFrameHeader{
            page_num: u32::from_be_bytes([
                wal_bytearray[page_ptr], 
                wal_bytearray[page_ptr + 1], 
                wal_bytearray[page_ptr + 2], 
                wal_bytearray[page_ptr + 3]]
            ),
            page_count_after_commit : u32::from_be_bytes([
                wal_bytearray[page_ptr + 4], 
                wal_bytearray[page_ptr + 5], 
                wal_bytearray[page_ptr + 6], 
                wal_bytearray[page_ptr + 7]]
            ),
            salt1: u32::from_be_bytes([
                wal_bytearray[page_ptr + 8], 
                wal_bytearray[page_ptr + 9], 
                wal_bytearray[page_ptr + 10], 
                wal_bytearray[page_ptr + 11]]
            ),
            salt2: u32::from_be_bytes([
                wal_bytearray[page_ptr + 12], 
                wal_bytearray[page_ptr + 13], 
                wal_bytearray[page_ptr + 14], 
                wal_bytearray[page_ptr + 15]]
            ), 
            checksum1: u32::from_be_bytes([
                wal_bytearray[page_ptr + 16], 
                wal_bytearray[page_ptr + 17],
                wal_bytearray[page_ptr + 18], 
                wal_bytearray[page_ptr + 19]]
            ),
            checksum2: u32::from_be_bytes([
                wal_bytearray[page_ptr + 20],
                wal_bytearray[page_ptr + 21], 
                wal_bytearray[page_ptr + 22], 
                wal_bytearray[page_ptr + 23]]
            ),
        }
    }

    pub fn page_num(&self) -> u32 {
        self.page_num
    }
}

impl std::fmt::Debug for WALFrameHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result = writeln!(f, "\tPAGE NUM:\t\t\t\t\t{}", self.page_num);

        res = writeln!(f, "\tPAGE COUNT AFTER COMMIT:\t{}", self.page_count_after_commit);
        res = writeln!(f, "\tPAGE FIRST SALT:\t\t\t0x{:02X?}", self.salt1);
        res = writeln!(f, "\tPAGE SECOND SALT:\t\t\t0x{:02X?}", self.salt2);
        res = writeln!(f, "\tPAGE FIRST CHECKSUM:\t\t0x{:02X?}", self.checksum1);
        res = writeln!(f, "\tPAGE SECOND CHECKSUM:\t\t0x{:02X?}", self.checksum2);
        res = writeln!(f, "");

        res
    }
}

/// Representation of a frame in WAL file
#[derive(Clone)]
pub struct WALFrame {
    i: u32,
    header: WALFrameHeader,
    page: Page,
}

impl WALFrame {

    /// Parses a region of raw bytes of the file and returns a frame
    fn new(
        bytearray: &[u8], 
        frame_num: u32, 
        offset: usize, 
    ) -> Option<WALFrame> {

        let header: WALFrameHeader = WALFrameHeader::new(bytearray, offset);
        debug!("FRAME WITH PAGE: {:?}", header.page_num);

        //let mainpage_offset = (header.page_num - 1) as usize * 4096;
        //debug!("{}", &bytearray[offset + 24 .. offset + 24 + 4096] == &maindbbytes[mainpage_offset .. mainpage_offset + 4096]);

        unsafe{
            /* If page is a free page, do not parse it now */
            if FREEPAGES.contains(&(header.page_num)){
                warn!("Page {} is a free page. Let's go to the next one", header.page_num);
                debug!("{:*<20}", "");
                return None;
            }
            
            /* If page is an overflow page, its content has already been taken */
            if OVERFLOW_PAGES.contains(&(header.page_num)){
                warn!("Page {} is an overflow page. Let's go to the next one", header.page_num);
                debug!("{:*<20}", "");
                return None;
            }
        }

        let page: Page = Page::new(
            bytearray, 
            offset + 24, 
            header.page_num - 1,
            true
        );

        Some(WALFrame{
            i: frame_num,
            header,
            page 
        })
    }

    pub fn header(&self) -> WALFrameHeader {
        self.header.clone()
    }

    pub fn page(&self) -> Page {
        self.page.clone()
    }
}

impl std::fmt::Debug for WALFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result;

        res = writeln!(f, "FRAME {}", self.i);
        res = writeln!(f, "{:?}", self.header);
        //res = writeln!(f, "{:?}", self.page);

        res
    }
}


/// Representation of the header of the WAL file
#[derive(Clone)]
pub struct WALFileHeader {
    magic: u32,
    format_version: u32,
    page_size: u32,
    checkpoint_seq_num: u32,
    salt1: u32,
    salt2: u32,
    checksum1: u32,
    checksum2: u32,
    frame_count: u32,
}

impl WALFileHeader {
    fn new(wal_bytearray: &[u8], file_size: u64) -> WALFileHeader {
        let page_size = u32::from_be_bytes([wal_bytearray[8], wal_bytearray[9], wal_bytearray[10], wal_bytearray[11]]);
        WALFileHeader {
            magic: u32::from_be_bytes([wal_bytearray[0], wal_bytearray[1], wal_bytearray[2], wal_bytearray[3]]),
            format_version: u32::from_be_bytes([wal_bytearray[4], wal_bytearray[5], wal_bytearray[6], wal_bytearray[7]]),
            page_size,
            checkpoint_seq_num: u32::from_be_bytes([wal_bytearray[12], wal_bytearray[13], wal_bytearray[14], wal_bytearray[15]]),
            salt1: u32::from_be_bytes([wal_bytearray[16], wal_bytearray[17], wal_bytearray[18], wal_bytearray[19]]),
            salt2: u32::from_be_bytes([wal_bytearray[20], wal_bytearray[21], wal_bytearray[22], wal_bytearray[23]]),
            checksum1: u32::from_be_bytes([wal_bytearray[24], wal_bytearray[25], wal_bytearray[26], wal_bytearray[27]]),
            checksum2: u32::from_be_bytes([wal_bytearray[28], wal_bytearray[29], wal_bytearray[30], wal_bytearray[31]]),
            frame_count: ((file_size - 32) / (page_size as u64 + 24)) as u32,
        }
    }
}

impl std::fmt::Debug for WALFileHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result;

        res = writeln!(f, "\tMAGIC:\t\t\t\t0x{:02X?}", self.magic);
        res = writeln!(f, "\tVERSION:\t\t\t{:?}", self.format_version);
        res = writeln!(f, "\tPAGE_SIZE:\t\t\t{:?}", self.page_size);
        res = writeln!(f, "\tCHECKPOINT_SEQ_NUM:\t{:?}", self.checkpoint_seq_num);
        res = writeln!(f, "\tFIRST_SALT:\t\t\t0x{:02X?}", self.salt1);
        res = writeln!(f, "\tSECOND_SALT:\t\t0x{:02X?}", self.salt2);
        res = writeln!(f, "\tFIRST_CHECKSUM:\t\t0x{:02X?}", self.checksum1);
        res = writeln!(f, "\tSECOND_CHECKSUM:\t0x{:02X?}", self.checksum2);
        res = writeln!(f, "\tFRAME COUNT:\t\t{:?}", self.frame_count);
        res = writeln!(f, "");

        res
    }
}


/// Representation of a WAL file
#[derive(Clone)]
pub struct WALFile {
    header :    WALFileHeader,
    frames :    Vec<WALFrame>,
}

impl WALFile {

    /// Parses the whole file
    pub fn new(bytearray: &[u8], file_size: u64) -> WALFile {
        info!("Parsing WAL file...");

        let header: WALFileHeader = WALFileHeader::new(&bytearray, file_size);
        
        let mut frames: Vec<WALFrame> = vec![];
        let mut frame_offset: u32;
        for i in 0..header.frame_count{

            frame_offset = (header.page_size + 24) * i + 32;

            //debug!("Parsing frame {} at 0x{:02x?}", i, frame_offset);
            /*let db_filepath: &String = &String::from("/tmp/Keychains/keychain-2.db");
            let filename: &str = Path::new(db_filepath).file_stem().unwrap().to_str().unwrap();
            let maindbbytes = read(db_filepath).unwrap();*/

            match WALFrame::new(
                &bytearray, 
                i, 
                frame_offset as usize,
            ){
                Some(frame) => frames.push(frame),
                None => ()
            };
        }

        WALFile {
            header,
            frames,
        }
    }

    pub fn frames(&self) -> Vec<WALFrame> {
        self.frames.clone()
    }

    /*pub fn header(&self) -> WALFileHeader {
        self.header.clone()
    }

    pub fn get_frame_by_num(&self, frame_num: u32) -> WALFrame {
        self.frames[frame_num as usize].clone()
    }

    pub fn get_all_mod_pages(&self) -> HashMap<u32, u32> {
        let mut pages: HashMap<u32, u32> = HashMap::new();
        
        for frame in self.frames.iter(){
            if frame.page.is_table_leaf_page() {
                let page_num: u32 = frame.header.page_num - 1;
                let value: Option<&mut u32> = pages.get_mut(&page_num);
                match value {
                    Some(x) => {
                        *x += 1;
                    },
                    None => {
                        pages.insert(page_num, 1);
                    },
                };
            }   
        }

        trace!("{:?}", pages);
        pages
    }   

    pub fn get_frames_by_page_num(&self, number: u32) -> Vec<&WALFrame> {
        let mut res = vec![];
        for frame in self.frames.iter(){
            if frame.header.page_num == number {
                res.push(frame);
            }
        }

        res
    }*/

}

impl std::fmt::Debug for WALFile {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut res: std::fmt::Result = writeln!(f, "{:?}", self.header);

        for frame in self.frames.iter(){
            res = write!(f, "{:?}", frame);
            res = writeln!(f, "{:=<20}\n", "");          
        }

        res
    }
}
