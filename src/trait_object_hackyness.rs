use mime::Mime;
use std::{
	fmt::Debug,
	io::Result,
	path::{Path, PathBuf},
	time::SystemTime,
};

use crate::{adapters::Adapter, contents::Contents, Visibility};

#[async_trait::async_trait]
pub trait AdapterObject: Debug + Send + Sync {
	/// Check if a file exists.
	async fn file_exists(&self, path: &Path) -> Result<bool>;

	/// Check if a directory exists.
	async fn directory_exists(&self, path: &Path) -> Result<bool>;

	/// Write to a file.
	async fn write(&mut self, path: &Path, content: &[u8]) -> Result<()>;

	/// Read a file.
	async fn read(&self, path: &Path) -> Result<Contents>;

	/// Delete a file.
	async fn delete(&mut self, path: &Path) -> Result<()>;

	/// Delete a directory.
	async fn delete_directory(&mut self, path: &Path) -> Result<()>;

	/// Create a directory.
	async fn create_directory(&mut self, path: &Path) -> Result<()>;

	/// Set the visibility of a file.
	async fn set_visibility(&mut self, path: &Path, visibility: Visibility) -> Result<()>;

	/// Get the visibility of a file.
	async fn visibility(&self, path: &Path) -> Result<Visibility>;

	/// Get the MIME type of a file.
	async fn mime_type(&self, path: &Path) -> Result<Mime>;

	/// Get the last modified time of a file.
	async fn last_modified(&self, path: &Path) -> Result<SystemTime>;

	/// Get the size of a file.
	async fn file_size(&self, path: &Path) -> Result<u64>;

	/// List the contents of a directory.
	async fn list_contents(&self, path: &Path, deep: bool) -> Result<Vec<PathBuf>>;

	/// Move a file.
	async fn r#move(&mut self, source: &Path, destination: &Path) -> Result<()>;

	/// Copy a file.
	async fn copy(&mut self, source: &Path, destination: &Path) -> Result<()>;

	/// Get the checksum of a file.
	async fn checksum(&self, path: &Path) -> Result<String>;
}

#[async_trait::async_trait]
impl<A: Adapter + Send> AdapterObject for A {
	async fn file_exists(&self, path: &Path) -> Result<bool> {
		self.file_exists(path).await
	}

	async fn directory_exists(&self, path: &Path) -> Result<bool> {
		self.directory_exists(path).await
	}

	async fn write(&mut self, path: &Path, content: &[u8]) -> Result<()> {
		self.write(path, content).await
	}

	async fn read(&self, path: &Path) -> Result<Contents> {
		self.read(path).await
	}

	async fn delete(&mut self, path: &Path) -> Result<()> {
		self.delete(path).await
	}

	async fn delete_directory(&mut self, path: &Path) -> Result<()> {
		self.delete_directory(path).await
	}

	async fn create_directory(&mut self, path: &Path) -> Result<()> {
		self.create_directory(path).await
	}

	async fn set_visibility(&mut self, path: &Path, visibility: Visibility) -> Result<()> {
		self.set_visibility(path, visibility).await
	}

	async fn visibility(&self, path: &Path) -> Result<Visibility> {
		self.visibility(path).await
	}

	async fn mime_type(&self, path: &Path) -> Result<Mime> {
		self.mime_type(path).await
	}

	async fn last_modified(&self, path: &Path) -> Result<SystemTime> {
		self.last_modified(path).await
	}

	async fn file_size(&self, path: &Path) -> Result<u64> {
		self.file_size(path).await
	}

	async fn list_contents(&self, path: &Path, deep: bool) -> Result<Vec<PathBuf>> {
		self.list_contents(path, deep).await
	}

	async fn r#move(&mut self, source: &Path, destination: &Path) -> Result<()> {
		self.r#move(source, destination).await
	}

	async fn copy(&mut self, source: &Path, destination: &Path) -> Result<()> {
		self.copy(source, destination).await
	}

	async fn checksum(&self, path: &Path) -> Result<String> {
		self.checksum(path).await
	}
}
