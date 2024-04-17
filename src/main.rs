#![allow(unused, dead_code, unused_variables, unused_imports)]

use std::env;
use std::fmt::format;
use std::fs;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use thiserror::Error;
use tokio::task::JoinHandle;

// read a file
fn folder_exists(path: &str) -> bool {
    let p = Path::new(path);
    p.exists() && p.is_dir()
}

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

struct ChunkyBoyoConfig {
    executor_size: u32,
    chunk_size: u32,
    output_folder: String,
    input_folder: String,
    separator: u8,
    verbose: bool,
}

struct ReaderBoyo {
    config: ChunkyBoyoConfig,
}

struct SplitterBoyo {
    chunk_size: usize,
    input_path: PathBuf,
    output_folder: String,
    executor_size: u32,
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
    fn new(
        input_path: PathBuf,
        output_folder: String,
        cs: usize,
        es: u32,
        sep: u8,
    ) -> SplitterBoyo {
        SplitterBoyo {
            input_path,
            output_folder,
            chunk_size: cs,
            executor_size: es,
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

    fn run(&self) -> Result<Vec<JoinHandle<Result<String, WriterError>>>, SplitterError> {
        // split file into chunks
        let file = File::open(&self.input_path)?;
        let mut spliterator = BufReader::new(file).split(self.separator).peekable();
        let mut chunk: Vec<Vec<u8>> = Vec::with_capacity(self.chunk_size);
        let mut c = Counter::new();
        let mut fc = Counter::new();

        let mut handles = Vec::new();

        while let Some(Ok(value)) = spliterator.next() {
            let is_last = spliterator.peek().is_none();
            chunk.push(value);
            c.increment_get();

            if (c.get() % self.chunk_size == 0 || is_last) {
                // stuff
                let opf = self.create_output_file_path(fc.increment_get())?;
                let mut wb = WriterBoyo::new(opf, self.separator, chunk.clone());

                let handle = tokio::spawn(async move { wb.run() });
                handles.push(handle);
                chunk.clear();
            }
        }

        Ok(handles)
    }
}

struct ProgressBoyo {}

impl ProgressBoyo {
    fn new() -> ProgressBoyo {
        ProgressBoyo {}
    }
}

struct WriterBoyo {
    separator: u8,
    output_path: PathBuf,
    source: Vec<Vec<u8>>,
    progress_boyo: ProgressBoyo,
}

#[derive(Debug, Error)]
enum WriterError {
    #[error("SplitterError: {0}")]
    FileCreateError(#[from] std::io::Error),
    #[error("Unexpected filename {0:?}")]
    UnexpectedFilename(PathBuf),
}

impl WriterBoyo {
    fn new(output_path: PathBuf, separator: u8, source: Vec<Vec<u8>>) -> WriterBoyo {
        WriterBoyo {
            progress_boyo: ProgressBoyo::new(),
            separator,
            output_path,
            source,
        }
    }

    fn run(&mut self) -> Result<String, WriterError> {
        match self.output_path.file_name().map(|f| f.to_str()).flatten() {
            Some(fname) => {
                let file = File::create(&self.output_path)?;
                let mut bw = BufWriter::new(file);

                while let Some(d) = self.source.pop() {
                    let s = d.as_slice();
                    bw.write(s)?;
                }

                Ok(String::from(fname))
            }
            _ => Err(WriterError::UnexpectedFilename(self.output_path.clone())),
        }
    }
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let paths = gather_files(".in")?;

    for path in paths {
        println!("{:?}", path);

        let splitter = SplitterBoyo::new(path, String::from(".out"), 10, 10, b'\n');
        let f = tokio::spawn(async move {
            let handles = splitter.run();
            match handles {
                Ok(hs) => {
                    for h in hs {
                        h.await;
                    }
                }
                Err(e) => print!("errerr"),
            }
        })
        .await;
    }
    Ok(())
}
