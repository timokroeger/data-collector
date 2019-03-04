use std::collections::HashMap;
use std::fs;
use std::thread::sleep;
use std::time::Duration;

use influx_db_client as influxdb;
use log::error;
use serde::Deserialize;
use tokio_modbus::prelude::*;

#[derive(Deserialize, Debug)]
struct Config {
    modbus_hostname: String,
    scan_interval_sec: u64,
    influxdb: ConfigInfluxDb,
    measurements: HashMap<String, Vec<ConfigMeasurement>>,
}

#[derive(Deserialize, Debug)]
struct ConfigInfluxDb {
    hostname: String,
    database: String,
    username: Option<String>,
    password: Option<String>,
}

type ConfigMeasurement = HashMap<String, toml::Value>;

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    env_logger::init();

    let config_str = fs::read_to_string("datacollector.toml")?;
    let config: Config = toml::from_str(&config_str)?;

    // TODO: connect to modbus server

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
            let mut points = Vec::new();
            for (measurement_name, measurements) in &config.measurements {
                for m in measurements {
                    let mut p = influxdb::Point::new(&measurement_name);

                    let id = m["id"].as_integer().unwrap() as u8;
                    ctx.set_slave(Slave(id));

                    let register = m["register"].as_integer().unwrap() as u16;
                    let value = match ctx.read_input_registers(register, 1) {
                        Ok(v) => v[0],
                        Err(e) => {
                            error!("Modbus: {}, reconnecting...", e);
                            break 'connection;
                        }
                    };

                    p.add_field("value", influxdb::Value::Integer(i64::from(value)));
                    for (tag_name, tag_value) in m {
                        p.add_tag(tag_name, influxdb::Value::String(tag_value.to_string()));
                    }

                    points.push(p);
                }
            }

            db.write_points(
                influxdb::Points::create_new(points),
                Some(influxdb::Precision::Seconds),
                None,
            )
            .unwrap_or_else(|e| error!("InfluxDB: {}", e));

            sleep(Duration::from_secs(config.scan_interval_sec));
        }
    }
}
