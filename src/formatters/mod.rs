use log::info;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::db::DataBase;
use crate::{mainfile::MainFile, wal::WALFile};

pub fn csv_run(
    main_db_file: MainFile,
    wal_file: Option<WALFile>,
    missing_ids: bool,
    triggers: bool,
    indices: bool,
) -> () {
    info!("Write CSV files");

    let db: DataBase = DataBase::new(main_db_file, wal_file, missing_ids, triggers, indices);

    db.tables().iter().for_each(|table| {
        let mut outfile: File = File::create(
            Path::new(".")
                .join("output")
                .join(format!("{}.csv", &table.name)),
        )
        .unwrap();
        write!(outfile, "{}", table.to_csv()).unwrap();
    });
}

pub fn json_run(
    main_db_file: MainFile,
    wal_file: Option<WALFile>,
    mut outfile: File,
    missing_ids: bool,
    triggers: bool,
    indices: bool,
) -> () {
    info!("Write JSON file");

    let db: DataBase = DataBase::new(
        main_db_file,
        wal_file,
        missing_ids,
        triggers,
        indices, //args.indices
    );

    write!(outfile, "{}", db.to_json()).unwrap();
}
