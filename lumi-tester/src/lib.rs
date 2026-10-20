pub mod ai;
pub mod adapter;
pub mod camera;
pub mod commands;
pub mod driver;
pub mod hardware;
pub mod hc;
pub mod inspector;
pub mod parser;
pub mod recorder;
pub mod report;
pub mod requirements;
pub mod runner;
pub mod orchestrator;
pub mod updater;
pub mod utils;

// Re-export common items
pub use driver::list_devices;
pub use report::generate_report;
pub use runner::run_tests;
