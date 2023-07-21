use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

pub fn run(command_string: String) {
    let parsed_command = command_string.split_whitespace().collect::<Vec<&str>>();
    let (command, params) = parsed_command.split_at(1);

    let mut cmd = Command::new(command[0])
        .args(params)
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to execute process");

    {
        let stdout = cmd.stdout.as_mut().unwrap();
        let stdout_reader = BufReader::new(stdout);
        let stdout_lines = stdout_reader.lines();

        for line in stdout_lines {
            println!("{}", line.unwrap());
        }
    }

    cmd.wait().unwrap();
}
