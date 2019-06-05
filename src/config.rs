use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use crate::sensor::RegisterMap;
use modbus::tcp::Config as ModbusTcpConfig;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub modbus: ModbusConfig,
    pub influxdb: InfluxDbConfig,

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

impl ModbusConfig {
    pub fn to_modbus_tcp_config(&self) -> ModbusTcpConfig {
        ModbusTcpConfig {
            tcp_port: self.port,
            tcp_connect_timeout: None,
            tcp_read_timeout: Some(self.timeout),
            tcp_write_timeout: Some(self.timeout),
            modbus_uid: 0,
        }
    }
}

#[derive(Deserialize)]
pub struct InfluxDbConfig {
    hostname: String,
    database: String,
    username: Option<String>,
    password: Option<String>,
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
