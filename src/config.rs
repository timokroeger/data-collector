use std::collections::BTreeMap;
use std::fs;

use influx_db_client::Client;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub modbus: ConfigModbus,
    pub influxdb: InfluxDbConfig,

    #[serde(flatten)]
    pub sensor_groups: BTreeMap<String, ConfigSensorGroup>,
}

#[derive(Deserialize)]
pub struct ConfigModbus {
    pub hostname: String,
    pub port: u16,
    pub timeout_sec: u64,
}

#[derive(Deserialize)]
pub struct InfluxDbConfig {
    hostname: String,
    database: String,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
pub struct ConfigSensorGroup {
    pub scan_interval_sec: u64,
    pub measurement_registers: BTreeMap<String, u16>,
    pub sensors: Vec<ConfigSensor>,
}

#[derive(Deserialize)]
pub struct ConfigSensor {
    pub id: u8,

    #[serde(flatten)]
    pub tags: BTreeMap<String, toml::Value>,
}

impl Config {
    pub fn new(filename: &str) -> Self {
        let config_str = fs::read_to_string(filename).unwrap();
        toml::from_str(&config_str).unwrap()
    }
}

impl InfluxDbConfig {
    pub fn into_client(self) -> Client {
        let client = Client::new(self.hostname, self.database);
        match (self.username, self.password) {
            (Some(username), Some(password)) => client.set_authentication(username, password),
            (_, _) => client,
        }
    }
}
