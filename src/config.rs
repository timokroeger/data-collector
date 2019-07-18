use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use crate::sensor::RegisterMap;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub modbus: Option<ModbusConfig>,
    pub influxdb: Option<InfluxDbConfig>,
    pub influxdb2: Option<InfluxDb2Config>,

    #[serde(flatten)]
    pub sensor_groups: BTreeMap<String, SensorGroupConfig>,
}

impl Config {
    pub fn new(filename: &str) -> Self {
        let config_str = fs::read_to_string(filename).unwrap();
        toml::from_str(&config_str).unwrap()
    }
}

#[derive(Deserialize)]
pub struct ModbusConfig {
    pub hostname: String,
    pub port: u16,
    #[serde(with = "serde_humantime")]
    pub timeout: Duration,
}

#[derive(Deserialize)]
pub struct InfluxDbConfig {
    pub hostname: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Deserialize)]
pub struct InfluxDb2Config {
    pub hostname: String,
    pub organization: String,
    pub bucket: String,
    pub auth_token: String,
}

#[derive(Deserialize, Clone)]
pub struct SensorGroupConfig {
    #[serde(with = "serde_humantime")]
    pub scan_interval: Duration,
    pub measurement_registers: RegisterConfig,
    pub sensors: Vec<ConfigSensor>,
}

#[derive(Deserialize, Clone)]
pub struct RegisterConfig(BTreeMap<String, u16>);

impl RegisterConfig {
    pub fn into_register_map(self) -> RegisterMap {
        // Swap key and value of the toml configuration table
        RegisterMap::new(self.0.into_iter().map(|(k, v)| (v, k)).collect())
    }
}

#[derive(Deserialize, Clone)]
pub struct ConfigSensor {
    pub id: u8,

    #[serde(flatten)]
    pub tags: BTreeMap<String, toml::Value>,
}
