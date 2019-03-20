mod config;
mod sensor;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::Duration;

use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use config::*;
use influx_db_client::{Client as InfluxDbClient, Points, Precision};
use log::{debug, error, info, warn};
use modbus::{tcp::Transport, Client as ModbusClient, Error as ModbusError};
use sensor::Sensor;
use simplelog::{Config as LogConfig, TermLogger, WriteLogger};
use socket2::Socket;

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
                    ModbusError::Io(ref e)
                        if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock =>
                    {
                        warn!("Modbus: Sensor {}: {}", sensor.id(), e)
                    }
                    ModbusError::Io(e) => return Err(e),
                    _ => warn!("Modbus: Sensor {}: {}", sensor.id(), e),
                },
            }
        }

        db.write_points(Points::create_new(points), Some(Precision::Seconds), None)
            .unwrap_or_else(|e| warn!("InfluxDB: {}", e));

        thread::sleep(sensor_group.scan_interval);
    }
}

fn connect(config: &ModbusConfig, keepalive: Duration) -> Result<TcpStream, Error> {
    let addr = (config.hostname.as_str(), config.port)
        .to_socket_addrs()?
        .next()
        .ok_or(Error::new(ErrorKind::AddrNotAvailable, "Host not resolved"))?;

    let socket = Socket::from(TcpStream::connect(addr)?);
    socket.set_read_timeout(Some(config.timeout))?;
    socket.set_write_timeout(Some(config.timeout))?;
    socket.set_nodelay(true)?;
    socket.set_keepalive(Some(keepalive))?;
    Ok(socket.into_tcp_stream())
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
            Arg::with_name("loglevel")
                .long("loglevel")
                .takes_value(true)
                .default_value("warn")
                .possible_values(&["off", "error", "warn", "info", "debug", "trace"])
                .help("Sets the logging level"),
        )
        .arg(
            Arg::with_name("logfile")
                .long("logfile")
                .takes_value(true)
                .help("Sets a custom log file"),
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

    // Use scan interval of first group as keepalive interval.
    // TODO: Use min of all groups as keepalive interval.
    let keepalive = config.sensor_groups.values().next().unwrap().scan_interval;

    // Retry to connect forever
    loop {
        debug!("ModbusTCP: Connecting to {}", config.modbus.hostname);
        let e = match connect(&config.modbus, keepalive) {
            Ok(stream) => {
                info!(
                    "ModbusTCP: Successfully connected to {}",
                    stream.peer_addr()?
                );
                let mb = Transport::new(Box::new(stream));
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
