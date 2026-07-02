pub mod config;
pub mod core;
pub mod core_agent_api;
pub mod crc64;
pub mod load_config;
pub mod parse_job;
pub mod parser;
pub mod tpd;
pub mod util;
pub mod writer;

use std::collections::HashMap;

use clap::ValueEnum;
use indexmap::IndexMap;

pub type Row = IndexMap<String, String>;
pub type TableRows = HashMap<String, Vec<Row>>;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum LoadType {
    Postgresql,
    Clickhouse,
}
