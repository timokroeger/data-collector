use std::collections::{BTreeMap, HashMap};
use std::io::Error;

use tokio_modbus::prelude::*;

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

pub struct Sensor<'a> {
    pub id: u8,
    pub group: &'a str,
    pub registers: &'a RegisterMap,
    pub tags: Vec<(String, String)>,
}

impl<'a> Sensor<'a> {
    pub fn read_registers(&self, mb: &mut impl SyncReader) -> Result<HashMap<u16, u16>, Error> {
        let mut result = HashMap::new();

        mb.set_slave(Slave(self.id));
        for param in self.registers.read_groups() {
            let values = mb.read_input_registers(param.0, param.1)?;
            result.extend(&mut (param.0..).zip(values));
        }

        Ok(result)
    }
}
