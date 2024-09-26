use mime::Mime;
use std::{
	error::Error,
	fmt::Debug,
	future::Future,
	io::Result,
	path::{Path, PathBuf},
	time::{Duration, SystemTime},
};
use url::Url;

#[cfg(feature = "local")]
pub mod local;
pub mod memory;
#[cfg(feature = "s3")]
pub mod s3;

#[cfg(feature = "local")]
pub use local::LocalAdapter;
pub use memory::MemoryAdapter;
#[cfg(feature = "s3")]
pub use s3::S3Adapter;

use crate::{contents::Contents, Visibility};

pub trait AdapterInit: Adapter + Sized + 'static {
	type Config: Clone + Send + Sized;
	type Error: Debug + Error + Send + Sized;

	/// Create a new adapter instance.
	fn new(
		config: Self::Config,
	) -> impl Future<Output = std::result::Result<Self, Self::Error>> + Send;
}

/// A storage adapter.
pub trait Adapter: Debug + Send + Sync {
	// /// The configuration this adapter requires.
	// type Config: Clone + Send + Sized;

	// /// Create a new adapter instance.
	// fn new(config: Self::Config) -> impl Future<Output = Result<Self, Self::Error>> + Send;

	/// Check if a file exists.
	fn file_exists(&self, path: &Path) -> impl Future<Output = Result<bool>> + Send;

	/// Check if a directory exists.
	fn directory_exists(&self, path: &Path) -> impl Future<Output = Result<bool>> + Send;

	/// Write to a file.
	fn write(&mut self, path: &Path, content: &[u8]) -> impl Future<Output = Result<()>> + Send;

	/// Read a file.
	fn read(&self, path: &Path) -> impl Future<Output = Result<Contents>> + Send;

	/// Delete a file.
	fn delete(&mut self, path: &Path) -> impl Future<Output = Result<()>> + Send;

	/// Delete a directory.
	fn delete_directory(&mut self, path: &Path) -> impl Future<Output = Result<()>> + Send;

	/// Create a directory.
	fn create_directory(&mut self, path: &Path) -> impl Future<Output = Result<()>> + Send;

	/// Set the visibility of a file.
	fn set_visibility(
		&mut self,
		path: &Path,
		visibility: Visibility,
	) -> impl Future<Output = Result<()>> + Send;

	/// Get the visibility of a file.
	fn visibility(&self, path: &Path) -> impl Future<Output = Result<Visibility>> + Send;

	/// Get the MIME type of a file.
	fn mime_type(&self, path: &Path) -> impl Future<Output = Result<Mime>> + Send;

	/// Get the last modified time of a file.
	fn last_modified(&self, path: &Path) -> impl Future<Output = Result<SystemTime>> + Send;

	/// Get the size of a file.
	fn file_size(&self, path: &Path) -> impl Future<Output = Result<u64>> + Send;

	/// List the contents of a directory.
	fn list_contents(
		&self,
		path: &Path,
		deep: bool,
	) -> impl Future<Output = Result<Vec<PathBuf>>> + Send;

	/// Move a file.
	fn r#move(
		&mut self,
		source: &Path,
		destination: &Path,
	) -> impl Future<Output = Result<()>> + Send;

	/// Copy a file.
	fn copy(
		&mut self,
		source: &Path,
		destination: &Path,
	) -> impl Future<Output = Result<()>> + Send;

	/// Get the checksum of a file.
	fn checksum(&self, path: &Path) -> impl Future<Output = Result<String>> + Send;
}

pub trait PublicUrlGenerator {
	type Error;

	/// Get the public URL of a file.
	fn public_url(
		&self,
		path: &Path,
	) -> impl Future<Output = std::result::Result<String, Self::Error>> + Send;
}

pub trait TemporaryUrlGenerator {
	/// Get a temporary URL of a file.
	fn temporary_url(
		&self,
		path: &Path,
		expires_in: Duration,
	) -> impl Future<Output = Result<Url>> + Send;
}
