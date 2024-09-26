use std::{
	collections::HashMap,
	convert::Infallible,
	io::{Error, ErrorKind, Result},
	path::{Path, PathBuf},
	time::SystemTime,
};

use async_recursion::async_recursion;

use super::{Adapter, AdapterInit};
use crate::{contents::Contents, Visibility};

#[derive(Debug, Clone)]
struct File {
	content: Vec<u8>,
	visibility: Visibility,
	last_modified: SystemTime,
}

impl File {
	fn updated_now(mut self) -> Self {
		self.last_modified = SystemTime::now();

		self
	}
}

#[derive(Debug, Clone)]
#[allow(clippy::module_name_repetitions)]
pub struct MemoryAdapter {
	files: HashMap<PathBuf, File>,
	directory: HashMap<PathBuf, Vec<PathBuf>>,
}

impl MemoryAdapter {
	#[async_recursion]
	async fn get_files_deep(&self, path: &Path, deep: bool) -> Result<Vec<PathBuf>> {
		let mut contents = self
			.directory
			.get(path)
			.ok_or_else(|| Error::from(ErrorKind::NotFound))?
			.clone();

		if deep {
			for directory in self.directory.keys() {
				if directory.starts_with(path) && directory != path {
					contents.extend(self.get_files_deep(directory, deep).await?);
				}
			}
		}

		Ok(contents)
	}
}

impl AdapterInit for MemoryAdapter {
	type Config = ();
	type Error = Infallible;

	async fn new((): Self::Config) -> std::result::Result<Self, Self::Error> {
		Ok(Self {
			files: HashMap::new(),
			directory: HashMap::new(),
		})
	}
}

impl Adapter for MemoryAdapter {
	async fn file_exists(&self, path: &Path) -> Result<bool> {
		Ok(self.files.contains_key(path))
	}

	async fn directory_exists(&self, path: &Path) -> Result<bool> {
		Ok(self.directory.contains_key(path))
	}

	async fn write(&mut self, path: &Path, content: &[u8]) -> Result<()> {
		self.files.insert(
			path.to_path_buf(),
			File {
				visibility: Visibility::Public,
				last_modified: SystemTime::now(),
				content: content.as_ref().to_vec(),
			},
		);

		if let Some(parent) = path.parent() {
			self.directory
				.entry(parent.to_path_buf())
				.or_default()
				.push(path.to_path_buf());
		}

		Ok(())
	}

	async fn read(&self, path: &Path) -> Result<Contents> {
		let file = self
			.files
			.get(path)
			.ok_or_else(|| Error::from(ErrorKind::NotFound))?;

		Ok(Contents::from(file.content.clone()))
	}

	async fn delete(&mut self, path: &Path) -> Result<()> {
		if self.files.remove(path).is_none() {
			return Err(Error::from(ErrorKind::NotFound));
		}

		self.directory
			.entry(path.parent().unwrap().to_path_buf())
			.or_default()
			.retain(|file_path| file_path != path);

		Ok(())
	}

	async fn delete_directory(&mut self, path: &Path) -> Result<()> {
		self.directory
			.remove(path)
			.ok_or_else(|| Error::from(ErrorKind::NotFound))?;

		self.files
			.retain(|file_path, _| !file_path.starts_with(path));

		Ok(())
	}

	async fn create_directory(&mut self, path: &Path) -> Result<()> {
		let mut current_path = PathBuf::new();
		for component in path.components() {
			current_path.push(component);
			self.directory.entry(current_path.clone()).or_default();
		}

		Ok(())
	}

	async fn set_visibility(&mut self, path: &Path, visibility: Visibility) -> Result<()> {
		let Some(file) = self.files.get_mut(path) else {
			return Err(Error::from(ErrorKind::NotFound));
		};

		file.visibility = visibility;

		Ok(())
	}

	async fn visibility(&self, path: &Path) -> Result<Visibility> {
		let file = self
			.files
			.get(path)
			.ok_or_else(|| Error::from(ErrorKind::NotFound))?;

		Ok(file.visibility)
	}

	async fn mime_type(&self, path: &Path) -> Result<mime::Mime> {
		Ok(mime_guess::from_path(path).first_or_octet_stream())
	}

	async fn last_modified(&self, path: &Path) -> Result<std::time::SystemTime> {
		let file = self
			.files
			.get(path)
			.ok_or_else(|| Error::from(ErrorKind::NotFound))?;

		Ok(file.last_modified)
	}

	async fn file_size(&self, path: &Path) -> Result<u64> {
		let file = self
			.files
			.get(path)
			.ok_or_else(|| Error::from(ErrorKind::NotFound))?;

		Ok(file.content.len() as u64)
	}

	async fn list_contents(&self, path: &Path, deep: bool) -> Result<Vec<PathBuf>> {
		self.get_files_deep(path, deep).await
	}

	async fn r#move(&mut self, source: &Path, destination: &Path) -> Result<()> {
		self.copy(source, destination).await?;

		self.delete(source).await
	}

	async fn copy(&mut self, source: &Path, destination: &Path) -> Result<()> {
		let file = self
			.files
			.get(source)
			.ok_or_else(|| Error::from(ErrorKind::NotFound))?;

		self.files
			.insert(destination.to_path_buf(), file.clone().updated_now());

		if let Some(parent) = destination.parent() {
			self.directory
				.entry(parent.to_path_buf())
				.or_default()
				.push(destination.to_path_buf());
		}

		Ok(())
	}

	async fn checksum(&self, path: &Path) -> Result<String> {
		Ok(sha256::digest(self.read(path).await?.data))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_file_exists() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		assert!(!client
			.file_exists(Path::new("test_file_exists.txt"))
			.await
			.unwrap());

		client
			.write(Path::new("test_file_exists.txt"), b"Hello, world!")
			.await
			.unwrap();

		assert!(client
			.file_exists(Path::new("test_file_exists.txt"))
			.await
			.unwrap());

		client
			.delete(Path::new("test_file_exists.txt"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_directory_exists() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		assert!(!client
			.directory_exists(Path::new("test_directory_exists"))
			.await
			.unwrap());

		client
			.create_directory(Path::new("test_directory_exists"))
			.await
			.unwrap();

		assert!(client
			.directory_exists(Path::new("test_directory_exists"))
			.await
			.unwrap());

		client
			.delete_directory(Path::new("test_directory_exists"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_write() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		assert!(!client
			.file_exists(Path::new("test_write.txt"))
			.await
			.unwrap());

		client
			.write(Path::new("test_write.txt"), b"Hello, world!")
			.await
			.unwrap();

		assert_eq!(
			client.read(Path::new("test_write.txt")).await.unwrap().data,
			b"Hello, world!"
		);

		client.delete(Path::new("test_write.txt")).await.unwrap();
	}

	#[tokio::test]
	async fn test_read() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(Path::new("test_read.txt"), b"Hello, world!")
			.await
			.unwrap();

		assert_eq!(
			client.read(Path::new("test_read.txt")).await.unwrap().data,
			b"Hello, world!"
		);

		client.delete(Path::new("test_read.txt")).await.unwrap();
	}

	#[tokio::test]
	async fn test_delete() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(Path::new("test_delete.txt"), b"Hello, world!")
			.await
			.unwrap();

		client.delete(Path::new("test_delete.txt")).await.unwrap();

		assert!(!client
			.file_exists(Path::new("test_delete.txt"))
			.await
			.unwrap());
	}

	#[tokio::test]
	async fn test_delete_directory() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.create_directory(Path::new("test_delete_directory"))
			.await
			.unwrap();

		assert!(client
			.directory_exists(Path::new("test_delete_directory"))
			.await
			.unwrap());

		client
			.delete_directory(Path::new("test_delete_directory"))
			.await
			.unwrap();

		assert!(!client
			.directory_exists(Path::new("test_delete_directory"))
			.await
			.unwrap());
	}

	#[tokio::test]
	async fn test_create_directory() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		assert!(!client
			.directory_exists(Path::new("test_create_directory"))
			.await
			.unwrap());

		client
			.create_directory(Path::new("test_create_directory"))
			.await
			.unwrap();

		assert!(client
			.directory_exists(Path::new("test_create_directory"))
			.await
			.unwrap());

		client
			.delete_directory(Path::new("test_create_directory"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_create_directory_with_parents() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		assert!(!client
			.directory_exists(Path::new("test_create_directory_with_parents/test_dir"))
			.await
			.unwrap());

		client
			.create_directory(Path::new("test_create_directory_with_parents/test_dir"))
			.await
			.unwrap();

		assert!(client
			.directory_exists(Path::new("test_create_directory_with_parents/test_dir"))
			.await
			.unwrap());

		client
			.delete_directory(Path::new("test_create_directory_with_parents"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_set_visibility() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(Path::new("test_set_visibility.txt"), &[])
			.await
			.unwrap();

		assert_eq!(
			client
				.visibility(Path::new("test_set_visibility.txt"))
				.await
				.unwrap(),
			Visibility::Public
		);

		client
			.set_visibility(Path::new("test_set_visibility.txt"), Visibility::Private)
			.await
			.unwrap();

		assert_eq!(
			client
				.visibility(Path::new("test_set_visibility.txt"))
				.await
				.unwrap(),
			Visibility::Private
		);

		client
			.delete(Path::new("test_set_visibility.txt"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_visibility() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(Path::new("test_visibility.txt"), &[])
			.await
			.unwrap();

		assert_eq!(
			client
				.visibility(Path::new("test_visibility.txt"))
				.await
				.unwrap(),
			Visibility::Public
		);

		client
			.set_visibility(Path::new("test_visibility.txt"), Visibility::Private)
			.await
			.unwrap();

		assert_eq!(
			client
				.visibility(Path::new("test_visibility.txt"))
				.await
				.unwrap(),
			Visibility::Private
		);

		client
			.delete(Path::new("test_visibility.txt"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_mime_type() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(Path::new("test_mime.txt"), b"Hello, world!")
			.await
			.unwrap();

		assert_eq!(
			client.mime_type(Path::new("test_mime.txt")).await.unwrap(),
			mime::TEXT_PLAIN
		);

		client.delete(Path::new("test_mime.txt")).await.unwrap();
	}

	#[tokio::test]
	async fn test_last_modified() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(Path::new("test_last_modified.txt"), &[])
			.await
			.unwrap();

		let last_updated = match client
			.last_modified(Path::new("test_last_modified.txt"))
			.await
			.unwrap()
			.elapsed()
		{
			Ok(elapsed) => elapsed,
			Err(e) => e.duration(),
		};

		assert!(last_updated.as_secs() < 5);

		client
			.delete(Path::new("test_last_modified.txt"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_file_size() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(Path::new("test_file_size.txt"), b"Hello, world!")
			.await
			.unwrap();

		assert_eq!(
			client
				.file_size(Path::new("test_file_size.txt"))
				.await
				.unwrap(),
			13
		);

		client
			.delete(Path::new("test_file_size.txt"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_list_contents() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(
				Path::new("test_list_contents/test_file.txt"),
				b"Hello, world!",
			)
			.await
			.unwrap();
		client
			.write(
				Path::new("test_list_contents/test_recursive_dir/test_file.txt"),
				b"Hello, world!",
			)
			.await
			.unwrap();

		assert_eq!(
			client
				.list_contents(Path::new("test_list_contents"), false)
				.await
				.unwrap(),
			vec![PathBuf::from("test_list_contents/test_file.txt")]
		);
		assert_eq!(
			client
				.list_contents(Path::new("test_list_contents"), true)
				.await
				.unwrap(),
			vec![
				PathBuf::from("test_list_contents/test_file.txt"),
				PathBuf::from("test_list_contents/test_recursive_dir/test_file.txt")
			]
		);

		client
			.delete_directory(Path::new("test_list_contents"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_move() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(Path::new("test_move.txt"), b"Hello, world!")
			.await
			.unwrap();

		assert!(!client
			.file_exists(Path::new("test_move_destination.txt"))
			.await
			.unwrap());

		client
			.r#move(
				Path::new("test_move.txt"),
				Path::new("test_move_destination.txt"),
			)
			.await
			.unwrap();

		assert!(client
			.file_exists(Path::new("test_move_destination.txt"))
			.await
			.unwrap());
		assert!(!client
			.file_exists(Path::new("test_move.txt"))
			.await
			.unwrap());

		client
			.delete(Path::new("test_move_destination.txt"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_copy() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(Path::new("test_copy.txt"), b"Hello, world!")
			.await
			.unwrap();
		assert!(!client
			.file_exists(Path::new("test_copy_destination.txt"))
			.await
			.unwrap());

		client
			.copy(
				Path::new("test_copy.txt"),
				Path::new("test_copy_destination.txt"),
			)
			.await
			.unwrap();

		assert!(client
			.file_exists(Path::new("test_copy.txt"))
			.await
			.unwrap());
		assert!(client
			.file_exists(Path::new("test_copy_destination.txt"))
			.await
			.unwrap());

		client.delete(Path::new("test_copy.txt")).await.unwrap();
		client
			.delete(Path::new("test_copy_destination.txt"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_checksum() {
		let mut client = MemoryAdapter::new(()).await.unwrap();

		client
			.write(Path::new("test_checksum.txt"), b"Hello, world!")
			.await
			.unwrap();

		assert_eq!(
			client
				.checksum(Path::new("test_checksum.txt"))
				.await
				.unwrap(),
			"315f5bdb76d078c43b8ac0064e4a0164612b1fce77c869345bfc94c75894edd3"
		);

		client.delete(Path::new("test_checksum.txt")).await.unwrap();
	}
}
