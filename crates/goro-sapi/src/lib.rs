pub mod cli;

use goro_core::array::PhpArray;

/// SAPI module trait - abstraction between PHP engine and the outside world
pub trait SapiModule {
    fn name(&self) -> &str;
    fn pretty_name(&self) -> &str;
    fn startup(&mut self) -> std::io::Result<()>;
    fn shutdown(&mut self) -> std::io::Result<()>;
    fn write_stdout(&mut self, data: &[u8]) -> std::io::Result<usize>;
    fn write_stderr(&mut self, data: &[u8]) -> std::io::Result<usize>;
    fn read_stdin(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
    fn register_server_variables(&self, _vars: &mut PhpArray) {}
}
