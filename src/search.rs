use anyhow::{Result};
use grep::searcher::{self, Searcher, SearcherBuilder, SinkMatch};
use grep::regex::{RegexMatcher, RegexMatcherBuilder};
use ignore::{overrides::{Override, OverrideBuilder}};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use regex;

pub struct SearchResultEntry {
    pub line_number: u64,
    pub text: String,
}

pub struct SearchResult {
    pub path: PathBuf,
    pub entries: Vec<SearchResultEntry>,
}

#[derive(Debug)]
pub struct SearchError;
impl searcher::SinkError for SearchError {
    fn error_message<T: std::fmt::Display>(message: T) -> Self {
        println!("Error: {}", message);
        Self
    }
}

// @Cleanup: Remove the pub here
pub struct SearchSink<'a>(pub &'a mut Vec<SearchResultEntry>);
impl searcher::Sink for SearchSink<'_> {
    type Error = SearchError;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let text = String::from_utf8_lossy(mat.lines().next().unwrap()).into_owned();
        let result = SearchResultEntry {
            line_number: mat.line_number().unwrap(),
            text: text,
        };
        self.0.push(result);

        // Continue search
        Ok(true)
    }
}

pub struct PendingSearch {
    rx: mpsc::Receiver<SearchResult>,
    quit: Arc<AtomicBool>,
    start_time: Instant,
}

impl PendingSearch {
    pub fn new(rx: mpsc::Receiver<SearchResult>) -> Self {
        let quit = Arc::new(AtomicBool::new(false));
        let start_time = Instant::now();
        Self { rx, quit, start_time: start_time }
    }

    pub fn signal_stop(&self) {
        self.quit.store(true, Ordering::Relaxed);
    }

    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

impl Drop for PendingSearch {
    fn drop(&mut self) {
        self.signal_stop();
    }
}

#[derive(Clone)]
pub struct SearchWorker {
    matcher: RegexMatcher,
    searcher: Searcher,
}

impl SearchWorker {
    pub fn search_path(&mut self, path: PathBuf) -> Option<SearchResult> {
        let mut entries = Vec::new();
        let search_sink = SearchSink(&mut entries);

        if let Err(err) = self.searcher.search_path(&self.matcher, &path, search_sink) {
            println!("Failed to search in path '{:?}', error: {:?}", path, err);
            return None;
        }

        let result = SearchResult {
            path: path.clone(),
            entries: entries,
        };

        return Some(result);
    }
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub regex_syntax: bool,
    pub ignore_case: bool,
    pub invert_match: bool,
    pub before_context: usize,
    pub after_context: usize,
}

impl SearchQuery {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            regex_syntax: false,
            ignore_case: true,
            invert_match: false,
            before_context: 0,
            after_context: 0,
        }
    }

    fn matcher(&self) -> Result<RegexMatcher> {
        let mut builder = RegexMatcherBuilder::new();
        builder
            .case_smart(self.ignore_case)
            .case_insensitive(self.ignore_case)
            .multi_line(true)
            .unicode(true)
            .octal(false)
            .line_terminator(Some(b'\n'))
            .dot_matches_new_line(false);

        let matcher = if self.regex_syntax {
            builder.build(&self.query)
        } else {
            let escaped_query = regex::escape(&self.query);
            builder.build_literals(&[escaped_query])
        }?;

        return Ok(matcher);
    }

    fn searcher(&self, line_number: bool) -> Searcher {
        let mut builder = SearcherBuilder::new();
        let searcher = builder
            .invert_match(self.invert_match)
            .line_number(line_number)
            .before_context(self.before_context)
            .after_context(self.after_context)
            .build();
        return searcher;
    }

    fn search_worker(&self, line_number: bool) -> Result<SearchWorker> {
        let matcher = self.matcher()?;
        let searcher = self.searcher(line_number);
        return Ok(SearchWorker { matcher, searcher });
    }
}

#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// List of paths seperated by a semi-column ';'.
    pub paths: String,

    /// List of globs seperated by a space.
    pub globs: String,

    /// List of queries that are to be executed sequentially.
    pub queries: Vec<SearchQuery>,
}

impl SearchConfig {
    pub fn with_paths(paths: String) -> Self {
        let queries = vec![SearchQuery::new()];
        Self { paths, globs: String::new(), queries }
    }

    pub fn paths(&self) -> Vec<&Path> {
        let paths: Vec<&Path> = self
            .paths
            .split(';')
            .filter(|value| !value.is_empty())
            .map(|value| Path::new(value))
            .collect();
        paths
    }

    pub fn overrides(&self) -> Override {
        if self.globs.is_empty() {
            Override::empty()
        } else {
            let path = std::env::current_dir().unwrap_or(PathBuf::from("/"));
            let mut builder = OverrideBuilder::new(path);
            for glob in self.globs.split(' ').filter(|value| !value.is_empty()) {
                if let Err(err) = builder.add(&glob) {
                    println!("Failed to add glob '{}' with error: {}", glob, err);
                }
            }

            builder.build().unwrap_or(Override::empty())
        }
    }

    pub fn workers(&self) -> Vec<SearchWorker> {
        let mut workers = Vec::with_capacity(self.queries.len());
        if let Some((first, remaining)) = self.queries.split_first() {
            if let Ok(worker) = first.search_worker(true) {
                workers.push(worker);
            } else {
                println!("Couldn't build the workers");
                return workers;
            }

            for query in remaining.iter() {
                if let Ok(worker) = query.search_worker(false) {
                    workers.push(worker);
                } else {
                    println!("Failed to create a worker for query '{}'", query.query);
                }
            }
        }

        return workers;
    }
}

pub fn search(config: &SearchConfig) -> PendingSearch {
    unimplemented!();
}
