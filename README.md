# Flysystem

> A filesystem abstraction layer for Rust.

[![crates.io](https://img.shields.io/crates/v/flysystem.svg)](https://crates.io/crates/flysystem)
[![download count badge](https://img.shields.io/crates/d/flysystem.svg)](https://crates.io/crates/flysystem)
[![docs.rs](https://img.shields.io/badge/docs-latest-blue.svg)](https://docs.rs/flysystem)

## About Flysystem

Flysystem is a file storage library for Rust. It provides one interface to interact with many types of filesystems. When you use Flysystem, you're not only protected from vendor lock-in, you'll also have a consistent experience for which ever storage is right for you.

It's inspired by the [PHP library of the same name](https://flysystem.thephpleague.com/docs/).

## Getting Started

```rust
use flysystem::{Filesystem, adapters::{S3Adapter, s3::Config}};

// instantly swap between storage backends
// (like S3/FTP/etc) by changing the type 👇👇👇 here.
let mut filesystem = Filesystem::new::<S3Adapter>(Config {
    region: env::var("S3_REGION").ok(),
    bucket: env::var("S3_BUCKET").unwrap(),
    endpoint: env::var("S3_ENDPOINT").unwrap(),
    access_key: env::var("S3_ACCESS_KEY").unwrap(),
    secret_key: env::var("S3_SECRET_KEY").unwrap(),
}).await?;

filesystem.write(Path::new("my-first-file.txt"), "Hello, world!").await?;
```

Refer to the [documentation on docs.rs](https://docs.rs/flysystem) for detailed usage instructions.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
