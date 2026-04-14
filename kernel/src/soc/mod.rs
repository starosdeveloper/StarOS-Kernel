pub mod detection;
pub mod pmic;
pub mod clock;
pub mod reset;

pub use detection::{SocFamily, detect_soc};
pub use pmic::PmicDriver;
pub use clock::ClockController;
pub use reset::ResetController;
