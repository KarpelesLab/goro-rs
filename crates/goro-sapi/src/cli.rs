use std::io::{Read, Write};

use crate::SapiModule;

/// CLI SAPI - simplest SAPI for command-line execution
pub struct CliSapi;

impl CliSapi {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CliSapi {
    fn default() -> Self {
        Self::new()
    }
}

impl SapiModule for CliSapi {
    fn name(&self) -> &str {
        "cli"
    }

    fn pretty_name(&self) -> &str {
        "Command Line Interface"
    }

    fn startup(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn shutdown(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn write_stdout(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(data)?;
        Ok(data.len())
    }

    fn write_stderr(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let stderr = std::io::stderr();
        let mut handle = stderr.lock();
        handle.write_all(data)?;
        Ok(data.len())
    }

    fn read_stdin(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::stdin().read(buf)
    }
}
