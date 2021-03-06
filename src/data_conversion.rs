//! This mainly concerns converting collected data into things that the canvas
//! can actually handle.
use crate::Pid;
use crate::{
    app::{data_farmer, data_harvester, App, Filter, ProcWidgetState},
    utils::{self, gen_util::*},
};
use data_harvester::processes::ProcessSorting;
use indexmap::IndexSet;
use std::collections::{HashMap, HashSet, VecDeque};

/// Point is of time, data
type Point = (f64, f64);

#[derive(Default, Debug)]
pub struct ConvertedBatteryData {
    pub battery_name: String,
    pub charge_percentage: f64,
    pub watt_consumption: String,
    pub duration_until_full: Option<String>,
    pub duration_until_empty: Option<String>,
    pub health: String,
}

#[derive(Default, Debug)]
pub struct ConvertedNetworkData {
    pub rx: Vec<Point>,
    pub tx: Vec<Point>,
    pub rx_display: String,
    pub tx_display: String,
    pub total_rx_display: Option<String>,
    pub total_tx_display: Option<String>,
    // TODO: [NETWORKING] add min/max/mean of each
    // min_rx : f64,
    // max_rx : f64,
    // mean_rx: f64,
    // min_tx: f64,
    // max_tx: f64,
    // mean_tx: f64,
}

// TODO: [REFACTOR] Process data... stuff really needs a rewrite.  Again.
#[derive(Clone, Default, Debug)]
pub struct ConvertedProcessData {
    pub pid: Pid,
    pub ppid: Option<Pid>,
    pub name: String,
    pub command: String,
    pub is_thread: Option<bool>,
    pub cpu_percent_usage: f64,
    pub mem_percent_usage: f64,
    pub mem_usage_bytes: u64,
    pub mem_usage_str: (f64, String),
    pub group_pids: Vec<Pid>,
    pub read_per_sec: String,
    pub write_per_sec: String,
    pub total_read: String,
    pub total_write: String,
    pub rps_f64: f64,
    pub wps_f64: f64,
    pub tr_f64: f64,
    pub tw_f64: f64,
    pub process_state: String,
    pub process_char: char,

    /// Prefix printed before the process when displayed.
    pub process_description_prefix: Option<String>,
    /// Whether to mark this process entry as disabled (mostly for tree mode).
    pub is_disabled_entry: bool,
    /// Whether this entry is collapsed, hiding all its children (for tree mode).
    pub is_collapsed_entry: bool,
}

#[derive(Clone, Default, Debug)]
pub struct ConvertedCpuData {
    pub cpu_name: String,
    pub short_cpu_name: String,
    /// Tuple is time, value
    pub cpu_data: Vec<Point>,
    /// Represents the value displayed on the legend.
    pub legend_value: String,
}

pub fn convert_temp_row(app: &App) -> Vec<Vec<String>> {
    let current_data = &app.data_collection;
    let temp_type = &app.app_config_fields.temperature_type;
    let temp_filter = &app.filters.temp_filter;

    let mut sensor_vector: Vec<Vec<String>> = current_data
        .temp_harvest
        .iter()
        .filter_map(|temp_harvest| {
            let name = match (&temp_harvest.component_name, &temp_harvest.component_label) {
                (Some(name), Some(label)) => format!("{}: {}", name, label),
                (None, Some(label)) => label.to_string(),
                (Some(name), None) => name.to_string(),
                (None, None) => String::default(),
            };

            let to_keep = if let Some(temp_filter) = temp_filter {
                let mut ret = temp_filter.is_list_ignored;
                for r in &temp_filter.list {
                    if r.is_match(&name) {
                        ret = !temp_filter.is_list_ignored;
                        break;
                    }
                }
                ret
            } else {
                true
            };

            if to_keep {
                Some(vec![
                    name,
                    (temp_harvest.temperature.ceil() as u64).to_string()
                        + match temp_type {
                            data_harvester::temperature::TemperatureType::Celsius => "C",
                            data_harvester::temperature::TemperatureType::Kelvin => "K",
                            data_harvester::temperature::TemperatureType::Fahrenheit => "F",
                        },
                ])
            } else {
                None
            }
        })
        .collect();

    if sensor_vector.is_empty() {
        sensor_vector.push(vec!["No Sensors Found".to_string(), "".to_string()]);
    }

    sensor_vector
}

pub fn convert_disk_row(
    current_data: &data_farmer::DataCollection, disk_filter: &Option<Filter>,
) -> Vec<Vec<String>> {
    let mut disk_vector: Vec<Vec<String>> = Vec::new();

    current_data
        .disk_harvest
        .iter()
        .filter(|disk_harvest| {
            if let Some(disk_filter) = disk_filter {
                for r in &disk_filter.list {
                    if r.is_match(&disk_harvest.name) {
                        return !disk_filter.is_list_ignored;
                    }
                }
                disk_filter.is_list_ignored
            } else {
                true
            }
        })
        .zip(&current_data.io_labels)
        .for_each(|(disk, (io_read, io_write))| {
            let converted_free_space = get_simple_byte_values(disk.free_space, false);
            let converted_total_space = get_simple_byte_values(disk.total_space, false);
            disk_vector.push(vec![
                disk.name.to_string(),
                disk.mount_point.to_string(),
                format!(
                    "{:.0}%",
                    disk.used_space as f64 / disk.total_space as f64 * 100_f64
                ),
                format!("{:.*}{}", 0, converted_free_space.0, converted_free_space.1),
                format!(
                    "{:.*}{}",
                    0, converted_total_space.0, converted_total_space.1
                ),
                io_read.to_string(),
                io_write.to_string(),
            ]);
        });

    disk_vector
}

pub fn convert_cpu_data_points(
    current_data: &data_farmer::DataCollection, existing_cpu_data: &mut Vec<ConvertedCpuData>,
    is_frozen: bool,
) {
    let current_time = if is_frozen {
        if let Some(frozen_instant) = current_data.frozen_instant {
            frozen_instant
        } else {
            current_data.current_instant
        }
    } else {
        current_data.current_instant
    };

    // Initialize cpu_data_vector if the lengths don't match...
    if let Some((_time, data)) = &current_data.timed_data_vec.last() {
        if data.cpu_data.len() + 1 != existing_cpu_data.len() {
            *existing_cpu_data = vec![ConvertedCpuData {
                cpu_name: "All".to_string(),
                short_cpu_name: "All".to_string(),
                cpu_data: vec![],
                legend_value: String::new(),
            }];

            existing_cpu_data.extend(
                data.cpu_data
                    .iter()
                    .enumerate()
                    .map(|(itx, cpu_usage)| ConvertedCpuData {
                        cpu_name: if let Some(cpu_harvest) = current_data.cpu_harvest.get(itx) {
                            if let Some(cpu_count) = cpu_harvest.cpu_count {
                                format!("{}{}", cpu_harvest.cpu_prefix, cpu_count)
                            } else {
                                cpu_harvest.cpu_prefix.to_string()
                            }
                        } else {
                            String::default()
                        },
                        short_cpu_name: if let Some(cpu_harvest) = current_data.cpu_harvest.get(itx)
                        {
                            if let Some(cpu_count) = cpu_harvest.cpu_count {
                                cpu_count.to_string()
                            } else {
                                cpu_harvest.cpu_prefix.to_string()
                            }
                        } else {
                            String::default()
                        },
                        legend_value: format!("{:.0}%", cpu_usage.round()),
                        cpu_data: vec![],
                    })
                    .collect::<Vec<ConvertedCpuData>>(),
            );
        } else {
            existing_cpu_data
                .iter_mut()
                .skip(1)
                .zip(&data.cpu_data)
                .for_each(|(cpu, cpu_usage)| {
                    cpu.cpu_data = vec![];
                    cpu.legend_value = format!("{:.0}%", cpu_usage.round());
                });
        }
    }

    for (time, data) in &current_data.timed_data_vec {
        let time_from_start: f64 = (current_time.duration_since(*time).as_millis() as f64).floor();

        for (itx, cpu) in data.cpu_data.iter().enumerate() {
            if let Some(cpu_data) = existing_cpu_data.get_mut(itx + 1) {
                cpu_data.cpu_data.push((-time_from_start, *cpu));
            }
        }

        if *time == current_time {
            break;
        }
    }
}

pub fn convert_mem_data_points(
    current_data: &data_farmer::DataCollection, is_frozen: bool,
) -> Vec<Point> {
    let mut result: Vec<Point> = Vec::new();
    let current_time = if is_frozen {
        if let Some(frozen_instant) = current_data.frozen_instant {
            frozen_instant
        } else {
            current_data.current_instant
        }
    } else {
        current_data.current_instant
    };

    for (time, data) in &current_data.timed_data_vec {
        let time_from_start: f64 = (current_time.duration_since(*time).as_millis() as f64).floor();
        result.push((-time_from_start, data.mem_data));
        if *time == current_time {
            break;
        }
    }

    result
}

pub fn convert_swap_data_points(
    current_data: &data_farmer::DataCollection, is_frozen: bool,
) -> Vec<Point> {
    let mut result: Vec<Point> = Vec::new();
    let current_time = if is_frozen {
        if let Some(frozen_instant) = current_data.frozen_instant {
            frozen_instant
        } else {
            current_data.current_instant
        }
    } else {
        current_data.current_instant
    };

    for (time, data) in &current_data.timed_data_vec {
        let time_from_start: f64 = (current_time.duration_since(*time).as_millis() as f64).floor();
        result.push((-time_from_start, data.swap_data));
        if *time == current_time {
            break;
        }
    }

    result
}

pub fn convert_mem_labels(
    current_data: &data_farmer::DataCollection,
) -> (String, String, String, String) {
    (
        format!(
            "{:3.0}%",
            match current_data.memory_harvest.mem_total_in_mb {
                0 => 0.0,
                _ =>
                    current_data.memory_harvest.mem_used_in_mb as f64 * 100.0
                        / current_data.memory_harvest.mem_total_in_mb as f64,
            }
        ),
        format!(
            "   {:.1}GB/{:.1}GB",
            current_data.memory_harvest.mem_used_in_mb as f64 / 1024.0,
            (current_data.memory_harvest.mem_total_in_mb as f64 / 1024.0)
        ),
        format!(
            "{:3.0}%",
            match current_data.swap_harvest.mem_total_in_mb {
                0 => 0.0,
                _ =>
                    current_data.swap_harvest.mem_used_in_mb as f64 * 100.0
                        / current_data.swap_harvest.mem_total_in_mb as f64,
            }
        ),
        format!(
            "   {:.1}GB/{:.1}GB",
            current_data.swap_harvest.mem_used_in_mb as f64 / 1024.0,
            (current_data.swap_harvest.mem_total_in_mb as f64 / 1024.0)
        ),
    )
}

pub fn get_rx_tx_data_points(
    current_data: &data_farmer::DataCollection, is_frozen: bool,
) -> (Vec<Point>, Vec<Point>) {
    let mut rx: Vec<Point> = Vec::new();
    let mut tx: Vec<Point> = Vec::new();

    let current_time = if is_frozen {
        if let Some(frozen_instant) = current_data.frozen_instant {
            frozen_instant
        } else {
            current_data.current_instant
        }
    } else {
        current_data.current_instant
    };

    for (time, data) in &current_data.timed_data_vec {
        let time_from_start: f64 = (current_time.duration_since(*time).as_millis() as f64).floor();
        rx.push((-time_from_start, data.rx_data));
        tx.push((-time_from_start, data.tx_data));
        if *time == current_time {
            break;
        }
    }

    (rx, tx)
}

pub fn convert_network_data_points(
    current_data: &data_farmer::DataCollection, is_frozen: bool, need_four_points: bool,
) -> ConvertedNetworkData {
    let (rx, tx) = get_rx_tx_data_points(current_data, is_frozen);

    let total_rx_converted_result: (f64, String);
    let rx_converted_result: (f64, String);
    let total_tx_converted_result: (f64, String);
    let tx_converted_result: (f64, String);

    rx_converted_result = get_exact_byte_values(current_data.network_harvest.rx, false);
    total_rx_converted_result = get_exact_byte_values(current_data.network_harvest.total_rx, false);

    tx_converted_result = get_exact_byte_values(current_data.network_harvest.tx, false);
    total_tx_converted_result = get_exact_byte_values(current_data.network_harvest.total_tx, false);

    if need_four_points {
        let rx_display = format!("{:.*}{}", 1, rx_converted_result.0, rx_converted_result.1);
        let total_rx_display = Some(format!(
            "{:.*}{}",
            1, total_rx_converted_result.0, total_rx_converted_result.1
        ));
        let tx_display = format!("{:.*}{}", 1, tx_converted_result.0, tx_converted_result.1);
        let total_tx_display = Some(format!(
            "{:.*}{}",
            1, total_tx_converted_result.0, total_tx_converted_result.1
        ));
        ConvertedNetworkData {
            rx,
            tx,
            rx_display,
            tx_display,
            total_rx_display,
            total_tx_display,
        }
    } else {
        let rx_display = format!(
            "RX: {:<9} All: {:<9}",
            format!("{:.1}{:3}", rx_converted_result.0, rx_converted_result.1),
            format!(
                "{:.1}{:3}",
                total_rx_converted_result.0, total_rx_converted_result.1
            )
        );
        let tx_display = format!(
            "TX: {:<9} All: {:<9}",
            format!("{:.1}{:3}", tx_converted_result.0, tx_converted_result.1),
            format!(
                "{:.1}{:3}",
                total_tx_converted_result.0, total_tx_converted_result.1
            )
        );

        ConvertedNetworkData {
            rx,
            tx,
            rx_display,
            tx_display,
            total_rx_display: None,
            total_tx_display: None,
        }
    }
}

pub enum ProcessGroupingType {
    Grouped,
    Ungrouped,
}

pub enum ProcessNamingType {
    Name,
    Path,
}

/// Because we needed to UPDATE data entries rather than REPLACING entries, we instead update
/// the existing vector.
pub fn convert_process_data(
    current_data: &data_farmer::DataCollection,
    existing_converted_process_data: &mut HashMap<Pid, ConvertedProcessData>,
) {
    // TODO [THREAD]: Thread highlighting and hiding support
    // For macOS see https://github.com/hishamhm/htop/pull/848/files

    let mut complete_pid_set: HashSet<Pid> =
        existing_converted_process_data.keys().copied().collect();

    for process in &current_data.process_harvest {
        let converted_rps = get_exact_byte_values(process.read_bytes_per_sec, false);
        let converted_wps = get_exact_byte_values(process.write_bytes_per_sec, false);
        let converted_total_read = get_exact_byte_values(process.total_read_bytes, false);
        let converted_total_write = get_exact_byte_values(process.total_write_bytes, false);

        let read_per_sec = format!("{:.*}{}/s", 0, converted_rps.0, converted_rps.1);
        let write_per_sec = format!("{:.*}{}/s", 0, converted_wps.0, converted_wps.1);
        let total_read = format!("{:.*}{}", 0, converted_total_read.0, converted_total_read.1);
        let total_write = format!(
            "{:.*}{}",
            0, converted_total_write.0, converted_total_write.1
        );

        if let Some(process_entry) = existing_converted_process_data.get_mut(&process.pid) {
            complete_pid_set.remove(&process.pid);

            // Very dumb way to see if there's PID reuse...
            if process_entry.ppid == process.parent_pid {
                process_entry.name = process.name.to_string();
                process_entry.command = process.command.to_string();
                process_entry.cpu_percent_usage = process.cpu_usage_percent;
                process_entry.mem_percent_usage = process.mem_usage_percent;
                process_entry.mem_usage_bytes = process.mem_usage_bytes;
                process_entry.mem_usage_str = get_exact_byte_values(process.mem_usage_bytes, false);
                process_entry.group_pids = vec![process.pid];
                process_entry.read_per_sec = read_per_sec;
                process_entry.write_per_sec = write_per_sec;
                process_entry.total_read = total_read;
                process_entry.total_write = total_write;
                process_entry.rps_f64 = process.read_bytes_per_sec as f64;
                process_entry.wps_f64 = process.write_bytes_per_sec as f64;
                process_entry.tr_f64 = process.total_read_bytes as f64;
                process_entry.tw_f64 = process.total_write_bytes as f64;
                process_entry.process_state = process.process_state.to_owned();
                process_entry.process_char = process.process_state_char;
                process_entry.process_description_prefix = None;
                process_entry.is_disabled_entry = false;
            } else {
                // ...I hate that I can't combine if let and an if statement in one line...
                *process_entry = ConvertedProcessData {
                    pid: process.pid,
                    ppid: process.parent_pid,
                    is_thread: None,
                    name: process.name.to_string(),
                    command: process.command.to_string(),
                    cpu_percent_usage: process.cpu_usage_percent,
                    mem_percent_usage: process.mem_usage_percent,
                    mem_usage_bytes: process.mem_usage_bytes,
                    mem_usage_str: get_exact_byte_values(process.mem_usage_bytes, false),
                    group_pids: vec![process.pid],
                    read_per_sec,
                    write_per_sec,
                    total_read,
                    total_write,
                    rps_f64: process.read_bytes_per_sec as f64,
                    wps_f64: process.write_bytes_per_sec as f64,
                    tr_f64: process.total_read_bytes as f64,
                    tw_f64: process.total_write_bytes as f64,
                    process_state: process.process_state.to_owned(),
                    process_char: process.process_state_char,
                    process_description_prefix: None,
                    is_disabled_entry: false,
                    is_collapsed_entry: false,
                };
            }
        } else {
            existing_converted_process_data.insert(
                process.pid,
                ConvertedProcessData {
                    pid: process.pid,
                    ppid: process.parent_pid,
                    is_thread: None,
                    name: process.name.to_string(),
                    command: process.command.to_string(),
                    cpu_percent_usage: process.cpu_usage_percent,
                    mem_percent_usage: process.mem_usage_percent,
                    mem_usage_bytes: process.mem_usage_bytes,
                    mem_usage_str: get_exact_byte_values(process.mem_usage_bytes, false),
                    group_pids: vec![process.pid],
                    read_per_sec,
                    write_per_sec,
                    total_read,
                    total_write,
                    rps_f64: process.read_bytes_per_sec as f64,
                    wps_f64: process.write_bytes_per_sec as f64,
                    tr_f64: process.total_read_bytes as f64,
                    tw_f64: process.total_write_bytes as f64,
                    process_state: process.process_state.to_owned(),
                    process_char: process.process_state_char,
                    process_description_prefix: None,
                    is_disabled_entry: false,
                    is_collapsed_entry: false,
                },
            );
        }
    }

    // Now clean up any spare entries that weren't visited, to avoid clutter:
    complete_pid_set.iter().for_each(|pid| {
        existing_converted_process_data.remove(pid);
    })
}

const BRANCH_ENDING: char = '└';
const BRANCH_VERTICAL: char = '│';
const BRANCH_SPLIT: char = '├';
const BRANCH_HORIZONTAL: char = '─';

pub fn tree_process_data(
    filtered_process_data: &[ConvertedProcessData], is_using_command: bool,
    sorting_type: &ProcessSorting, is_sort_descending: bool,
) -> Vec<ConvertedProcessData> {
    // TODO: [TREE] Option to sort usage by total branch usage or individual value usage?

    // Let's first build up a (really terrible) parent -> child mapping...
    // At the same time, let's make a mapping of PID -> process data!
    let mut parent_child_mapping: HashMap<Pid, IndexSet<Pid>> = HashMap::default();
    let mut pid_process_mapping: HashMap<Pid, &ConvertedProcessData> = HashMap::default(); // We actually already have this stored, but it's unfiltered... oh well.
    let mut orphan_set: IndexSet<Pid> = IndexSet::new();
    let mut collapsed_set: IndexSet<Pid> = IndexSet::new();

    filtered_process_data.iter().for_each(|process| {
        if let Some(ppid) = process.ppid {
            orphan_set.insert(ppid);
        }
        orphan_set.insert(process.pid);
    });

    filtered_process_data.iter().for_each(|process| {
        // Create a mapping for the process if it DNE.
        parent_child_mapping
            .entry(process.pid)
            .or_insert_with(IndexSet::new);
        pid_process_mapping.insert(process.pid, process);

        if process.is_collapsed_entry {
            collapsed_set.insert(process.pid);
        }

        // Insert its mapping to the process' parent if needed (create if it DNE).
        if let Some(ppid) = process.ppid {
            orphan_set.remove(&process.pid);
            parent_child_mapping
                .entry(ppid)
                .or_insert_with(IndexSet::new)
                .insert(process.pid);
        }
    });

    // Keep only orphans, or promote children of orphans to a top-level orphan
    // if their parents DNE in our pid to process mapping...
    let old_orphan_set = orphan_set.clone();
    old_orphan_set.iter().for_each(|pid| {
        if pid_process_mapping.get(pid).is_none() {
            // DNE!  Promote the mapped children and remove the current parent...
            orphan_set.remove(pid);
            if let Some(children) = parent_child_mapping.get(pid) {
                orphan_set.extend(children);
            }
        }
    });

    // Turn the parent-child mapping into a "list" via DFS...
    let mut pids_to_explore: VecDeque<Pid> = orphan_set.into_iter().collect();
    let mut explored_pids: Vec<Pid> = vec![];
    let mut lines: Vec<String> = vec![];

    /// A post-order traversal to correctly prune entire branches that only contain children
    /// that are disabled and themselves are also disabled ~~wait that sounds wrong~~.
    ///
    /// Basically, go through the hashmap, and prune out all branches that are no longer relevant.
    fn prune_disabled_pids(
        current_pid: Pid, parent_child_mapping: &mut HashMap<Pid, IndexSet<Pid>>,
        pid_process_mapping: &HashMap<Pid, &ConvertedProcessData>,
    ) -> bool {
        // Let's explore all the children first, and make sure they (and their children)
        // aren't all disabled...
        let mut are_all_children_disabled = true;
        if let Some(children) = parent_child_mapping.get(&current_pid) {
            for child_pid in children.clone() {
                let is_child_disabled =
                    prune_disabled_pids(child_pid, parent_child_mapping, pid_process_mapping);

                if is_child_disabled {
                    if let Some(current_mapping) = parent_child_mapping.get_mut(&current_pid) {
                        current_mapping.remove(&child_pid);
                    }
                } else if are_all_children_disabled {
                    are_all_children_disabled = false;
                }
            }
        }

        // Now consider the current pid and whether to prune...
        // If the node itself is not disabled, then never prune.  If it is, then check if all
        // of its are disabled.
        if let Some(process) = pid_process_mapping.get(&current_pid) {
            if process.is_disabled_entry && are_all_children_disabled {
                parent_child_mapping.remove(&current_pid);
                return true;
            }
        }

        false
    }

    fn sort_remaining_pids(
        current_pid: Pid, sort_type: &ProcessSorting, is_sort_descending: bool,
        parent_child_mapping: &mut HashMap<Pid, IndexSet<Pid>>,
        pid_process_mapping: &HashMap<Pid, &ConvertedProcessData>,
    ) {
        // Sorting is special for tree data.  So, by default, things are "sorted"
        // via the DFS.  Otherwise, since this is DFS of the scanned PIDs (which are in order),
        // you actually get a REVERSE order --- so, you get higher PIDs earlier than lower ones.
        //
        // So how do we "sort"?  The current idea is that:
        // - We sort *per-level*.  Say, I want to sort by CPU.  The "first level" is sorted
        //   by CPU in terms of its usage.  All its direct children are sorted by CPU
        //   with *their* siblings.  Etc.
        // - The default is thus PIDs in ascending order.  We set it to this when
        //   we first enable the mode.

        // So first, let's look at the children... (post-order again)
        if let Some(children) = parent_child_mapping.get(&current_pid) {
            let mut to_sort_vec: Vec<(Pid, &ConvertedProcessData)> = vec![];
            for child_pid in children.clone() {
                if let Some(child_process) = pid_process_mapping.get(&child_pid) {
                    to_sort_vec.push((child_pid, child_process));
                }
                sort_remaining_pids(
                    child_pid,
                    sort_type,
                    is_sort_descending,
                    parent_child_mapping,
                    pid_process_mapping,
                );
            }

            // Now let's sort the immediate children!
            sort_vec(&mut to_sort_vec, sort_type, is_sort_descending);

            // Need to reverse what we got, apparently...
            if let Some(current_mapping) = parent_child_mapping.get_mut(&current_pid) {
                *current_mapping = to_sort_vec
                    .iter()
                    .rev()
                    .map(|(pid, _proc)| *pid)
                    .collect::<IndexSet<Pid>>();
            }
        }
    }

    fn sort_vec(
        to_sort_vec: &mut Vec<(Pid, &ConvertedProcessData)>, sort_type: &ProcessSorting,
        is_sort_descending: bool,
    ) {
        // Sort by PID first (descending)
        to_sort_vec.sort_by(|a, b| utils::gen_util::get_ordering(a.1.pid, b.1.pid, false));

        match sort_type {
            ProcessSorting::CpuPercent => {
                to_sort_vec.sort_by(|a, b| {
                    utils::gen_util::get_ordering(
                        a.1.cpu_percent_usage,
                        b.1.cpu_percent_usage,
                        is_sort_descending,
                    )
                });
            }
            ProcessSorting::Mem => {
                to_sort_vec.sort_by(|a, b| {
                    utils::gen_util::get_ordering(
                        a.1.mem_usage_bytes,
                        b.1.mem_usage_bytes,
                        is_sort_descending,
                    )
                });
            }
            ProcessSorting::MemPercent => {
                to_sort_vec.sort_by(|a, b| {
                    utils::gen_util::get_ordering(
                        a.1.mem_percent_usage,
                        b.1.mem_percent_usage,
                        is_sort_descending,
                    )
                });
            }
            ProcessSorting::ProcessName => {
                to_sort_vec.sort_by(|a, b| {
                    utils::gen_util::get_ordering(
                        &a.1.name.to_lowercase(),
                        &b.1.name.to_lowercase(),
                        is_sort_descending,
                    )
                });
            }
            ProcessSorting::Command => to_sort_vec.sort_by(|a, b| {
                utils::gen_util::get_ordering(
                    &a.1.command.to_lowercase(),
                    &b.1.command.to_lowercase(),
                    is_sort_descending,
                )
            }),
            ProcessSorting::Pid => {
                if is_sort_descending {
                    to_sort_vec.sort_by(|a, b| {
                        utils::gen_util::get_ordering(a.0, b.0, is_sort_descending)
                    });
                }
            }
            ProcessSorting::ReadPerSecond => {
                to_sort_vec.sort_by(|a, b| {
                    utils::gen_util::get_ordering(a.1.rps_f64, b.1.rps_f64, is_sort_descending)
                });
            }
            ProcessSorting::WritePerSecond => {
                to_sort_vec.sort_by(|a, b| {
                    utils::gen_util::get_ordering(a.1.wps_f64, b.1.wps_f64, is_sort_descending)
                });
            }
            ProcessSorting::TotalRead => {
                to_sort_vec.sort_by(|a, b| {
                    utils::gen_util::get_ordering(a.1.tr_f64, b.1.tr_f64, is_sort_descending)
                });
            }
            ProcessSorting::TotalWrite => {
                to_sort_vec.sort_by(|a, b| {
                    utils::gen_util::get_ordering(a.1.tw_f64, b.1.tw_f64, is_sort_descending)
                });
            }
            ProcessSorting::State => to_sort_vec.sort_by(|a, b| {
                utils::gen_util::get_ordering(
                    &a.1.process_state.to_lowercase(),
                    &b.1.process_state.to_lowercase(),
                    is_sort_descending,
                )
            }),
            ProcessSorting::Count => {
                // Should never occur in this case.
            }
        }
    }

    /// A DFS traversal to correctly build the prefix lines (the pretty '├' and '─' lines) and
    /// the correct order to the PID tree as a vector.
    fn build_explored_pids(
        current_pid: Pid, parent_child_mapping: &HashMap<Pid, IndexSet<Pid>>,
        prev_drawn_lines: &str, collapsed_set: &IndexSet<Pid>,
    ) -> (Vec<Pid>, Vec<String>) {
        let mut explored_pids: Vec<Pid> = vec![current_pid];
        let mut lines: Vec<String> = vec![];

        if collapsed_set.contains(&current_pid) {
            return (explored_pids, lines);
        } else if let Some(children) = parent_child_mapping.get(&current_pid) {
            for (itx, child) in children.iter().rev().enumerate() {
                let new_drawn_lines = if itx == children.len() - 1 {
                    format!("{}   ", prev_drawn_lines)
                } else {
                    format!("{}{}  ", prev_drawn_lines, BRANCH_VERTICAL)
                };

                let (pid_res, branch_res) = build_explored_pids(
                    *child,
                    parent_child_mapping,
                    new_drawn_lines.as_str(),
                    collapsed_set,
                );

                if itx == children.len() - 1 {
                    lines.push(format!(
                        "{}{}",
                        prev_drawn_lines,
                        if !new_drawn_lines.is_empty() {
                            format!("{}{} ", BRANCH_ENDING, BRANCH_HORIZONTAL)
                        } else {
                            String::default()
                        }
                    ));
                } else {
                    lines.push(format!(
                        "{}{}",
                        prev_drawn_lines,
                        if !new_drawn_lines.is_empty() {
                            format!("{}{} ", BRANCH_SPLIT, BRANCH_HORIZONTAL)
                        } else {
                            String::default()
                        }
                    ));
                }

                explored_pids.extend(pid_res);
                lines.extend(branch_res);
            }
        }

        (explored_pids, lines)
    }

    let mut to_sort_vec = Vec::new();
    for pid in pids_to_explore {
        if let Some(process) = pid_process_mapping.get(&pid) {
            to_sort_vec.push((pid, *process));
        }
    }
    sort_vec(&mut to_sort_vec, sorting_type, is_sort_descending);
    pids_to_explore = to_sort_vec.iter().map(|(pid, _proc)| *pid).collect();

    while let Some(current_pid) = pids_to_explore.pop_front() {
        if !prune_disabled_pids(current_pid, &mut parent_child_mapping, &pid_process_mapping) {
            sort_remaining_pids(
                current_pid,
                sorting_type,
                is_sort_descending,
                &mut parent_child_mapping,
                &pid_process_mapping,
            );

            let (pid_res, branch_res) =
                build_explored_pids(current_pid, &parent_child_mapping, "", &collapsed_set);
            lines.push(String::default());
            lines.extend(branch_res);
            explored_pids.extend(pid_res);
        }
    }

    // Now let's "rearrange" our current list of converted process data into the correct
    // order required... and we're done!
    explored_pids
        .iter()
        .zip(lines)
        .filter_map(|(pid, prefix)| match pid_process_mapping.remove(pid) {
            Some(process) => {
                let mut p = process.clone();
                p.process_description_prefix = Some(format!(
                    "{}{}{}",
                    prefix,
                    if p.is_collapsed_entry { "+ " } else { "" }, // I do the + sign thing here because I'm kinda too lazy to do it in the prefix, tbh.
                    if is_using_command {
                        &p.command
                    } else {
                        &p.name
                    }
                ));
                Some(p)
            }
            None => None,
        })
        .collect::<Vec<_>>()
}

// FIXME: [OPT] This is an easy target for optimization, too many to_strings!
pub fn stringify_process_data(
    proc_widget_state: &ProcWidgetState, finalized_process_data: &[ConvertedProcessData],
) -> Vec<(Vec<(String, Option<String>)>, bool)> {
    let is_proc_widget_grouped = proc_widget_state.is_grouped;
    let is_using_command = proc_widget_state.is_using_command;
    let is_tree = proc_widget_state.is_tree_mode;
    let mem_enabled = proc_widget_state.columns.is_enabled(&ProcessSorting::Mem);

    finalized_process_data
        .iter()
        .map(|process| {
            (
                vec![
                    (
                        if is_proc_widget_grouped {
                            process.group_pids.len().to_string()
                        } else {
                            process.pid.to_string()
                        },
                        None,
                    ),
                    (
                        if is_tree {
                            if let Some(prefix) = &process.process_description_prefix {
                                prefix.clone()
                            } else {
                                String::default()
                            }
                        } else if is_using_command {
                            process.command.clone()
                        } else {
                            process.name.clone()
                        },
                        None,
                    ),
                    (format!("{:.1}%", process.cpu_percent_usage), None),
                    (
                        if mem_enabled {
                            format!("{:.0}{}", process.mem_usage_str.0, process.mem_usage_str.1)
                        } else {
                            format!("{:.1}%", process.mem_percent_usage)
                        },
                        None,
                    ),
                    (process.read_per_sec.clone(), None),
                    (process.write_per_sec.clone(), None),
                    (process.total_read.clone(), None),
                    (process.total_write.clone(), None),
                    (
                        process.process_state.clone(),
                        Some(process.process_char.to_string()),
                    ),
                ],
                process.is_disabled_entry,
            )
        })
        .collect()
}

pub fn group_process_data(
    single_process_data: &[ConvertedProcessData], is_using_command: bool,
) -> Vec<ConvertedProcessData> {
    #[derive(Clone, Default, Debug)]
    struct SingleProcessData {
        pub pid: Pid,
        pub cpu_percent_usage: f64,
        pub mem_percent_usage: f64,
        pub mem_usage_bytes: u64,
        pub group_pids: Vec<Pid>,
        pub read_per_sec: f64,
        pub write_per_sec: f64,
        pub total_read: f64,
        pub total_write: f64,
        pub process_state: String,
    }

    let mut grouped_hashmap: HashMap<String, SingleProcessData> = std::collections::HashMap::new();

    single_process_data.iter().for_each(|process| {
        let entry = grouped_hashmap
            .entry(if is_using_command {
                process.command.to_string()
            } else {
                process.name.to_string()
            })
            .or_insert(SingleProcessData {
                pid: process.pid,
                ..SingleProcessData::default()
            });

        (*entry).cpu_percent_usage += process.cpu_percent_usage;
        (*entry).mem_percent_usage += process.mem_percent_usage;
        (*entry).mem_usage_bytes += process.mem_usage_bytes;
        (*entry).group_pids.push(process.pid);
        (*entry).read_per_sec += process.rps_f64;
        (*entry).write_per_sec += process.wps_f64;
        (*entry).total_read += process.tr_f64;
        (*entry).total_write += process.tw_f64;
    });

    grouped_hashmap
        .iter()
        .map(|(identifier, process_details)| {
            let p = process_details.clone();
            let converted_rps = get_exact_byte_values(p.read_per_sec as u64, false);
            let converted_wps = get_exact_byte_values(p.write_per_sec as u64, false);
            let converted_total_read = get_exact_byte_values(p.total_read as u64, false);
            let converted_total_write = get_exact_byte_values(p.total_write as u64, false);

            let read_per_sec = format!("{:.*}{}/s", 0, converted_rps.0, converted_rps.1);
            let write_per_sec = format!("{:.*}{}/s", 0, converted_wps.0, converted_wps.1);
            let total_read = format!("{:.*}{}", 0, converted_total_read.0, converted_total_read.1);
            let total_write = format!(
                "{:.*}{}",
                0, converted_total_write.0, converted_total_write.1
            );

            ConvertedProcessData {
                pid: p.pid,
                ppid: None,
                is_thread: None,
                name: identifier.to_string(),
                command: identifier.to_string(),
                cpu_percent_usage: p.cpu_percent_usage,
                mem_percent_usage: p.mem_percent_usage,
                mem_usage_bytes: p.mem_usage_bytes,
                mem_usage_str: get_exact_byte_values(p.mem_usage_bytes, false),
                group_pids: p.group_pids,
                read_per_sec,
                write_per_sec,
                total_read,
                total_write,
                rps_f64: p.read_per_sec,
                wps_f64: p.write_per_sec,
                tr_f64: p.total_read,
                tw_f64: p.total_write,
                process_state: p.process_state,
                process_description_prefix: None,
                process_char: char::default(),
                is_disabled_entry: false,
                is_collapsed_entry: false,
            }
        })
        .collect::<Vec<_>>()
}

pub fn convert_battery_harvest(
    current_data: &data_farmer::DataCollection,
) -> Vec<ConvertedBatteryData> {
    current_data
        .battery_harvest
        .iter()
        .enumerate()
        .map(|(itx, battery_harvest)| ConvertedBatteryData {
            battery_name: format!("Battery {}", itx),
            charge_percentage: battery_harvest.charge_percent,
            watt_consumption: format!("{:.2}W", battery_harvest.power_consumption_rate_watts),
            duration_until_empty: if let Some(secs_till_empty) = battery_harvest.secs_until_empty {
                let time = chrono::Duration::seconds(secs_till_empty);
                let num_minutes = time.num_minutes() - time.num_hours() * 60;
                let num_seconds = time.num_seconds() - time.num_minutes() * 60;
                Some(format!(
                    "{} hour{}, {} minute{}, {} second{}",
                    time.num_hours(),
                    if time.num_hours() == 1 { "" } else { "s" },
                    num_minutes,
                    if num_minutes == 1 { "" } else { "s" },
                    num_seconds,
                    if num_seconds == 1 { "" } else { "s" },
                ))
            } else {
                None
            },
            duration_until_full: if let Some(secs_till_full) = battery_harvest.secs_until_full {
                let time = chrono::Duration::seconds(secs_till_full);
                let num_minutes = time.num_minutes() - time.num_hours() * 60;
                let num_seconds = time.num_seconds() - time.num_minutes() * 60;
                Some(format!(
                    "{} hour{}, {} minute{}, {} second{}",
                    time.num_hours(),
                    if time.num_hours() == 1 { "" } else { "s" },
                    num_minutes,
                    if num_minutes == 1 { "" } else { "s" },
                    num_seconds,
                    if num_seconds == 1 { "" } else { "s" },
                ))
            } else {
                None
            },
            health: format!("{:.2}%", battery_harvest.health_percent),
        })
        .collect()
}
