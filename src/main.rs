use log::{info, warn, error};
use log::LevelFilter;
use simple_logger::SimpleLogger;
use std::fs::{
    read,
    File
};
use std::path::{Path, MAIN_SEPARATOR};
use std::io::Write;
use time::macros::format_description;

use clap::Parser;


mod mainfile;
mod wal;
mod db;
mod utils;
mod structs;
mod formatters;
mod args;
mod constants;

use mainfile::{FileHeader, MainFile};
use wal::WALFile;
use args::Args;


fn generate() -> String {
    format!("
    ______          _   _     _     _     
    | ___ \\        | | | |   (_)   | |    
    | |_/ /   _ ___| |_| |__  _ ___| |__  
    |    / | | / __| __| '_ \\| / __| '_ \\ 
    | |\\ \\ |_| \\__ \\ |_| |_) | \\__ \\ | | |
    \\_| \\_\\__,_|___/\\__|_.__/|_|___/_| |_|
                                          
                                          by p1tsi\n\n")
}

fn main() -> () {

    let args: Args = Args::parse();

    println!("{}", generate());

    let log_filter_level = match args.debug {
        true => LevelFilter::Debug,
        false => LevelFilter::Info
    };

    SimpleLogger::new()
        .env()
        .with_timestamp_format(format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"))
        .with_level(log_filter_level)
        .init()
        .unwrap();

    let db_filepath: &String = &args.filepath;
    
    if !Path::new(db_filepath).exists() {
        error!("{} not found", db_filepath);
        return;
    }
    
    let filename: &str = Path::new(db_filepath)
    .file_stem()
    .unwrap()
    .to_str()
    .unwrap();

    let bytearray: Vec<u8> = read(db_filepath).unwrap();

    if bytearray.len() == 0{
        error!("Given file ({}) is empty", db_filepath);
        
        return;
    }

    // Only print header of main file
    if args.fileheader {
        match FileHeader::new(&bytearray){
            Ok(file_header) => println!("{:?}", file_header),
            Err(e) => error!("{}", e)
        };
        
        return;
    }

    // Creating output dir
    if !Path::new(&args.output_dir.clone().to_string()).exists(){
        info!("Create output dir: {}", args.output_dir.to_string());
        let _ = std::fs::create_dir_all(
            Path::new(".")
                .join(args.output_dir.to_string())
        );
    }

    // Main file
    info!("Main DB file: {}", db_filepath);
    let parsed_main_file: MainFile = match MainFile::new(&bytearray) {
        Ok(mainfile) => mainfile,
        Err(e) => {
            error!("{}", e);
            return;
        } 
    };

    if args.parsed_files {
        let mut out_db_file: File = File::create(
            Path::new(".")
            .join(args.output_dir.to_string())
            .join(format!("{}.txt", filename))
        ).unwrap();
        let _ = write!(out_db_file, "{:?}", parsed_main_file);
    }
    
    info!("{:=<40}", "");

    // WAL file
    let mut parsed_wal_file: Option<WALFile> = None;
    if args.wal {
        let wal_filepath: String = format!("{}{}", db_filepath, "-wal");
        if !Path::new(&wal_filepath).exists() {
            warn!("{} not found", wal_filepath);
        }
        else{
            info!("WAL file: {}", wal_filepath);
            let wal_bytearray: Vec<u8> = read(&wal_filepath).unwrap();
            if wal_bytearray.len() == 0 {
                warn!("WAL file is empty");
            }
            else{
                parsed_wal_file = Some(WALFile::new(&wal_bytearray, wal_bytearray.len() as u64));
                
                // Print the txt of extracted data from WAL
                if args.parsed_files {
                    let mut out_wal_file: File = File::create(
                        Path::new(".")
                        .join(args.output_dir.to_string())
                        .join(format!("{}-wal.txt", filename))
                    ).unwrap();
                    let _ = write!(out_wal_file, "{:?}", parsed_wal_file);
                }
                
            }
        }
        info!("{:=<45}", "");
    }
    
    let out_format = args.format;
    match out_format.as_str() {
        "JSON" => {
            let json_filename: String = format!("{}.json", filename);
            let outfile: File = File::create(
                Path::new(".")
                .join(args.output_dir.to_string())
                .join(&json_filename)
            ).unwrap();
            formatters::json_run(
                parsed_main_file,
                parsed_wal_file,
                outfile,
                args.missingids,
                args.triggers,
                args.indices);
            info!("Created JSON file to: {}{}{}", args.output_dir, MAIN_SEPARATOR, json_filename);
        },
        "CSV" => {
            formatters::csv_run(
                parsed_main_file, 
                parsed_wal_file,
                args.missingids,
                args.triggers,
                args.indices
            );
        },
        undefined_formatter => warn!("{undefined_formatter} formatter not found.")
    };

    info!("Done. I had a nice trip. See you soon!");

}
