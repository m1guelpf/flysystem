#![warn(clippy::all, clippy::pedantic, clippy::nursery)]

//! ## About Flysystem
//!
//! Flysystem is a file storage library for Rust. It provides one interface to interact with many types of filesystems.
//! When you use Flysystem, you're not only protected from vendor lock-in, you'll also have a consistent experience for which ever storage is right for you.
//!
//! It's inspired by the [PHP library of the same name](https://flysystem.thephpleague.com/docs/).
//!
//! ## Getting Started
//!
//! ```rust
//! use flysystem::{Filesystem, adapters::{S3Adapter, s3::Config}};
//! # use std::{env, path::Path};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // instantly swap between storage backends (like S3/Local/FTP)
//! // by changing the type here 👇👇👇
//! let mut filesystem = Filesystem::new::<S3Adapter>(Config {
//!     region: env::var("S3_REGION").ok(),
//!     bucket: env::var("S3_BUCKET").unwrap(),
//!     endpoint: env::var("S3_ENDPOINT").unwrap(),
//!     access_key: env::var("S3_ACCESS_KEY").unwrap(),
//!     secret_key: env::var("S3_SECRET_KEY").unwrap(),
//! }).await?;
//!
//! filesystem.write(Path::new("my-first-file.txt"), "Hello, world!").await?;
//! # Ok(())
//! # }
//! ```

use adapters::{Adapter, AdapterInit};
use contents::Contents;
use mime::Mime;
use std::{
	io::{Error, ErrorKind, Result},
	path::{Path, PathBuf},
	time::SystemTime,
};
use trait_object_hackyness::AdapterObject;

pub mod adapters;
mod contents;
mod trait_object_hackyness;

/// The visibility of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
	Public,
	Private,
}

/// The type of resource.
enum Resource {
	File,
	Directory,
}

#[derive(Debug)]
/// Abstraction over a filesystem.
pub struct Filesystem {
	adapter: Box<dyn AdapterObject>,
}

impl Filesystem {
	pub async fn new<T: AdapterInit>(config: T::Config) -> std::result::Result<Self, T::Error> {
		Ok(Self {
			adapter: Box::new(T::new(config).await?),
		})
	}

	/// Create a new filesystem instance.
	pub fn from_adapter<T: Adapter + 'static>(adapter: T) -> Self {
		Self {
			adapter: Box::new(adapter),
		}
	}

	/// Check if a file exists.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to check if the file exists.
	pub async fn file_exists(&self, path: &Path) -> Result<bool> {
		self.adapter.file_exists(path).await
	}

	/// Check if a directory exists.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to check if the directory exists.
	pub async fn directory_exists(&self, path: &Path) -> Result<bool> {
		self.adapter.directory_exists(path).await
	}

	/// Check if a file or directory exists.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to check if the file or directory exists.
	pub async fn has(&self, path: &Path) -> Result<bool> {
		let (file_exists, directory_exists) = futures::future::join(
			self.adapter.file_exists(path),
			self.adapter.directory_exists(path),
		)
		.await;

		Ok(file_exists? || directory_exists?)
	}

	/// Write a file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to write the file.
	pub async fn write(&mut self, path: &Path, contents: &[u8]) -> Result<()> {
		self.adapter.write(path, contents).await
	}

	/// Get the contents of a file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to read the file.
	pub async fn read<R: TryFrom<Contents>>(&mut self, path: &Path) -> Result<R> {
		self.adapter.read(path).await.and_then(|c| {
			c.try_into()
				.map_err(|_| Error::new(ErrorKind::InvalidData, "Could not decode contents."))
		})
	}

	/// Delete a file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to delete the file or directory.
	pub async fn delete(&mut self, path: &Path) -> Result<()> {
		self.adapter.delete(path).await
	}

	/// Delete a directory.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to delete the directory.
	pub async fn delete_directory(&mut self, path: &Path) -> Result<()> {
		self.adapter.delete_directory(path).await
	}

	/// Create a directory.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to create the directory.
	pub async fn create_directory(&mut self, path: &Path) -> Result<()> {
		self.adapter.create_directory(path).await
	}

	/// Get a list of files in a directory (optionally recursively).
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to list the contents of the directory.
	pub async fn list_contents(&self, path: &Path, deep: bool) -> Result<Vec<PathBuf>> {
		self.adapter.list_contents(path, deep).await
	}

	/// Move a file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to move the file.
	pub async fn r#move(&mut self, source: &Path, destination: &Path) -> Result<()> {
		self.adapter.r#move(source, destination).await
	}

	/// Copy a file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to copy the file.
	pub async fn copy(&mut self, source: &Path, destination: &Path) -> Result<()> {
		self.adapter.copy(source, destination).await
	}

	/// Get the date and time the file was last modified at.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the last modified date and time.
	pub async fn last_modified(&self, path: &Path) -> Result<SystemTime> {
		self.adapter.last_modified(path).await
	}

	/// Get the size of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the file size.
	pub async fn file_size(&self, path: &Path) -> Result<u64> {
		self.adapter.file_size(path).await
	}

	/// Get the mime type of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the mime type.
	pub async fn mime_type(&self, path: &Path) -> Result<Mime> {
		self.adapter.mime_type(path).await
	}

	/// Set the visibility of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to set the visibility.
	pub async fn set_visibility(&mut self, path: &Path, visibility: Visibility) -> Result<()> {
		self.adapter.set_visibility(path, visibility).await
	}

	/// Get the visibility of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the visibility.
	pub async fn visibility(&self, path: &Path) -> Result<Visibility> {
		self.adapter.visibility(path).await
	}

	/// Get the checksum of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the checksum.
	pub async fn checksum(&self, path: &Path) -> Result<String> {
		self.adapter.checksum(path).await
	}
}
