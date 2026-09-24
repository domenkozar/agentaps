use crate::discovery::find_executable;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

#[derive(Debug)]
pub enum Event {
    Message { agent_id: u64, value: Value },
    Disconnected { agent_id: u64, reason: String },
}

pub struct Connection {
    child: Child,
    outgoing: Sender<Value>,
}

impl Connection {
    pub fn spawn(
        agent_id: u64,
        command: &[String],
        cwd: &Path,
        events: Sender<Event>,
    ) -> Result<Self, String> {
        let (program, args) = command.split_first().ok_or("Agent command is empty")?;
        let mut process = Command::new(program);
        process
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if Path::new("/etc/NIXOS").exists()
            && command.iter().any(|part| part.contains("claude-agent-acp"))
            && std::env::var_os("CLAUDE_CODE_EXECUTABLE").is_none()
            && let Some(claude) = find_executable("claude")
        {
            process.env("CLAUDE_CODE_EXECUTABLE", claude);
        }
        let mut child = process
            .spawn()
            .map_err(|error| format!("Could not start {program}: {error}"))?;
        let mut stdin = child.stdin.take().ok_or("Agent stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("Agent stdout unavailable")?;
        let (outgoing, incoming): (Sender<Value>, Receiver<Value>) = mpsc::channel();

        let writer_events = events.clone();
        thread::spawn(move || {
            for value in incoming {
                if writeln!(stdin, "{value}")
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    let _ = writer_events.send(Event::Disconnected {
                        agent_id,
                        reason: "Could not write to agent".into(),
                    });
                    break;
                }
            }
        });
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => match serde_json::from_str(&line) {
                        Ok(value) => {
                            if events.send(Event::Message { agent_id, value }).is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            let _ = events.send(Event::Disconnected {
                                agent_id,
                                reason: format!("Invalid ACP message: {error}"),
                            });
                            return;
                        }
                    },
                    Err(error) => {
                        let _ = events.send(Event::Disconnected {
                            agent_id,
                            reason: format!("Agent stream failed: {error}"),
                        });
                        return;
                    }
                }
            }
            let _ = events.send(Event::Disconnected {
                agent_id,
                reason: "Agent disconnected".into(),
            });
        });
        Ok(Self { child, outgoing })
    }

    pub fn send(&self, value: Value) -> Result<(), String> {
        self.outgoing
            .send(value)
            .map_err(|_| "Agent disconnected".into())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn exchanges_json_rpc_lines_with_stdio_agent() {
        let script = "IFS= read -r request; printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":1}}'";
        let command = vec!["/bin/sh".into(), "-c".into(), script.into()];
        let (tx, rx) = mpsc::channel();
        let connection = Connection::spawn(7, &command, Path::new("/"), tx).unwrap();
        connection
            .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
            .unwrap();
        let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        match event {
            Event::Message { agent_id, value } => {
                assert_eq!(agent_id, 7);
                assert_eq!(value["result"]["protocolVersion"], 1);
            }
            Event::Disconnected { reason, .. } => panic!("Disconnected before reply: {reason}"),
        }
    }
}
