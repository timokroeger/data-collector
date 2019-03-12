use std::collections::HashMap;
use std::fs;
use std::io::Error;
use std::thread::sleep;
use std::time::Duration;

use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use influx_db_client as influxdb;
use log::{debug, error, info, warn};
use modbus::{
    tcp::{Config as ModbusTcpConfig, Transport},
    Client, Error as ModbusError,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    scan_interval_sec: u64,
    modbus: ConfigModbus,
    influxdb: ConfigInfluxDb,
    measurements: ConfigMeasurements,
}

#[derive(Deserialize)]
struct ConfigModbus {
    hostname: String,
    port: u16,
    timeout_sec: u64,
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
    ctx: &mut Client,
    db: &influxdb::Client,
    scan_interval: Duration,
    meas_config: &ConfigMeasurements,
) -> Result<(), Error> {
    loop {
        let mut points = Vec::new();

        // Read all points into a vector. Ignore invalid data but return early on other errors.
        for (meas_name, meas_points) in meas_config {
            for point_config in meas_points {
                ctx.set_slave(point_config.id);
                match ctx.read_input_registers(point_config.register, 1) {
                    Ok(values) => points.push(Point::new(meas_name, values[0], point_config)),
                    Err(e) => match e {
                        ModbusError::Io(e) => return Err(e),
                        _ => warn!("Modbus: {}", e),
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

    let db = influxdb::Client::new(config.influxdb.hostname, config.influxdb.database);
    let db = match (config.influxdb.username, config.influxdb.password) {
        (Some(username), Some(password)) => db.set_authentication(username, password),
        (_, _) => db,
    };

    let modbus_hostname = &config.modbus.hostname;
    let modbus_config = ModbusTcpConfig {
        tcp_port: config.modbus.port,
        tcp_connect_timeout: Some(Duration::from_secs(config.modbus.timeout_sec)),
        tcp_read_timeout: Some(Duration::from_secs(config.modbus.timeout_sec)),
        tcp_write_timeout: Some(Duration::from_secs(config.modbus.timeout_sec)),
        modbus_uid: 0,
    };

    loop {
        // Retry to connect forever
        debug!("ModbusTCP: Connecting to {}", modbus_hostname);
        match Transport::new_with_cfg(modbus_hostname, modbus_config) {
            Ok(mut ctx) => {
                info!("ModbusTCP: Successfully connected to {}", modbus_hostname);
                connection_task(
                    &mut ctx,
                    &db,
                    Duration::from_secs(config.scan_interval_sec),
                    &config.measurements,
                )
                .unwrap_or_else(|e| {
                    error!("ModbusTCP: {}, reconnecting...", e);
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
