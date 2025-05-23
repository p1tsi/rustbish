use clap::Parser;

//#[derive(Debug, Clone)]
/*enum OutputFormat{
    JSON,
    CSV
}*/

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Path of the main SQLite file
    pub filepath: String,

    /// Output format: JSON | CSV
    #[arg(long, default_value_t = String::from("JSON"))]
    pub format: String,

    /// Output directory contaning generated files
    #[arg(short, long, default_value_t = String::from("output"))]
    pub output_dir: String,

    /// If present, parse also WAL file. It should be inside the same directory of main file
    #[arg(long, short, action)]
    pub wal: bool,

    /// If present, create TXTs of parsed files
    #[arg(long, short, action)]
    pub parsed_files: bool,

    /// If present, only print SQLite file header
    #[arg(long, short, action)]
    pub fileheader: bool,

    /// If present, try to discover missing row ids for each table
    #[arg(long, short, action)]
    pub missingids: bool,

    /// If present, try to get triggers queries
    #[arg(long, short, action)]
    pub triggers: bool,

    /// If present, try to extract indices
    #[arg(long, short, action)]
    pub indices: bool,

    /// If present, print DEBUG info to stdout
    #[arg(long, short, action)]
    pub debug: bool,
}
