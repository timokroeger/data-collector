use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use influx_db_client::Client;
use modbus::tcp::Config as ModbusTcpConfig;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub modbus: ModbusConfig,
    pub influxdb: InfluxDbConfig,

    #[serde(flatten)]
    pub sensor_groups: BTreeMap<String, ConfigSensorGroup>,
}

#[derive(Deserialize)]
pub struct ModbusConfig {
    hostname: String,
    port: u16,
    timeout_sec: u64,
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

impl From<ModbusConfig> for (String, ModbusTcpConfig) {
    fn from(cfg: ModbusConfig) -> Self {
        (
            cfg.hostname,
            ModbusTcpConfig {
                tcp_port: cfg.port,
                tcp_connect_timeout: Some(Duration::from_secs(cfg.timeout_sec)),
                tcp_read_timeout: Some(Duration::from_secs(cfg.timeout_sec)),
                tcp_write_timeout: Some(Duration::from_secs(cfg.timeout_sec)),
                modbus_uid: 0,
            },
        )
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
