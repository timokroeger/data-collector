mod config;
mod device;

use std::fs::File;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::config::Config;
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use log::{debug, info};
use modbus::{tcp::Transport};
use reqwest::{Client as HttpClient};
use simplelog::{Config as LogConfig, TermLogger, WriteLogger};

fn influxdb_line(measurement: &str, tags: &[(&str, &str)], value: u16, timestamp: u64) -> String {
    let escape_meas = |s: &str| s.replace(',', "\\,").replace(' ', "\\ ");
    let escape_tag = |s: &str| escape_meas(s).replace('=', "\\=");

    let mut line = escape_meas(measurement);
    for (k, v) in tags {
        line.push_str(&format!(",{}={}", escape_tag(k), escape_tag(v)));
    }
    line.push_str(&format!(" value={} {}\n", value, timestamp));
    line
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
    let req = if let Some(influx) = config.influxdb {
        let req = client
            .post(&format!("{}/write", influx.hostname))
            .query(&[("db", influx.database)]);
        match (influx.username, influx.password) {
            (Some(username), Some(password)) => req.query(&[("u", username), ("p", password)]),
            (_, _) => req,
        }
    } else if let Some(influx2) = config.influxdb2 {
        client
            .post(&format!("{}/write", influx2.hostname))
            .query(&[("org", influx2.organization), ("bucket", influx2.bucket)])
            .header("Authorization", format!("Token {}", influx2.auth_token))
    } else {
        panic!("No influxdb configuration found!");
    };
    let req = req.query(&[("precision", "s")]);

    let devices = config.devices.into_devices();

    debug!("ModbusTCP: Connecting to {}", modbus_hostname);
    let mb = Transport::new_with_cfg(&modbus_hostname, modbus_config).unwrap();

    // Wrapper for thread safe access of the Modbus connection.
    let mb = Arc::new(Mutex::new(mb));

    let mut threads = Vec::new();
    for dev in devices {
        let mb = mb.clone();
        threads.push(thread::spawn(move || loop {
            dev.read(&mut *mb.lock().unwrap()).unwrap();
            thread::sleep(dev.get_scan_interval());
        }));
    }

    // Wait for all sensors to fail before exiting.
    for thread in threads {
        thread.join().unwrap();
    }

    Ok(())
}
