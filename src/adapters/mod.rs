use mime::Mime;
use std::{
	error::Error,
	fmt::Debug,
	future::Future,
	path::{Path, PathBuf},
	time::{Duration, SystemTime},
};

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

/// A storage adapter.
pub trait Adapter: Clone + Sized + Send + Sync {
	/// The error type returned by the adapter.
	type Error: Debug + Error + Send + Sized;

	/// The configuration this adapter requires.
	type Config: Clone + Send + Sized;

	/// Create a new adapter instance.
	fn new(config: Self::Config) -> impl Future<Output = Result<Self, Self::Error>> + Send;

	/// Check if a file exists.
	fn file_exists(&self, path: &Path) -> impl Future<Output = Result<bool, Self::Error>> + Send;

	/// Check if a directory exists.
	fn directory_exists(
		&self,
		path: &Path,
	) -> impl Future<Output = Result<bool, Self::Error>> + Send;

	/// Write to a file.
	fn write<C: AsRef<[u8]> + Send>(
		&mut self,
		path: &Path,
		content: C,
	) -> impl Future<Output = Result<(), Self::Error>> + Send;

	/// Read a file.
	fn read<T: TryFrom<Contents>>(
		&self,
		path: &Path,
	) -> impl Future<Output = Result<T, Self::Error>> + Send;

	/// Delete a file.
	fn delete(&mut self, path: &Path) -> impl Future<Output = Result<(), Self::Error>> + Send;

	/// Delete a directory.
	fn delete_directory(
		&mut self,
		path: &Path,
	) -> impl Future<Output = Result<(), Self::Error>> + Send;

	/// Create a directory.
	fn create_directory(
		&mut self,
		path: &Path,
	) -> impl Future<Output = Result<(), Self::Error>> + Send;

	/// Set the visibility of a file.
	fn set_visibility(
		&mut self,
		path: &Path,
		visibility: Visibility,
	) -> impl Future<Output = Result<(), Self::Error>> + Send;

	/// Get the visibility of a file.
	fn visibility(
		&self,
		path: &Path,
	) -> impl Future<Output = Result<Visibility, Self::Error>> + Send;

	/// Get the MIME type of a file.
	fn mime_type(&self, path: &Path) -> impl Future<Output = Result<Mime, Self::Error>> + Send;

	/// Get the last modified time of a file.
	fn last_modified(
		&self,
		path: &Path,
	) -> impl Future<Output = Result<SystemTime, Self::Error>> + Send;

	/// Get the size of a file.
	fn file_size(&self, path: &Path) -> impl Future<Output = Result<u64, Self::Error>> + Send;

	/// List the contents of a directory.
	fn list_contents(
		&self,
		path: &Path,
		deep: bool,
	) -> impl Future<Output = Result<Vec<PathBuf>, Self::Error>> + Send;

	/// Move a file.
	fn r#move(
		&mut self,
		source: &Path,
		destination: &Path,
	) -> impl Future<Output = Result<(), Self::Error>> + Send;

	/// Copy a file.
	fn copy(
		&mut self,
		source: &Path,
		destination: &Path,
	) -> impl Future<Output = Result<(), Self::Error>> + Send;

	/// Get the checksum of a file.
	fn checksum(&self, path: &Path) -> impl Future<Output = Result<String, Self::Error>> + Send;
}

pub trait PublicUrlGenerator {
	type Error;

	/// Get the public URL of a file.
	fn public_url(&self, path: &Path) -> impl Future<Output = Result<String, Self::Error>> + Send;
}

pub trait TemporaryUrlGenerator {
	type Error;

	/// Get a temporary URL of a file.
	fn temporary_url(
		&self,
		path: &Path,
		expires_in: Duration,
	) -> impl Future<Output = Result<String, Self::Error>> + Send;
}
