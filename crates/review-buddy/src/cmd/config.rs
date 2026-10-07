//! `review-buddy config …`

use rb_paths::PathsReport;
use serde_json::{json, Value};

use super::context::Context;
use super::error::CmdError;
use super::output::{self, json, Cell, Column, Table};
use super::DEMO_LABEL;

const PATH_FIELDS: &[&str] = &[
    "configDir",
    "dataDir",
    "cacheDir",
    "stateDir",
    "runtimeDir",
    "systemConfigDirs",
    "systemDataDirs",
    "configFiles",
    "writeTarget",
    "sessionFile",
    "draftsDir",
];

fn display(path: &std::path::Path) -> String {
    path.display().to_string()
}

pub(super) fn paths_json(report: &PathsReport) -> Value {
    let p = &report.paths;
    let dirs =
        |dirs: &[std::path::PathBuf]| -> Vec<String> { dirs.iter().map(|d| display(d)).collect() };
    json!({
        "configDir": display(&p.config_dir),
        "dataDir": display(&p.data_dir),
        "cacheDir": display(&p.cache_dir),
        "stateDir": display(&p.state_dir),
        "runtimeDir": p.runtime_dir.as_deref().map(display),
        "systemConfigDirs": dirs(&p.system_config_dirs),
        "systemDataDirs": dirs(&p.system_data_dirs),
        "configFiles": report.config_files.iter().map(|f| json!({
            "path": display(&f.path),
            "origin": f.origin.to_string(),
            "exists": f.exists,
        })).collect::<Vec<_>>(),
        "writeTarget": display(&report.write_target),
        "sessionFile": display(&p.session_file()),
        "draftsDir": display(&p.drafts_dir()),
    })
}

/// One row per directory or file, three columns, for a pipe.
fn paths_table(report: &PathsReport) -> Table {
    let mut table = Table::new(vec![
        Column::fixed("Kind"),
        Column::fixed("Path"),
        Column::fixed("Status"),
    ]);
    let p = &report.paths;
    let mut dir = |kind: &str, path: &std::path::Path| {
        table.push(vec![
            Cell::plain(kind),
            Cell::plain(display(path)),
            Cell::plain(""),
        ]);
    };
    dir("config-dir", &p.config_dir);
    dir("data-dir", &p.data_dir);
    dir("cache-dir", &p.cache_dir);
    dir("state-dir", &p.state_dir);
    if let Some(runtime) = &p.runtime_dir {
        dir("runtime-dir", runtime);
    }
    for d in &p.system_config_dirs {
        dir("system-config-dir", d);
    }
    for d in &p.system_data_dirs {
        dir("system-data-dir", d);
    }
    for file in &report.config_files {
        table.push(vec![
            Cell::plain("config-file"),
            Cell::plain(display(&file.path)),
            Cell::plain(if file.exists { "loaded" } else { "not found" }),
        ]);
    }
    let session = p.session_file();
    table.push(vec![
        Cell::plain("session-file"),
        Cell::plain(display(&session)),
        Cell::plain(if session.exists() {
            "present"
        } else {
            "not found"
        }),
    ]);
    let drafts = p.drafts_dir();
    table.push(vec![
        Cell::plain("drafts-dir"),
        Cell::plain(display(&drafts)),
        Cell::plain(if drafts.exists() {
            "present"
        } else {
            "not found"
        }),
    ]);
    table.push(vec![
        Cell::plain("write-target"),
        Cell::plain(display(&report.write_target)),
        Cell::plain(""),
    ]);
    table
}

/// Every resolved directory and which config files were loaded.
pub fn paths(ctx: &Context) -> Result<(), CmdError> {
    let report = &ctx.paths;
    if let Some(text) = json::render(&ctx.args, PATH_FIELDS, &paths_json(report), ctx.out.tty) {
        return output::print(&text?);
    }
    if ctx.out.tty {
        let mut text = format!("{report}\n");
        if ctx.is_demo() {
            text.push_str(&format!(
                "\nThese are throwaway demo directories {}\n",
                DEMO_LABEL
            ));
        }
        output::print(&text)
    } else {
        output::print(&ctx.out.table(&paths_table(report)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_paths::{MapEnv, PathsReport};

    fn report() -> PathsReport {
        PathsReport::build(&MapEnv::new("/home/a"), None).unwrap()
    }

    #[test]
    fn json_has_every_documented_field() {
        let value = paths_json(&report());
        for field in PATH_FIELDS {
            assert!(value.get(field).is_some(), "{field}");
        }
        assert_eq!(
            value["configDir"],
            report().paths.config_dir.display().to_string()
        );
        assert_eq!(value["runtimeDir"], Value::Null);
        assert!(value["configFiles"].is_array());
    }

    #[test]
    fn the_table_has_a_row_per_directory_file_and_target() {
        let rows = paths_table(&report());
        let text = super::super::output::render_tsv(&rows);
        for kind in [
            "config-dir",
            "data-dir",
            "cache-dir",
            "state-dir",
            "config-file",
            "write-target",
        ] {
            assert!(text.lines().any(|l| l.starts_with(kind)), "{kind}\n{text}");
        }
        assert!(text.lines().all(|l| l.split('\t').count() == 3));
    }
}
