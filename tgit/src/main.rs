extern crate termion;

use std::process::{Command, Output};

fn main() {
    let output = Command::new("git")
        .arg("branch")
        .output()
        .expect("failed to execute process");
    println!("status: {}", output.status);
    // let tmp_store = &output.stdout;
    let mut branch_output: Vec<char> = output.stdout.iter().map(|&t| t as char).collect();
    println!("branch_output {:?}", branch_output);
    branch_output.pop();
    let mut branch_iter = branch_output.split(|&x| x == '\n');
    let mut branches: Vec<String> = vec![];
    loop {
        if let Some(val) = branch_iter.next() {
            println!("{}", val.iter().collect::<String>());
            branches.push(val.iter().collect());
        } else {
            break;
        }
    }
    println!("{:?}", branches);

    assert!(output.status.success());
}
