mod config;
mod device;

use std::fs::{self, File};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::Duration;

use crate::{
    config::{Config, InfluxDbConfig},
    device::Device,
};
use anyhow::{ensure, Result};
use attohttpc::Response;
use clap::{command, Arg};
use log::{debug, info, warn};
use modbus::tcp::Transport;
use simplelog::{ConfigBuilder as LogConfigBuilder, TermLogger, TerminalMode, WriteLogger};

static FAIL_COUNT: AtomicUsize = AtomicUsize::new(0);

fn main() -> Result<()> {
    // Parse command line arguments
    let matches = command!()
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .takes_value(true)
                .value_name("FILE")
                .default_value("config.toml")
                .help("Sets a custom config file"),
        )
        .arg(
            Arg::new("logfile")
                .long("logfile")
                .takes_value(true)
                .value_name("FILE")
                .help("Sets a custom log file"),
        )
        .arg(
            Arg::new("loglevel")
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
        .set_time_format_rfc3339()
        .set_time_offset_to_local()
        .unwrap()
        .add_filter_allow_str("data_collector")
        .build();
    let log_level = matches.value_of("loglevel").unwrap().parse()?;
    match matches.value_of("logfile") {
        Some(logfile) => {
            let log_file = File::create(logfile)?;
            WriteLogger::init(log_level, log_config, log_file)?
        }
        None => {
            let _ = TermLogger::init(
                log_level,
                log_config,
                TerminalMode::Mixed,
                simplelog::ColorChoice::Auto,
            );
        }
    }

    // Read configuration file
    let config_file = matches.value_of("config").unwrap();
    info!("Reading configuration file: {}", &config_file);

    let config_str = fs::read_to_string(config_file)?;
    let config: Config = toml::from_str(&config_str)?;

    let devices = config.devices.to_devices();

    // Connect Modbus
    let (modbus_hostname, modbus_config) = config.modbus.to_modbus_tcp_config();

    debug!("Connecting to {}", modbus_hostname);
    let mb = Transport::new_with_cfg(&modbus_hostname, modbus_config)?;
    let mb = Mutex::new(mb); // Make it accessible from multiple threads.
    let mb = Box::leak(Box::new(mb)) as &_;

    // Share one failure counter for all devices.
    // With each failed device communication the counter is increased.
    // With each successfull device communicationt the counter is decreased.
    // When the counter reaches the threshold (e.g. all devices on the bus failed
    // two times in a row) action is taken.
    let fastest_device = devices.iter().min_by_key(|d| d.scan_interval).unwrap();
    let slowest_device = devices.iter().max_by_key(|d| d.scan_interval).unwrap();
    let interval_ratio = (slowest_device.scan_interval.as_secs_f64()
        / fastest_device.scan_interval.as_secs_f64()) as usize;
    let fail_count_threshold = 2 * devices.len() * interval_ratio;
    debug!("fail_count_threshold={}", fail_count_threshold);

    // Spawn a thread for each configured modbus device
    for dev in devices {
        let influxdb_config = config.influxdb.clone();
        thread::spawn(move || device_thread(dev, mb, influxdb_config, &FAIL_COUNT));
    }

    // Handling for graceful shutdown
    let (shutdown_tx, shutdown_rx) = mpsc::sync_channel(1);
    ctrlc::set_handler(move || shutdown_tx.send(()).unwrap()).unwrap();

    loop {
        if shutdown_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
            info!("Graceful exit");
            break;
        }

        let fail_count = FAIL_COUNT.load(Ordering::Acquire);
        ensure!(
            fail_count < fail_count_threshold,
            "{} modbus communication errors, exiting...",
            fail_count
        );
    }

    Ok(())
}

fn device_thread(
    dev: Device,
    mb: &Mutex<Transport>,
    influxdb_config: InfluxDbConfig,
    fail_count: &AtomicUsize,
) {
    loop {
        match process_device(&dev, &mut mb.lock().unwrap(), &influxdb_config) {
            Ok(_) => {
                debug!("Device {} processed successfully", dev.id);
                fail_count
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |fail_count| {
                        if fail_count > 0 {
                            let fail_count = fail_count - 1;
                            Some(fail_count)
                        } else {
                            Some(0)
                        }
                    })
                    .unwrap();
            }
            Err(e) => {
                warn!("{}", e);
                fail_count.fetch_add(1, Ordering::SeqCst);
            }
        }

        thread::sleep(dev.scan_interval);
    }
}

fn process_device(
    dev: &Device,
    mb: &mut Transport,
    influxdb_config: &InfluxDbConfig,
) -> Result<()> {
    let lines = dev.read(mb)?;
    let resp = write_influxdb(lines, influxdb_config)?;
    ensure!(resp.status().is_success(), "{:?}", resp);
    Ok(())
}

fn write_influxdb(lines: String, influxdb_config: &InfluxDbConfig) -> Result<Response> {
    let req = match influxdb_config {
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
            attohttpc::post(uri)
        }
        InfluxDbConfig::V2 {
            hostname,
            organization,
            bucket,
            auth_token,
        } => attohttpc::post(format!(
            "{}/write?org={}&bucket={}",
            hostname, organization, bucket
        ))
        .header("Authorization", format!("Token {}", auth_token)),
    };

    let resp = req.text(lines).send()?;
    Ok(resp)
}
