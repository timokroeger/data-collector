use std::collections::HashMap;
use std::fs;
use std::io::{Error, ErrorKind};
use std::thread::sleep;
use std::time::Duration;

use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use influx_db_client as influxdb;
use log::{debug, error, info, warn};
use serde::Deserialize;
use tokio_modbus::client::sync::Context;
use tokio_modbus::prelude::*;

#[derive(Deserialize)]
struct Config {
    modbus_hostname: String,
    scan_interval_sec: u64,
    influxdb: ConfigInfluxDb,
    measurements: ConfigMeasurements,
}

#[derive(Deserialize)]
struct ConfigInfluxDb {
    hostname: String,
    database: String,
    username: Option<String>,
    password: Option<String>,
}

type ConfigMeasurements = HashMap<String, Vec<ConfigMeasurement>>;

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

fn connection_task(
    ctx: &mut Context,
    db: &influxdb::Client,
    scan_interval: Duration,
    meas_config: &ConfigMeasurements,
) -> Result<(), Error> {
    loop {
        let mut points = Vec::new();

        // Read all points into a vector. Ignore invalid data but return early on other errors.
        for (meas_name, meas_points) in meas_config {
            for point_config in meas_points {
                ctx.set_slave(Slave(point_config.id));
                match ctx.read_input_registers(point_config.register, 1) {
                    Ok(values) => points.push(Point::new(meas_name, values[0], point_config)),
                    Err(e) => match e.kind() {
                        ErrorKind::InvalidData => warn!("Modbus: {}", e),
                        _ => return Err(e),
                    },
                };
            }
        }

        db.write_points(
            influxdb::Points::create_new(points.into_iter().map(|p| p.0).collect()),
            Some(influxdb::Precision::Seconds),
            None,
        )
        .unwrap_or_else(|e| warn!("InfluxDB: {}", e));

        sleep(scan_interval);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    env_logger::init();

    let matches = app_from_crate!()
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("Sets a custom config file")
                .takes_value(true),
        )
        .get_matches();

    let config_file = matches.value_of("config").unwrap_or("datacollector.toml");
    let config_str = fs::read_to_string(config_file)?;
    let config: Config = toml::from_str(&config_str)?;
    info!(
        "Configuration loaded from {} with {} measurement points",
        config_file,
        config
            .measurements
            .iter()
            .map(|(_, points)| points.len())
            .sum::<usize>()
    );

    let modbus_host = config.modbus_hostname.parse().unwrap();

    let db = influxdb::Client::new(config.influxdb.hostname, config.influxdb.database);
    let db = match (config.influxdb.username, config.influxdb.password) {
        (Some(username), Some(password)) => db.set_authentication(username, password),
        (_, _) => db,
    };

    loop {
        // Retry to connect forever
        debug!("ModbusTCP: Connecting to {}", modbus_host);
        match sync::tcp::connect(modbus_host) {
            Ok(mut ctx) => {
                info!("ModbusTCP: Successfully connected to {}", modbus_host);
                connection_task(
                    &mut ctx,
                    &db,
                    Duration::from_secs(config.scan_interval_sec),
                    &config.measurements,
                )
                .unwrap_or_else(|e| {
                    error!("Modbus Connection: {}, reconnecting...", e);
                });
            }
            Err(e) => {
                error!("ModbusTCP: {}, retrying in 10 seconds...", e);
                sleep(Duration::from_secs(10));
                continue;
            }
        };
    }
}
