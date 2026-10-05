use std::io::Write;
use std::process::{Command, Stdio};

use crate::PlatformError;

/// Result of a finished command.
#[derive(Debug, Clone, Default)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs external programs. Injected so tests never spawn anything.
pub trait CommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<CommandOutput, PlatformError>;
}

/// Spawns real processes and waits for them.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<CommandOutput, PlatformError> {
        let spawn_err = |e: std::io::Error| PlatformError::Spawn {
            program: program.to_string(),
            reason: e.to_string(),
        };
        let mut child = Command::new(program)
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(spawn_err)?;
        if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(data).map_err(spawn_err)?;
        }
        let out = child.wait_with_output().map_err(spawn_err)?;
        Ok(CommandOutput {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    pub type Call = (String, Vec<String>, Option<Vec<u8>>);

    /// Records calls; programs not in `outputs` fail to spawn.
    #[derive(Default)]
    pub struct FakeRunner {
        pub outputs: HashMap<String, CommandOutput>,
        pub calls: RefCell<Vec<Call>>,
    }

    impl FakeRunner {
        pub fn with(mut self, program: &str, success: bool, stdout: &str, stderr: &str) -> Self {
            self.outputs.insert(
                program.to_string(),
                CommandOutput {
                    success,
                    stdout: stdout.into(),
                    stderr: stderr.into(),
                },
            );
            self
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(
            &self,
            program: &str,
            args: &[&str],
            stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            self.calls.borrow_mut().push((
                program.to_string(),
                args.iter().map(|a| a.to_string()).collect(),
                stdin.map(<[u8]>::to_vec),
            ));
            self.outputs
                .get(program)
                .cloned()
                .ok_or_else(|| PlatformError::Spawn {
                    program: program.to_string(),
                    reason: "not found".into(),
                })
        }
    }
}
