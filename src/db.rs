use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use log::{info, debug, warn};

use crate::structs::{
    Cell,
    LeafCell,
    Page
};

use crate::mainfile::{
    MainFile,
    TableInfo
};
use crate::wal::WALFile;
use crate::utils::get_column_names_from_creation_query;


fn diff_pages(first: Option<Page>, second: Page, diff: &mut Diff) -> () {
    let mut second_page_rowids = second.get_all_rowids();
    match first {
        Some(f) =>{
            let first_page_rowids = f.get_all_rowids();
            

            debug!("{:?}", first_page_rowids);
            debug!("{:?}", second_page_rowids);


            let mut i = 0;
            while i < first_page_rowids.len() {
                if second_page_rowids.contains(&first_page_rowids[i]){
                    let first_cell = f.get_cell_by_rowid(first_page_rowids[i]).unwrap();
                    let second_cell = second.get_cell_by_rowid(first_page_rowids[i]).unwrap();

                    let records1 = first_cell.data();
                    let records2 = second_cell.data();
                    for (j, field) in records1.iter().enumerate(){
                        if field != records2.get(j).unwrap(){
                            debug!("RECORD IN FIRST WAL PAGE DIFFERENT FORM RECORD IN MAIN DB");
                            debug!("{:?}", first_cell);
                            debug!("{:?}", second_cell);
                            debug!("\t\t\t---\t\t\t");
                            
                            if diff.modifications_contains_rowid(first_page_rowids[i]){
                                diff.aggregate_modified_cells(second_cell);
                            }
                            else{
                                diff.add_modification(second_cell)
                            }

                            break;
                        }
                    }
                    
                    for (j, &ids) in second_page_rowids.iter().enumerate(){
                        if ids == first_page_rowids[i]{
                            second_page_rowids.remove(j);
                            break;
                        }
                    }
                }
                else {
                    debug!("{} REMOVED ", first_page_rowids[i]);
                    diff.add_deletion(f.get_cell_by_rowid(first_page_rowids[i]).unwrap());
                }

                i += 1;
            }

            if second_page_rowids.len() > 0 {
                debug!("ADDED {:?}", second_page_rowids);
                for &new_rowid in second_page_rowids.iter(){

                    diff.add_insertion(second.get_cell_by_rowid(new_rowid).unwrap());
                }
            }

            debug!("{:*<10}", "");
        }
        None => {
            debug!("ADDED {:?}", second_page_rowids);
            for &new_rowid in second_page_rowids.iter(){
                diff.add_insertion(second.get_cell_by_rowid(new_rowid).unwrap());
            }
        }
    };
}


type Row = Vec<String>;

#[derive(Serialize, Deserialize, Clone)]
pub struct ModsSequence {
    rowid: u32,
    sequence: Vec<Row>
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Diff {
    insertions: Vec<LeafCell>,
    deletions: Vec<LeafCell>,
    modifications: Vec<ModsSequence>
}

impl Diff {

    fn add_deletion(&mut self, deleted_cell: LeafCell) -> () {
        self.deletions.push(deleted_cell);
    }

    fn add_insertion(&mut self, inserted_cell: LeafCell) -> () {
        self.insertions.push(inserted_cell);
    }

    fn add_modification(&mut self, mod_cell: LeafCell) -> () {
        self.modifications.push(ModsSequence { rowid: mod_cell.rowid().unwrap(), sequence: vec![mod_cell.data()] })
    }

    fn modifications_contains_rowid(&self, rowid: u32) -> bool {
        for mod_seq in self.modifications.iter(){
            if mod_seq.rowid == rowid {
                return true;
            }
        }
        
        false
    }

    fn aggregate_modified_cells(&mut self, cell: LeafCell) -> () {
        for mods_seq in self.modifications.iter_mut(){
            if mods_seq.rowid == cell.rowid().unwrap(){
                mods_seq.sequence.push(cell.data());
            }   
        }

    }

}



#[derive(Serialize, Deserialize, Clone)]
pub struct Table {
    pub name: String,
    pub columns: Vec<String>,
    pub rows_count: usize,
    pub rows: Vec<LeafCell>,
    pub missing_rowids: Option<Vec<u32>>,
    pub wal: Option<Diff>
}

impl Table {

    pub fn new(db_file: &MainFile, wal_file: &Option<WALFile>, table_name: String, info: TableInfo) -> Result<Table, &'static str> {

        debug!("{} - {}", table_name, info.sql);
        let columns: Vec<String> = match get_column_names_from_creation_query(&(info.sql)){
            Ok(columns) => columns,
            Err(e) => return Err(e)
        };

        debug!("{} - {:?} - {}", table_name, columns, columns.len());

        let mut rows: Vec<LeafCell> = vec![];
        let root_page_num: u32 = info.root_page.parse::<u32>().unwrap();
        let (mut leaves, mut internals): (Vec<u32>, Vec<u32>) = Table::init_leaf_internal_array(&db_file, root_page_num);
        debug!("LEAVES: {:?}\nINTERNALS: {:?}", leaves, internals);
        for &page_num in leaves.iter(){
            for cell in db_file.get_page_by_number(page_num).unwrap().live_cells().iter() {
                match cell {
                    Cell::LC(c) => {
                        rows.push(c.clone());
                    }
                    _ => (),
                }
            }
        }

        let wal: Option<Diff> = match wal_file.as_ref() {
            Some(f) => {

                /* Check if in WAL has been added/removed pages linked to current table */
                let diff = &mut Diff { deletions: vec![], insertions: vec![], modifications: vec![] };

                let mut previous: HashMap<u32, Page> = HashMap::new(); 

                for frame in f.frames().iter(){
                    let page_num = frame.header().page_num();

                    //TODO: in WAL file, a page which previously was of type 13, could change to type 5...

                    if leaves.contains(&page_num){
                        let previous_page: Option<Page> = match previous.get(&page_num) {
                            Some(p) => Some(p.clone()),
                            None => db_file.get_page_by_number(page_num)
                        };
                        diff_pages(
                            previous_page,
                            frame.page(),
                            diff
                        );

                        previous.insert(page_num, frame.page());
                    }
                    else if internals.contains(&page_num) {
                        Table::update_arrays(&db_file, frame.page(), &mut leaves, &mut internals);
                        debug!("LEAVES: {:?}\nINTERNALS: {:?}", leaves, internals);
                    }
                }
                
                Some(diff.to_owned())
            }
            None => None
        };

        let rows_count = rows.len();

        Ok(Table{
            name: table_name.to_string(),
            columns,
            rows,
            rows_count,
            missing_rowids: None,
            wal
        })
    }

    fn init_leaf_internal_array(db_file: &MainFile, root_page_num: u32) -> (Vec<u32>, Vec<u32>) {
        let mut leaves = vec![];
        let mut internals = vec![];
        let root_page = db_file.get_page_by_number(root_page_num).unwrap();

        if root_page.is_internal_table_page(){
            internals.push(root_page_num);
            for cell in root_page.live_cells().iter() {
                match cell {
                    Cell::ITC(c) => {
                        if db_file.get_page_by_number(c.left_pointer()).unwrap().is_internal_table_page(){
                            internals.push(c.left_pointer());
                            let (mut l, mut i) = Table::init_leaf_internal_array(&db_file, c.left_pointer());
                            leaves.append(
                                l.as_mut()
                            );
                            internals.append(
                                i.as_mut()
                            );
                        }
                        else{
                            leaves.push(c.left_pointer());
                        }
                    },
                    _ => (),
                }
            }

            if root_page.header().rightmost_ptr().is_some() {
                debug!("OF PAGE:{}", root_page.header().rightmost_ptr().unwrap());
                let overflow_page_num: u32 = root_page.header().rightmost_ptr().unwrap();
                if db_file.get_page_by_number(overflow_page_num).unwrap().is_internal_table_page(){
                    internals.push(overflow_page_num);
                    let (mut l, mut i) = Table::init_leaf_internal_array(&db_file, overflow_page_num);
                    leaves.append(
                        l.as_mut()
                    );
                    internals.append(
                        i.as_mut()
                    );
                }
                else{
                    leaves.push(overflow_page_num);
                }
            }
        }
        else{
            leaves.push(root_page_num);
        }

        (leaves, internals)
    }

    fn update_arrays(db_file: &MainFile, page: Page, leaves: &mut Vec<u32>, internals: &mut Vec<u32>) -> (){

        for cell in page.live_cells().iter() {
            match cell {
                Cell::ITC(c) => {
                    if db_file.get_page_by_number(c.left_pointer()).is_none(){
                        leaves.push(c.left_pointer());
                    }
                    else if db_file.get_page_by_number(c.left_pointer()).unwrap().is_internal_table_page(){
                        if !internals.contains(&c.left_pointer()){
                            internals.push(c.left_pointer());
                        }
                        // Recall update_arrays
                    }
                    else{
                        if !leaves.contains(&c.left_pointer()){
                            leaves.push(c.left_pointer());
                        }
                    }
                },
                _ => (),
            }
        }

        if page.header().rightmost_ptr().is_some() {
            let overflow_page_num: u32 = page.header().rightmost_ptr().unwrap();
            if db_file.get_page_by_number(overflow_page_num).is_none(){
                leaves.push(overflow_page_num);
            }
            else if db_file.get_page_by_number(overflow_page_num).unwrap().is_internal_table_page(){
                internals.push(overflow_page_num);
                let (mut l, mut i) = Table::init_leaf_internal_array(&db_file, overflow_page_num);
                leaves.append(
                    l.as_mut()
                );
                internals.append(
                    i.as_mut()
                );
            }
            else{
                leaves.push(overflow_page_num);
            }
        }
    }

    pub fn find_missing_rowids(&mut self) -> () {
        if self.rows.len() == 0 {
            return;
        }
        
        let first_id = self.rows.first().unwrap().rowid().unwrap();
        let last_id = self.rows.last().unwrap().rowid().unwrap();

        if last_id - first_id != (self.rows.len() - 1) as u32 {
            let mut missing_ids: Vec<u32> = vec![];             
            let mut i: u32 = first_id;
            for row in self.rows.iter(){
                let cur_rowid: u32 = row.rowid().unwrap();
                if cur_rowid != i {
                    for j in i .. cur_rowid {
                        missing_ids.push(j);
                    }
                    i = cur_rowid;
                } 
                i += 1;
            }
                       
            self.missing_rowids = Some(missing_ids);
        }

    }

    /*pub fn has_page(&self, page_num: u32) -> bool {
        self.pages.contains(&page_num)
    }*/

    /*pub fn set_deleted_rows(& mut self, deleted_rows: Option<Vec<HashMap<String, String>>>) -> () {
        self.deleted_rows = deleted_rows;
    }*/
}


type Trigger = String;

#[derive(Serialize, Deserialize, Clone)]
pub struct DataBase {
    tables: Vec<Table>,
    //indexes: Vec<Index>,
    triggers: Option<Vec<Trigger>>
}

impl DataBase {

    pub fn new(db_file: MainFile, wal_file: Option<WALFile>, add_missing_ids: bool, get_triggers: bool, get_indices: bool) -> DataBase {
        let mut tables: Vec<Table> = vec![];
        let table_info: HashMap<String, TableInfo> = db_file.get_tables_info();

        /* Tables */
        info!("Creating tables...");
        for (table_name, info) in table_info {
            info!("Table: {}", table_name);
            let mut table: Table = match Table::new(&db_file, &wal_file, table_name, info) {
                Ok(t) => t,
                Err(e) => {
                    warn!("{}", e);
                    continue;
                }
            };
            if add_missing_ids {
                info!("Looking for missing row ids...");
                table.find_missing_rowids();
            }
            tables.push(table);
        }

        /* Indices */
        if get_indices {
            // TODO
            warn!("TODO: Index extraction not implemented yet.");
        }

        /* Triggers */
        let triggers: Option<Vec<String>> = match get_triggers {
            true => {
                info!("Getting triggers...");
                Some(db_file.get_triggers())
            }
            false => None,
        };

        DataBase {
            tables,
            triggers
        }
    }

    /*pub fn tables(&self) -> Vec<Table> {
        self.tables.clone()
    }*/

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self).unwrap()
    }

    /*pub fn look_for_missing_ids(&self) -> () {
        trace!("LOOKING FOR MISSING IDS");
        for table in self.tables.iter() {
            match table.find_missing_rowids() {
                Some(m) => trace!("{}\t-\t{:?}", table.name, m),
                None => trace!("{}\t-\t[]", table.name),
            }   
        }
    }*/
}