# Chunker with Rust 🔥

Chunks the files from input folder into similar sized files using a separator. 

It has a cli which you can view help for with 

```
cargo -- -help
```

without building the binary.

For example given the repository it can be run like so with 10 row division against the default files in .in into .out directory.

```
cargo run -- -c 10 
```