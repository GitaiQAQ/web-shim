use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct ProcessTreeNode {
    name: String,
    pid: u32,
    ppid: u32,
    children: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct ProcessTree {
    pub root: ProcessTreeNode,
    pub pid_map: HashMap<u32, ProcessTreeNode>,
}

fn get_process_record(status_path: &Path) -> Option<ProcessTreeNode> {
    let mut pid: Option<u32> = None;
    let mut ppid: Option<u32> = None;
    let mut name: Option<String> = None;

    let mut reader = std::io::BufReader::new(File::open(status_path).unwrap());
    loop {
        let mut linebuf = String::new();
        match reader.read_line(&mut linebuf) {
            Ok(_) => {
                if linebuf.is_empty() {
                    break;
                }
                let parts: Vec<&str> = linebuf[..].splitn(2, ':').collect();
                if parts.len() == 2 {
                    let key = parts[0].trim();
                    let value = parts[1].trim();
                    match key {
                        "Name" => name = Some(value.to_string()),
                        "Pid" => pid = value.parse().ok(),
                        "PPid" => ppid = value.parse().ok(),
                        _ => (),
                    }
                }
            }
            Err(_) => break,
        }
    }
    return if pid.is_some() && ppid.is_some() && name.is_some() {
        Some(ProcessTreeNode {
            name: name.unwrap(),
            pid: pid.unwrap(),
            ppid: ppid.unwrap(),
            children: Vec::new(),
        })
    } else {
        None
    };
}

fn get_process_records() -> HashMap<u32, ProcessTreeNode> {
    let proc_directory = Path::new("/proc");

    // find potential process directories under /proc
    let proc_directory_contents = fs::read_dir(&proc_directory).unwrap();
    let records: Vec<ProcessTreeNode> = proc_directory_contents
        .filter_map(|entry| {
            let entry_path = entry.unwrap().path();
            if fs::metadata(entry_path.as_path()).unwrap().is_dir() {
                let status_path = entry_path.join("status");
                if let Ok(metadata) = fs::metadata(status_path.as_path()) {
                    if metadata.is_file() {
                        return get_process_record(status_path.as_path());
                    }
                }
            }
            None
        })
        .collect();

    let mut pid_map: HashMap<u32, ProcessTreeNode> = HashMap::new();
    for record in records {
        pid_map.insert(record.pid, record);
    }
    pid_map
}

pub fn build_process_tree() -> HashMap<u32, ProcessTreeNode> {
    let mut pid_map = get_process_records();

    // add a root node with pid 0 and ppid -1
    // this is a hack to make the tree work
    pid_map.insert(
        0,
        ProcessTreeNode {
            name: "/".to_string(),
            pid: 0,
            ppid: 0,
            children: Vec::new(),
        },
    );

    let ppid_map: Vec<(u32, u32)> = pid_map
        .values()
        .map(|record| (record.ppid, record.pid))
        .collect();

    for (ppid, pid) in ppid_map {
        match pid_map.get_mut(&ppid) {
            Some(parent) => {
                parent.children.push(pid);
            }
            None => (),
        }
    }

    pid_map
}

pub fn format_node(
    node: &ProcessTreeNode,
    indent_level: u32,
    pid_map: &HashMap<u32, ProcessTreeNode>,
) -> String {
    let mut res: String = String::new();

    // print indentation
    for _ in 0..indent_level {
        res = res + "  ";
    }

    res = res + format!("- {} #{}\n", node.name, node.pid).as_str();
    for child in node.children.iter() {
        res = res + format_node(pid_map.get(child).unwrap(), indent_level + 1, pid_map).as_str();
        // recurse
    }

    res
}
