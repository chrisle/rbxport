//! The golden gate: our analysis against rekordbox's own stamps, on the
//! curated `RBX-BPM-GRID-TEST` playlist. READ-ONLY against the library.
//!
//! Two modes, because a full decode per experiment is what makes tuning slow:
//!
//! `cargo run --release -p rbl-analysis --example golden -- cache [playlist]`
//!   decodes every track in the playlist once and keeps the mono PCM beside
//!   what rekordbox recorded for it (BPM, key, the `PQTZ` grid) under
//!   `target/golden/`.
//!
//! `cache genre:<genre>[,<genre>…][:<limit>]` caches the first `limit`
//!   tracks of each named genre instead of a playlist, to compare a style
//!   of music against rekordbox's grids without editing the library. Point
//!   `RB_LITE_GOLDEN` at a separate directory for each set.
//!
//! `cargo run --release -p rbl-analysis --example golden -- eval [filter]`
//!   analyses every cached track and scores three things, each pass/fail per
//!   track, and prints every failure:
//!
//!   - **bpm**: within `BPM_TOLERANCE` of rekordbox's, no octave folding.
//!   - **downbeat**: our downbeats fall on rekordbox's, i.e. the offset
//!     between the two grids, taken modulo one bar, is within `MS_TOLERANCE`.
//!   - **grid**: at least `GRID_PASS` of rekordbox's beats have one of ours
//!     within `MS_TOLERANCE` carrying the same beat number. This is the one
//!     that fails when the tempo is right to 0.05 BPM and still drifts a beat
//!     off by the end of a track, and the one a variable-tempo track needs.
//!   - **key**: the same name rekordbox chose.
//!
//! `filter` is a substring of the title, to look at one track. `score` is
//! `eval` under another name. With a filter or `RB_LITE_VERBOSE`, every
//! track is listed, a track with a tempo change with both grids' tempo
//! runs and the drift between the grids at ten points along it.
//!
//! `cargo run --release -p rbl-analysis --example golden -- transition [filter]`
//!   prints what the audio holds around every hand-gridded tempo change,
//!   per beat of rekordbox's grid (`RB_LITE_BARS` bars either side), or
//!   with `RB_LITE_BGRID=1` per bar of rekordbox's incoming grid extended
//!   back over the change; `RB_LITE_PEAKS=kick|flux|attack,from,to[,min]`
//!   lists one band's raw peaks in a range.
//!
//! `cargo run --release -p rbl-analysis --example golden -- align [filter]`
//!   measures how each grid sits on the music, ours and rekordbox's alike:
//!   the offset of the onsets from the beats in each fifth of the track,
//!   and how many grids drift. It needs no grid to be right, so it can
//!   judge a playlist whose rekordbox grids are not checked by hand.
//!
//! `RB_LITE_PRESET=rekordbox|rbxport` picks the app preset under test and
//! `RB_LITE_RANGE=<min>-<max>` its BPM range.
//!
//! `RUST_LOG=rbl_analysis=debug` on any mode prints the tempo stage's
//! decisions at each change: the settled levels, the kick's runs, the cut.
#![allow(clippy::pedantic, clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rbl_analysis::tempo::Beat;

const MAGIC: &[u8; 4] = b"RBGS";
const VERSION: u32 = 1;
const DEFAULT_PLAYLIST: &str = "RBX-BPM-GRID-TEST";

#[path = "common/score.rs"]
mod score;
use score::{grid_match, offsets, BPM_TOLERANCE, GRID_PASS, MS_TOLERANCE};

/// Under `target/`, not the system temp directory: macOS empties that on its
/// own schedule.
fn cache_dir() -> PathBuf {
    std::env::var("RB_LITE_GOLDEN").map_or_else(
        |_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/golden"),
        PathBuf::from,
    )
}

/// One track, decoded, with what rekordbox said about it.
struct Track {
    id: u64,
    title: String,
    extension: String,
    sample_rate: u32,
    samples: Vec<f32>,
    bpm: f64,
    key: String,
    grid: Vec<Beat>,
}

fn main() {
    // `RUST_LOG=rbl_analysis=debug` prints the tempo stage's decisions.
    if std::env::var("RUST_LOG").is_ok() {
        let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_target(false).without_time().try_init();
    }
    let mode = std::env::args().nth(1).unwrap_or_else(|| "eval".to_owned());
    match mode.as_str() {
        "cache" => cache(),
        "eval" | "score" => eval(),
        "downbeat" => downbeat_experiment(),
        "kick" => kick_experiment(),
        "transition" => transition_experiment(),
        "align" => align_experiment(),
        "key" => key_experiment(),
        "bassroot" => bassroot_experiment(),
        other => println!("unknown mode {other:?}; use `cache` or `eval`"),
    }
}

// ---------------------------------------------------------------- caching

fn cache() {
    let name = std::env::args().nth(2).unwrap_or_else(|| DEFAULT_PLAYLIST.to_owned());
    let out = cache_dir();
    std::fs::create_dir_all(&out).expect("cache dir");

    let db = match rbl_db::Library::open_installed_read_only() {
        Ok(db) => db,
        Err(e) => {
            println!("cannot open library: {e}");
            return;
        }
    };
    let share = db.location().share_root.clone();
    let (library, _stats) = rbl_index::load(&db).expect("index");
    let members = if let Some(selector) = name.strip_prefix("genre:") {
        // `genre:<name>[,<name>…][:<limit>]`: the first `limit` tracks
        // (by library row) of each named genre, matched case-insensitively,
        // whose file is present. For comparing rekordbox's grids on a
        // style of music without building a playlist in the library.
        let (names, limit) = match selector.rsplit_once(':') {
            Some((names, limit)) if limit.parse::<usize>().is_ok() => (names, limit.parse::<usize>().unwrap_or(usize::MAX)),
            _ => (selector, usize::MAX),
        };
        let wanted: Vec<String> = names.split(',').map(|g| g.trim().to_lowercase()).filter(|g| !g.is_empty()).collect();
        let mut taken = vec![0usize; wanted.len()];
        let mut rows = Vec::new();
        for i in 0..library.ids.len() {
            let row = i as rbl_index::Row;
            let genre = library.genre_name(row).to_lowercase();
            let Some(g) = wanted.iter().position(|w| *w == genre) else { continue };
            if taken[g] >= limit || !Path::new(library.folder_path.get(i)).exists() {
                continue;
            }
            taken[g] += 1;
            rows.push(row);
        }
        println!("genres {wanted:?}: {} tracks", rows.len());
        rows
    } else {
        let playlists = library.playlists();
        let Some(index) = (0..playlists.len()).find(|&i| playlists.name(i) == name) else {
            println!("no playlist named {name:?}");
            return;
        };
        let members = playlists.members[index].clone();
        println!("playlist {name:?}: {} tracks", members.len());
        members
    };

    let started = Instant::now();
    let mut done = 0usize;
    let mut skipped = 0usize;
    for &row in &members {
        let i = row as usize;
        let id = library.ids[i];
        let target = out.join(format!("{id}.gold"));
        if target.exists() {
            done += 1;
            continue;
        }
        let path = Path::new(library.folder_path.get(i));
        let dat = rbl_anlz::resolve(&share, library.analysis_path.get(i));
        let Some(grid) = rbl_anlz::Anlz::read(&dat).ok().and_then(|d| d.beat_grid()) else {
            println!("  no grid for {}", library.title.get(i));
            skipped += 1;
            continue;
        };
        let Ok(audio) = rbl_audio::decode_mono(path, None) else {
            println!("  cannot decode {}", path.display());
            skipped += 1;
            continue;
        };
        let track = Track {
            id,
            title: library.title.get(i).to_owned(),
            extension: path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase(),
            sample_rate: audio.sample_rate,
            samples: audio.samples,
            bpm: f64::from(library.bpm_x100[i]) / 100.0,
            key: library.key_name(row).to_owned(),
            grid: grid
                .iter()
                .map(|b| Beat { beat_number: b.beat_number, tempo_x100: b.tempo_x100, time_ms: b.time_ms })
                .collect(),
        };
        write_track(&target, &track);
        done += 1;
        if done % 10 == 0 {
            println!("  … {done} cached ({:.0}s)", started.elapsed().as_secs_f64());
            let _ = std::io::stdout().flush();
        }
    }
    println!("cached {done} tracks in {} ({skipped} skipped)", out.display());
}

fn write_track(path: &Path, track: &Track) {
    let mut bytes = Vec::with_capacity(track.samples.len() * 2 + 1024);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&track.id.to_le_bytes());
    bytes.extend_from_slice(&track.sample_rate.to_le_bytes());
    bytes.extend_from_slice(&track.bpm.to_le_bytes());
    write_str(&mut bytes, &track.title);
    write_str(&mut bytes, &track.extension);
    write_str(&mut bytes, &track.key);
    bytes.extend_from_slice(&(track.grid.len() as u32).to_le_bytes());
    for beat in &track.grid {
        bytes.extend_from_slice(&beat.time_ms.to_le_bytes());
        bytes.extend_from_slice(&beat.beat_number.to_le_bytes());
        bytes.extend_from_slice(&beat.tempo_x100.to_le_bytes());
    }
    bytes.extend_from_slice(&(track.samples.len() as u64).to_le_bytes());
    for &s in &track.samples {
        bytes.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write cache");
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn read_track(path: &Path) -> Option<Track> {
    let bytes = std::fs::read(path).ok()?;
    let mut at = 0usize;
    let take = |at: &mut usize, n: usize| -> Option<&[u8]> {
        let s = bytes.get(*at..*at + n)?;
        *at += n;
        Some(s)
    };
    if take(&mut at, 4)? != MAGIC {
        return None;
    }
    let u32_at = |at: &mut usize| take(at, 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let u16_at = |at: &mut usize| take(at, 2).map(|b| u16::from_le_bytes(b.try_into().unwrap()));
    let u64_at = |at: &mut usize| take(at, 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
    let f64_at = |at: &mut usize| take(at, 8).map(|b| f64::from_le_bytes(b.try_into().unwrap()));
    let str_at = |at: &mut usize| -> Option<String> {
        let n = u32_at(at)? as usize;
        Some(String::from_utf8_lossy(take(at, n)?).into_owned())
    };
    if u32_at(&mut at)? != VERSION {
        return None;
    }
    let id = u64_at(&mut at)?;
    let sample_rate = u32_at(&mut at)?;
    let bpm = f64_at(&mut at)?;
    let title = str_at(&mut at)?;
    let extension = str_at(&mut at)?;
    let key = str_at(&mut at)?;
    let beats = u32_at(&mut at)? as usize;
    let mut grid = Vec::with_capacity(beats);
    for _ in 0..beats {
        let time_ms = u32_at(&mut at)?;
        let beat_number = u16_at(&mut at)?;
        let tempo_x100 = u16_at(&mut at)?;
        grid.push(Beat { beat_number, tempo_x100, time_ms });
    }
    let n = u64_at(&mut at)? as usize;
    let raw = take(&mut at, n * 2)?;
    let samples = raw
        .chunks_exact(2)
        .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32767.0)
        .collect();
    Some(Track { id, title, extension, sample_rate, samples, bpm, key, grid })
}

// ---------------------------------------------------------------- scoring

/// How one track did.
struct Score {
    title: String,
    extension: String,
    rb_bpm: f64,
    our_bpm: f64,
    /// Both grids as tempo runs, for the verbose listing.
    rb_runs: String,
    our_runs: String,
    /// Signed offset of our downbeats from rekordbox's, modulo a bar, in ms.
    downbeat_offset_ms: f64,
    /// Signed offset of our beats from rekordbox's, modulo a beat, in ms.
    beat_offset_ms: f64,
    /// Which of rekordbox's beat numbers our downbeat lands on.
    beat_in_bar: u16,
    grid_matched: f64,
    rb_key: String,
    our_key: String,
    elapsed_ms: u128,
}

impl Score {
    fn bpm_ok(&self) -> bool {
        (self.our_bpm - self.rb_bpm).abs() <= BPM_TOLERANCE
    }
    fn downbeat_ok(&self) -> bool {
        self.downbeat_offset_ms.abs() <= MS_TOLERANCE
    }
    fn grid_ok(&self) -> bool {
        self.grid_matched >= GRID_PASS
    }
    fn key_ok(&self) -> bool {
        self.our_key == self.rb_key
    }
}

/// The options the gate scores: the defaults, or a placement named by
/// `RB_LITE_PLACEMENT` (`envelope`, `attack`).
#[allow(clippy::panic, reason = "a bad CLI flag should fail fast rather than be silently misread")]
fn options_under_test() -> rbl_analysis::AnalysisOptions {
    let mut options = match std::env::var("RB_LITE_PRESET").as_deref() {
        Ok("baseline") => {
            let mut options = rbl_analysis::AnalysisOptions::default();
            options.tempo.max_bpm = 200.0;
            options
        },
        Ok("rekordbox") => rbl_analysis::AnalysisPreset::Rekordbox.options(),
        Ok("rbxport") | Err(_) => rbl_analysis::AnalysisPreset::Rbxport.options(),
        Ok(other) => panic!("Unknown analysis preset {other:?}"),
    };
    if let Some(r) = std::env::var("RB_LITE_ATTACK_REACH").ok().and_then(|v| v.parse::<f64>().ok()) {
        options.attacks.reach_secs = r;
    }
    if let Some((min, max)) = std::env::var("RB_LITE_RANGE").ok().and_then(|v| {
        let (a, b) = v.split_once('-')?;
        Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?))
    }) {
        options.tempo.min_bpm = min;
        options.tempo.max_bpm = max;
    }
    match std::env::var("RB_LITE_PLACEMENT").as_deref() {
        Ok("envelope") => options.tempo.placement = rbl_analysis::tempo::Placement::Envelope,
        Ok("attack") => options.tempo.placement = rbl_analysis::tempo::Placement::Attack,
        _ => {}
    }
    options
}

fn score(track: &Track) -> Score {
    let started = Instant::now();
    let analysis = rbl_analysis::analyse_with(&track.samples, track.sample_rate, options_under_test());
    if std::env::var("RB_LITE_CANDIDATES").is_ok() {
        let onsets = rbl_analysis::onset::onset_envelope(&track.samples, track.sample_rate);
        let table = rbl_analysis::tempo::tempo_candidates(&onsets, rbl_analysis::tempo::TempoOptions::default());
        println!("candidates for {} (rb {:.2}):", track.title, track.bpm);
        for c in table.iter().take(12) {
            println!("  {:>7.2}  acf {:.3}  fourier {:.3}  prior {:.3}  score {:.4}", c.bpm, c.acf, c.fourier, c.prior, c.score);
        }
        for s in &analysis.tempo.segments {
            println!("  segment {:.3}s..{:.3}s: first beat {:.3}s period {:.5}s ({:.3} BPM) beats {}", s.from_secs, s.to_secs, s.start_secs(), s.period_secs, s.bpm(), s.beats());
        }
        // Where the downbeat stage put the bar, and the first beats side by side.
        let beat_secs: Vec<f64> = analysis.tempo.beats.iter().map(|b| f64::from(b.time_ms) / 1000.0).collect();
        let mut halves = Vec::new();
        for pair in beat_secs.windows(2) { halves.push(pair[0]); halves.push((pair[0] + pair[1]) / 2.0); }
        let profiles = rbl_analysis::downbeat::beat_profiles(&track.samples, track.sample_rate, &halves);
        let scores = rbl_analysis::downbeat::position_scores(&profiles, 8, &[8, 16, 32, 64]);
        println!("  half-beat position scores (from our first beat): {}", scores.iter().map(|v| format!("{v:.3}")).collect::<Vec<_>>().join(" "));
        let ours: Vec<String> = analysis.tempo.beats.iter().take(6).map(|b| format!("{}@{}", b.beat_number, b.time_ms)).collect();
        let theirs: Vec<String> = track.grid.iter().take(6).map(|b| format!("{}@{}", b.beat_number, b.time_ms)).collect();
        println!("  first beats ours {} | rb {}", ours.join(" "), theirs.join(" "));
        // The incoming grid's strength per bar across each tempo change, to
        // see where rekordbox places the switch relative to the drop.
        if analysis.tempo.segments.len() > 1 {
            let onsets = rbl_analysis::onset::onset_envelope(&track.samples, track.sample_rate);
            for pair in analysis.tempo.segments.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let rb_change = track.grid.windows(2).find(|w| w[0].tempo_x100 != w[1].tempo_x100).map_or(0.0, |w| f64::from(w[1].time_ms) / 1000.0);
                println!("  change ours {:.3}s rb {:.3}s; incoming grid ({:.2} BPM) strength per bar, and outgoing ({:.2} BPM):", b.from_secs, rb_change, b.bpm(), a.bpm());
                let strength = |seg: rbl_analysis::tempo::Segment, t: f64| -> f64 {
                    let x = (t - onsets.origin_secs) * onsets.rate;
                    let reach = seg.period_secs * onsets.rate * 0.2;
                    let lo = (x - reach).max(0.0) as usize;
                    let hi = ((x + reach) as usize).min(onsets.len().saturating_sub(1));
                    (lo..=hi).map(|i| onsets.values[i] as f64).fold(0.0, f64::max)
                };
                let from = (b.from_secs - 40.0).max(0.0);
                let to = (b.from_secs + 130.0).min(onsets.time_of(onsets.len() as f64));
                let mut line = String::new();
                let mut t = b.phase_secs + ((from - b.phase_secs) / b.period_secs).ceil() * b.period_secs;
                while t < to {
                    let bar: f64 = (0..4).map(|k| strength(b, t + k as f64 * b.period_secs)).sum::<f64>() / 4.0;
                    let bar_a: f64 = (0..4).map(|k| strength(a, t + k as f64 * a.period_secs)).sum::<f64>() / 4.0;
                    line.push_str(&format!(" {:.1}:{:.2}/{:.2}", t, bar, bar_a));
                    t += 4.0 * b.period_secs;
                }
                println!("   {line}");
            }
        }
        let attacks = rbl_analysis::attack::AttackMap::new(&track.samples, track.sample_rate, rbl_analysis::attack::AttackOptions::default());
        for (name, map) in [("envelope", None), ("attacks", Some(&attacks))] {
            let report = rbl_analysis::tempo::fit_report(&onsets, map, table.first().map_or(120.0, |c| c.bpm), rbl_analysis::tempo::TempoOptions::default());
            for (half, (passes, support)) in report.iter().enumerate() {
                println!("  fit on {name} from {}: comb {:.4} then {} (support {support:.1})", if half == 0 { "the beat" } else { "the half beat" }, passes[0], passes[1..].iter().map(|b| format!("{b:.4}")).collect::<Vec<_>>().join(" -> "));
            }
        }
        if analysis.tempo.segments.len() > 1 {
            let attacks = rbl_analysis::attack::AttackMap::new(&track.samples, track.sample_rate, rbl_analysis::attack::AttackOptions::default());
            let (a, b) = (analysis.tempo.segments[0], analysis.tempo.segments[analysis.tempo.segments.len() - 1]);
            let (walk, why) = rbl_analysis::tempo::walk_report(&onsets, Some(&attacks), a.bpm(), b.bpm(), (b.from_secs - 45.0).max(0.0), b.from_secs + 30.0);
            let line: Vec<String> = walk.iter().map(|(t, bpm)| format!("{t:.2}:{bpm:.1}")).collect();
            println!("  walk from {:.1}s ({why}): {}", (b.from_secs - 45.0).max(0.0), line.join(" "));
        }
        let windows = rbl_analysis::tempo::local_tempos(&onsets, table.first().map_or(120.0, |c| c.bpm), rbl_analysis::tempo::TempoOptions::default());
        let line: Vec<String> = windows.iter().map(|(t, r)| format!("{:.0}s:{}", t, r.map_or("-".to_owned(), |r| format!("{r:.3}")))).collect();
        println!("  windows: {}", line.join(" "));
    }
    let elapsed_ms = started.elapsed().as_millis();
    let ours = &analysis.tempo.beats;
    let rb = &track.grid;

    let (downbeat_offset_ms, beat_offset_ms, beat_in_bar) = offsets(ours, rb);
    let grid_matched = grid_match(ours, rb);

    Score {
        title: track.title.clone(),
        extension: track.extension.clone(),
        rb_bpm: track.bpm,
        our_bpm: analysis.tempo.bpm,
        downbeat_offset_ms,
        beat_offset_ms,
        beat_in_bar,
        grid_matched,
        rb_runs: score::runs_line(rb),
        our_runs: format!("{}\n    drift (ours - rb, ms, at ten points): {}", score::runs_line(ours), drift_line(ours, rb)),
        rb_key: track.key.clone(),
        our_key: analysis.key.map(|k| k.name).unwrap_or_default(),
        elapsed_ms,
    }
}

// ---------------------------------------------------------------- eval

fn eval() {
    let filter = std::env::args().nth(2).map(|s| s.to_lowercase());
    let dir = cache_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "gold")).collect())
        .unwrap_or_default();
    paths.sort();
    if paths.is_empty() {
        println!("nothing cached in {}; run `cache` first", dir.display());
        return;
    }

    let started = Instant::now();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(12);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let scores = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(path) = paths.get(i) else { break };
                let Some(track) = read_track(path) else { continue };
                if let Some(f) = &filter {
                    if !track.title.to_lowercase().contains(f.as_str()) {
                        continue;
                    }
                }
                let s = score(&track);
                scores.lock().unwrap().push(s);
            });
        }
    });
    let mut scores = scores.into_inner().unwrap();
    scores.sort_by(|a, b| a.title.cmp(&b.title));
    let n = scores.len();
    if n == 0 {
        println!("no track matched");
        return;
    }

    let verbose = filter.is_some() || std::env::var("RB_LITE_VERBOSE").is_ok();
    println!("{:<52} {:>7} {:>7} | {:>7} {:>6} b | {:>5} | {:<4} {:<4} | ms", "title", "rb", "ours", "down", "beat", "grid", "rb", "ours");
    for s in &scores {
        let failed = !s.bpm_ok() || !s.downbeat_ok() || !s.grid_ok() || !s.key_ok();
        if !failed && !verbose {
            continue;
        }
        println!(
            "{:<52} {:>7.2} {:>7.2}{} | {:>+7.1}{} {:>+6.1} {} | {:>4.0}%{} | {:<4} {:<4}{} | {} {}",
            truncate(&s.title, 52),
            s.rb_bpm,
            s.our_bpm,
            if s.bpm_ok() { " " } else { "!" },
            s.downbeat_offset_ms,
            if s.downbeat_ok() { " " } else { "!" },
            s.beat_offset_ms,
            s.beat_in_bar,
            s.grid_matched * 100.0,
            if s.grid_ok() { " " } else { "!" },
            s.rb_key,
            s.our_key,
            if s.key_ok() { " " } else { "!" },
            s.elapsed_ms,
            s.extension,
        );
        if verbose && (s.rb_runs.contains('|') || s.our_runs.contains('|')) {
            println!("    rb   runs: {}", s.rb_runs);
            println!("    ours runs: {}", s.our_runs);
        }
    }

    // Every failure by metric, so a run reads as a to-do list.
    let failures = |name: &str, f: fn(&Score) -> bool| {
        let failed: Vec<&str> = scores.iter().filter(|s| !f(s)).map(|s| s.title.as_str()).collect();
        if !failed.is_empty() {
            println!("\n{name} failures ({}):", failed.len());
            for title in failed {
                println!("  {}", truncate(title, 70));
            }
        }
    };
    failures("bpm", Score::bpm_ok);
    failures("downbeat", Score::downbeat_ok);
    failures("grid", Score::grid_ok);
    let count = |f: fn(&Score) -> bool| scores.iter().filter(|s| f(s)).count();
    let line = |name: &str, ok: usize| {
        println!("  {name:<9} {ok:>3} / {n}  ({:.1}%){}", ok as f64 / n as f64 * 100.0, if ok as f64 / n as f64 >= 0.99 { "" } else { "  <-- below 99%" });
    };
    println!("\n== {n} tracks, {:.1}s ==", started.elapsed().as_secs_f64());
    line("bpm", count(Score::bpm_ok));
    line("downbeat", count(Score::downbeat_ok));
    line("grid", count(Score::grid_ok));
    line("key", count(Score::key_ok));
    // A tempo change rekordbox's grid does not have is a grid that is wrong
    // after it, whatever the bpm column says.
    let changes = |runs: &str| runs.lines().next().is_some_and(|l| l.contains('|'));
    let extra: Vec<&str> = scores.iter().filter(|s| changes(&s.our_runs) && !changes(&s.rb_runs)).map(|s| s.title.as_str()).collect();
    println!("  tempo changes rekordbox's grid does not have: {} / {n}", extra.len());
    if verbose {
        for title in extra {
            println!("    {}", truncate(title, 70));
        }
    }
    let mut offsets: Vec<f64> = scores.iter().filter(|s| s.downbeat_ok()).map(|s| s.downbeat_offset_ms).collect();
    offsets.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if !offsets.is_empty() {
        let pct = |p: f64| offsets[((offsets.len() as f64 * p) as usize).min(offsets.len() - 1)];
        println!("  downbeat offset among passes: p10 {:+.1}  p50 {:+.1}  p90 {:+.1} ms", pct(0.1), pct(0.5), pct(0.9));
    }
    let total_ms: u128 = scores.iter().map(|s| s.elapsed_ms).sum();
    println!("  analysis time: mean {} ms per track", total_ms / n as u128);
}

/// The offset of our nearest beat from rekordbox's, in ms, at ten of
/// rekordbox's beats spread over the track, with the beat numbers where
/// they differ: where a grid drifts or a change lands a beat off.
fn drift_line(ours: &[Beat], rb: &[Beat]) -> String {
    if rb.is_empty() || ours.is_empty() {
        return String::new();
    }
    (0..10)
        .map(|k| {
            let beat = &rb[(rb.len() - 1) * k / 9];
            let t = f64::from(beat.time_ms);
            let nearest = ours.iter().min_by(|a, b| (f64::from(a.time_ms) - t).abs().partial_cmp(&(f64::from(b.time_ms) - t).abs()).unwrap()).unwrap();
            let numbers = if nearest.beat_number == beat.beat_number { String::new() } else { format!("({}≠{})", nearest.beat_number, beat.beat_number) };
            format!("{:.0}s:{:+.0}{numbers}", t / 1000.0, f64::from(nearest.time_ms) - t)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_owned() } else { s.chars().take(n - 1).collect::<String>() + "…" }
}

// ---------------------------------------------------------------- experiments

/// On rekordbox's own grid, does the novelty pick rekordbox's downbeat out
/// of the eight half-beat positions in the bar? This separates the downbeat
/// stage from the grid it is normally given.
fn downbeat_experiment() {
    use rbl_analysis::downbeat::{beat_profiles, position_scores};
    let dir = cache_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "gold")).collect())
        .unwrap_or_default();
    paths.sort();
    let (mut n, mut ok) = (0usize, 0usize);
    for path in &paths {
        let Some(track) = read_track(path) else { continue };
        // Rekordbox's grid from its first downbeat, so position 0 is the
        // truth, on half beats.
        let Some(first_down) = track.grid.iter().position(|b| b.beat_number == 1) else { continue };
        let beats: Vec<f64> = track.grid[first_down..].iter().map(|b| f64::from(b.time_ms) / 1000.0).collect();
        let mut halves: Vec<f64> = Vec::with_capacity(beats.len() * 2);
        for pair in beats.windows(2) {
            halves.push(pair[0]);
            halves.push((pair[0] + pair[1]) / 2.0);
        }
        let profiles = beat_profiles(&track.samples, track.sample_rate, &halves);
        let scores = position_scores(&profiles, 8, &[8, 16, 32, 64]);
        let best = scores.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).map_or(0, |(i, _)| i);
        n += 1;
        if best == 0 {
            ok += 1;
        } else {
            let fmt = scores.iter().map(|v| format!("{v:.3}")).collect::<Vec<_>>().join(" ");
            println!("{:<50} picked {best}: {fmt}", truncate(&track.title, 50));
        }
    }
    println!("downbeat right on rekordbox's grid: {ok} / {n}");
}

/// Measures the key rules against rekordbox's key names: the profile match
/// alone, then each rule set, with what every rule fired on, fixed and
/// broke; and a search over the `BassRoot` knobs.
fn key_experiment() {
    use rbl_analysis::key::{gather_evidence, judge, BassSource, FrontEnd, KeyEvidence, KeyOptions, Profile, Rule, DEFAULT_RULES};
    let dir = cache_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "gold")).collect())
        .unwrap_or_default();
    paths.sort();
    let options = KeyOptions::default();
    let edges = [30.0, 45.0, 90.0];
    use rbl_analysis::key::EdmkeyOptions;
    let e = EdmkeyOptions::DEFAULT;
    let fronts = [
        FrontEnd::Chroma,
        FrontEnd::Edmkey(e),
        FrontEnd::Edmkey(EdmkeyOptions { min_hz: 55.0, ..e }),
        FrontEnd::Edmkey(EdmkeyOptions { min_hz: 80.0, ..e }),
        FrontEnd::Edmkey(EdmkeyOptions { tilt: false, ..e }),
        FrontEnd::Edmkey(EdmkeyOptions { min_hz: 55.0, tilt: false, ..e }),
        FrontEnd::Edmkey(EdmkeyOptions { whitening: false, ..e }),
        FrontEnd::Edmkey(EdmkeyOptions { min_hz: 55.0, whitening: false, ..e }),
        FrontEnd::Edmkey(EdmkeyOptions { min_hz: 55.0, gate: 0.0, ..e }),
    ];
    let front_names = ["chroma", "edmkey", "edmkey 55Hz", "edmkey 80Hz", "edmkey no tilt", "edmkey 55Hz no tilt", "edmkey no whitening", "edmkey 55Hz no whitening", "edmkey 55Hz no gate"];

    // Per track: rekordbox's key and, per front end, the evidence for each
    // edge length, with the bass read against the grid our own analysis
    // finds.
    struct Row { rb: String, title: String, evidence: Vec<Vec<KeyEvidence>> }
    let started = Instant::now();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(12);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let rows = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(path) = paths.get(i) else { break };
                let Some(track) = read_track(path) else { continue };
                let analysis = rbl_analysis::analyse(&track.samples, track.sample_rate);
                let grid = key_grid_of(&track, &analysis);
                let evidence: Vec<Vec<KeyEvidence>> = fronts.iter().map(|&front_end| {
                    edges.iter().filter_map(|&secs| {
                        let rules = [Rule::BassRoot { margin: 1.0, source: BassSource::Edges { secs } }];
                        gather_evidence(&track.samples, track.sample_rate, KeyOptions { front_end, ..options }, &rules, &grid)
                    }).collect()
                }).collect();
                if evidence.iter().all(|e| e.len() == edges.len()) {
                    rows.lock().unwrap().push(Row { rb: track.key.clone(), title: track.title.clone(), evidence });
                }
            });
        }
    });
    let rows = rows.into_inner().unwrap();
    let n = rows.len();
    println!("evidence for {n} tracks in {:.1}s", started.elapsed().as_secs_f64());

    // A rule set's score with one front end and profile, and what each
    // rule did.
    let score_with = |front: usize, profile: Profile, rules: &[Rule], edge_index: usize, verbose: bool| -> usize {
        let options = KeyOptions { front_end: fronts[front], profile, ..options };
        let mut exact = 0usize;
        let mut per_rule: Vec<(usize, usize, usize)> = vec![(0, 0, 0); rules.len()]; // fired, fixed, broke
        for row in &rows {
            let Some(report) = judge(&row.evidence[front][edge_index], options, rules) else { continue };
            if report.key.name == row.rb { exact += 1; }
            let name_of = |v: rbl_analysis::key::Verdict| if v.minor { MINORS[v.tonic] } else { MAJORS[v.tonic] };
            for applied in &report.applied {
                let index = rules.iter().position(|r| *r == applied.rule).unwrap_or(0);
                per_rule[index].0 += 1;
                let was_right = name_of(applied.before) == row.rb;
                let is_right = name_of(applied.after) == row.rb;
                if !was_right && is_right { per_rule[index].1 += 1; }
                if was_right && !is_right { per_rule[index].2 += 1; }
                if verbose && was_right != is_right {
                    println!("    {:<44} rb {:<4} {:?}: {} -> {}  {}", truncate(&row.title, 44), row.rb, applied.rule, name_of(applied.before), name_of(applied.after), if is_right { "fixed" } else { "BROKE" });
                }
            }
        }
        if verbose {
            for (rule, (fired, fixed, broke)) in rules.iter().zip(&per_rule) {
                println!("  {rule:?}: fired {fired}, fixed {fixed}, broke {broke}");
            }
        }
        exact
    };
    let shipped_front = fronts.iter().position(|f| *f == options.front_end).unwrap_or(0);
    let score = |rules: &[Rule], edge_index: usize, verbose: bool| score_with(shipped_front, options.profile, rules, edge_index, verbose);

    // Front end × profile × PreferMinor bias.
    println!("front end × profile × PreferMinor (exact of {n}):");
    let profiles = [("edma", Profile::EDMA), ("bgate", Profile::BGATE), ("braw", Profile::BRAW), ("edmm", Profile::EDMM), ("shaath", Profile::SHAATH), ("krumhansl", Profile::KRUMHANSL)];
    let mut table: Vec<(usize, String)> = Vec::new();
    for (fi, fname) in front_names.iter().enumerate() {
        for (pname, profile) in profiles {
            for bias in [0.0, 0.1, 0.2, 0.3, 0.4, 0.5] {
                let rules: Vec<Rule> = if bias > 0.0 { vec![Rule::PreferMinor { bias }] } else { Vec::new() };
                table.push((score_with(fi, profile, &rules, 1, false), format!("{fname:<26} {pname:<9} bias {bias:.1}")));
            }
        }
    }
    table.sort_by_key(|r| std::cmp::Reverse(r.0));
    for (exact, name) in table.iter().take(24) {
        println!("  {exact:>3}  {name}");
    }
    // The best per front end, so a front end that never tops the table is
    // still seen.
    println!("best per front end:");
    for name in front_names {
        let prefix = format!("{name:<26} ");
        let best = table.iter().filter(|(_, n)| n.starts_with(&prefix)).max_by_key(|r| r.0);
        if let Some((exact, n)) = best { println!("  {exact:>3}  {n}"); }
    }

    println!("profile match alone: {} / {n}", score(&[], 1, false));
    println!("PreferMinor 0.3 alone: {} / {n}", score(&[Rule::PreferMinor { bias: 0.3 }], 1, false));
    println!("shipped rules ({DEFAULT_RULES:?}): {} / {n}", score(DEFAULT_RULES, 1, true));

    println!("BassRoot search (after PreferMinor 0.3):");
    let mut results: Vec<(usize, String)> = Vec::new();
    for (ei, secs) in edges.iter().enumerate() {
        let sources = [
            BassSource::Edges { secs: *secs }, BassSource::Downbeats, BassSource::EdgesAndDownbeats { secs: *secs },
            BassSource::OffBeats, BassSource::SecondEighth, BassSource::PhraseStart { bars: 4 }, BassSource::Whole,
        ];
        for source in sources {
            if ei > 0 && !matches!(source, BassSource::Edges { .. } | BassSource::EdgesAndDownbeats { .. }) {
                continue;
            }
            for margin in [0.01, 0.02, 0.05, 0.1, 0.15, 0.2, 0.3, 1.0] {
                let rules = [Rule::PreferMinor { bias: 0.3 }, Rule::BassRoot { margin, source }];
                results.push((score(&rules, ei, false), format!("margin {margin:.2} {source:?}")));
            }
        }
    }
    for source in [BassSource::SecondEighth, BassSource::PhraseStart { bars: 4 }, BassSource::OffBeats, BassSource::Whole, BassSource::Edges { secs: 45.0 }] {
        for weight in [0.05, 0.1, 0.15, 0.2, 0.3, 0.5] {
            let rules = [Rule::PreferMinor { bias: 0.3 }, Rule::BassVote { weight, source }];
            results.push((score(&rules, 1, false), format!("vote {weight:.2} {source:?}")));
            let rules = [Rule::BassVote { weight, source }, Rule::PreferMinor { bias: 0.3 }];
            results.push((score(&rules, 1, false), format!("vote {weight:.2} {source:?} before PreferMinor")));
        }
    }
    results.sort_by_key(|r| std::cmp::Reverse(r.0));
    for (exact, name) in results.iter().take(16) {
        println!("  {exact:>3} / {n}  {name}");
    }
    let second: Vec<&(usize, String)> = results.iter().filter(|(_, n)| n.contains("SecondEighth") || n.contains("PhraseStart")).take(8).collect();
    for (exact, name) in second { println!("  {exact:>3} / {n}  {name}"); }
    println!("failures of the shipped rules:");
    let mut kinds: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for row in &rows {
        let Some(report) = judge(&row.evidence[shipped_front][1], options, DEFAULT_RULES) else { continue };
        let ours = report.key.name;
        if ours == row.rb { continue; }
        let kind = if relative(&ours) == row.rb { "relative" } else if a_fifth_away(&ours, &row.rb) { "fifth" } else if parallel(&ours) == row.rb { "parallel" } else { "other" };
        *kinds.entry(kind).or_default() += 1;
        println!("  {:<46} rb {:<4} ours {:<4} {kind}", truncate(&row.title, 46), row.rb, ours);
    }
    println!("  {kinds:?}");
}

/// The grid a track's analysis gives the key rules.
fn key_grid_of(track: &Track, analysis: &rbl_analysis::Analysis) -> rbl_analysis::key::KeyGrid {
    let beat_secs: Vec<f64> = analysis.tempo.beats.iter().map(|b| f64::from(b.time_ms) / 1000.0).collect();
    let phase = rbl_analysis::downbeat::grid_phase(&track.samples, track.sample_rate, &beat_secs);
    rbl_analysis::key::KeyGrid {
        beats: analysis.tempo.beats.iter().map(|b| (f64::from(b.time_ms) / 1000.0, b.beat_number)).collect(),
        phrase_starts: phase.phrase_starts,
    }
}

const MAJORS: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
const MINORS: [&str; 12] = ["Cm", "Dbm", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "Abm", "Am", "Bbm", "Bm"];

/// The relative major of a minor key, or the relative minor of a major key.
fn relative(name: &str) -> String {
    if let Some(i) = MINORS.iter().position(|k| *k == name) {
        return MAJORS[(i + 3) % 12].to_owned();
    }
    if let Some(i) = MAJORS.iter().position(|k| *k == name) {
        return MINORS[(i + 9) % 12].to_owned();
    }
    String::new()
}

/// The same tonic in the other mode.
fn parallel(name: &str) -> String {
    if let Some(i) = MINORS.iter().position(|k| *k == name) {
        return MAJORS[i].to_owned();
    }
    if let Some(i) = MAJORS.iter().position(|k| *k == name) {
        return MINORS[i].to_owned();
    }
    String::new()
}

/// Same mode, tonic a fifth up or down.
fn a_fifth_away(a: &str, b: &str) -> bool {
    let index = |name: &str| -> Option<(usize, bool)> {
        MINORS.iter().position(|k| *k == name).map(|i| (i, true))
            .or_else(|| MAJORS.iter().position(|k| *k == name).map(|i| (i, false)))
    };
    match (index(a), index(b)) {
        (Some((ia, ma)), Some((ib, mb))) => ma == mb && ((ia + 7) % 12 == ib || (ib + 7) % 12 == ia),
        _ => false,
    }
}

/// Does the strongest pitch class of the bass name rekordbox's tonic? For
/// several bands and windows: the whole track, the first and last 45 s,
/// the first frame of each bar, and the frames between beats.
fn bassroot_experiment() {
    use rbl_analysis::key::{chroma_frames, fold_frames, KeyOptions, BINS};
    let dir = cache_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "gold")).collect())
        .unwrap_or_default();
    paths.sort();
    let bands: Vec<(&str, f64, f64, usize)> = vec![
        ("40-120 h1", 40.0, 120.0, 1), ("55-250 h1", 55.0, 250.0, 1), ("80-250 h1", 80.0, 250.0, 1),
        ("55-500 h2", 55.0, 500.0, 2), ("80-400 h2", 80.0, 400.0, 2), ("40-250 h1", 40.0, 250.0, 1),
    ];
    let windows = ["whole", "edges 45s", "downbeats", "off-beats", "edges+downbeats", "2nd eighth", "phrase 2 bars", "phrase 4 bars", "phrase 8 bars"];
    let tonic_of = |name: &str| -> Option<usize> {
        MINORS.iter().position(|k| *k == name).or_else(|| MAJORS.iter().position(|k| *k == name))
    };
    let counts = std::sync::Mutex::new(vec![vec![(0usize, 0usize); windows.len()]; bands.len()]); // (argmax hit, top-2 hit)
    let phrase_counts = std::sync::Mutex::new((0usize, 0usize)); // tracks, phrase starts
    let n = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(12);
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(path) = paths.get(i) else { break };
                let Some(track) = read_track(path) else { continue };
                let Some(rb_tonic) = tonic_of(&track.key) else { continue };
                let analysis = rbl_analysis::analyse(&track.samples, track.sample_rate);
                let beats: Vec<f64> = analysis.tempo.beats.iter().map(|b| f64::from(b.time_ms) / 1000.0).collect();
                let downbeats: Vec<f64> = analysis.tempo.beats.iter().filter(|b| b.beat_number == 1).map(|b| f64::from(b.time_ms) / 1000.0).collect();
                let phrase_starts = key_grid_of(&track, &analysis).phrase_starts;
                let bar_secs = beats.windows(2).map(|w| w[1] - w[0]).next().unwrap_or(0.5) * 4.0;
                let hop_secs = 4096.0 / f64::from(track.sample_rate);
                n.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                { let mut pc = phrase_counts.lock().unwrap(); pc.0 += 1; pc.1 += phrase_starts.len(); }
                for (bi, (_, lo, hi, h)) in bands.iter().enumerate() {
                    let options = KeyOptions { low_hz: *lo, high_hz: *hi, harmonics: *h, ..KeyOptions::default() };
                    let Some(frames) = chroma_frames(&track.samples, track.sample_rate, options) else { continue };
                    let total_secs = frames.len() as f64 * hop_secs;
                    let pick = |keep: &dyn Fn(usize) -> bool| -> Vec<[f64; BINS]> {
                        frames.iter().enumerate().filter(|(i, _)| keep(*i)).map(|(_, f)| *f).collect()
                    };
                    let frame_of = |t: f64| (t / hop_secs).floor().max(0.0) as usize;
                    let down: std::collections::HashSet<usize> = downbeats.iter().map(|&t| frame_of(t)).collect();
                    let on_beat: std::collections::HashSet<usize> = beats.iter().map(|&t| frame_of(t)).collect();
                    let centre = |i: usize| i as f64 * hop_secs + 8192.0 / 2.0 / f64::from(track.sample_rate);
                    let second_eighth = |i: usize| {
                        let t = centre(i);
                        let b = beats.partition_point(|&x| x <= t);
                        b > 0 && b < beats.len() && t >= (beats[b - 1] + beats[b]) / 2.0
                    };
                    let sets: Vec<Vec<[f64; BINS]>> = vec![
                        frames.clone(),
                        pick(&|i| { let t = i as f64 * hop_secs; t < 45.0 || t >= total_secs - 45.0 }),
                        pick(&|i| down.contains(&i)),
                        pick(&|i| !on_beat.contains(&i)),
                        pick(&|i| { let t = i as f64 * hop_secs; down.contains(&i) || t < 45.0 || t >= total_secs - 45.0 }),
                        pick(&second_eighth),
                        pick(&|i| second_eighth(i) && phrase_starts.iter().any(|&s| centre(i) >= s && centre(i) < s + 2.0 * bar_secs)),
                        pick(&|i| second_eighth(i) && phrase_starts.iter().any(|&s| centre(i) >= s && centre(i) < s + 4.0 * bar_secs)),
                        pick(&|i| second_eighth(i) && phrase_starts.iter().any(|&s| centre(i) >= s && centre(i) < s + 8.0 * bar_secs)),
                    ];
                    for (wi, set) in sets.iter().enumerate() {
                        let chroma = fold_frames(set, false, 0);
                        let mut order: Vec<usize> = (0..12).collect();
                        order.sort_by(|a, b| chroma[*b].partial_cmp(&chroma[*a]).unwrap());
                        let mut c = counts.lock().unwrap();
                        if order[0] == rb_tonic { c[bi][wi].0 += 1; }
                        if order[0] == rb_tonic || order[1] == rb_tonic { c[bi][wi].1 += 1; }
                    }
                }
            });
        }
    });
    let n = n.load(std::sync::atomic::Ordering::Relaxed);
    let counts = counts.into_inner().unwrap();
    let pc = phrase_counts.into_inner().unwrap();
    println!("phrase starts found: {:.1} per track", pc.1 as f64 / pc.0.max(1) as f64);
    println!("strongest bass class == rekordbox tonic (top-1 / top-2) of {n}:");
    print!("{:<12}", "band");
    for w in &windows { print!(" {w:>16}"); }
    println!();
    for (row, (name, ..)) in counts.iter().zip(&bands) {
        print!("{name:<12}");
        for (hit, top2) in row { print!(" {hit:>7} / {top2:<6}"); }
        println!();
    }
}

/// Does the kick-attack detector put more attack on rekordbox's beats than
/// on the midpoints between them? One line per track that says no, and
/// the count.
fn kick_experiment() {
    use rbl_analysis::attack::{AttackMap, AttackOptions};
    let dir = cache_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "gold")).collect())
        .unwrap_or_default();
    paths.sort();
    let (mut n, mut wrong) = (0usize, 0usize);
    for path in &paths {
        let Some(track) = read_track(path) else { continue };
        let map = AttackMap::new(&track.samples, track.sample_rate, AttackOptions::default());
        let period = 60.0 / track.bpm;
        let strength = |offset: f64| -> (f64, usize) {
            let mut total = 0.0;
            let mut hits = 0usize;
            for b in &track.grid {
                if let Some(a) = map.attack_within(f64::from(b.time_ms) / 1000.0 + offset, 0.03) {
                    total += a.height;
                    hits += 1;
                }
            }
            (total, hits)
        };
        let (on, on_hits) = strength(0.0);
        let (off, off_hits) = strength(period / 2.0);
        n += 1;
        let ratio = on / off.max(1e-9);
        let watched = std::env::var("RB_LITE_KICK_SHOW").map(|w| track.title.to_lowercase().contains(&w.to_lowercase())).unwrap_or(false);
        if ratio < 1.0 || watched {
            if ratio < 1.0 { wrong += 1; }
            println!("  {:<46} on {on:.2} ({on_hits}) off {off:.2} ({off_hits}) ratio {ratio:.2}", truncate(&track.title, 46));
        }
        if watched {
            // The same on our own grid.
            let analysis = rbl_analysis::analyse_with(&track.samples, track.sample_rate, options_under_test());
            let ours: Vec<f64> = analysis.tempo.beats.iter().map(|b| f64::from(b.time_ms) / 1000.0).collect();
            let strength_ours = |offset: f64| -> (f64, usize) {
                let mut total = 0.0; let mut hits = 0usize;
                for &t in &ours { if let Some(a) = map.attack_within(t + offset, 0.03) { total += a.height; hits += 1; } }
                (total, hits)
            };
            let (o, oh) = strength_ours(0.0);
            let (f, fh) = strength_ours(period / 2.0);
            println!("    on our grid: beats {o:.2} ({oh}) midpoints {f:.2} ({fh}) ratio {:.2}", o / f.max(1e-9));
        }
    }
    println!("midpoints have more kick than rekordbox's beats on {wrong} / {n}");
}

/// What the audio holds around every hand-gridded tempo change, per beat of
/// rekordbox's grid: the kick band (onset flux under 200 Hz), the full-band
/// flux, the click-band attack, and the flux on the half beat after — so
/// the rule for placing a change is read off the material rather than
/// guessed. `transition <filter>`; `RB_LITE_BARS` bars either side (12).
fn transition_experiment() {
    use rbl_analysis::onset::{onset_envelope_band, Band};
    use rbl_analysis::attack::{AttackMap, AttackOptions};
    let filter = std::env::args().nth(2).map(|s| s.to_lowercase()).unwrap_or_default();
    let bars: i64 = std::env::var("RB_LITE_BARS").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
    let dir = cache_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "gold")).collect())
        .unwrap_or_default();
    paths.sort();
    for path in &paths {
        let Some(track) = read_track(path) else { continue };
        if !track.title.to_lowercase().contains(&filter) { continue; }
        let full = onset_envelope_band(&track.samples, track.sample_rate, Band::FULL);
        let low = onset_envelope_band(&track.samples, track.sample_rate, Band::LOW);
        let attacks = AttackMap::new(&track.samples, track.sample_rate, AttackOptions::default());
        let thumps = AttackMap::new(&track.samples, track.sample_rate, AttackOptions { low_hz: 30.0, high_hz: 200.0, step_secs: 0.002, reach_secs: 0.04 });
        let analysis = rbl_analysis::analyse_with(&track.samples, track.sample_rate, options_under_test());
        let peak = |env: &rbl_analysis::onset::OnsetEnvelope, secs: f64, reach_secs: f64| -> f64 {
            let x = (secs - env.origin_secs) * env.rate;
            let r = reach_secs * env.rate;
            let lo = (x - r).max(0.0) as usize;
            let hi = ((x + r) as usize).min(env.len().saturating_sub(1));
            (lo..=hi).map(|i| f64::from(env.values[i])).fold(0.0, f64::max)
        };
        // `RB_LITE_PEAKS=kick|flux|attack,from,to[,min]` lists every peak
        // of that band in the range, with its time.
        if let Some((band, from, to, min)) = std::env::var("RB_LITE_PEAKS").ok().and_then(|v| {
            let f: Vec<&str> = v.split(',').collect();
            Some((f.first()?.to_string(), f.get(1)?.parse::<f64>().ok()?, f.get(2)?.parse::<f64>().ok()?, f.get(3).and_then(|m| m.parse::<f64>().ok()).unwrap_or(0.1)))
        }) {
            let mut line = String::new();
            if band == "attack" {
                let mut t = from;
                while t < to {
                    if let Some(a) = attacks.attack_within(t, 0.005) {
                        if a.height >= min { line.push_str(&format!(" {:.3}:{:.2}", a.secs, a.height)); }
                    }
                    t += 0.01;
                }
            } else {
                let env = if band == "kick" { &low } else { &full };
                let lo = ((from - env.origin_secs) * env.rate).max(0.0) as usize;
                let hi = (((to - env.origin_secs) * env.rate) as usize).min(env.len().saturating_sub(2));
                for i in lo.max(1)..hi {
                    let v = f64::from(env.values[i]);
                    if v >= min && v > f64::from(env.values[i - 1]) && v >= f64::from(env.values[i + 1]) {
                        line.push_str(&format!(" {:.3}:{:.2}", env.time_of(i as f64), v));
                    }
                }
            }
            println!("{band} peaks {from}..{to} in {}:{line}", track.title);
        }
        println!("{}  (rb {:.2}; our changes at {})", track.title, track.bpm,
            analysis.tempo.segments.iter().skip(1).map(|s| format!("{:.3}s->{:.2}", s.from_secs, s.bpm())).collect::<Vec<_>>().join(", "));
        let changes: Vec<usize> = track.grid.windows(2).enumerate().filter(|(_, w)| w[0].tempo_x100 != w[1].tempo_x100).map(|(i, _)| i + 1).collect();
        // `RB_LITE_BGRID=1`: rekordbox's incoming grid extended back over
        // the zone, per bar: kick, attack, flux, half-beat flux means and
        // how many of the four beats have any onset within reach.
        if std::env::var("RB_LITE_BGRID").is_ok() {
            for &c in &changes {
                let b = track.grid[c];
                let period = 60.0 / (f64::from(b.tempo_x100) / 100.0);
                let cut = f64::from(b.time_ms) / 1000.0;
                let a_period = 60.0 / (f64::from(track.grid[c - 1].tempo_x100) / 100.0);
                let a_cut = f64::from(track.grid[c - 1].time_ms) / 1000.0;
                println!("  b-grid bars around the cut at {cut:.3}s ({:.2} -> {:.2}): start | kick attack flux half | beats-with-onset | old grid's kick attack flux", f64::from(track.grid[c - 1].tempo_x100) / 100.0, f64::from(b.tempo_x100) / 100.0);
                for bar in -(bars)..8 {
                    let t0 = cut + bar as f64 * 4.0 * period;
                    if t0 < 0.0 { continue; }
                    let (mut k, mut a, mut f, mut h, mut n) = (0.0, 0.0, 0.0, 0.0, 0usize);
                    let mut th = 0.0;
                    let (mut ka, mut aa, mut fa) = (0.0, 0.0, 0.0);
                    for i in 0..4 {
                        let t = t0 + i as f64 * period;
                        let reach = period * 0.1;
                        let kk = peak(&low, t, reach); let ff = peak(&full, t, reach);
                        let at = attacks.attack_within(t, 0.015).map_or(0.0, |x| x.height);
                        k += kk; f += ff; a += at; h += peak(&full, t + period / 2.0, reach);
                        th += thumps.attack_within(t, 0.04).map_or(0.0, |x| x.height);
                        if kk >= 0.1 || ff >= 0.1 || at >= 0.1 { n += 1; }
                        // The old grid's beat nearest this time.
                        let ta = a_cut + ((t - a_cut) / a_period).round() * a_period;
                        ka += peak(&low, ta, a_period * 0.1); fa += peak(&full, ta, a_period * 0.1);
                        aa += attacks.attack_within(ta, 0.015).map_or(0.0, |x| x.height);
                    }
                    println!("   {:>8.3}{} | {:.2} {:.2} {:.2} {:.2} | {n} | {:.2} {:.2} {:.2} | thump {:.4}", t0, if bar == 0 { "*" } else { " " }, k / 4.0, a / 4.0, f / 4.0, h / 4.0, ka / 4.0, aa / 4.0, fa / 4.0, th / 4.0);
                }
            }
            continue;
        }
        for &c in &changes {
            let from = (c as i64 - bars * 4).max(0) as usize;
            let to = (c + (bars as usize) * 4 / 3).min(track.grid.len() - 1);
            println!("  change at beat {c}: {:.3}s {:.2} -> {:.2} BPM   (t  b  kick  flux  attack  half-flux)", f64::from(track.grid[c].time_ms) / 1000.0, f64::from(track.grid[c - 1].tempo_x100) / 100.0, f64::from(track.grid[c].tempo_x100) / 100.0);
            let mut line = String::new();
            for i in from..to {
                let b = track.grid[i];
                let t = f64::from(b.time_ms) / 1000.0;
                let next = f64::from(track.grid[i + 1].time_ms) / 1000.0;
                let period = next - t;
                let reach = period * 0.1;
                let k = peak(&low, t, reach);
                let f = peak(&full, t, reach);
                let a = attacks.attack_within(t, 0.015).map_or(0.0, |a| a.height);
                let h = peak(&full, t + period / 2.0, reach);
                if b.beat_number == 1 {
                    if !line.is_empty() { println!("{line}"); }
                    line = format!("   {:>8.3}{} |", t, if i == c { "*" } else { " " });
                }
                line.push_str(&format!(" {}{}:{:.2}/{:.2}/{:.2}/{:.2}", b.beat_number, if i == c { "*" } else { "" }, k, f, a, h));
            }
            if !line.is_empty() { println!("{line}"); }
        }
    }
}

/// How well each grid sits on the music along the track: per fifth of the
/// track, the median offset of the strongest onset within a sixth of a beat
/// of each beat, and the mean onset strength at the beats over the whole
/// track. A grid whose offsets wander from fifth to fifth has the wrong
/// tempo, whichever grid it is; the one with the stronger beats is on more
/// of the music.
fn align_experiment() {
    let filter = std::env::args().nth(2).map(|s| s.to_lowercase());
    let dir = cache_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "gold")).collect())
        .unwrap_or_default();
    paths.sort();
    // A grid whose offset from the music moves more than this between
    // fifths of the track has drifted off it somewhere.
    const DRIFTS_MS: f64 = 40.0;
    let (mut ours_better, mut rb_better, mut n) = (0usize, 0usize, 0usize);
    let (mut ours_drift, mut rb_drifts) = (0usize, 0usize);
    for path in &paths {
        let Some(track) = read_track(path) else { continue };
        if filter.as_ref().is_some_and(|f| !track.title.to_lowercase().contains(f.as_str())) {
            continue;
        }
        if track.grid.len() < 16 {
            continue;
        }
        let onsets = rbl_analysis::onset::onset_envelope(&track.samples, track.sample_rate);
        let analysis = rbl_analysis::analyse_with(&track.samples, track.sample_rate, options_under_test());
        let describe = |beats: &[Beat]| -> (String, f64, f64) {
            let secs: Vec<f64> = beats.iter().map(|b| f64::from(b.time_ms) / 1000.0).collect();
            let end = secs.last().copied().unwrap_or(0.0);
            let mut parts: Vec<Vec<f64>> = vec![Vec::new(); 5];
            let mut strength = 0.0;
            for (i, &t) in secs.iter().enumerate() {
                let period = secs.get(i + 1).or(secs.get(i.wrapping_sub(1))).map_or(0.5, |&u| (u - t).abs());
                let x = (t - onsets.origin_secs) * onsets.rate;
                strength += f64::from(onsets.sample_at(x));
                let reach = period / 6.0 * onsets.rate;
                let lo = (x - reach).max(0.0) as usize;
                let hi = ((x + reach) as usize).min(onsets.len().saturating_sub(1));
                let Some((at, h)) = (lo..=hi).map(|j| (j, onsets.values[j])).max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()) else { continue };
                if h < 0.1 {
                    continue;
                }
                let part = ((t / end.max(1e-9)) * 5.0).floor().clamp(0.0, 4.0) as usize;
                parts[part].push((onsets.time_of(at as f64) - t) * 1000.0);
            }
            let medians: Vec<String> = parts
                .iter_mut()
                .map(|p| {
                    p.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    p.get(p.len() / 2).map_or("   -".to_owned(), |m| format!("{m:+4.0}"))
                })
                .collect();
            let values: Vec<f64> = parts.iter().filter_map(|p| p.get(p.len() / 2).copied()).collect();
            let spread = values.iter().copied().fold(f64::NEG_INFINITY, f64::max) - values.iter().copied().fold(f64::INFINITY, f64::min);
            (medians.join(" "), strength / secs.len().max(1) as f64, spread)
        };
        let (rb_line, rb_strength, rb_spread) = describe(&track.grid);
        let (our_line, our_strength, our_spread) = describe(&analysis.tempo.beats);
        n += 1;
        if rb_spread > DRIFTS_MS {
            rb_drifts += 1;
        }
        if our_spread > DRIFTS_MS {
            ours_drift += 1;
        }
        if our_strength > rb_strength * 1.02 {
            ours_better += 1;
        } else if rb_strength > our_strength * 1.02 {
            rb_better += 1;
        }
        println!(
            "{:<40} rb {:>7.2} [{rb_line}] {rb_strength:.3} | ours {:>7.2} [{our_line}] {our_strength:.3}{}",
            truncate(&track.title, 40),
            track.bpm,
            analysis.tempo.bpm,
            if our_spread > DRIFTS_MS && rb_spread <= DRIFTS_MS { "  <-- ours drifts" } else { "" },
        );
    }
    println!("{n} tracks: our beats stronger on {ours_better}, rekordbox's on {rb_better}");
    println!("drifting more than {DRIFTS_MS} ms between fifths: ours {ours_drift}, rekordbox's {rb_drifts}");
}
