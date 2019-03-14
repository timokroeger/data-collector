mod config;
mod sensor;

use std::collections::BTreeMap;
use std::io::Error;
use std::thread;
use std::time::Duration;

use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use config::*;
use influx_db_client::{
    Client as InfluxDbClient, Points, Precision,
};
use log::{debug, error, info, warn};
use modbus::{tcp::Transport, Client as ModbusClient};
use sensor::{RegisterMap, Sensor};

fn connection_task(
    mb: &mut ModbusClient,
    db: &InfluxDbClient,
    sensor_groups: &BTreeMap<String, SensorGroupConfig>,
) -> Result<(), Error> {
    // TODO: Support more than one sensor group
    let (group, sensor_group) = sensor_groups.iter().next().unwrap();

    // TODO: Remove clone()
    let register_map: RegisterMap = sensor_group.measurement_registers.clone().into();

    let mut sensors = Vec::new();
    for sensor in &sensor_group.sensors {
        let tags = sensor.tags.iter().map(|(k, v)| (k.clone(), v.to_string())).collect();
        sensors.push(Sensor::new(sensor.id, &group, &register_map, tags));
    }

    loop {
        let mut points = Vec::new();

        for sensor in &mut sensors {
            let register_values = sensor.read_registers(mb)?;
            points.append(&mut sensor.get_points(&register_values));
        }

        db.write_points(Points::create_new(points), Some(Precision::Seconds), None)
            .unwrap_or_else(|e| warn!("InfluxDB: {}", e));

        thread::sleep(Duration::from_secs(sensor_group.scan_interval_sec));
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
    let config = Config::new(&config_file);
    info!(
        "Configuration loaded from {} with {} sensor groups",
        config_file,
        config.sensor_groups.len()
    );

    let db = config.influxdb.into_client();
    let (modbus_hostname, modbus_config) = config.modbus.into();

    loop {
        // Retry to connect forever
        debug!("ModbusTCP: Connecting to {}", modbus_hostname);
        match Transport::new_with_cfg(&modbus_hostname, modbus_config) {
            Ok(mut mb) => {
                info!("ModbusTCP: Successfully connected to {}", modbus_hostname);
                connection_task(&mut mb, &db, &config.sensor_groups)
                    .unwrap_or_else(|e| error!("ModbusTCP: {}, reconnecting...", e));
            }
            Err(e) => {
                error!("ModbusTCP: {}, retrying in 10 seconds...", e);
                thread::sleep(Duration::from_secs(10));
                continue;
            }
        };
    }
}
