mod config;
mod device;

use std::fs::File;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::config::Config;
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use log::{debug, info, warn};
use modbus::tcp::Transport;
use reqwest::Client as HttpClient;
use simplelog::{Config as LogConfig, TermLogger, WriteLogger};

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
    info!("Configuration file: {}", config_file);

    let modbus_config = config.modbus;
    let (modbus_hostname, modbus_config) = modbus_config.into_modbus_tcp_config();

    let client = HttpClient::new();
    let req = match config.influxdb {
        InfluxDbConfig::V1 {
            hostname,
            database,
            username,
            password,
        } => {
            let req = client
                .post(&format!("{}/write", hostname))
                .query(&[("db", database)]);
            match (username, password) {
                (Some(u), Some(p)) => req.query(&[("u", u), ("p", p)]),
                (_, _) => req,
            }
        }
        InfluxDbConfig::V2 {
            hostname,
            organization,
            bucket,
            auth_token,
        } => client
            .post(&format!("{}/write", hostname))
            .query(&[("org", organization), ("bucket", bucket)])
            .header("Authorization", format!("Token {}", auth_token)),
    };

    let devices = config.devices.into_devices();

    debug!("ModbusTCP: Connecting to {}", modbus_hostname);
    let mb = Transport::new_with_cfg(&modbus_hostname, modbus_config).unwrap();

    // Wrapper for thread safe access of the Modbus connection.
    let mb = Arc::new(Mutex::new(mb));

    let mut threads = Vec::new();
    for dev in devices {
        let mb = mb.clone();
        let req = req.try_clone().unwrap();
        threads.push(thread::spawn(move || loop {
            let lines = dev.read(&mut *mb.lock().unwrap()).unwrap();
            let resp = req.try_clone().unwrap().body(lines).send();
            match resp {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        warn!("InfluxDB: {:?}", resp);
                    }
                }
                Err(e) => warn!("InfluxDB: {}", e),
            }
            thread::sleep(dev.get_scan_interval());
        }));
    }

    // Wait for all sensors to fail before exiting.
    for thread in threads {
        thread.join().unwrap();
    }

    Ok(())
}
