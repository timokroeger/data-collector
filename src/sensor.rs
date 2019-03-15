use std::collections::{BTreeMap, HashMap};
use std::io::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use influx_db_client::{Point, Value};
use log::warn;
use modbus::{Client, Error as ModbusError};

/// A Register map that has the register address as key and measurement name as value.
pub struct RegisterMap {
    registers: BTreeMap<u16, String>,

    /// A group contains the starting address and number of registers required by read call.
    read_groups: Vec<(u16, u16)>,
}

impl RegisterMap {
    pub fn new(registers: BTreeMap<u16, String>) -> Self {
        // A BTreeMap garantues sorted keys, addresses will be in order.
        let addrs = registers.keys().cloned().collect();
        Self {
            registers,
            read_groups: RegisterMap::group_registers(addrs),
        }
    }

    /// Groups consecutive registers into one request.
    /// Assumes the addresses to be sorted.
    fn group_registers(addrs: Vec<u16>) -> Vec<(u16, u16)> {
        let mut groups: Vec<(u16, u16)> = Vec::new();
        for addr in addrs {
            match groups.last_mut() {
                Some(ref mut p) if addr == p.0 + p.1 => p.1 += 1,
                _ => groups.push((addr, 1)),
            }
        }
        groups
    }

    pub fn read_groups(&self) -> &[(u16, u16)] {
        &self.read_groups
    }

    pub fn get_name(&self, addr: u16) -> &str {
        &self.registers[&addr]
    }
}

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
        }
    }

    pub fn read_registers(&self, mb: &mut Client) -> Result<HashMap<u16, u16>, Error> {
        let mut result = HashMap::new();

        mb.set_slave(self.id);
        for param in self.registers.read_groups() {
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

        for (&reg_addr, &value) in register_values {
            let mut tags = Vec::new();
            tags.push(("group".to_string(), self.group.to_string()));
            tags.push(("id".to_string(), self.id.to_string()));
            tags.push(("register".to_string(), reg_addr.to_string()));
            tags.append(&mut self.tags.clone());

            points.push(new_influxdb_point(
                &self.registers.get_name(reg_addr),
                SystemTime::now(),
                value,
                &tags,
            ));
        }

        points
    }
}
