//! The template profiler (WITH_PROFILER parts of transform.c and
//! xsltutils.c): XSLTProcessor::setProfiling().

use crate::style::Sheet;
use crate::util::tw;
use std::cell::Cell;
use std::time::Instant;

/// XSLT_TIMESTAMP_TICS_PER_SEC
const TICS_PER_SEC: i64 = 100_000;

/// Per-template profile data (kept on the stylesheet's templates, so it
/// accumulates over transformations, as libxslt's xsltTemplate fields do).
#[derive(Debug, Default, Clone)]
pub struct ProfStat {
    pub nb_calls: i32,
    pub time: i64,
    /// (caller template, count): templCalledTab / templCountTab
    pub called: Vec<(Option<usize>, i32)>,
}

thread_local! {
    static STARTUP: Cell<Option<Instant>> = const { Cell::new(None) };
    static CALIBRATION: Cell<i64> = const { Cell::new(-1) };
}

fn raw_tics() -> i64 {
    let start = STARTUP.with(|s| s.get()).unwrap_or_else(Instant::now);
    let d = start.elapsed();
    d.as_secs() as i64 * TICS_PER_SEC + d.subsec_nanos() as i64 / (1_000_000_000 / TICS_PER_SEC)
}

/// xsltTimestamp
pub fn timestamp() -> i64 {
    if CALIBRATION.with(|c| c.get()) < 0 {
        STARTUP.with(|s| s.set(Some(Instant::now())));
        CALIBRATION.with(|c| c.set(0));
        // xsltCalibrateTimestamps
        for _ in 0..999 {
            timestamp();
        }
        let cal = timestamp() / 1000;
        CALIBRATION.with(|c| c.set(cal));
        STARTUP.with(|s| s.set(Some(Instant::now())));
        return 0;
    }
    raw_tics() - CALIBRATION.with(|c| c.get())
}

/// xsltCalibrateAdjust
pub fn calibrate_adjust(delta: i64) {
    CALIBRATION.with(|c| c.set(c.get() + delta));
}

/// profCallgraphAdd
pub fn callgraph_add(sheet: &Sheet, templ: usize, parent: Option<usize>) {
    let mut prof = sheet.prof.borrow_mut();
    if prof.len() <= templ {
        prof.resize(sheet.templates.len().max(templ + 1), ProfStat::default());
    }
    let st = &mut prof[templ];
    match st.called.iter_mut().find(|(p, _)| *p == parent) {
        Some(e) => e.1 += 1,
        None => st.called.push((parent, 1)),
    }
}

pub fn with_stat<R>(sheet: &Sheet, templ: usize, f: impl FnOnce(&mut ProfStat) -> R) -> R {
    let mut prof = sheet.prof.borrow_mut();
    if prof.len() <= templ {
        prof.resize(sheet.templates.len().max(templ + 1), ProfStat::default());
    }
    f(&mut prof[templ])
}

/// printf("%Ns") / printf("%-Ns")
fn rj(s: &str, w: usize) -> String {
    format!("{:>w$}", s, w = w)
}

fn lj(s: &str, w: usize) -> String {
    format!("{:<w$}", s, w = w)
}

/// pretty_templ_match
fn pretty_match(sheet: &Sheet, t: usize) -> String {
    let tp = &sheet.templates[t];
    let mut out: String = tp
        .match_
        .as_deref()
        .unwrap_or("")
        .chars()
        .filter(|&c| c != ' ')
        .collect();
    if let Some(m) = &tp.mode {
        out.push('[');
        out.push_str(m);
        out.push(']');
    }
    out
}

fn display_name(sheet: &Sheet, t: usize) -> String {
    match &sheet.templates[t].name {
        Some(n) => n.clone(),
        None => pretty_match(sheet, t),
    }
}

fn secs(t: i64) -> String {
    format!("{:8.3}", t as f32 / TICS_PER_SEC as f32)
}

/// xsltSaveProfiling
pub fn save_profiling(sheet: &Sheet) -> String {
    let prof = sheet.prof.borrow().clone();
    let stat = |t: usize| prof.get(t).cloned().unwrap_or_default();
    let mut templates: Vec<usize> = Vec::new();
    for s in sheet.import_order() {
        for &t in sheet.styles[s].templates.iter().rev() {
            if templates.len() >= 10000 {
                break;
            }
            if stat(t).nb_calls > 0 {
                templates.push(t);
            }
        }
    }
    let nb = templates.len();
    for i in 0..nb.saturating_sub(1) {
        for j in i + 1..nb {
            let (a, b) = (stat(templates[i]), stat(templates[j]));
            if a.time <= b.time || (a.time == b.time && a.nb_calls <= b.nb_calls) {
                templates.swap(i, j);
            }
        }
    }
    let mut o = String::new();
    o.push_str(&format!(
        "{}{}{}{}  Calls Tot 100us Avg\n\n",
        rj("number", 6),
        rj("match", 20),
        rj("name", 20),
        rj("mode", 10)
    ));
    let mut total = 0i32;
    let mut totalt = 0i64;
    for (i, &t) in templates.iter().enumerate() {
        let tp = &sheet.templates[t];
        let st = stat(t);
        o.push_str(&format!("{:5} ", i));
        match &tp.match_ {
            Some(m) if m.len() > 20 => o.push_str(&format!("{}\n{}", m, rj("", 26))),
            Some(m) => o.push_str(&rj(m, 20)),
            None => o.push_str(&rj("", 20)),
        }
        match &tp.name {
            Some(m) if m.len() > 20 => o.push_str(&format!("{}\n{}", m, rj("", 46))),
            Some(m) => o.push_str(&rj(m, 20)),
            None => o.push_str(&rj("", 20)),
        }
        match &tp.mode {
            Some(m) if m.len() > 10 => o.push_str(&format!("{}\n{}", m, rj("", 56))),
            Some(m) => o.push_str(&rj(m, 10)),
            None => o.push_str(&rj("", 10)),
        }
        o.push_str(&format!(" {:6}", st.nb_calls));
        o.push_str(&format!(
            " {:6} {:6}\n",
            st.time,
            st.time / st.nb_calls.max(1) as i64
        ));
        total += st.nb_calls;
        totalt += st.time;
    }
    o.push_str(&format!(
        "\n{}{} {:6} {:6}\n",
        rj("Total", 30),
        rj("", 26),
        total,
        totalt
    ));
    // call graph
    let mut childt = vec![0i64; nb + 1];
    for (i, &t1) in templates.iter().enumerate() {
        for &t2 in &templates {
            for (p, _) in &stat(t2).called {
                if *p == Some(t1) {
                    childt[i] += stat(t2).time;
                }
            }
        }
    }
    o.push_str("\nindex % time    self  children    called     name\n");
    for (i, &t1) in templates.iter().enumerate() {
        let s1 = stat(t1);
        for (p, cnt) in &s1.called {
            let k = match p {
                Some(t2) => templates.iter().position(|x| x == t2).unwrap_or(nb),
                None => nb,
            };
            let tt = match p {
                Some(t2) => stat(*t2).time,
                None => totalt,
            };
            let called = format!("{:6}/{}", cnt, s1.nb_calls);
            let name = match p {
                Some(t2) => display_name(sheet, *t2),
                None => "-".to_string(),
            };
            o.push_str(&format!(
                "             {} {} {}     {} [{}]\n",
                lj(&secs(tt), 8),
                lj(&secs(childt[k]), 8),
                lj(&called, 12),
                name,
                k
            ));
        }
        let ix = format!("[{}]", i);
        let pct = if totalt != 0 {
            format!("{:6.2}", s1.time as f32 * 100.0 / totalt as f32)
        } else if s1.time == 0 {
            "  -nan".to_string()
        } else {
            "   inf".to_string()
        };
        o.push_str(&format!(
            "{} {} {} {} {:6}     {} [{}]\n",
            lj(&ix, 5),
            lj(&pct, 6),
            lj(&secs(s1.time), 8),
            lj(&secs(childt[i]), 8),
            s1.nb_calls,
            display_name(sheet, t1),
            i
        ));
        let mut tot = 0;
        for &t2 in &templates {
            for (p, c) in &stat(t2).called {
                if *p == Some(t1) {
                    tot += c;
                }
            }
        }
        for (k, &t2) in templates.iter().enumerate() {
            for (p, c) in &stat(t2).called {
                if *p == Some(t1) {
                    let called = format!("{:6}/{}", c, tot);
                    o.push_str(&format!(
                        "             {} {} {}     {} [{}]\n",
                        lj(&secs(stat(t2).time), 8),
                        lj(&secs(childt[k]), 8),
                        lj(&called, 12),
                        display_name(sheet, t2),
                        k
                    ));
                }
            }
        }
        o.push_str("-----------------------------------------------\n");
    }
    o.push_str("\u{c}\nIndex by function name\n");
    for (i, &t) in templates.iter().enumerate() {
        let tp = &sheet.templates[t];
        let (url, line) = tw(|tr| {
            (
                tr.doc_url(sheet.styles[tp.style].doc).unwrap_or_default(),
                tr.nodes[tp.elem].line,
            )
        });
        o.push_str(&format!(
            "[{}] {} ({}:{})\n",
            i,
            display_name(sheet, t),
            url,
            line
        ));
    }
    o.push_str("\u{c}\n");
    o
}
