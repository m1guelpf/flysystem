use aws_sdk_s3::{
	config::Credentials,
	error::SdkError,
	operation::get_object_acl::GetObjectAclOutput,
	presigning::PresigningConfig,
	primitives::ByteStream,
	types::{Delete, ObjectCannedAcl, ObjectIdentifier, Permission},
	Client,
};
use aws_types::region::Region;
use mime::Mime;
use std::{
	convert::Infallible,
	io::{Error, ErrorKind, Result},
	path::{Path, PathBuf},
	str::FromStr,
	time::{Duration, SystemTime},
};
use url::Url;

use super::{Adapter, AdapterInit, TemporaryUrlGenerator};
use crate::{contents::Contents, Visibility};

#[derive(Debug, Clone, Default)]
pub struct Config {
	pub bucket: String,
	pub region: String,
	pub endpoint: String,
	pub access_key: String,
	pub secret_key: String,
}

#[derive(Debug, Clone)]
#[allow(clippy::module_name_repetitions)]
pub struct S3Adapter {
	client: Client,
	bucket: String,
}

impl AdapterInit for S3Adapter {
	type Error = Infallible;
	type Config = Config;

	async fn new(config: Self::Config) -> std::result::Result<Self, Self::Error> {
		let cred = Credentials::new(config.access_key, config.secret_key, None, None, "custom");

		Ok(Self {
			bucket: config.bucket,
			client: Client::from_conf(
				aws_sdk_s3::Config::builder()
					.force_path_style(true)
					.credentials_provider(cred)
					.endpoint_url(config.endpoint)
					.region(Region::new(config.region))
					.build(),
			),
		})
	}
}

impl Adapter for S3Adapter {
	async fn file_exists(&self, path: &Path) -> Result<bool> {
		let request = self
			.client
			.head_object()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.send()
			.await;

		match request {
			Ok(_) => Ok(true),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_not_found() {
					return Ok(false);
				}

				Err(Error::new(ErrorKind::Other, error.into_err()))
			},
			Err(e) => Err(Error::new(ErrorKind::Other, e)),
		}
	}

	async fn directory_exists(&self, path: &Path) -> Result<bool> {
		let request = self
			.client
			.list_objects_v2()
			.bucket(&self.bucket)
			.prefix(format!(
				"{}/",
				path.to_str()
					.ok_or_else(
						|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8",)
					)?
			))
			.max_keys(1)
			.delimiter('/')
			.send()
			.await;

		match request {
			Ok(request) => Ok(request.contents.is_some() || request.common_prefixes.is_some()),
			Err(SdkError::ServiceError(error)) => {
				if error.err().meta().code() == Some("NoSuchKey") {
					return Ok(false);
				}

				Err(Error::new(ErrorKind::Other, error.into_err()))
			},
			Err(e) => Err(Error::new(ErrorKind::Other, e)),
		}
	}

	async fn write(&mut self, path: &Path, content: &[u8]) -> Result<()> {
		self.client
			.put_object()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.body(ByteStream::from(content.as_ref().to_vec()))
			.content_type(
				mime_guess::from_path(path)
					.first_or_octet_stream()
					.to_string(),
			)
			.send()
			.await
			.map_err(|e| Error::new(ErrorKind::Other, e))?;

		Ok(())
	}

	async fn read(&self, path: &Path) -> Result<Contents> {
		let request = match self
			.client
			.get_object()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.send()
			.await
		{
			Ok(request) => request,
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_no_such_key() {
					return Err(Error::from(ErrorKind::NotFound));
				}

				return Err(Error::new(ErrorKind::Other, error.into_err()));
			},
			Err(e) => return Err(Error::new(ErrorKind::Other, e)),
		};

		Ok(Contents::from_bytestream(request.body).await?)
	}

	async fn delete_directory(&mut self, path: &Path) -> Result<()> {
		let matching_files = self.list_contents(path, true).await?;

		self.client
			.delete_objects()
			.bucket(&self.bucket)
			.delete(
				Delete::builder()
					.set_objects(Some(
						matching_files
							.iter()
							.map(|path| {
								ObjectIdentifier::builder()
									.key(path.to_str().unwrap().to_string())
									.build()
									.unwrap()
							})
							.collect(),
					))
					.build()
					.unwrap(),
			)
			.send()
			.await
			.map_err(|e| Error::new(ErrorKind::Other, e))?;

		Ok(())
	}

	async fn create_directory(&mut self, path: &Path) -> Result<()> {
		self.client
			.put_object()
			.bucket(&self.bucket)
			.key(format!(
				"{}/",
				path.to_str()
					.ok_or_else(
						|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8",)
					)?
			))
			.body(ByteStream::default())
			.send()
			.await
			.map_err(|e| Error::new(ErrorKind::Other, e))?;

		Ok(())
	}

	/// Set the visibility of a file.
	///
	/// Note that some S3 providers (like Minio) don't implement this feature.
	async fn set_visibility(&mut self, path: &Path, visibility: Visibility) -> Result<()> {
		let response = self
			.client
			.put_object_acl()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.acl(visibility.into())
			.send()
			.await;

		match response {
			Ok(_) => Ok(()),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_no_such_key() {
					return Err(Error::from(ErrorKind::NotFound));
				}

				Err(Error::new(ErrorKind::Other, error.into_err()))
			},
			Err(e) => Err(Error::new(ErrorKind::Other, e)),
		}
	}

	async fn visibility(&self, path: &Path) -> Result<Visibility> {
		let response = self
			.client
			.get_object_acl()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.send()
			.await;

		match response {
			Ok(response) => Ok(response.into()),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_no_such_key() {
					return Err(Error::from(ErrorKind::NotFound));
				}

				Err(Error::new(ErrorKind::Other, error.into_err()))
			},
			Err(e) => Err(Error::new(ErrorKind::Other, e)),
		}
	}

	async fn mime_type(&self, path: &Path) -> Result<Mime> {
		let response = self
			.client
			.head_object()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.send()
			.await;

		match response {
			Ok(response) => Ok(response
				.content_type()
				.map(Mime::from_str)
				.ok_or_else(|| Error::from(ErrorKind::NotFound))?
				.map_err(|e| Error::new(ErrorKind::Other, e))?),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_not_found() {
					return Err(Error::from(ErrorKind::NotFound));
				}

				Err(Error::new(ErrorKind::Other, error.into_err()))
			},
			Err(e) => Err(Error::new(ErrorKind::Other, e)),
		}
	}

	async fn last_modified(&self, path: &Path) -> Result<SystemTime> {
		let response = self
			.client
			.head_object()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.send()
			.await;

		match response {
			Ok(response) => Ok(SystemTime::try_from(response.last_modified.ok_or_else(|| {
				Error::new(
					ErrorKind::Other,
					"S3 did not return a Last-Modified header.",
				)
			})?)
			.map_err(|e| Error::new(ErrorKind::Other, e))?),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_not_found() {
					return Err(Error::from(ErrorKind::NotFound));
				}

				Err(Error::new(ErrorKind::Other, error.into_err()))
			},
			Err(e) => Err(Error::new(ErrorKind::Other, e)),
		}
	}

	async fn file_size(&self, path: &Path) -> Result<u64> {
		let response = self
			.client
			.head_object()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.send()
			.await;

		match response {
			#[allow(clippy::cast_sign_loss)]
			Ok(response) => Ok(response.content_length.ok_or_else(|| {
				Error::new(
					ErrorKind::Other,
					"S3 did not return a Content-Length header",
				)
			})? as u64),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_not_found() {
					return Err(Error::from(ErrorKind::NotFound));
				}

				Err(Error::new(ErrorKind::Other, error.into_err()))
			},
			Err(e) => Err(Error::new(ErrorKind::Other, e)),
		}
	}

	/// Delete a file from the filesystem.
	///
	/// Note that some S3 providers will return a success response even if the file does not exist.
	async fn delete(&mut self, path: &Path) -> Result<()> {
		self.client
			.delete_object()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.send()
			.await
			.map_err(|e| Error::new(ErrorKind::Other, e))?;

		Ok(())
	}

	async fn list_contents(&self, path: &Path, deep: bool) -> Result<Vec<std::path::PathBuf>> {
		let mut paths = Vec::new();

		let mut request = self
			.client
			.list_objects_v2()
			.bucket(&self.bucket)
			.prefix(format!(
				"{}/",
				path.to_str()
					.ok_or_else(
						|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8",)
					)?
			));

		if !deep {
			request = request.delimiter('/');
		}

		let mut response = request.into_paginator().send();

		while let Some(result) = response.next().await {
			let contents = match result {
				Ok(result) => result.contents.unwrap_or_default(),
				Err(SdkError::ServiceError(error)) => {
					if error.err().meta().code() == Some("NoSuchKey") {
						continue;
					}

					return Err(Error::new(ErrorKind::Other, error.into_err()));
				},
				Err(e) => return Err(Error::new(ErrorKind::Other, e)),
			};

			paths.extend(
				contents
					.iter()
					.filter_map(|content| content.key())
					.map(|s| PathBuf::from_str(s).unwrap()),
			);
		}

		Ok(paths)
	}

	async fn r#move(&mut self, source: &Path, destination: &Path) -> Result<()> {
		self.copy(source, destination).await?;
		self.delete(source).await?;

		Ok(())
	}

	async fn copy(&mut self, source: &Path, destination: &Path) -> Result<()> {
		let request =
			self.client
				.copy_object()
				.copy_source(format!(
					"{}/{}",
					self.bucket,
					source.to_str().ok_or_else(|| Error::new(
						ErrorKind::InvalidData,
						"path is not valid utf-8",
					))?
				))
				.bucket(&self.bucket)
				.key(
					destination.to_str().ok_or_else(|| {
						Error::new(ErrorKind::InvalidData, "path is not valid utf-8")
					})?,
				)
				.send()
				.await;

		match request {
			Ok(_) => Ok(()),
			Err(SdkError::ServiceError(error)) => {
				if error.err().meta().code() == Some("NoSuchKey") {
					return Err(Error::from(ErrorKind::NotFound));
				}

				Err(Error::new(ErrorKind::Other, error.into_err()))
			},
			Err(e) => Err(Error::new(ErrorKind::Other, e)),
		}
	}

	async fn checksum(&self, path: &Path) -> Result<String> {
		let request = self
			.client
			.head_object()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.send()
			.await;

		match request {
			Ok(request) => request
				.e_tag
				.ok_or_else(|| Error::new(ErrorKind::Other, "S3 did not return an ETag header")),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_not_found() {
					return Err(Error::from(ErrorKind::NotFound));
				}

				Err(Error::new(ErrorKind::Other, error.into_err()))
			},
			Err(e) => Err(Error::new(ErrorKind::Other, e)),
		}
	}
}

impl TemporaryUrlGenerator for S3Adapter {
	async fn temporary_url(&self, path: &Path, expires_in: Duration) -> Result<Url> {
		let request = self
			.client
			.get_object()
			.bucket(&self.bucket)
			.key(
				path.to_str()
					.ok_or_else(|| Error::new(ErrorKind::InvalidData, "path is not valid utf-8"))?,
			)
			.presigned(
				PresigningConfig::expires_in(expires_in)
					.map_err(|e| Error::new(ErrorKind::InvalidInput, e))?,
			)
			.await;

		match request {
			Ok(presigned_req) => Ok(presigned_req
				.uri()
				.parse()
				.map_err(|e| Error::new(ErrorKind::Other, e))?),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_no_such_key() {
					return Err(Error::from(ErrorKind::NotFound));
				}

				Err(Error::new(ErrorKind::Other, error.into_err()))
			},
			Err(e) => Err(Error::new(ErrorKind::Other, e)),
		}
	}
}

impl From<Visibility> for ObjectCannedAcl {
	fn from(visibility: Visibility) -> Self {
		match visibility {
			Visibility::Private => Self::Private,
			Visibility::Public => Self::PublicRead,
		}
	}
}

impl From<GetObjectAclOutput> for Visibility {
	fn from(value: GetObjectAclOutput) -> Self {
		for grant in value.grants() {
			let affects_all_users = grant
				.grantee()
				.and_then(|grantee| grantee.uri())
				.is_some_and(|uri| uri == "http://acs.amazonaws.com/groups/global/AllUsers");

			let can_read = grant.permission().is_some_and(|p| p == &Permission::Read);

			if affects_all_users && can_read {
				return Self::Public;
			}
		}

		Self::Private
	}
}

#[cfg(test)]
mod tests {
	use std::env;

	use super::*;

	async fn get_client() -> S3Adapter {
		S3Adapter::new(Config {
			bucket: env::var("S3_BUCKET").unwrap(),
			region: env::var("S3_REGION").unwrap(),
			endpoint: env::var("S3_ENDPOINT").unwrap(),
			access_key: env::var("S3_ACCESS_KEY").unwrap(),
			secret_key: env::var("S3_SECRET_KEY").unwrap(),
		})
		.await
		.unwrap()
	}

	#[tokio::test]
	async fn test_file_exists() {
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
	#[ignore] // not supported by MinIO
	async fn test_set_visibility() {
		let mut client = get_client().await;

		client
			.write(Path::new("test_set_visibility.txt"), &[])
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
			.set_visibility(Path::new("test_set_visibility.txt"), Visibility::Public)
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
			.delete(Path::new("test_set_visibility.txt"))
			.await
			.unwrap();
	}

	#[tokio::test]
	#[ignore] // not supported by MinIO
	async fn test_visibility() {
		let mut client = get_client().await;

		client
			.write(Path::new("test_visibility.txt"), &[])
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
			.set_visibility(Path::new("test_visibility.txt"), Visibility::Public)
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
			.delete(Path::new("test_visibility.txt"))
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn test_mime_type() {
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
		let mut client = get_client().await;

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
