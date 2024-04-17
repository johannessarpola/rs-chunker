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
}

impl SplitterBoyo {
    fn new(input_path: PathBuf, output_folder: String, cs: usize, es: u32, sep: u8) -> SplitterBoyo {
        SplitterBoyo {
            input_path,
            output_folder,
            chunk_size: cs,
            executor_size: es,
            separator: sep,
        }
    }

    fn run(&self) -> Result<(), SplitterError> {
        // split file into chunks
        let file = File::open(&self.input_path)?;
        let mut spliterator = BufReader::new(file).split(self.separator).peekable();
        let mut chunk: Vec<Vec<u8>> = Vec::with_capacity(self.chunk_size);
        let mut counter: usize = 0;
        let mut file_counter: usize = 0;


        let file_stem = self.input_path.file_stem().unwrap().to_str().unwrap();
        let extension = self.input_path.extension().unwrap_or_default().to_str().unwrap();

        while let Some(Ok(value)) = spliterator.next() {
            let last = spliterator.peek().is_none();
            println!("{}", String::from_utf8(value.clone()).unwrap());
            chunk.push(value);
            counter += 1;

            if (counter % self.chunk_size == 0 || last) {
                file_counter += 1;
                // start writer
                let new_file_name = format!("{}_{}.{}", file_stem, file_counter, extension);
                let output_file: PathBuf = [r"", &self.output_folder, &new_file_name].iter().collect();
                println!("{}", new_file_name);
                let mut wb = WriterBoyo::new(output_file, self.separator, chunk);
                wb.run();

                if !last {
                    chunk = Vec::with_capacity(self.chunk_size);
                } else {
                    // needs to be here to prevent errors about borrowing
                    chunk = Vec::new();
                }
            }
        }

        Ok(())
    }
}

struct ProgressBoyo {}

impl ProgressBoyo {
    fn new() -> ProgressBoyo {
        ProgressBoyo{}
    }
}

struct WriterBoyo {
    separator: u8,
    output_path: PathBuf,
    source: Vec<Vec<u8>>,
    progress_boyo: ProgressBoyo,
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

    fn run(&mut self) -> Result<&str, std::io::Error> {
        // TODO beautify
        let op = self.output_path.file_name().unwrap().to_str().unwrap();
        let file = File::create(&self.output_path)?;
        let mut bw = BufWriter::new(file);
        
        while let Some(d) = self.source.pop() {
            let s = d.as_slice();
            bw.write(s)?;
        }

        Ok(op)
    }
}

fn main() -> anyhow::Result<()> {
    let paths = gather_files(".in")?;

    for path in paths {
        println!("{:?}", path);

        let splitter = SplitterBoyo::new(path, String::from(".out"),10, 10, b'\n');
        splitter.run();
    }
    Ok(())
}
