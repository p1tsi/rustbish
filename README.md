# Rustbish (WIP)

Rustbish is a tool written in Rust that lets you dumpster dive
into SQLite files looking for some valuable data.
Rustbish is an SQLite database parser through which extract data
without perform an "sqlite3_open" call, which cause a WAL checkpoint. 
The tool is designed specifically for forensic investigators and security experts
who need to extract crucial information from SQLite databases.

## Known Issue

Currently I am facing some problems with WAL frame containing overflow pages.

## Note 

It's important to note that the tool may not be able to extract data from all types of SQLite database files, for instance those that are encrypted or otherwise protected. If you encounter any issues while using Rustbish, or if you need support for extracting data from a specific type of SQLite database, please do not hesitate to reach me out for assistance.

