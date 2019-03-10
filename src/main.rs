use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::thread::sleep;
use std::time::Duration;

use clap::{App, Arg};
use influx_db_client as influxdb;
use log::{debug, error, info, warn};
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

    let matches = App::new("data-collector")
        .author("Timo Kröger")
        .about("Reads data points from a ModbusTCP server and stores them in InfluxDB")
        .arg(Arg::with_name("config")
            .short("c")
            .long("config")
            .value_name("FILE")
            .help("Sets a custom config file")
            .takes_value(true))
        .get_matches();

    let config_file = matches.value_of("config").unwrap_or("datacollector.toml");
    let config_str = fs::read_to_string(config_file)?;
    let config: Config = toml::from_str(&config_str)?;
    debug!(
        "Configuration loaded with {} measurement points",
        config
            .measurements
            .iter()
            .map(|(_, points)| points.len())
            .sum::<usize>()
    );

    let db = influxdb::Client::new(config.influxdb.hostname, config.influxdb.database);
    let db = match (config.influxdb.username, config.influxdb.password) {
        (Some(username), Some(password)) => db.set_authentication(username, password),
        (_, _) => db,
    };

    let modbus_host = config.modbus_hostname.parse().unwrap();

    loop {
        // Retry to connect forever
        debug!("ModbusTCP: Connecting to {}", modbus_host);
        let mut ctx = match sync::tcp::connect(modbus_host) {
            Ok(ctx) => {
                info!("ModbusTCP: Successfully connected to {}", modbus_host);
                ctx
            }
            Err(e) => {
                error!("ModbusTCP: {}, retrying in 10 seconds...", e);
                sleep(Duration::from_secs(10));
                continue;
            }
        };

        'connection: loop {
            let mut influx_points = Vec::new();

            for (name, points) in &config.measurements {
                for p in points {
                    ctx.set_slave(Slave(p.id));
                    match ctx.read_input_registers(p.register, 1) {
                        Ok(values) => influx_points.push(Point::new(name, values[0], p).0),
                        Err(e) => match e.kind() {
                            ErrorKind::InvalidData => warn!("Modbus: {}", e),
                            _ => {
                                error!("Modbus: {}, reconnecting...", e);
                                break 'connection;
                            }
                        },
                    };
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
