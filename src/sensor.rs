use std::collections::{BTreeMap, HashMap};
use std::io::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::RegisterConfig;
use influx_db_client::{Point, Value};
use log::warn;
use modbus::{Client, Error as ModbusError};

/// A Register map that has the register address as key and measurement name as value.
pub struct RegisterMap(BTreeMap<u16, String>);

impl RegisterMap {
    // Group consecutive registers into one request
    fn merged_reads(&self) -> Vec<ReadRegistersParams> {
        let mut params: Vec<ReadRegistersParams> = Vec::new();
        for &r in self.0.keys() {
            match params.last_mut() {
                Some(ref mut p) if r == p.0 + p.1 => p.1 += 1,
                _ => params.push(ReadRegistersParams(r, 1)),
            }
        }
        params
    }
}

impl From<RegisterConfig> for RegisterMap {
    fn from(cfg: RegisterConfig) -> Self {
        // Swap key and value of the toml configuration table
        RegisterMap(cfg.into_iter().map(|(k, v)| (v, k)).collect())
    }
}

#[derive(PartialEq)]
struct ReadRegistersParams(u16, u16);

fn new_influxdb_point(
    measurement: &str,
    timestamp: SystemTime,
    value: u16,
    tags: &[(String, String)],
) -> Point {
    let mut p = Point::new(measurement);
    p.add_field("value", Value::Integer(i64::from(value)));
    for (k, v) in tags {
        p.add_tag(k, Value::String(v.clone()));
    }
    p.add_timestamp(timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64);
    p
}

pub struct Sensor<'a> {
    id: u8,
    group: &'a str,
    registers: &'a RegisterMap,
    tags: Vec<(String, String)>,

    // TODO: Move this field to RegisterMap
    read_reg_calls: Vec<ReadRegistersParams>,
}

impl<'a> Sensor<'a> {
    pub fn new(
        id: u8,
        group: &'a str,
        registers: &'a RegisterMap,
        tags: Vec<(String, String)>,
    ) -> Self {
        Self {
            id,
            group,
            registers,
            tags,
            read_reg_calls: registers.merged_reads(),
        }
    }

    pub fn read_registers(&self, mb: &mut Client) -> Result<HashMap<u16, u16>, Error> {
        let mut result = HashMap::new();

        mb.set_slave(self.id);
        for param in &self.read_reg_calls {
            match mb.read_input_registers(param.0, param.1) {
                Ok(values) => {
                    result.extend(&mut (param.0..param.0 + param.1).zip(values));
                }
                Err(e) => match e {
                    ModbusError::Io(e) => return Err(e),
                    _ => warn!("Modbus: {}", e),
                },
            }
        }

        Ok(result)
    }

    pub fn get_points(&self, register_values: &HashMap<u16, u16>) -> Vec<Point> {
        let mut points = Vec::new();

        for (r, v) in register_values {
            let mut tags = Vec::new();
            tags.push(("group".to_string(), self.group.to_string()));
            tags.push(("id".to_string(), self.id.to_string()));
            tags.push(("register".to_string(), r.to_string()));
            tags.append(&mut self.tags.clone());
            points.push(new_influxdb_point(
                &self.registers.0[&r],
                SystemTime::now(),
                *v,
                &tags,
            ));
        }

        points
    }
}
