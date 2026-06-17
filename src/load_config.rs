use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct LoadConfig {
    pub clickhouse: ClickHouseConfig,
    pub postgresql: PostgresConfig,
}

#[derive(Debug, Deserialize)]
pub struct ClickHouseConfig {
    #[serde(default = "default_clickhouse_client")]
    pub client: String,
    #[serde(default = "default_localhost")]
    pub host: String,
    #[serde(default = "default_clickhouse_port")]
    pub port: u16,
    #[serde(default = "default_clickhouse_user")]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_database")]
    pub database: String,
    #[serde(default = "default_table_name_case")]
    pub table_name_case: String,
}

#[derive(Debug, Deserialize)]
pub struct PostgresConfig {
    #[serde(default = "default_psql_client")]
    pub client: String,
    #[serde(default = "default_localhost")]
    pub host: String,
    #[serde(default = "default_postgres_port")]
    pub port: u16,
    #[serde(default = "default_postgres_user")]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_postgres_database")]
    pub database: String,
}

pub fn load_config(path: &Path) -> Result<LoadConfig> {
    let text = fs::read_to_string(path)?;
    let mut config: LoadConfig = toml::from_str(&text)?;
    resolve_load_config(&mut config)?;
    Ok(config)
}

fn resolve_load_config(config: &mut LoadConfig) -> Result<()> {
    resolve_clickhouse_config(&mut config.clickhouse)?;
    resolve_postgres_config(&mut config.postgresql)?;
    Ok(())
}

fn resolve_clickhouse_config(config: &mut ClickHouseConfig) -> Result<()> {
    config.client = resolve_env_value(&config.client)?;
    config.host = resolve_env_value(&config.host)?;
    config.user = resolve_env_value(&config.user)?;
    config.password = resolve_env_value(&config.password)?;
    config.database = resolve_env_value(&config.database)?;
    config.table_name_case = resolve_env_value(&config.table_name_case)?.to_ascii_lowercase();
    if config.table_name_case != "lower" && config.table_name_case != "upper" {
        bail!(
            "invalid clickhouse.table_name_case: {}, expected lower or upper",
            config.table_name_case
        );
    }
    Ok(())
}

fn resolve_postgres_config(config: &mut PostgresConfig) -> Result<()> {
    config.client = resolve_env_value(&config.client)?;
    config.host = resolve_env_value(&config.host)?;
    config.user = resolve_env_value(&config.user)?;
    config.password = resolve_env_value(&config.password)?;
    config.database = resolve_env_value(&config.database)?;
    Ok(())
}

fn resolve_env_value(value: &str) -> Result<String> {
    let Some(inner) = value.strip_prefix("${").and_then(|v| v.strip_suffix('}')) else {
        return Ok(value.to_string());
    };
    let (key, default_value) = inner
        .split_once(":-")
        .map_or((inner, None), |(key, default)| (key, Some(default)));
    match std::env::var(key) {
        Ok(value) => Ok(value),
        Err(_) => default_value
            .map(str::to_string)
            .with_context(|| format!("environment variable {key} is required by load config")),
    }
}

fn default_clickhouse_client() -> String {
    "clickhouse-client".to_string()
}

fn default_psql_client() -> String {
    "psql".to_string()
}

fn default_localhost() -> String {
    "127.0.0.1".to_string()
}

fn default_clickhouse_port() -> u16 {
    9000
}

fn default_postgres_port() -> u16 {
    5432
}

fn default_clickhouse_user() -> String {
    "default".to_string()
}

fn default_postgres_user() -> String {
    "postgres".to_string()
}

fn default_database() -> String {
    "default".to_string()
}

fn default_table_name_case() -> String {
    "lower".to_string()
}

fn default_postgres_database() -> String {
    "postgres".to_string()
}
