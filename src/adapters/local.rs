use async_recursion::async_recursion;
use mime::Mime;
use std::{
	fs::Permissions,
	io::{self, Result},
	os::unix::fs::PermissionsExt,
	path::{Path, PathBuf},
	time::SystemTime,
};
use tokio::fs;

use super::{Adapter, AdapterInit};
use crate::{contents::Contents, Resource, Visibility};

#[derive(Debug, Clone)]
pub struct Config {
	pub location: PathBuf,
	pub lazy_root_creation: bool,
}

#[derive(Debug, Clone)]
#[allow(clippy::module_name_repetitions)]
pub struct LocalAdapter {
	location: PathBuf,
}

impl LocalAdapter {
	async fn create_parent_if_not_exists(&self, path: &Path) -> Result<()> {
		if let Some(parent) = path.parent() {
			if !parent.exists() {
				fs::create_dir_all(parent).await?;
			}
		}

		Ok(())
	}

	#[async_recursion]
	async fn get_files_deep(path: &Path, deep: bool) -> Result<Vec<PathBuf>> {
		let mut paths = Vec::new();
		let mut dir = fs::read_dir(path).await?;

		while let Some(entry) = dir.next_entry().await? {
			let entry_path = entry.path();

			if entry_path.is_dir() {
				if deep {
					paths.append(&mut Self::get_files_deep(&entry_path, deep).await?);
				}
			} else {
				paths.push(entry_path);
			}
		}

		Ok(paths)
	}
}

impl AdapterInit for LocalAdapter {
	type Config = Config;
	type Error = io::Error;

	async fn new(config: Self::Config) -> Result<Self> {
		if !config.location.exists() {
			if !config.lazy_root_creation {
				return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "The root at {:?} does not exist. You can manually create it or enable lazy root creation.",
                        config.location
                    ),
                ));
			}

			fs::create_dir_all(&config.location).await?;
		}

		Ok(Self {
			location: config.location,
		})
	}
}

impl Adapter for LocalAdapter {
	async fn file_exists(&self, path: &Path) -> Result<bool> {
		if !path.is_file() {
			return Ok(false);
		}

		Ok(path.exists())
	}

	async fn directory_exists(&self, path: &Path) -> Result<bool> {
		if !path.is_dir() {
			return Ok(false);
		}

		Ok(path.exists())
	}

	async fn write(&mut self, path: &Path, content: &[u8]) -> Result<()> {
		let path = self.location.join(path);
		self.create_parent_if_not_exists(&path).await?;

		fs::write(path, content).await
	}

	async fn read(&self, path: &Path) -> Result<Contents> {
		let path = self.location.join(path);

		Ok(Contents::from(fs::read(path).await?))
	}

	async fn delete(&mut self, path: &Path) -> Result<()> {
		let path = self.location.join(path);

		fs::remove_file(path).await
	}

	async fn delete_directory(&mut self, path: &Path) -> Result<()> {
		let path = self.location.join(path);

		fs::remove_dir_all(path).await
	}

	async fn create_directory(&mut self, path: &Path) -> Result<()> {
		let path = self.location.join(path);

		fs::create_dir_all(path).await
	}

	async fn set_visibility(&mut self, path: &Path, visibility: Visibility) -> Result<()> {
		let path = self.location.join(path);
		let permissions = Permissions::from_mode(visibility_to_unix((&path).into(), visibility));

		fs::set_permissions(path, permissions).await?;

		Ok(())
	}

	async fn visibility(&self, path: &Path) -> Result<Visibility> {
		let path = self.location.join(path);

		Ok(unix_to_visibility(
			(&path).into(),
			fs::metadata(path).await?.permissions().mode(),
		))
	}

	async fn mime_type(&self, path: &Path) -> Result<Mime> {
		let path = self.location.join(path);

		Ok(mime_guess::from_path(path).first_or_octet_stream())
	}

	async fn last_modified(&self, path: &Path) -> Result<SystemTime> {
		let path = self.location.join(path);
		let metadata = fs::metadata(path).await?;

		metadata.modified()
	}

	async fn file_size(&self, path: &Path) -> Result<u64> {
		let path = self.location.join(path);
		let metadata = fs::metadata(path).await?;

		Ok(metadata.len())
	}

	async fn list_contents(&self, path: &Path, deep: bool) -> Result<Vec<PathBuf>> {
		let path = self.location.join(path);

		if !path.is_dir() {
			return Err(io::Error::new(
				io::ErrorKind::NotFound,
				format!("The path {path:?} is not a directory."),
			));
		}

		Self::get_files_deep(&path, deep).await
	}

	async fn r#move(&mut self, source: &Path, destination: &Path) -> Result<()> {
		let source = self.location.join(source);
		let destination = self.location.join(destination);

		self.create_parent_if_not_exists(&destination).await?;

		fs::rename(source, destination).await
	}

	async fn copy(&mut self, source: &Path, destination: &Path) -> Result<()> {
		let source = self.location.join(source);
		let destination = self.location.join(destination);

		fs::copy(source, destination).await?;

		Ok(())
	}

	async fn checksum(&self, path: &Path) -> Result<String> {
		Ok(sha256::digest(self.read(path).await?.data))
	}
}

impl From<&PathBuf> for Resource {
	fn from(path: &PathBuf) -> Self {
		if path.is_file() {
			Self::File
		} else {
			Self::Directory
		}
	}
}

const fn visibility_to_unix(resource: Resource, visibility: Visibility) -> u32 {
	match (resource, visibility) {
		(Resource::File, Visibility::Public) => 0o644,
		(Resource::File, Visibility::Private) => 0o600,
		(Resource::Directory, Visibility::Public) => 0o755,
		(Resource::Directory, Visibility::Private) => 0o700,
	}
}

const fn unix_to_visibility(resource: Resource, unix: u32) -> Visibility {
	match (resource, unix) {
		(Resource::Directory, 0o700) | (Resource::File, 0o600) => Visibility::Private,
		_ => Visibility::Public,
	}
}

#[cfg(test)]
mod tests {
	use std::os::unix::fs::PermissionsExt;

	use super::*;

	#[tokio::test]
	async fn test_file_exists() {
		let adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write("/tmp/flysystem_tests/test_file_exists.txt", "Hello, world!").unwrap();

		assert!(adapter
			.file_exists(Path::new("/tmp/flysystem_tests/test_file_exists.txt"))
			.await
			.unwrap());
		assert!(!adapter
			.file_exists(Path::new("non-existent-file.txt"))
			.await
			.unwrap());

		std::fs::remove_file("/tmp/flysystem_tests/test_file_exists.txt").unwrap();
	}

	#[tokio::test]
	async fn test_directory_exists() {
		let adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::create_dir("/tmp/flysystem_tests/test").unwrap();

		assert!(adapter
			.directory_exists(Path::new("/tmp/flysystem_tests/test"))
			.await
			.unwrap());
		assert!(!adapter
			.directory_exists(Path::new("non-existent-directory"))
			.await
			.unwrap());

		std::fs::remove_dir("/tmp/flysystem_tests/test").unwrap();
	}

	#[tokio::test]
	async fn test_write() {
		let mut adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		adapter
			.write(Path::new("test_write.txt"), b"Hello, world!")
			.await
			.unwrap();

		assert_eq!(
			std::fs::read_to_string("/tmp/flysystem_tests/test_write.txt").unwrap(),
			"Hello, world!"
		);

		std::fs::remove_file("/tmp/flysystem_tests/test_write.txt").unwrap();
	}

	#[tokio::test]
	async fn test_read() {
		let adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write("/tmp/flysystem_tests/test_read.txt", "Hello, world!").unwrap();

		assert_eq!(
			adapter.read(Path::new("test_read.txt")).await.unwrap().data,
			b"Hello, world!"
		);

		std::fs::remove_file("/tmp/flysystem_tests/test_read.txt").unwrap();
	}

	#[tokio::test]
	async fn test_delete() {
		let mut adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write("/tmp/flysystem_tests/test.txt", "Hello, world!").unwrap();

		adapter.delete(Path::new("test.txt")).await.unwrap();

		assert!(!Path::new("/tmp/flysystem_tests/test.txt").exists());
	}

	#[tokio::test]
	async fn test_delete_directory() {
		let mut adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::create_dir("/tmp/flysystem_tests/test_dir").unwrap();

		adapter
			.delete_directory(Path::new("test_dir"))
			.await
			.unwrap();

		assert!(!Path::new("/tmp/flysystem_tests/test_dir").exists());
	}

	#[tokio::test]
	async fn test_create_directory() {
		let mut adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		assert!(!Path::new("/tmp/flysystem_tests/test_dir_2").exists());

		adapter
			.create_directory(Path::new("test_dir_2"))
			.await
			.unwrap();

		assert!(Path::new("/tmp/flysystem_tests/test_dir_2").exists());

		std::fs::remove_dir("/tmp/flysystem_tests/test_dir_2").unwrap();
	}

	#[tokio::test]
	async fn test_create_directory_with_parents() {
		let mut adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		assert!(!Path::new("/tmp/flysystem_tests/test_create_directory_with_parents").exists());

		adapter
			.create_directory(Path::new("test_create_directory_with_parents/test_dir"))
			.await
			.unwrap();

		assert!(
			Path::new("/tmp/flysystem_tests/test_create_directory_with_parents/test_dir").exists()
		);

		std::fs::remove_dir_all("/tmp/flysystem_tests/test_create_directory_with_parents").unwrap();
	}

	#[tokio::test]
	async fn test_set_visibility() {
		let mut adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write(
			"/tmp/flysystem_tests/test_set_visibility.txt",
			"Hello, world!",
		)
		.unwrap();

		adapter
			.set_visibility(Path::new("test_set_visibility.txt"), Visibility::Private)
			.await
			.unwrap();

		assert_eq!(
			std::fs::metadata("/tmp/flysystem_tests/test_set_visibility.txt")
				.unwrap()
				.permissions()
				.mode() & 0o777,
			0o777
		);

		std::fs::remove_file("/tmp/flysystem_tests/test_set_visibility.txt").unwrap();
	}

	#[tokio::test]
	async fn test_visibility() {
		let adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write("/tmp/flysystem_tests/test_perm.txt", "Hello, world!").unwrap();

		assert_eq!(
			adapter
				.visibility(Path::new("test_perm.txt"))
				.await
				.unwrap(),
			Visibility::Public
		);

		std::fs::remove_file("/tmp/flysystem_tests/test_perm.txt").unwrap();
	}

	#[tokio::test]
	async fn test_mime_type() {
		let adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write("/tmp/flysystem_tests/test_mime.txt", "Hello, world!").unwrap();

		assert_eq!(
			adapter.mime_type(Path::new("test_mime.txt")).await.unwrap(),
			mime::TEXT_PLAIN
		);

		std::fs::remove_file("/tmp/flysystem_tests/test_mime.txt").unwrap();
	}

	#[tokio::test]
	async fn test_last_modified() {
		let adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write(
			"/tmp/flysystem_tests/test_last_modified.txt",
			"Hello, world!",
		)
		.unwrap();

		assert!(
			adapter
				.last_modified(Path::new("test_last_modified.txt"))
				.await
				.unwrap()
				.elapsed()
				.unwrap()
				.as_secs() < 1
		);

		std::fs::remove_file("/tmp/flysystem_tests/test_last_modified.txt").unwrap();
	}

	#[tokio::test]
	async fn test_file_size() {
		let adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write("/tmp/flysystem_tests/test_file_size.txt", "Hello, world!").unwrap();

		assert_eq!(
			adapter
				.file_size(Path::new("test_file_size.txt"))
				.await
				.unwrap(),
			13
		);

		std::fs::remove_file("/tmp/flysystem_tests/test_file_size.txt").unwrap();
	}

	#[tokio::test]
	async fn test_list_contents() {
		let adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::create_dir_all("/tmp/flysystem_tests/test_list_contents").unwrap();
		std::fs::write(
			"/tmp/flysystem_tests/test_list_contents/test_file.txt",
			"Hello, world!",
		)
		.unwrap();
		std::fs::create_dir_all("/tmp/flysystem_tests/test_list_contents/test_recursive_dir")
			.unwrap();
		std::fs::write(
			"/tmp/flysystem_tests/test_list_contents/test_recursive_dir/test_file.txt",
			"Hello, world!",
		)
		.unwrap();

		assert_eq!(
			adapter
				.list_contents(Path::new("test_list_contents"), false)
				.await
				.unwrap(),
			vec![PathBuf::from(
				"/tmp/flysystem_tests/test_list_contents/test_file.txt"
			)]
		);
		assert_eq!(
			adapter
				.list_contents(Path::new("test_list_contents"), true)
				.await
				.unwrap(),
			vec![
				PathBuf::from("/tmp/flysystem_tests/test_list_contents/test_file.txt"),
				PathBuf::from(
					"/tmp/flysystem_tests/test_list_contents/test_recursive_dir/test_file.txt"
				)
			]
		);

		std::fs::remove_dir_all("/tmp/flysystem_tests/test_list_contents").unwrap();
	}

	#[tokio::test]
	async fn test_move() {
		let mut adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write("/tmp/flysystem_tests/test_move.txt", "Hello, world!").unwrap();

		adapter
			.r#move(
				Path::new("test_move.txt"),
				Path::new("test_move_destination.txt"),
			)
			.await
			.unwrap();

		assert!(!Path::new("/tmp/flysystem_tests/test_move.txt").exists());
		assert!(Path::new("/tmp/flysystem_tests/test_move_destination.txt").exists());

		std::fs::remove_file("/tmp/flysystem_tests/test_move_destination.txt").unwrap();
	}

	#[tokio::test]
	async fn test_copy() {
		let mut adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write("/tmp/flysystem_tests/test_copy.txt", "Hello, world!").unwrap();
		assert!(!Path::new("/tmp/flysystem_tests/test_copy_destination.txt").exists());

		adapter
			.copy(
				Path::new("test_copy.txt"),
				Path::new("test_copy_destination.txt"),
			)
			.await
			.unwrap();

		assert!(Path::new("/tmp/flysystem_tests/test_copy.txt").exists());
		assert!(Path::new("/tmp/flysystem_tests/test_copy_destination.txt").exists());

		std::fs::remove_file("/tmp/flysystem_tests/test_copy.txt").unwrap();
		std::fs::remove_file("/tmp/flysystem_tests/test_copy_destination.txt").unwrap();
	}

	#[tokio::test]
	async fn test_checksum() {
		let client = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/"),
			lazy_root_creation: true,
		})
		.await
		.unwrap();

		std::fs::write("/tmp/flysystem_tests/test_checksum.txt", "Hello, world!").unwrap();

		assert_eq!(
			client
				.checksum(Path::new("test_checksum.txt"))
				.await
				.unwrap(),
			"315f5bdb76d078c43b8ac0064e4a0164612b1fce77c869345bfc94c75894edd3"
		);

		std::fs::remove_file("/tmp/flysystem_tests/test_checksum.txt").unwrap();
	}

	#[tokio::test]
	async fn test_new_with_non_existent_root() {
		let adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/non_existent_root/"),
			lazy_root_creation: false,
		})
		.await;

		assert!(adapter.is_err());
	}

	#[tokio::test]
	async fn test_new_with_non_existent_root_and_lazy_root_creation() {
		let adapter = LocalAdapter::new(Config {
			location: PathBuf::from("/tmp/flysystem_tests/non_existent_root/"),
			lazy_root_creation: true,
		})
		.await;

		assert!(adapter.is_ok());

		std::fs::remove_dir("/tmp/flysystem_tests/non_existent_root").unwrap();
	}
}
