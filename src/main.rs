mod config;

use std::collections::BTreeMap;
use std::io::Error;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use config::*;
use influx_db_client::{
    Client as InfluxDbClient, Point as InfluxDbPoint, Points, Precision, Value as InfluxDbValue,
};
use log::{debug, error, info, warn};
use modbus::{
    tcp::{Config as ModbusTcpConfig, Transport},
    Client as ModbusClient, Error as ModbusError,
};

struct Point<'a> {
    measurement: &'a str,
    timestamp: SystemTime,
    value: u16,
    sensor_group: &'a str,
    sensor_config: &'a ConfigSensor,
    register: u16,
}

impl<'a> Point<'a> {
    fn as_influxdb_point(&self) -> InfluxDbPoint {
        let mut p = InfluxDbPoint::new(self.measurement);
        p.add_field("value", InfluxDbValue::Integer(i64::from(self.value)));
        p.add_tag("group", InfluxDbValue::String(self.sensor_group.to_owned()));
        p.add_tag(
            "id",
            InfluxDbValue::String(self.sensor_config.id.to_string()),
        );
        for (tag_name, tag_value) in &self.sensor_config.tags {
            p.add_tag(tag_name, InfluxDbValue::String(tag_value.to_string()));
        }
        p.add_tag("register", InfluxDbValue::String(self.register.to_string()));
        p.add_timestamp(self.timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64);
        p
    }
}

#[derive(Debug, PartialEq)]
struct ReadRegistersParams(u16, u16);

// Group consecutive registers into one request
fn merge_read_regs<I>(regs: I) -> Vec<ReadRegistersParams>
where
    I: IntoIterator<Item = u16>,
{
    let mut params: Vec<ReadRegistersParams> = Vec::new();
    for r in regs {
        match params.last_mut() {
            Some(ref mut p) if r == p.0 + p.1 => p.1 += 1,
            _ => params.push(ReadRegistersParams(r, 1)),
        }
    }
    params
}

fn connection_task(
    mb: &mut ModbusClient,
    db: &InfluxDbClient,
    sensor_groups: &BTreeMap<String, ConfigSensorGroup>,
) -> Result<(), Error> {
    // TODO: Support more than one sensor group
    let (group, sensor_group) = sensor_groups.into_iter().next().unwrap();

    // Create a register map that can be indexed by the register address and holds the measurement
    // name for that entry by swapping key and value of the configuration table.
    let register_map: BTreeMap<u16, &String> = sensor_group
        .measurement_registers
        .iter()
        .map(|(k, &v)| (v, k))
        .collect();
    let read_reg_calls = merge_read_regs(register_map.keys().cloned());

    loop {
        let mut points = Vec::new();

        for sensor in &sensor_group.sensors {
            mb.set_slave(sensor.id);
            for param in &read_reg_calls {
                match mb.read_input_registers(param.0, param.1) {
                    Ok(values) => {
                        for (i, &v) in values.iter().enumerate() {
                            let reg = param.0 + i as u16;
                            points.push(Point {
                                measurement: &register_map[&reg],
                                timestamp: SystemTime::now(),
                                value: v,
                                sensor_group: &group,
                                sensor_config: sensor,
                                register: reg,
                            })
                        }
                    }
                    Err(e) => match e {
                        ModbusError::Io(e) => return Err(e),
                        _ => warn!("Modbus: {}", e),
                    },
                };
            }
        }

        db.write_points(
            Points::create_new(points.into_iter().map(|p| p.as_influxdb_point()).collect()),
            Some(Precision::Seconds),
            None,
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_read_regs() {
        assert_eq!(
            merge_read_regs([4, 5, 6, 9, 10, 12].iter().cloned()),
            vec![
                ReadRegistersParams(4, 3),
                ReadRegistersParams(9, 2),
                ReadRegistersParams(12, 1)
            ]
        );
    }
}
