use std::collections::HashMap;
use std::fs;
use std::thread::sleep;
use std::time::Duration;

use influx_db_client as influxdb;
use log::error;
use serde::Deserialize;
use tokio_modbus::prelude::*;

#[derive(Deserialize)]
struct Config {
    modbus_hostname: String,
    scan_interval_sec: u64,
    influxdb: ConfigInfluxDb,
    measurements: HashMap<String, Vec<ConfigMeasurement>>,
}

#[derive(Deserialize)]
struct ConfigInfluxDb {
    hostname: String,
    database: String,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
struct ConfigMeasurement {
    id: u8,
    register: u16,

    #[serde(flatten)]
    tags: HashMap<String, toml::Value>,
}

struct Point(influxdb::Point);

impl Point {
    fn new(measurement: &str, value: u16, config: &ConfigMeasurement) -> Point {
        let mut p = influxdb::Point::new(&measurement);
        p.add_field("value", influxdb::Value::Integer(i64::from(value)));
        p.add_tag("id", influxdb::Value::String(config.id.to_string()));
        p.add_tag(
            "register",
            influxdb::Value::String(config.register.to_string()),
        );
        for (tag_name, tag_value) in &config.tags {
            p.add_tag(tag_name, influxdb::Value::String(tag_value.to_string()));
        }
        Point(p)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    env_logger::init();

    let config_str = fs::read_to_string("datacollector.toml")?;
    let config: Config = toml::from_str(&config_str)?;

    let db = influxdb::Client::new(config.influxdb.hostname, config.influxdb.database);
    let db = match (config.influxdb.username, config.influxdb.password) {
        (Some(username), Some(password)) => db.set_authentication(username, password),
        (_, _) => db,
    };

    let modbus_host = config.modbus_hostname.parse().unwrap();

    loop {
        // Retry to connect forever
        let mut ctx = match sync::tcp::connect(modbus_host) {
            Ok(ctx) => ctx,
            Err(e) => {
                error!("ModbusTCP: {}, retrying...", e);
                continue;
            }
        };

        'connection: loop {
            let mut influx_points = Vec::new();

            for (name, points) in &config.measurements {
                for p in points {
                    ctx.set_slave(Slave(p.id));
                    let value = match ctx.read_input_registers(p.register, 1) {
                        Ok(v) => v[0],
                        Err(e) => {
                            error!("Modbus: {}, reconnecting...", e);
                            break 'connection;
                        }
                    };

                    influx_points.push(Point::new(name, value, p).0);
                }
            }

            db.write_points(
                influxdb::Points::create_new(influx_points),
                Some(influxdb::Precision::Seconds),
                None,
            )
            .unwrap_or_else(|e| error!("InfluxDB: {}", e));

            sleep(Duration::from_secs(config.scan_interval_sec));
        }
    }
}
