use wasm_bindgen::prelude::*;

mod constants;
mod db;
mod mainfile;
mod structs;
mod utils;
mod wal;

use db::DataBase;
use mainfile::MainFile;
use wal::WALFile;

/// WebAssembly entry point
#[wasm_bindgen]
pub fn process_files(bytearray: &[u8]) -> String {
    match MainFile::new(&bytearray) {
        Ok(mf) => {
            /*let mut wal_file = None;
            if wal_bytearray.len() > 0 {
                wal_file = Some(WALFile::new(&wal_bytearray, wal_bytearray.len() as u64));
            }*/
            
            //DataBase::new(mf, None, false, false, false).to_json()
            String::from("Ok")
        }
        Err(_) => String::from("Error"),
    }
}
