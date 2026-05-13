use crate::error::Result;
use crate::devicetree::parser::FdtParser;
use crate::devicetree::discovery::DeviceDiscovery;
use crate::safety::{watchdog::Watchdog, memory_map::MemoryMap, panic, boot_validator::BootValidator};
use crate::soc::{pmic::PmicDriver, clock::ClockController, detection::SocInfo};

pub struct RealDeviceInit {
    pub fdt: FdtParser,
    pub soc_info: SocInfo,
    pub memory_map: MemoryMap,
    pub pmic: PmicDriver,
    pub clocks: ClockController,
    pub watchdog: Option<Watchdog>,
}

impl RealDeviceInit {
    pub fn from_dtb(dtb_addr: usize) -> Result<Self> {
        BootValidator::validate_all(dtb_addr)?;
        
        let fdt = FdtParser::new(dtb_addr)?;
        
        let soc_info = SocInfo::from_device_tree(&fdt)?;
        
        let memory_map = MemoryMap::from_device_tree(dtb_addr)?;
        
        let discovery = DeviceDiscovery::new(&fdt);
        
        if let Some(uart_info) = discovery.find_uart() {
            panic::set_uart_base(uart_info.base_addr as usize);
        }
        
        let pmic = PmicDriver::from_device_tree(&fdt)?;
        
        let clocks = ClockController::from_device_tree(&fdt)?;
        
        let watchdog = if let Some(timer_info) = discovery.find_timer() {
            Some(Watchdog::init_from_dt(
                timer_info.base_addr as usize,
                timer_info.clock_frequency,
                5000,
            )?)
        } else {
            None
        };
        
        Ok(Self {
            fdt,
            soc_info,
            memory_map,
            pmic,
            clocks,
            watchdog,
        })
    }

    pub fn print_info(&self) {
        // Will be implemented with proper UART driver
    }

    pub fn kick_watchdog(&self) {
        if self.watchdog.is_some() {
            Watchdog::kick();
        }
    }
}

pub fn init_real_device(dtb_addr: usize) -> Result<RealDeviceInit> {
    RealDeviceInit::from_dtb(dtb_addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_real_device_init_structure() {
        // Test requires valid DTB
    }
}
