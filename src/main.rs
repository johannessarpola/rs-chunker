use clap::Parser;
use std::collections::VecDeque;
use std::fs;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio::time::{sleep, Duration};

fn gather_files(path: &str) -> Result<Vec<PathBuf>, io::Error> {
    let files = fs::read_dir(path)?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.is_file() {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    Ok(files)
}
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct ChunkyBoyoConfig {
    #[arg(short, long, default_value_t = 10000)]
    chunk_size: usize,
    #[arg(short, long, default_value_t = String::from(".out"))]
    output_folder: String,
    #[arg(short, long, default_value_t = String::from(".in"))]
    input_folder: String,
    #[arg(short, long, default_value_t = b'\n')]
    separator: u8,
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

struct SplitterBoyo {
    chunk_size: usize,
    input_path: PathBuf,
    output_folder: String,
    separator: u8,
}

#[derive(Debug, Error)]
enum SplitterError {
    #[error("SplitterError: {0}")]
    FileError(#[from] std::io::Error),
    #[error("SplitterError: {0}")]
    InvalidPath(PathBuf),
}

struct Counter(usize);
impl Counter {
    fn add_get(&mut self, amount: usize) -> usize {
        self.0 += amount;
        self.0
    }
    fn increment_get(&mut self) -> usize {
        self.0 += 1;
        self.0
    }

    fn new() -> Counter {
        Counter(0)
    }

    fn get(&self) -> usize {
        self.0
    }
}

impl SplitterBoyo {
    fn new(input_path: PathBuf, output_folder: String, cs: usize, sep: u8) -> SplitterBoyo {
        SplitterBoyo {
            input_path,
            output_folder,
            chunk_size: cs,
            separator: sep,
        }
    }

    fn create_output_file_path(&self, counter: usize) -> Result<PathBuf, SplitterError> {
        match (
            self.input_path.file_stem().map(|f| f.to_str()).flatten(),
            self.input_path.extension().map(|f| f.to_str()).flatten(),
        ) {
            (Some(stem), Some(extension)) => {
                let output_file_name: String = format!("{}_{}.{}", stem, counter, extension);
                Ok([&self.output_folder, &output_file_name].iter().collect())
            }
            _ => Err(SplitterError::InvalidPath(self.input_path.clone())),
        }
    }

    fn run(
        &self,
        progress_tx: Option<UnboundedSender<bool>>,
        total_tx: Option<UnboundedSender<usize>>,
    ) -> Result<JoinSet<Result<String, WriterError>>, SplitterError> {
        // split file into chunks
        let file = File::open(&self.input_path)?;
        let mut spliterator = BufReader::new(file).split(self.separator).peekable();
        let mut chunk: VecDeque<Vec<u8>> = VecDeque::with_capacity(self.chunk_size);
        let mut c = Counter::new();
        let mut fc = Counter::new();

        let mut handles = JoinSet::new();

        // create output dir if it does not exist
        if !Path::new(&self.output_folder).exists() {
            std::fs::create_dir(&self.output_folder)?;
        }

        while let Some(Ok(value)) = spliterator.next() {
            let is_last = spliterator.peek().is_none();

            chunk.push_back(value);
            c.increment_get();

            if c.get() % self.chunk_size == 0 || is_last {
                // forward the chunk size
                if let Some(tx) = &total_tx {
                    let _ = tx.send(chunk.len());
                }
                // stuff
                let opf = self.create_output_file_path(fc.increment_get())?;
                let mut wb = WriterBoyo::new(opf, chunk);
                let ptx = progress_tx.clone();

                handles.spawn(async move { wb.run(ptx).await });

                chunk = VecDeque::with_capacity(self.chunk_size);
            }
        }

        Ok(handles)
    }
}

struct WriterBoyo {
    output_path: PathBuf,
    source: VecDeque<Vec<u8>>,
}

#[derive(Debug, Error)]
enum WriterError {
    #[error("SplitterError: {0}")]
    FileCreateError(#[from] std::io::Error),
    #[error("Unexpected filename {0:?}")]
    UnexpectedFilename(PathBuf),
}

impl WriterBoyo {
    fn new(output_path: PathBuf, source: VecDeque<Vec<u8>>) -> WriterBoyo {
        WriterBoyo {
            output_path,
            source,
        }
    }

    async fn run(
        &mut self,
        progress_tx: Option<UnboundedSender<bool>>,
    ) -> Result<String, WriterError> {
        match self.output_path.file_name().map(|f| f.to_str()).flatten() {
            Some(fname) => {
                let file = File::create(&self.output_path)?;
                let mut bw = BufWriter::new(file);

                while let Some(d) = self.source.pop_front() {
                    let s = d.as_slice();
                    bw.write(s)?;

                    //sleep(Duration::from_millis(100)).await; // TODO rm
                    if let Some(rx) = &progress_tx {
                        // we can ignore errors in this case
                        let _ = rx.send(true);
                    }
                }

                Ok(fname.to_owned())
            }
            _ => Err(WriterError::UnexpectedFilename(self.output_path.clone())),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ChunkyBoyoConfig::parse();

    let paths = gather_files(&config.input_folder)?;

    // print stuff
    let mut acc = String::new();
    for path in &paths {
        if acc.is_empty() {
            acc = String::from(path.to_str().unwrap_or_default());
        } else {
            acc = acc + "," + path.to_str().unwrap_or_default();
        }
    }

    println!("Splitting files {}", acc);

    let mut sjoin_set = JoinSet::new();

    // TODO Skip if not verbose
    let (progress_tx, progress_rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
    let (total_tx, total_rx) = tokio::sync::mpsc::unbounded_channel::<usize>();

    for path in paths {
        let splitter = SplitterBoyo::new(
            path,
            config.output_folder.clone(),
            config.chunk_size,
            config.separator,
        );

        let ptx = progress_tx.clone();
        let ttx = total_tx.clone();

        sjoin_set.spawn(async move {
            match splitter.run(Some(ptx), Some(ttx)) {
                Ok(mut wjoin_set) => {
                    while let Some(writer_result) = wjoin_set.join_next().await {
                        match writer_result {
                            Ok(Ok(v)) => println!("outputted file {}", v),
                            Ok(Err(e)) => println!("error in writer task: {}", e),
                            Err(e) => println!("error in joining: {}", e),
                        }
                    }
                }
                Err(e) => println!("error running splitter: {}", e),
            }
        });
    }

    let (cancel_tx, _) = broadcast::channel(1);

    // updates progress
    let pcntr_lock: Arc<RwLock<Counter>> = Arc::new(RwLock::new(Counter::new()));
    let progress_updater = tokio::spawn(update_progress_process(
        pcntr_lock.clone(),
        progress_rx,
        cancel_tx.subscribe(),
    ));

    // updates total
    let tcntr_lock: Arc<RwLock<Counter>> = Arc::new(RwLock::new(Counter::new()));
    let totals_updater = tokio::spawn(update_totals_process(
        tcntr_lock.clone(),
        total_rx,
        cancel_tx.subscribe(),
    ));

    let progress_bar_updater = tokio::spawn(progress_bar_process(
        pcntr_lock.clone(),
        tcntr_lock.clone(),
        cancel_tx.subscribe(),
    ));

    // not sure is there better way for this
    while let Some(res) = sjoin_set.join_next().await {
        res?
    }

    let _ = cancel_tx.send(());
    let _ = progress_bar_updater.await;
    let _ = progress_updater.await;
    let _ = totals_updater.await;
    Ok(())
}

async fn progress_bar_process(
    pcntr: Arc<RwLock<Counter>>,
    tcntr: Arc<RwLock<Counter>>,
    mut cancel_receiver: broadcast::Receiver<()>,
) {
    let refresh_interval_ms = 10;
    let bar_max_len = 10;

    loop {
        let progress = pcntr.read().await.get();
        let total = tcntr.read().await.get();

        if progress > 0 && total > 0 {
            let progress_pcnt = progress as f32 / total as f32 * 100. + 0.;
            let bar_len = progress_pcnt.ceil() as usize / bar_max_len;

            let pa = repeat_char('=', bar_len);
            let pb = repeat_char('-', bar_max_len - bar_len);

            print!("\rDone {:.2}% |{pa}{pb}|\n", progress_pcnt);
        }

        if let Ok(_) = cancel_receiver.try_recv() {
            break;
        } else {
            sleep(Duration::from_millis(refresh_interval_ms)).await
        }
    }
}

fn repeat_char(c: char, count: usize) -> String {
    std::iter::repeat(c).take(count).collect()
}

async fn update_progress_process(
    pcntr: Arc<RwLock<Counter>>,
    mut progress_rx: tokio::sync::mpsc::UnboundedReceiver<bool>,
    mut cancel_receiver: broadcast::Receiver<()>,
) {
    let refresh_interval_ms = 10;

    loop {
        while let Ok(_) = progress_rx.try_recv() {
            pcntr.write().await.increment_get();
        }
        if let Ok(_) = cancel_receiver.try_recv() {
            println!(
                "stopped updating progress, end at {}",
                pcntr.read().await.get()
            );
            break;
        } else {
            sleep(Duration::from_millis(refresh_interval_ms)).await
        }
    }
}

async fn update_totals_process(
    tcntr: Arc<RwLock<Counter>>,
    mut total_rx: tokio::sync::mpsc::UnboundedReceiver<usize>,
    mut cancel_receiver: broadcast::Receiver<()>,
) {
    let refresh_interval_ms = 10;

    loop {
        while let Ok(t) = total_rx.try_recv() {
            tcntr.write().await.add_get(t);
        }

        if let Ok(_) = cancel_receiver.try_recv() {
            println!("stopped totals, end at {}", tcntr.read().await.get());
            break;
        } else {
            sleep(Duration::from_millis(refresh_interval_ms)).await
        }
    }
}
