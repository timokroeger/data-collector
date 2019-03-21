mod config;
mod sensor;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::thread;
use std::time::Duration;

use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use config::*;
use humantime;
use influx_db_client::{Client as InfluxDbClient, Points, Precision};
use log::{debug, error, info, warn};
use modbus::{tcp::Transport, Client as ModbusClient, Error as ModbusError};
use sensor::Sensor;
use simplelog::{Config as LogConfig, TermLogger, WriteLogger};

fn connection_task(
    mut mb: impl ModbusClient,
    db: &InfluxDbClient,
    sensor_groups: &BTreeMap<String, SensorGroupConfig>,
) -> Result<(), Error> {
    // TODO: Support more than one sensor group
    let (group, sensor_group) = sensor_groups.iter().next().unwrap();

    let register_map = sensor_group
        .measurement_registers
        .clone()
        .into_register_map();

    let mut sensors = Vec::new();
    for sensor in &sensor_group.sensors {
        let tags = sensor
            .tags
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();
        sensors.push(Sensor::new(sensor.id, &group, &register_map, tags));
    }

    loop {
        let mut points = Vec::new();

        for sensor in &mut sensors {
            match sensor.read_registers(&mut mb) {
                Ok(register_values) => points.append(&mut sensor.get_points(&register_values)),
                Err(e) => match e {
                    ModbusError::Exception(_)
                    | ModbusError::InvalidData(_)
                    | ModbusError::InvalidFunction => {
                        error!("ModbusTCP: Sensor {}: {}", sensor.id(), e);
                        panic!("Please check the connected sensors and the configuration.");
                    }
                    _ => warn!("ModbusTCP: Sensor {}: {}", sensor.id(), e),
                },
            }
        }

        if points.is_empty() {
            return Err(Error::new(
                ErrorKind::NotConnected,
                "Error detected at each sensor",
            ));
        }

        db.write_points(Points::create_new(points), Some(Precision::Seconds), None)
            .unwrap_or_else(|e| warn!("InfluxDB: {}", e));

        thread::sleep(sensor_group.scan_interval);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    // Parse command line arguments
    let matches = app_from_crate!()
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .takes_value(true)
                .value_name("FILE")
                .default_value("datacollector.toml")
                .help("Sets a custom config file"),
        )
        .arg(
            Arg::with_name("logfile")
                .long("logfile")
                .takes_value(true)
                .value_name("FILE")
                .help("Sets a custom log file"),
        )
        .arg(
            Arg::with_name("loglevel")
                .long("loglevel")
                .takes_value(true)
                .value_name("LEVEL")
                .default_value("warn")
                .possible_values(&["off", "error", "warn", "info", "debug", "trace"])
                .help("Sets the logging level"),
        )
        .get_matches();

    // Setup logging
    let mut log_config = LogConfig::default();
    log_config.time_format = Some("%+");
    let log_level = matches.value_of("loglevel").unwrap().parse()?;
    match matches.value_of("logfile") {
        Some(logfile) => {
            let log_file = File::create(logfile)?;
            WriteLogger::init(log_level, log_config, log_file)?
        }
        None => TermLogger::init(log_level, log_config)?,
    }

    let config_file = matches.value_of("config").unwrap();
    let config = Config::new(&config_file);
    info!(
        "Configuration loaded from {} with {} sensor groups",
        config_file,
        config.sensor_groups.len()
    );

    let db = config.influxdb.into_client();

    let modbus_hostname = &config.modbus.hostname;
    let modbus_config = config.modbus.to_modbus_tcp_config();

    // Retry to connect forever
    loop {
        debug!("ModbusTCP: Connecting to {}", modbus_hostname);
        let e = match Transport::new_with_cfg(modbus_hostname, modbus_config) {
            Ok(mb) => {
                info!("ModbusTCP: Successfully connected to {}", modbus_hostname);
                connection_task(mb, &db, &config.sensor_groups).unwrap_err()
            }
            Err(e) => e,
        };

        // TODO: Exponential backoff
        let delay = Duration::from_secs(10);
        error!(
            "ModbusTCP: {}, reconnecting in {}...",
            e,
            humantime::format_duration(delay)
        );
        thread::sleep(delay);
    }
}
