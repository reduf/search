use anyhow::{bail, Result};
use grep::{
    matcher::Matcher,
    regex::{RegexMatcher, RegexMatcherBuilder},
    searcher::{self, BinaryDetection, Searcher, SearcherBuilder, SinkContext, SinkMatch, SinkFinish},
};
use ignore::{
    overrides::{Override, OverrideBuilder},
    WalkBuilder, WalkState,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, TryRecvError},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub struct SearchResultLine {
    pub line_number: u64,
    pub bytes: Vec<u8>,
    pub matches: Vec<(usize, usize)>,
}

impl SearchResultLine {
    pub fn is_matched(&self) -> bool {
        return !self.matches.is_empty()
    }
}

#[derive(Default)]
pub struct SearchResultEntry {
    pub matched_line_number: u64,
    pub lines: Vec<SearchResultLine>,
}

struct SearchResultEntryBuilder(Option<SearchResultEntry>);
impl SearchResultEntryBuilder {
    pub fn new() -> Self {
        return Self(None);
    }

    pub fn take(&mut self) -> Option<SearchResultEntry> {
        return self.0.take();
    }

    pub fn with_match_line(&mut self, matcher: &RegexMatcher, line_number: u64, bytes: Vec<u8>) -> &mut Self {
        let mut entry = self.0.take().unwrap_or(SearchResultEntry::default());

        let mut at = 0;
        let mut matches = Vec::new();
        while let Ok(Some(matche)) = matcher.find_at(&bytes, at) {
            matches.push((matche.start(), matche.end()));
            at = matche.end();
        }

        let line = SearchResultLine {
            line_number,
            bytes,
            matches,
        };

        entry.matched_line_number = line_number;
        entry.lines.push(line);

        self.0 = Some(entry);
        return self;
    }

    pub fn with_context(&mut self, line_number: u64, bytes: Vec<u8>) -> &mut Self {
        let mut entry = self.0.take().unwrap_or(SearchResultEntry::default());

        let line = SearchResultLine {
            line_number,
            bytes,
            matches: Vec::new(),
        };

        entry.lines.push(line);
        self.0 = Some(entry);
        return self;
    }
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

struct SearchSink<'a, 'm> {
    // We need to track if we have extra context, because when we don't, `context_break` is never called.
    has_extra_context: bool,
    builder: SearchResultEntryBuilder,
    results: &'a mut Vec<SearchResultEntry>,
    matcher: &'m RegexMatcher,
}

impl searcher::Sink for SearchSink<'_, '_> {
    type Error = SearchError;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let line_number = mat.line_number().expect("Instruct the SearchBuilder to compute line numbers");
        self.builder.with_match_line(self.matcher, line_number, mat.bytes().to_vec());

        if !self.has_extra_context {
            self.results.extend(self.builder.take());
        }

        return Ok(true);
    }

    fn context(&mut self, _searcher: &Searcher, context: &SinkContext<'_>) -> Result<bool, Self::Error> {
        let line_number = context.line_number().expect("Instruct the SearchBuilder to compute line numbers");
        self.builder.with_context(line_number, context.bytes().to_vec());
        return Ok(true);
    }

    fn context_break(&mut self, _searcher: &Searcher) -> Result<bool, Self::Error> {
        self.results.extend(self.builder.take());
        return Ok(true);
    }

    fn finish(&mut self, _searcher: &Searcher, _: &SinkFinish) -> Result<(), Self::Error> {
        self.results.extend(self.builder.take());
        return Ok(());
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
        Self {
            rx,
            quit,
            start_time,
        }
    }

    pub fn signal_stop(&self) {
        self.quit.store(true, Ordering::Relaxed);
    }

    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn try_recv(&self) -> std::result::Result<SearchResult, TryRecvError> {
        self.rx.try_recv()
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
    pub fn search_path(
        &mut self,
        dir_entry: ignore::DirEntry,
        search_binary: bool,
    ) -> Option<SearchResult> {
        assert_eq!(self.searcher.before_context(), self.searcher.after_context(), "We currently only support equal context before and after");

        let mut entries = Vec::new();
        let search_sink = SearchSink {
            has_extra_context: self.searcher.before_context() != 0,
            builder: SearchResultEntryBuilder::new(),
            results: &mut entries,
            matcher: &self.matcher,
        };

        let bin_detection = if search_binary {
            BinaryDetection::none()
        } else if dir_entry.depth() == 0 {
            // If the depth of the entry is 0, it means the file was specified
            // explicitly. So, we don't exclude this file if we detect it to be
            // a binary.
            BinaryDetection::convert(b'\x00')
        } else {
            BinaryDetection::quit(b'\x00')
        };

        self.searcher.set_binary_detection(bin_detection);

        let path = dir_entry.into_path();
        if let Err(err) = self.searcher.search_path(&self.matcher, &path, search_sink) {
            println!("Failed to search in path '{:?}', error: {:?}", path, err);
            return None;
        }

        let result = SearchResult { path, entries };

        return Some(result);
    }
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub regex_syntax: bool,
    pub ignore_case: bool,
    pub invert_match: bool,
    pub extra_context: usize,
}

impl SearchQuery {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            regex_syntax: false,
            ignore_case: true,
            invert_match: false,
            extra_context: 0,
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
            .before_context(self.extra_context)
            .after_context(self.extra_context)
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
    pub fn default() -> Self {
        return Self {
            paths: String::new(),
            globs: String::new(),
            queries: Vec::new(),
        };
    }

    pub fn with_paths_and_patterns(paths: String, patterns: String) -> Self {
        let queries = vec![SearchQuery::new()];
        return Self {
            paths,
            globs: patterns,
            queries,
        };
    }

    pub fn paths(&self) -> Vec<&Path> {
        return self
            .paths
            .split(';')
            .filter(|value| !value.is_empty())
            .map(Path::new)
            .collect();
    }

    pub fn overrides(&self) -> Override {
        if self.globs.is_empty() {
            Override::empty()
        } else {
            let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            let mut builder = OverrideBuilder::new(path);
            for glob in self.globs.split(' ').filter(|value| !value.is_empty()) {
                if let Err(err) = builder.add(glob) {
                    println!("Failed to add glob '{}' with error: {}", glob, err);
                }
            }

            builder.build().unwrap_or_else(|_| Override::empty())
        }
    }

    pub fn workers(&self) -> Vec<SearchWorker> {
        let mut workers = Vec::with_capacity(self.queries.len());

        let mut it = self.queries.iter().filter(|query| !query.query.is_empty());

        // We need at least 1 worker which find the line numbers
        if let Some(worker) = it.next() {
            if let Ok(worker) = worker.search_worker(true) {
                workers.push(worker);
            } else {
                println!("Couldn't build the workers");
                return workers;
            }
        } else {
            return workers;
        }

        for query in it {
            if let Ok(worker) = query.search_worker(false) {
                workers.push(worker);
            } else {
                println!("Failed to create a worker for query '{}'", query.query);
            }
        }

        return workers;
    }
}

pub fn spawn_search(
    config: &SearchConfig,
    search_binary: bool,
    number_of_threads: usize,
) -> Result<PendingSearch> {
    let (tx, rx) = mpsc::channel();
    let pending_search = PendingSearch::new(rx);

    let workers = config.workers();
    if workers.is_empty() {
        bail!("No workers, search is not possible");
    }

    let mut builder = if let Some((first, remaining)) = config.paths().split_first() {
        let mut builder = WalkBuilder::new(first);
        for path in remaining {
            builder.add(path);
        }
        builder
    } else {
        bail!("Can't search with no path");
    };

    builder.overrides(config.overrides());

    let threads = if number_of_threads == 0 {
        thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(2)
    } else {
        number_of_threads
    };

    let walker = builder.threads(threads).build_parallel();

    let quit = pending_search.quit.clone();
    std::thread::spawn(move || {
        walker.run(|| {
            let tx = tx.clone();
            let quit = quit.clone();

            let mut workers = workers.clone();

            Box::new(move |result| {
                if quit.load(Ordering::Relaxed) {
                    return WalkState::Quit;
                }

                let entry = if let Ok(entry) = result {
                    entry
                } else {
                    return WalkState::Continue;
                };

                if let Some(file_type) = entry.file_type() {
                    if !file_type.is_file() {
                        return WalkState::Continue;
                    }
                } else {
                    return WalkState::Continue;
                };

                if let Some(result) = workers[0].search_path(entry, search_binary) {
                    return match tx.send(result) {
                        Ok(_) => WalkState::Continue,
                        Err(_) => WalkState::Quit,
                    };
                } else {
                    return WalkState::Continue;
                };
            })
        });
    });

    return Ok(pending_search);
}
