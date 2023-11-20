use log::warn;
use regex::Regex;
use lazy_static::lazy_static; 
use std::str::from_utf8;
use std::cell::RefCell;

lazy_static! {
    static ref CREATE_TABLE_RE: Regex = Regex::new(r#"(?i)^CREATE TABLE [^(]+\(([a-zA-Z0-9_ ,%()\[\]'"]*)\)"#).unwrap();
    static ref VARCHAR_RE: Regex = Regex::new(r"[N]+VARCHAR[ ]*(\(.*\))*").unwrap();
    static ref CAST_FUNC_RE: Regex = Regex::new(r"\(CAST\([a-zA-Z0-9, '_%()]*AS INTEGER\)\)").unwrap();
}

//https://github.com/MidasLamb/sqlite-varint/blob/master/src/lib.rs
pub fn read_varint(bytes: &[u8]) -> (i64, usize) {
    let mut varint: i64 = 0;
    let mut bytes_read: usize = 0;
    for (i, byte) in bytes.iter().enumerate().take(9) {
        bytes_read += 1;
        if i == 8 {
            varint = (varint << 8) | *byte as i64;
            break;
        } else {
            varint = (varint << 7) | (*byte & 0b0111_1111) as i64;
            if *byte < 0b1000_0000 {
                break;
            }
        }
    }

    (varint, bytes_read)
}

// Extracts columns name from creation query
pub fn get_column_names_from_creation_query(query: &str) -> Result<Vec<String>, &'static str> {
    match CREATE_TABLE_RE.captures(query.replace("\n", "")
        .replace("\r", "")
        .replace("\t", " ")
        .as_str()){
        Some(captures) => {
            let column_definition = captures.get(1).unwrap().as_str();
            let cast_func_removed = CAST_FUNC_RE.replace(column_definition, "");
            let mut columns: Vec<String> = vec![];

            for column_def in cast_func_removed.split(',') {
                // This words defines last part of the columns section.
                if column_def.trim().to_ascii_uppercase().starts_with("CONSTRAINT") ||
                    column_def.trim().to_ascii_uppercase().starts_with("FOREIGN") ||
                    column_def.trim().to_ascii_uppercase().starts_with("CHECK") ||
                    column_def.trim().to_ascii_uppercase().starts_with("UNIQUE") ||
                    column_def.trim().to_ascii_uppercase().starts_with("PRIMARY") {
                        break;
                }

                //TODO: Removed for now
                /*if VARCHAR_RE.replace(column_def, "").contains("VARCHAR"){
                    continue;
                }*/

                columns.push(column_def.trim()
                    .replace('"', "")
                    .replace("'", "")
                    .split(' ')
                    .next()
                    .unwrap()
                    .to_string()
                );
            }

            Ok(columns)
        },
        None => Err("Error parsing table creation query. Maybe this is a VIRTUAL table")
    }
}


/*pub fn combine_columns_values(columns: Vec<String>, values: &mut Vec<String>) -> HashMap<String, String> {


    // In some cases, it happens that if last column value is NULL, 
    //  it is not included in cell header (so there is no reference of it at all)
    if columns.len() == values.len() + 1 {
        values.insert(columns.len() - 2 , String::from("NULL"));
    }

    if columns.len() != values.len() {
        trace!("DIFFERENT LENGTH OF COLUMNS AND VALUES!");
        trace!("COLUMNS: {:?} ({})", columns, columns.len());
        trace!("VALUES: {:?} ({})", values, values.len());
        return HashMap::new();
    }

    let mut map: HashMap<String, String> = HashMap::new();
    for (col, val) in columns.into_iter().zip(values) {
        map.insert(col, val.to_string());
    }
    
    map
}


pub fn is_table_without_rowid(query: &str) -> bool {
    query.trim().to_ascii_uppercase().replace(";", "").ends_with("WITHOUT ROWID")
}*/


thread_local! {
    pub static STRING_ENCODING: RefCell<u32> = RefCell::new(1);
}


pub fn read_encoded_string(bytes: &[u8]) -> String {
    STRING_ENCODING.with(|encoding| {
        match *encoding.borrow() {
            0 => read_utf8_string(bytes),
            1 => read_utf8_string(bytes),
            2 => read_utf16le_string(bytes),
            3 => read_utf16be_string(bytes),
            _ => String::from("UNKNOWN STRING ENCODING VALUE")
        }
    })
}


fn read_utf8_string(bytes: &[u8]) -> String {
    match from_utf8(bytes) {
        Ok(v) => String::from(v),
        Err(e) => {
            warn!("ERROR: {}", e);
            String::from("ERROR")
        },
    }
} 

fn read_utf16le_string(bytes: &[u8]) -> String {
    let s: Vec<u16> = bytes
        .chunks(2)
        .map(|e| u16::from_le_bytes(e.try_into().unwrap()))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&s)
}

fn read_utf16be_string(bytes: &[u8]) -> String {
    let s: Vec<u16> = bytes
        .chunks(2)
        .map(|e| u16::from_be_bytes(e.try_into().unwrap()))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&s)
}

/*fn read_overflow_page(offset: usize, page_size: usize) -> usize {
    offset - (offset % page_size)
}

pub fn read_overflow_page_for_string(bytearray: &[u8], overflow_page_offset: usize, page_size: usize, bytes_to_read: usize, new_start_content: &mut usize) -> String {
    
    trace!("overflow page offset: 0x{:02x?}", overflow_page_offset);
    let mut remaining_bytes_to_read: usize = bytes_to_read;
    trace!("remaining bytes to read: {}", remaining_bytes_to_read);
    let next_overflow_page_offset: usize = read_overflow_page(overflow_page_offset, page_size);
    trace!("overflow page offset: 0x{:02x?}", overflow_page_offset);
    let next_overflow_page_num: u32 = u32::from_be_bytes([
        bytearray[next_overflow_page_offset],
        bytearray[next_overflow_page_offset + 1],
        bytearray[next_overflow_page_offset + 2],
        bytearray[next_overflow_page_offset + 3]
    ]);
    trace!("NEXT OF PAGE NUM: {}", next_overflow_page_num);

    let mut final_str: String = String::new();
    if next_overflow_page_num == 0 || remaining_bytes_to_read < page_size - 4 {
        let t: &[u8] = &bytearray[overflow_page_offset + 4 .. overflow_page_offset + 4 + bytes_to_read];

        final_str = match from_utf8(t) {
            Ok(v) => String::from(v),
            Err(e) => {
                trace!("ERROR: {}", e);
                String::from("ERROR")
            },
        };
        *new_start_content = overflow_page_offset + 4 + remaining_bytes_to_read;
    }
    else{  
        let t: &[u8] = &bytearray[overflow_page_offset + 4 .. overflow_page_offset + page_size];
        final_str = match from_utf8(t) {
            Ok(v) => String::from(v),
            Err(e) => {
                trace!("ERROR: {}", e);
                String::from("ERROR")
            },
        };
        remaining_bytes_to_read -= page_size - 4;
        final_str = format!(
            "{}{}", 
            final_str, 
            read_overflow_page_for_string(
                bytearray, 
                (next_overflow_page_num - 1) as usize * page_size, 
                page_size, 
                remaining_bytes_to_read,
                new_start_content
            ));
    }
    
    final_str
}


pub fn read_overflow_page_for_blob(bytearray: &[u8], page_num: u32, page_size: usize, bytes_to_read: usize, new_start_content: &mut usize) -> Vec<u8> {
    let page_offset: usize = page_num as usize * page_size;

    let mut remaining_bytes_to_read: usize = bytes_to_read; 

    let next_overflow_page_num: u32 = u32::from_be_bytes([
        bytearray[page_offset],
        bytearray[page_offset + 1],
        bytearray[page_offset + 2],
        bytearray[page_offset + 3]
    ]);
    
    trace!("BYTES TO READ IN THIS PAGE: {}", bytes_to_read);
    trace!("NEXT OF PAGE NUM: {}", next_overflow_page_num);

    let mut final_bytearray: Vec<u8>;
    if next_overflow_page_num == 0 || remaining_bytes_to_read < page_size - 4 {
        final_bytearray = (&bytearray[page_offset + 4 .. page_offset + 4 + remaining_bytes_to_read]).to_vec();
        *new_start_content = page_offset + 4 + remaining_bytes_to_read;
    }
    else{
        final_bytearray = (&bytearray[page_offset + 4 .. page_offset + page_size]).to_vec();
        remaining_bytes_to_read -= page_size - 4;
        final_bytearray.append(
            read_overflow_page_for_blob(
                bytearray, 
                next_overflow_page_num - 1, 
                page_size, 
                remaining_bytes_to_read,
                new_start_content
            ).as_mut());
    }
    
    final_bytearray
}
*/