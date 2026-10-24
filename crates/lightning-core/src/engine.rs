//! Combines all sources into one ranked, grouped result list.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::apps::{App, Task};
use crate::files::FileIndex;
use crate::fuzzy::{Matcher, Prepared, Ranges, char_mask};
use crate::power;
use crate::settings::SETTINGS;

pub const MAX_APPS: usize = 5;
pub const MAX_FILES: usize = 6;
pub const MAX_SETTINGS: usize = 5;
const MAX_RECENT: usize = 12;
/// How long a search waits for file results before answering without them.
const FILE_WAIT: Duration = Duration::from_millis(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    App,
    File,
    Setting,
    /// A power shortcut; never persisted to history.
    Power,
}

impl Kind {
    fn tag(self) -> &'static str {
        match self {
            Kind::App => "app",
            Kind::File => "file",
            Kind::Setting => "setting",
            Kind::Power => "power",
        }
    }
    fn from_tag(s: &str) -> Option<Self> {
        Some(match s {
            "app" => Kind::App,
            "file" => Kind::File,
            "setting" => Kind::Setting,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    /// Byte ranges into `title` to highlight.
    pub ranges: Ranges,
    /// What to open: `shell:AppsFolder\..`, an `ms-settings:` URI or a path.
    pub target: String,
}

#[derive(Debug, Default)]
pub struct Results {
    pub query: String,
    pub hits: Vec<Hit>,
    pub elapsed_us: u64,
    /// File results weren't ready in time; searching again picks them up.
    pub files_pending: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Sources {
    pub apps: bool,
    pub files: bool,
    pub settings: bool,
    pub power: bool,
}

struct Launch {
    kind: Kind,
    count: u32,
    title: String,
    subtitle: String,
    target: String,
}

pub struct Engine {
    apps: Vec<App>,
    app_masks: Vec<u64>,
    tasks: Vec<Task>,
    /// `SETTINGS` then `tasks`, index-aligned.
    prepared: Vec<Prepared>,
    files: Option<FileIndex>,
    pub sources: Sources,
    matcher: Matcher,
    /// Most recent first.
    history: Vec<Launch>,
    history_path: Option<PathBuf>,
}

impl Engine {
    pub fn new(apps: Vec<App>, files: Option<FileIndex>, data_dir: Option<&Path>) -> Self {
        let history_path = data_dir.map(|d| d.join("history.tsv"));
        let history = history_path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| parse_history(&s))
            .unwrap_or_default();
        let app_masks = apps.iter().map(|a| char_mask(&a.name)).collect();
        Self {
            apps,
            app_masks,
            tasks: Vec::new(),
            prepared: SETTINGS.iter().map(|s| Prepared::new(s.title, s.keywords)).collect(),
            files,
            sources: Sources { apps: true, files: true, settings: true, power: true },
            matcher: Matcher::default(),
            history,
            history_path,
        }
    }

    /// Control Panel tasks, searched alongside Settings pages.
    pub fn set_tasks(&mut self, tasks: Vec<Task>) {
        self.prepared.truncate(SETTINGS.len());
        self.prepared.extend(tasks.iter().map(|t| Prepared::new(&t.name, "")));
        self.tasks = tasks;
    }

    pub fn set_apps(&mut self, apps: Vec<App>) {
        self.app_masks = apps.iter().map(|a| char_mask(&a.name)).collect();
        self.apps = apps;
    }

    pub fn app_count(&self) -> usize {
        self.apps.len()
    }

    pub fn files(&self) -> Option<&FileIndex> {
        self.files.as_ref()
    }

    pub fn search(&mut self, query: &str) -> Results {
        let start = Instant::now();
        let mut hits = Vec::with_capacity(MAX_APPS + MAX_FILES + MAX_SETTINGS);
        let mut files_pending = false;
        self.matcher.set_query(query);

        if self.matcher.query_is_empty() {
            hits.extend(self.history.iter().take(MAX_RECENT).map(|l| Hit {
                kind: l.kind,
                title: l.title.clone(),
                subtitle: l.subtitle.clone(),
                ranges: Vec::new(),
                target: l.target.clone(),
            }));
        } else {
            let (apps, best_app) = if self.sources.apps { self.search_apps() } else { (Vec::new(), 0) };
            let (settings, best_setting) =
                if self.sources.settings { self.search_settings() } else { (Vec::new(), 0) };
            let files = match (&self.files, self.sources.files) {
                (Some(f), true) => match f.search_timeout(query, MAX_FILES * 2, FILE_WAIT) {
                    Some(mut found) => {
                        // Files the user opened before float up (stable: fff's
                        // order is kept among equals).
                        found.sort_by_key(|h| {
                            std::cmp::Reverse(self.launch_count(&h.path.to_string_lossy()))
                        });
                        found.truncate(MAX_FILES);
                        found
                    }
                    None => {
                        files_pending = true;
                        Vec::new()
                    }
                },
                _ => Vec::new(),
            };
            let files: Vec<Hit> = files
                .into_iter()
                .map(|f| Hit {
                    kind: Kind::File,
                    title: f.name,
                    subtitle: f.dir,
                    ranges: f.name_ranges,
                    target: f.path.to_string_lossy().into_owned(),
                })
                .collect();
            // Apps and settings share a scorer, so the stronger group leads.
            // Files (scored by fff on a different scale) follow the leader;
            // a weak trailing group goes last.
            let (lead, trail) =
                if best_setting > best_app { (settings, apps) } else { (apps, settings) };
            // A whole keyword ("lock") puts the buttons first, pre-selected.
            // The start of one ("sta", "pow") could be an app, so they go last.
            let buttons: Vec<Hit> = if self.sources.power {
                power::matches(query)
                    .into_iter()
                    .map(|p| Hit {
                        kind: Kind::Power,
                        title: p.label.into(),
                        subtitle: "Power shortcuts".into(),
                        ranges: Vec::new(),
                        target: p.action.target(),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let lead_buttons = !buttons.is_empty() && power::is_exact(query);
            if lead_buttons {
                hits.extend(buttons.iter().cloned());
            }
            hits.extend(lead);
            hits.extend(files);
            hits.extend(trail);
            if !buttons.is_empty() && !lead_buttons {
                hits.extend(buttons);
            }
        }
        Results {
            query: query.to_owned(),
            hits,
            elapsed_us: start.elapsed().as_micros() as u64,
            files_pending,
        }
    }

    fn search_apps(&mut self) -> (Vec<Hit>, i32) {
        let mut scored: Vec<(i32, usize, Ranges)> = Vec::new();
        for (i, app) in self.apps.iter().enumerate() {
            if let Some(m) = self.matcher.score_masked(&app.name, self.app_masks[i]) {
                let boost = self
                    .history
                    .iter()
                    .find(|l| l.kind == Kind::App && l.title == app.name)
                    .map_or(0, |l| l.count.min(10) as i32 * 30);
                scored.push((m.score + boost, i, m.ranges));
            }
        }
        top(&mut scored, MAX_APPS);
        let best = scored.first().map_or(0, |s| s.0);
        let hits = scored
            .into_iter()
            .map(|(_, i, ranges)| {
                let app = &self.apps[i];
                Hit {
                    kind: Kind::App,
                    title: app.name.clone(),
                    subtitle: "Application".into(),
                    ranges,
                    target: app.launch_target(),
                }
            })
            .collect();
        (hits, best)
    }

    fn search_settings(&mut self) -> (Vec<Hit>, i32) {
        // Settings pages and Control Panel tasks rank together.
        let titles = SETTINGS.iter().map(|s| s.title).chain(self.tasks.iter().map(|t| t.name.as_str()));
        let mut scored: Vec<(i32, usize, Ranges)> = Vec::new();
        for (i, (title, p)) in titles.zip(&self.prepared).enumerate() {
            if let Some(m) = self.matcher.score_masked(title, p.title_mask) {
                scored.push((m.score, i, m.ranges));
            } else if let Some(m) = self.matcher.word_score(p) {
                scored.push((m.score, i, m.ranges));
            }
        }
        top(&mut scored, MAX_SETTINGS);
        let best = scored.first().map_or(0, |s| s.0);
        let hits = scored
            .into_iter()
            .map(|(_, i, ranges)| match SETTINGS.get(i) {
                Some(s) => Hit {
                    kind: Kind::Setting,
                    title: s.title.into(),
                    subtitle: "Settings".into(),
                    ranges,
                    target: s.uri.into(),
                },
                None => {
                    let t = &self.tasks[i - SETTINGS.len()];
                    Hit {
                        kind: Kind::Setting,
                        title: t.name.clone(),
                        subtitle: "Control Panel".into(),
                        ranges,
                        target: t.target(),
                    }
                }
            })
            .collect();
        (hits, best)
    }

    fn launch_count(&self, target: &str) -> u32 {
        self.history.iter().find(|l| l.target == target).map_or(0, |l| l.count)
    }

    /// Call after the user opened `hit`; feeds ranking and the recent list.
    pub fn record_launch(&mut self, hit: &Hit) {
        let count = match self.history.iter().position(|l| l.target == hit.target) {
            Some(i) => self.history.remove(i).count + 1,
            None => 1,
        };
        self.history.insert(
            0,
            Launch {
                kind: hit.kind,
                count,
                title: hit.title.clone(),
                subtitle: hit.subtitle.clone(),
                target: hit.target.clone(),
            },
        );
        self.history.truncate(200);
        if let Some(p) = &self.history_path {
            let _ = std::fs::write(p, format_history(&self.history));
        }
    }
}

fn top(v: &mut Vec<(i32, usize, Ranges)>, n: usize) {
    if v.len() > n {
        v.select_nth_unstable_by(n - 1, |a, b| b.0.cmp(&a.0));
        v.truncate(n);
    }
    v.sort_unstable_by(|a, b| b.0.cmp(&a.0));
}

fn parse_history(s: &str) -> Vec<Launch> {
    s.lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            Some(Launch {
                kind: Kind::from_tag(f.next()?)?,
                count: f.next()?.parse().ok()?,
                title: f.next()?.to_owned(),
                subtitle: f.next()?.to_owned(),
                target: f.next()?.to_owned(),
            })
        })
        .collect()
}

fn format_history(h: &[Launch]) -> String {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    h.iter()
        .map(|l| {
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                l.kind.tag(),
                l.count,
                clean(&l.title),
                clean(&l.subtitle),
                clean(&l.target)
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        let apps = ["Visual Studio Code", "Calculator", "Windows Terminal", "Spotify"]
            .iter()
            .map(|n| App { name: n.to_string(), id: n.to_string() })
            .collect();
        Engine::new(apps, None, None)
    }

    #[test]
    fn groups_apps_then_settings() {
        let mut e = engine();
        let r = e.search("sound");
        assert!(r.hits.iter().any(|h| h.kind == Kind::Setting && h.title == "Sound"));
        let r = e.search("calc");
        assert_eq!(r.hits[0].title, "Calculator");
    }

    #[test]
    fn strong_setting_leads_weak_apps() {
        let mut e = engine();
        let r = e.search("wifi");
        assert_eq!(r.hits[0].title, "Wi-Fi");
    }

    #[test]
    fn control_panel_tasks_are_searchable() {
        let mut e = engine();
        e.set_tasks(vec![
            Task { name: "Edit the system environment variables".into(), pidl: Box::new([1]) },
            Task { name: "Adjust system volume".into(), pidl: Box::new([2]) },
        ]);
        let r = e.search("edit environment variables");
        assert_eq!(r.hits[0].title, "Edit the system environment variables");
        let r = e.search("volumne control");
        assert!(r.hits.iter().any(|h| h.title == "Volume mixer"));
        let r = e.search("system volume");
        assert_eq!(r.hits[0].title, "Adjust system volume");
    }

    #[test]
    fn power_buttons_lead_matching_queries_only() {
        let mut e = engine();
        let r = e.search("shut down");
        assert_eq!(r.hits[0].kind, Kind::Power);
        assert_eq!(r.hits[0].title, "Shut down");
        // A prefix still offers the button, after the real results.
        let r = e.search("shut");
        assert_eq!(r.hits.last().map(|h| h.title.as_str()), Some("Shut down"));
        // The start of a keyword never outranks a real result.
        let r = e.search("spot");
        assert_eq!(r.hits[0].title, "Spotify");
        let r = e.search("pow");
        assert_ne!(r.hits[0].kind, Kind::Power);
        assert!(r.hits.iter().any(|h| h.kind == Kind::Power));
        assert_eq!(e.search("power").hits[0].kind, Kind::Power);
        assert_eq!(e.search("lock").hits[0].title, "Lock");
        assert!(e.search("chrome").hits.iter().all(|h| h.kind != Kind::Power));
        e.sources.power = false;
        assert!(e.search("shut").hits.iter().all(|h| h.kind != Kind::Power));
    }

    #[test]
    fn keywords_find_settings() {
        let mut e = engine();
        let r = e.search("wallpaper");
        assert_eq!(r.hits[0].title, "Background");
    }

    #[test]
    fn launches_become_recents_and_boost() {
        let mut e = engine();
        let hit = e.search("spot").hits[0].clone();
        e.record_launch(&hit);
        let r = e.search("");
        assert_eq!(r.hits[0].title, "Spotify");
    }

    #[test]
    fn history_round_trips() {
        let h = vec![Launch {
            kind: Kind::File,
            count: 3,
            title: "a\tb".into(),
            subtitle: "D:\\x".into(),
            target: "D:\\x\\a b".into(),
        }];
        let back = parse_history(&format_history(&h));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].count, 3);
        assert_eq!(back[0].title, "a b");
    }
}
