mod config;
mod device;

use std::cell::RefCell;
use std::convert::TryFrom;
use std::fs::{self, File};

use crate::{
    config::{Config, InfluxDbConfig},
    device::Device,
};
use anyhow::{ensure, Result};
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use futures::{self, channel::mpsc, prelude::*, select, stream};
use log::{debug, info, warn};
use modbus::tcp::Transport;
use simplelog::{ConfigBuilder as LogConfigBuilder, TermLogger, TerminalMode, WriteLogger};

#[tokio::main]
async fn main() -> Result<()> {
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
    let log_config = LogConfigBuilder::new()
        .set_time_format_str("%Y-%m-%dT%H:%M:%S%.3f%:z") // RFC3339 format
        .set_time_to_local(true)
        .add_filter_allow_str("data_collector")
        .build();
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

    // Handling for graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
    ctrlc::set_handler(move || shutdown_tx.clone().try_send(()).unwrap()).unwrap();

    // A stream that yields a refence to a device every time its `scan_interval` is due.
    let mut device_intervals = stream::select_all(
        devices
            .iter()
            .map(|dev| tokio::time::interval(dev.scan_interval).map(move |_| dev)),
    );

    loop {
        select! {
            _ = shutdown_rx.next() => {
                info!("Graceful exit");
                break;
            }
            dev = device_intervals.next() => {
                let dev = dev.unwrap();
                match process_device(dev, &mut mb.borrow_mut(), influxdb_config) {
                    Ok(_) => {
                        debug!("Device {} processed successfully", dev.id);
                        if fail_count > 0 {
                            fail_count -= 1;
                            debug!("fail_count={}", fail_count);
                        }
                    }
                    Err(e) => {
                        warn!("{}", e);
                        fail_count += 1;
                        debug!("fail_count={}", fail_count);
                    }
                }

                ensure!(
                    fail_count < fail_count_threshold,
                    "{} modbus communication errors, exiting...",
                    fail_count
                );
            }
        }
    }

    Ok(())
}

fn process_device<'a>(
    dev: &'a Device,
    mb: &mut Transport,
    influxdb_config: &InfluxDbConfig,
) -> Result<()> {
    let lines = dev.read(mb)?;

    let req = influxdb_config.to_request(lines);
    let resp = isahc::send(req)?;
    ensure!(resp.status().is_success(), "{:?}", resp);

    Ok(())
}
