mod config;
mod device;

use std::cell::RefCell;
use std::convert::TryFrom;
use std::fs::{self, File};

use crate::config::{Config, InfluxDbConfig};
use crate::device::Device;
use chrono::Local;
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use ctrlc;
use futures::{self, channel::mpsc, executor, prelude::*, select, stream};
use futures_timer::Interval;
use isahc;
use log::{debug, error, info, warn};
use modbus::{tcp::Transport, Error as ModbusError};
use simplelog::{Config as LogConfig, TermLogger, TerminalMode, WriteLogger};

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    // Parse command line arguments
    let matches = app_from_crate!()
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .takes_value(true)
                .value_name("FILE")
                .default_value("config.toml")
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
    let log_config = LogConfig {
        time_format: Some("%Y-%m-%dT%H:%M:%S%.3f%:z"), // RFC3339 format
        offset: Local::today().offset().clone(),
        ..LogConfig::default()
    };
    let log_level = matches.value_of("loglevel").unwrap().parse()?;
    match matches.value_of("logfile") {
        Some(logfile) => {
            let log_file = File::create(logfile)?;
            WriteLogger::init(log_level, log_config, log_file)?
        }
        None => {
            let _ = TermLogger::init(log_level, log_config, TerminalMode::Mixed);
        }
    }

    // Read configuration file
    let config_file = matches.value_of("config").unwrap();
    info!("Reading configuration file: {}", &config_file);

    let config_str = fs::read_to_string(config_file)?;
    let config: Config = toml::from_str(&config_str)?;

    let devices = config.devices.into_devices();

    // Connect Modbus
    let modbus_config = config.modbus;
    let (modbus_hostname, modbus_config) = modbus_config.into_modbus_tcp_config();

    debug!("Connecting to {}", modbus_hostname);
    let mb = Transport::new_with_cfg(&modbus_hostname, modbus_config)?;
    let mb = &RefCell::new(mb);

    let influxdb_config = &config.influxdb;

    // Share one failure counter for all devices.
    // With each failed device communication the counter is increased.
    // With each successfull device communicationt the counter is decreased.
    // When the counter reaches the threshold (e.g. all devices on the bus failed
    // two times in a row) action is taken.
    let mut fail_count = 0;
    let scan_interval_iter = devices.iter().map(|d| d.scan_interval.as_nanos());
    let fail_count_threshold = 2
        * devices.len()
        * usize::try_from(
            scan_interval_iter.clone().max().unwrap() / scan_interval_iter.clone().min().unwrap(),
        )
        .unwrap();
    debug!("fail_count_threshold={}", fail_count_threshold);

    let device_streams = devices.into_iter().map(|dev| {
        Interval::new(dev.scan_interval).map(move |_| {
            let result = read_device(&dev, &mut mb.borrow_mut(), influxdb_config);
            match result {
                Ok(_) => debug!("Device {} read successfully", dev.id),
                Err(ref e) => warn!("ModbusTCP: {}", e),
            };
            result
        })
    });

    // Combine all device streams into one which to pull out results one by one.
    let mut device_stream = stream::select_all(device_streams);

    // Handling for graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
    ctrlc::set_handler(move || shutdown_tx.clone().try_send(()).unwrap()).unwrap();

    executor::block_on(async move {
        loop {
            select! {
                _ = shutdown_rx.next() => {
                    info!("Graceful exit");
                    break;
                }
                fail = device_stream.next() => {
                    if fail.unwrap().is_err() {
                        fail_count += 1;
                        debug!("fail_count={}", fail_count);
                    } else if fail_count > 0 {
                        fail_count -= 1;
                        debug!("fail_count={}", fail_count);
                    }

                    if fail_count >= fail_count_threshold {
                        error!("{} modbus communication errors, exiting...", fail_count);
                        break;
                    }
                }
            }
        }
    });

    Ok(())
}

fn read_device(
    dev: &Device,
    mb: &mut Transport,
    influxdb_config: &InfluxDbConfig,
) -> Result<(), ModbusError> {
    let lines = dev.read(mb)?;

    let req = influxdb_config
        .to_request(lines)
        .expect("Failed to create InfluxDB http request");
    let resp = isahc::send(req);
    match resp {
        Ok(resp) => {
            if !resp.status().is_success() {
                warn!("InfluxDB: {:?}", resp);
            }
        }
        Err(e) => warn!("InfluxDB: {}", e),
    }

    Ok(())
}
