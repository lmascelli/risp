//! Compact Markdown reports and explicit, versionable baselines.
//!
//! Row keys identify workloads: change the key when fixture size, setup boundaries,
//! or the measured operation changes, so an old baseline cannot silently compare
//! different work. `PERF_OUTPUT_DIR` redirects reports, but never the baseline.
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const TIMING_NOISE: f64 = 0.10;
const BLESS_ENV: &str = "PERF_BLESS";
const MACHINE_ENV: &str = "PERF_MACHINE";
const OUTPUT_ENV: &str = "PERF_OUTPUT_DIR";
const BUILD_ENV: &str = "PERF_BUILD_ID";
const BASELINE_MARKER: &str = "# baseline-v1";

pub(crate) struct Row {
    pub key: String,
    pub label: String,
    pub value: f64,
    pub display: String,
    pub note: String,
}

impl Row {
    pub fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        value: f64,
        display: impl Into<String>,
        note: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            value,
            display: display.into(),
            note: note.into(),
        }
    }

    pub fn timed(
        key: impl Into<String>,
        label: impl Into<String>,
        value: f64,
        display: impl Into<String>,
        note: impl Into<String>,
    ) -> Self {
        Self::new(key, label, value, display, note)
    }
}

pub(crate) struct Section {
    pub title: String,
    pub blurb: &'static str,
    pub rows: Vec<Row>,
}

pub(crate) struct Verdict {
    pub label: String,
    pub detail: String,
    pub ok: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Cost,
    Timing,
    Allocations,
}

impl Kind {
    fn fingerprint(self) -> String {
        match self {
            Self::Cost => "machine-independent fuel accounting".into(),
            Self::Timing | Self::Allocations => {
                let machine = machine_id().unwrap_or_else(|| "unidentified".into());
                let cpus = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(0);
                let build = env_label(BUILD_ENV).unwrap_or_else(|| "unspecified".into());
                let mode = if cfg!(debug_assertions) { "on" } else { "off" };
                let allocator = if self == Self::Allocations {
                    "; allocator=System test counter"
                } else {
                    ""
                };
                format!(
                    "{machine}; {}-{}; debug_assertions={mode}; {cpus} logical CPUs; runtime toolchain={}; build={build}{allocator}",
                    std::env::consts::ARCH,
                    std::env::consts::OS,
                    runtime_toolchain(),
                )
            }
        }
    }
}

pub(crate) struct Report {
    kind: Kind,
    output_path: PathBuf,
    baseline_path: PathBuf,
    title: &'static str,
    preamble: String,
    fingerprint: String,
    sections: Vec<Section>,
    verdicts: Vec<Verdict>,
}

impl Report {
    pub fn new(kind: Kind, file_name: &str, title: &'static str, preamble: String) -> Self {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = if env!("CARGO_PKG_NAME") == "rsedit_core" {
            manifest.parent().unwrap_or(manifest)
        } else {
            manifest
        };
        let output_dir = std::env::var_os(OUTPUT_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let output_path = report_output_path(root, file_name, output_dir.as_deref());
        let baseline_path = root.join(file_name.replace(".txt", ".baseline"));
        Self {
            kind,
            output_path,
            baseline_path,
            title,
            preamble,
            fingerprint: kind.fingerprint(),
            sections: Vec::new(),
            verdicts: Vec::new(),
        }
    }

    pub fn section(&mut self, title: impl Into<String>, blurb: &'static str, rows: Vec<Row>) {
        self.sections.push(Section {
            title: title.into(),
            blurb,
            rows,
        });
    }

    pub fn verdict(&mut self, ok: bool, label: impl Into<String>, detail: impl Into<String>) {
        self.verdicts.push(Verdict {
            label: label.into(),
            detail: detail.into(),
            ok,
        });
    }

    pub fn finish(self) -> Vec<String> {
        let previous = Baseline::load(&self.baseline_path);
        let comparable = match self.kind {
            Kind::Cost => previous.fingerprint.as_deref() == Some(self.fingerprint.as_str()),
            Kind::Timing | Kind::Allocations => {
                machine_id().is_some()
                    && previous.fingerprint.as_deref() == Some(self.fingerprint.as_str())
            }
        };
        let failures: Vec<String> = self
            .verdicts
            .iter()
            .filter(|v| !v.ok)
            .map(|v| format!("{}: {}", v.label, v.detail))
            .collect();
        let bless = std::env::var(BLESS_ENV).is_ok_and(|v| !v.is_empty() && v != "0");

        let mut out = String::new();
        let _ = writeln!(out, "# {}\n", self.title);
        let _ = writeln!(out, "{}\n", self.preamble.trim());
        let _ = writeln!(out, "- Environment: `{}`", self.fingerprint);
        if self.kind != Kind::Cost {
            let _ = writeln!(
                out,
                "- Build identity: the toolchain above is observed at run time; \
                 it does not prove which compiler built this binary. Keep compiler, \
                 optimization flags and allocator unchanged; set `{BUILD_ENV}` \
                 consistently when using custom build settings."
            );
        }
        if self.kind == Kind::Timing {
            let _ = writeln!(
                out,
                "- Change annotation: 10% is a fixed heuristic, not a statistical \
                 confidence interval. Timing changes do not fail the suite."
            );
        }
        let baseline_status = if bless && failures.is_empty() {
            format!(
                "will save reviewed measurements to `{}`",
                self.baseline_path.display()
            )
        } else if bless {
            "not updated because a behavioral or fuel-accounting check failed".into()
        } else if comparable {
            format!("`{}`", self.baseline_path.display())
        } else if self.kind != Kind::Cost && machine_id().is_none() {
            format!("disabled; set `{MACHINE_ENV}` to a stable machine or runner label")
        } else if let Some(old) = previous.fingerprint.as_deref() {
            format!("different environment (`{old}`); deltas suppressed")
        } else {
            format!("none; set `{BLESS_ENV}=1` to save a reviewed baseline")
        };
        let _ = writeln!(out, "- Baseline: {baseline_status}\n");

        for section in &self.sections {
            let _ = writeln!(out, "## {}\n\n{}\n", section.title, section.blurb);
            let mut rows = Vec::new();
            for row in &section.rows {
                let delta = if comparable {
                    describe_delta(self.kind, previous.get(&row.key), row.value)
                } else {
                    "--".into()
                };
                rows.push(vec![
                    row.label.clone(),
                    row.display.clone(),
                    delta,
                    row.note.clone(),
                ]);
            }
            write_ascii_table(
                &mut out,
                &["Measurement", "Current value", "Change", "Interpretation"],
                &rows,
                &[1, 2],
            );
            let _ = writeln!(out);
        }

        let _ = writeln!(out, "## Behavioral and accounting checks\n");
        let rows: Vec<Vec<String>> = self
            .verdicts
            .iter()
            .map(|verdict| {
                let mark = if verdict.ok { "PASS" } else { "FAIL" };
                vec![
                    mark.to_string(),
                    verdict.label.clone(),
                    verdict.detail.clone(),
                ]
            })
            .collect();
        write_ascii_table(&mut out, &["Result", "Check", "Detail"], &rows, &[]);
        let _ = writeln!(
            out,
            "\n{} of {} checks passed.\n",
            self.verdicts.len() - failures.len(),
            self.verdicts.len()
        );

        if let Some(parent) = self.output_path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("could not create {}: {e}", parent.display()));
        }
        fs::write(&self.output_path, &out)
            .unwrap_or_else(|e| panic!("could not write {}: {e}", self.output_path.display()));

        if bless && failures.is_empty() {
            Baseline::save(&self.baseline_path, &self.fingerprint, &self.sections);
        }
        failures
    }
}

fn report_output_path(root: &Path, file_name: &str, output: Option<&Path>) -> PathBuf {
    match output {
        Some(dir) => root.join(dir).join(file_name),
        None => root.join(file_name),
    }
}

fn write_ascii_table(out: &mut String, headers: &[&str], rows: &[Vec<String>], right: &[usize]) {
    let widths: Vec<usize> = (0..headers.len())
        .map(|column| {
            rows.iter()
                .map(|row| ascii(&row[column]).len())
                .chain([headers[column].len()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let border = |fill: char| {
        format!(
            "+{}+",
            widths
                .iter()
                .map(|width| fill.to_string().repeat(width + 2))
                .collect::<Vec<_>>()
                .join("+")
        )
    };
    let _ = writeln!(out, "{}", border('-'));
    write_ascii_row(
        out,
        &headers.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        &widths,
        right,
    );
    let _ = writeln!(out, "{}", border('='));
    for row in rows {
        let values: Vec<String> = row.iter().map(|value| ascii(value)).collect();
        write_ascii_row(out, &values, &widths, right);
    }
    let _ = writeln!(out, "{}", border('-'));
}

fn write_ascii_row(out: &mut String, values: &[String], widths: &[usize], right: &[usize]) {
    let _ = write!(out, "|");
    for (column, (value, width)) in values.iter().zip(widths).enumerate() {
        if right.contains(&column) {
            let _ = write!(out, " {:>width$} |", value, width = width);
        } else {
            let _ = write!(out, " {:<width$} |", value, width = width);
        }
    }
    let _ = writeln!(out);
}

fn ascii(value: &str) -> String {
    value
        .replace('–', "-")
        .replace('—', "--")
        .replace('±', "+/-")
}

#[derive(Default)]
struct Baseline {
    fingerprint: Option<String>,
    values: Vec<(String, f64)>,
}

impl Baseline {
    fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        if !text.lines().any(|line| line == BASELINE_MARKER) {
            return Self::default();
        }
        let mut baseline = Self::default();
        for line in text.lines() {
            if let Some(fp) = line.strip_prefix("fingerprint\t") {
                baseline.fingerprint = Some(fp.to_string());
            } else if let Some((key, value)) = line.split_once('\t') {
                if let Ok(value) = value.trim().parse::<f64>() {
                    baseline.values.push((key.to_string(), value));
                }
            }
        }
        baseline
    }

    fn get(&self, key: &str) -> Option<f64> {
        self.values.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }

    fn save(path: &Path, fingerprint: &str, sections: &[Section]) {
        let mut out = format!("{BASELINE_MARKER}\nfingerprint\t{fingerprint}\n");
        for row in sections.iter().flat_map(|s| &s.rows) {
            let _ = writeln!(out, "{}\t{}", row.key, row.value);
        }
        fs::write(path, out).unwrap_or_else(|e| panic!("could not save {}: {e}", path.display()));
    }
}

fn describe_delta(kind: Kind, previous: Option<f64>, now: f64) -> String {
    let Some(previous) = previous else {
        return "new".into();
    };
    if previous == now {
        return "unchanged".into();
    }
    if previous.abs() < f64::EPSILON {
        return "previously zero".into();
    }
    let pct = (now - previous) / previous.abs() * 100.0;
    if kind == Kind::Timing && pct.abs() <= TIMING_NOISE * 100.0 {
        format!("{pct:+.1}% (within fixed 10% band)")
    } else {
        format!("{pct:+.1}%")
    }
}

fn machine_id() -> Option<String> {
    env_label(MACHINE_ENV)
}

fn env_label(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().replace(['\n', '\r', '\t'], " "))
        .filter(|value| !value.is_empty())
}

fn runtime_toolchain() -> &'static str {
    static TOOLCHAIN: OnceLock<String> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        std::process::Command::new(rustc)
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|text| text.trim().replace(['\n', '\r', '\t'], " "))
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "unavailable".into())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_output_directory_resolves_relative_and_absolute_paths() {
        let root = Path::new("repo");
        assert_eq!(
            report_output_path(root, "timing.txt", Some(Path::new("runs/a"))),
            root.join("runs/a/timing.txt"),
        );
        assert_eq!(
            report_output_path(root, "timing.txt", None),
            root.join("timing.txt"),
        );
        let absolute = std::env::temp_dir().join("risp-perf-report-test");
        assert_eq!(
            report_output_path(root, "timing.txt", Some(&absolute)),
            absolute.join("timing.txt"),
        );
    }

    #[test]
    fn only_timing_uses_the_fixed_change_band() {
        assert!(describe_delta(Kind::Timing, Some(100.0), 105.0).contains("fixed 10%"));
        assert_eq!(
            describe_delta(Kind::Allocations, Some(100.0), 105.0),
            "+5.0%"
        );
        assert_eq!(describe_delta(Kind::Cost, Some(100.0), 105.0), "+5.0%");
    }

    #[test]
    fn deltas_handle_new_zero_and_unchanged_measurements() {
        assert_eq!(describe_delta(Kind::Timing, None, 1.0), "new");
        assert_eq!(
            describe_delta(Kind::Timing, Some(0.0), 1.0),
            "previously zero"
        );
        assert_eq!(describe_delta(Kind::Timing, Some(1.0), 1.0), "unchanged");
    }
}
