use std::collections::BTreeMap;

use crate::device::{DataType, Device, Register};
use isahc::http::Request;
use modbus::tcp::Config as ModbusTcpConfig;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub modbus: ModbusConfig,

    #[serde(flatten)]
    pub influxdb: InfluxDbConfig,

    #[serde(flatten)]
    pub devices: DevicesConfig,
}

#[derive(Deserialize)]
pub struct ModbusConfig {
    pub hostname: String,
    pub port: u16,
    pub timeout: String,
}

impl ModbusConfig {
    pub fn into_modbus_tcp_config(self) -> (String, ModbusTcpConfig) {
        let timeout = humantime::parse_duration(&self.timeout).unwrap();
        (
            self.hostname,
            ModbusTcpConfig {
                tcp_port: self.port,
                tcp_connect_timeout: None,
                tcp_read_timeout: Some(timeout),
                tcp_write_timeout: Some(timeout),
                modbus_uid: 0,
            },
        )
    }
}

#[derive(Deserialize)]
pub enum InfluxDbConfig {
    #[serde(rename = "influxdb")]
    V1 {
        hostname: String,
        database: String,
        username: Option<String>,
        password: Option<String>,
    },
    #[serde(rename = "influxdb2")]
    V2 {
        hostname: String,
        organization: String,
        bucket: String,
        auth_token: String,
    },
}

impl InfluxDbConfig {
    pub fn to_request<T>(&self, lines: T) -> Request<T> {
        let mut req = Request::builder();

        match self {
            InfluxDbConfig::V1 {
                hostname,
                database,
                username,
                password,
            } => {
                let mut uri = format!("{}/write?db={}", hostname, database);
                if let (Some(u), Some(p)) = (username, password) {
                    uri.push_str(&format!("&u={}&p={}", u, p));
                }
                req.uri(uri);
            }
            InfluxDbConfig::V2 {
                hostname,
                organization,
                bucket,
                auth_token,
            } => {
                req.uri(format!(
                    "{}/write?org={}&bucket={}",
                    hostname, organization, bucket
                ));
                req.header("Authorization", format!("Token {}", auth_token));
            }
        };

        req.method("POST")
            .body(lines)
            .expect("Failed to create InfluxDB http request")
    }
}

#[derive(Deserialize)]
pub struct DevicesConfig {
    #[serde(default)]
    templates: BTreeMap<String, DeviceConfig>,
    devices: Vec<DeviceConfig>,
}

impl DevicesConfig {
    pub fn into_devices(self) -> Vec<Device> {
        let mut devices = Vec::new();
        for config in self.devices {
            devices.push(device_from_config(&self.templates, config));
        }
        devices
    }
}

fn device_from_config(
    templates: &BTreeMap<String, DeviceConfig>,
    mut config: DeviceConfig,
) -> Device {
    // Use template if it exists
    let mut c = config
        .template
        .and_then(|name| templates.get(&name).cloned())
        .unwrap_or_default(); // All fields default to Option::None

    // Merge template and more specific config sections
    let id =
        c.id.xor(config.id)
            .expect("Field `id`: Is it missing or defined both in template and device section?");
    let scan_interval_str = c.scan_interval.xor(config.scan_interval).expect(
        "Field `scan_interval`: Is it missing or defined both in template and device section?",
    );
    c.input_registers.append(&mut config.input_registers);
    c.tags.append(&mut config.tags);

    // Create a device from the merged config sections
    Device::new(
        id,
        humantime::parse_duration(&scan_interval_str)
            .unwrap_or_else(|_| panic!("Invalid `scan_interval` for device with id `{}`", id)),
        c.tags.into_iter().collect(),
        c.input_registers
            .into_iter()
            .map(|r| match r {
                RegisterConfig::Simple(addr) => (
                    addr,
                    Register {
                        name: format!("input_register_{}", addr),
                        data_type: DataType::U16,
                        scaling: 1.0,
                        tags: BTreeMap::new(),
                    },
                ),
                RegisterConfig::Advanced {
                    addr,
                    data_type,
                    scaling,
                    name,
                    tags: register_tags,
                } => (
                    addr,
                    Register {
                        data_type: data_type
                            .map(|t| {
                                t.parse().unwrap_or_else(|_| {
                                    panic!("`{}`: Invalid register type `{}`", &name, &t)
                                })
                            })
                            .unwrap_or(DataType::U16),
                        scaling: scaling.unwrap_or(1.0),
                        name,
                        tags: register_tags.into_iter().collect(),
                    },
                ),
            })
            .collect(),
    )
}

#[derive(Clone, Default, Deserialize)]
struct DeviceConfig {
    template: Option<String>,
    id: Option<u8>,
    scan_interval: Option<String>,

    #[serde(default)]
    tags: BTreeMap<String, String>,

    #[serde(default)]
    input_registers: Vec<RegisterConfig>,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum RegisterConfig {
    Simple(u16),
    Advanced {
        addr: u16,
        name: String,

        // #[serde(default)] does not work here because of parsing ambiguities
        // https://github.com/serde-rs/serde/issues/368
        // Workaround: Option and unwrap_or()
        data_type: Option<String>,
        scaling: Option<f64>,

        #[serde(default)]
        tags: BTreeMap<String, String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_into_devices_simple() {
        let dc: DevicesConfig = toml::from_str(
            r#"
            [[devices]]
            id = 1
            scan_interval = "1s"
            input_registers = [1, 1234]
            "#,
        )
        .unwrap();

        let mut registers = BTreeMap::new();
        registers.insert(
            1,
            Register {
                name: String::from("input_register_1"),
                tags: BTreeMap::new(),
                data_type: DataType::U16,
                scaling: 1.0,
            },
        );
        registers.insert(
            1234,
            Register {
                name: String::from("input_register_1234"),
                tags: BTreeMap::new(),
                data_type: DataType::U16,
                scaling: 1.0,
            },
        );

        let devices = vec![Device::new(
            1,
            Duration::from_secs(1),
            BTreeMap::new(),
            registers,
        )];
        assert_eq!(dc.into_devices(), devices);
    }

    #[test]
    fn test_into_devices_advanced() {
        let dc: DevicesConfig = toml::from_str(
            r#"
            [[devices]]
            id = 1
            scan_interval = "1s"

            [[devices.input_registers]]
            addr = 1
            name = "foobar"
            data_type = "f32"
            scaling = 8.7
            tags.foo = "bar"

            [[devices.input_registers]]
            addr = 2
            name = "quxbaz"
            "#,
        )
        .unwrap();

        let mut tags = BTreeMap::new();
        tags.insert(String::from("foo"), String::from("bar"));

        let mut registers = BTreeMap::new();
        registers.insert(
            1,
            Register {
                name: String::from("foobar"),
                tags,
                data_type: DataType::F32,
                scaling: 8.7,
            },
        );
        registers.insert(
            2,
            Register {
                name: String::from("quxbaz"),
                tags: BTreeMap::new(),
                data_type: DataType::U16,
                scaling: 1.0,
            },
        );

        let devices = vec![Device::new(
            1,
            Duration::from_secs(1),
            BTreeMap::new(),
            registers,
        )];
        assert_eq!(dc.into_devices(), devices);
    }

    #[test]
    fn test_into_devices_merge_template() {
        let dc: DevicesConfig = toml::from_str(
            r#"
            [templates.foobar]
            scan_interval = "1s"
            tags.template = "template"

            [[devices]]
            template = "foobar"
            id = 1
            tags.device = "device"

            [[devices.input_registers]]
            addr = 1
            name = "quxbaz"
            tags.register = "register"
            "#,
        )
        .unwrap();

        let mut device_tags = BTreeMap::new();
        device_tags.insert(String::from("device"), String::from("device"));
        device_tags.insert(String::from("template"), String::from("template"));

        let mut register_tags = BTreeMap::new();
        register_tags.insert(String::from("register"), String::from("register"));

        let mut registers = BTreeMap::new();
        registers.insert(
            1,
            Register {
                name: String::from("quxbaz"),
                tags: register_tags,
                data_type: DataType::U16,
                scaling: 1.0,
            },
        );

        let devices = vec![Device::new(
            1,
            Duration::from_secs(1),
            device_tags,
            registers,
        )];
        assert_eq!(dc.into_devices(), devices);
    }
}
