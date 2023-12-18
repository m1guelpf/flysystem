#![warn(clippy::all, clippy::pedantic, clippy::nursery)]

use adapters::{Adapter, PublicUrlGenerator, TemporaryUrlGenerator};
use contents::Contents;
use mime::Mime;
use std::{
	path::{Path, PathBuf},
	time::Duration,
};

pub mod adapters;
mod contents;

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

/// Abstraction over a filesystem.
pub struct Filesystem<T: Adapter> {
	adapter: T,
}

impl<T: Adapter> Filesystem<T> {
	/// Create a new filesystem instance.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to initialize.
	pub async fn new(config: T::Config) -> Result<Self, T::Error> {
		Ok(Self {
			adapter: T::new(config).await?,
		})
	}

	/// Check if a file exists.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to check if the file exists.
	pub async fn file_exists(&self, path: &Path) -> Result<bool, T::Error> {
		self.adapter.file_exists(path).await
	}

	/// Check if a directory exists.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to check if the directory exists.
	pub async fn directory_exists(&self, path: &Path) -> Result<bool, T::Error> {
		self.adapter.directory_exists(path).await
	}

	/// Check if a file or directory exists.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to check if the file or directory exists.
	pub async fn has(&self, path: &Path) -> Result<bool, T::Error> {
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
	pub async fn write<C: AsRef<[u8]> + Send>(
		&mut self,
		path: &Path,
		contents: C,
	) -> Result<(), T::Error> {
		self.adapter.write(path, contents).await
	}

	/// Get the contents of a file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to read the file.
	pub async fn read<R: TryFrom<Contents>>(&mut self, path: &Path) -> Result<R, T::Error> {
		self.adapter.read(path).await
	}

	/// Delete a file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to delete the file or directory.
	pub async fn delete(&mut self, path: &Path) -> Result<(), T::Error> {
		self.adapter.delete(path).await
	}

	/// Delete a directory.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to delete the directory.
	pub async fn delete_directory(&mut self, path: &Path) -> Result<(), T::Error> {
		self.adapter.delete_directory(path).await
	}

	/// Create a directory.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to create the directory.
	pub async fn create_directory(&mut self, path: &Path) -> Result<(), T::Error> {
		self.adapter.create_directory(path).await
	}

	/// Get a list of files in a directory (optionally recursively).
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to list the contents of the directory.
	pub async fn list_contents(&self, path: &Path, deep: bool) -> Result<Vec<PathBuf>, T::Error> {
		self.adapter.list_contents(path, deep).await
	}

	/// Move a file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to move the file.
	pub async fn r#move(&mut self, source: &Path, destination: &Path) -> Result<(), T::Error> {
		self.adapter.r#move(source, destination).await
	}

	/// Copy a file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to copy the file.
	pub async fn copy(&mut self, source: &Path, destination: &Path) -> Result<(), T::Error> {
		self.adapter.copy(source, destination).await
	}

	/// Get the date and time the file was last modified at.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the last modified date and time.
	pub async fn last_modified(&self, path: &Path) -> Result<std::time::SystemTime, T::Error> {
		self.adapter.last_modified(path).await
	}

	/// Get the size of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the file size.
	pub async fn file_size(&self, path: &Path) -> Result<u64, T::Error> {
		self.adapter.file_size(path).await
	}

	/// Get the mime type of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the mime type.
	pub async fn mime_type(&self, path: &Path) -> Result<Mime, T::Error> {
		self.adapter.mime_type(path).await
	}

	/// Set the visibility of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to set the visibility.
	pub async fn set_visibility(
		&mut self,
		path: &Path,
		visibility: Visibility,
	) -> Result<(), T::Error> {
		self.adapter.set_visibility(path, visibility).await
	}

	/// Get the visibility of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the visibility.
	pub async fn visibility(&self, path: &Path) -> Result<Visibility, T::Error> {
		self.adapter.visibility(path).await
	}

	/// Get the checksum of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the checksum.
	pub async fn checksum(&self, path: &Path) -> Result<String, T::Error> {
		self.adapter.checksum(path).await
	}
}

impl<T: Adapter + PublicUrlGenerator> Filesystem<T> {
	/// Get the public URL of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the public URL.
	pub async fn public_url(
		&self,
		path: &Path,
	) -> Result<String, <T as PublicUrlGenerator>::Error> {
		self.adapter.public_url(path).await
	}
}

impl<T: Adapter + TemporaryUrlGenerator> Filesystem<T> {
	/// Get a temporary URL of the file.
	///
	/// # Errors
	///
	/// This function will return an error if the adapter fails to get the temporary URL.
	pub async fn temporary_url(
		&self,
		path: &Path,
		expires_in: Duration,
	) -> Result<String, <T as TemporaryUrlGenerator>::Error> {
		self.adapter.temporary_url(path, expires_in).await
	}
}
