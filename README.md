# Rustbish 

Rustbish is an SQLite parser written in Rust for dumpster diving
into main database files and their corresponding WAL to resurrect lost or deleted data.
Rustbish extracts data without opening any database connections (neither in write nor in read-only permission):
it reads files as bytearray and tries to reconstruct tables and rows.
In particular, Rustbish tries to resume and show all new rows not already committed to main file, all deleted rows which are 
still present in the main file but that would be removed with the next WAL checkpoint and all modifications of a particular row.

The tool is designed specifically for forensic investigators and security experts
who need to extract crucial information from SQLite databases.


## Installation

Clone the project and build it from sources with:
```
$ cargo b --release 
```

```
$ target/release/rustbish --help
A tool to parse raw SQLite and their WAL files

Usage: rustbish [OPTIONS] <FILEPATH>

Arguments:
  <FILEPATH>  Path of the main SQLite file

Options:
      --format <FORMAT>          Output format: JSON | CSV [default: JSON]
  -o, --output-dir <OUTPUT_DIR>  Output directory contaning generated files [default: output]
  -w, --wal                      If present, parse also WAL file. It should be inside the same directory of main file
  -p, --parsed-files             If present, create TXTs of parsed files
  -f, --fileheader               If present, only print SQLite file header
  -m, --missingids               If present, try to discover missing row ids for each table
  -t, --triggers                 If present, try to get triggers queries
  -i, --indices                  If present, try to extract indices
  -d, --debug                    If present, print DEBUG info to stdout
  -h, --help                     Print help
  -V, --version                  Print version
```

## Known Issue

- Currently I am facing some problems with WAL frame containing overflow pages.
- The procedure that extracts column names from creation table query sometimes fails to get correclty all names.

## Note 

It's important to note that the tool may not be able to extract data from all types of SQLite database files, 
for instance those that are encrypted or otherwise protected. 
If you encounter any issues while using Rustbish, or if you need support for extracting data from a specific type of SQLite database, 
please do not hesitate to reach me out for assistance.

