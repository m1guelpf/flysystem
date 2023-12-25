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
use mime::{FromStrError, Mime};
use std::{
	path::{Path, PathBuf},
	str::FromStr,
	time::{Duration, SystemTime},
};

use super::{Adapter, TemporaryUrlGenerator};
use crate::{contents::Contents, Visibility};

#[derive(Debug, Clone, Default)]
pub struct Config {
	pub bucket: String,
	pub endpoint: String,
	pub access_key: String,
	pub secret_key: String,
	pub region: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(clippy::module_name_repetitions)]
pub struct S3Adapter {
	client: Client,
	bucket: String,
}

impl Adapter for S3Adapter {
	type Error = Error;
	type Config = Config;

	async fn new(config: Self::Config) -> Result<Self, Self::Error> {
		let cred = Credentials::new(config.access_key, config.secret_key, None, None, "custom");

		Ok(Self {
			bucket: config.bucket,
			client: Client::from_conf(
				aws_sdk_s3::Config::builder()
					.force_path_style(true)
					.credentials_provider(cred)
					.endpoint_url(config.endpoint)
					.region(Region::new(
						config.region.unwrap_or_else(|| "eu-central-1".to_string()),
					))
					.build(),
			),
		})
	}

	async fn file_exists(&self, path: &Path) -> Result<bool, Self::Error> {
		let request = self
			.client
			.head_object()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.send()
			.await;

		match request {
			Ok(_) => Ok(true),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_not_found() {
					return Ok(false);
				}

				Err(SdkError::ServiceError(error).into())
			},
			Err(e) => Err(e.into()),
		}
	}

	async fn directory_exists(&self, path: &Path) -> Result<bool, Self::Error> {
		let request = self
			.client
			.list_objects_v2()
			.bucket(&self.bucket)
			.prefix(format!("{}/", path.to_str().ok_or(Error::InvalidPath)?))
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

				dbg!(error.err().meta().code());

				Err(SdkError::ServiceError(error).into())
			},
			Err(e) => Err(e.into()),
		}
	}

	async fn write<C: AsRef<[u8]> + Send>(
		&mut self,
		path: &Path,
		content: C,
	) -> Result<(), Self::Error> {
		self.client
			.put_object()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.body(ByteStream::from(content.as_ref().to_vec()))
			.content_type(
				mime_guess::from_path(path)
					.first_or_octet_stream()
					.to_string(),
			)
			.send()
			.await?;

		Ok(())
	}

	async fn read<T: TryFrom<Contents>>(&self, path: &Path) -> Result<T, Self::Error> {
		let request = match self
			.client
			.get_object()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.send()
			.await
		{
			Ok(request) => request,
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_no_such_key() {
					return Err(Error::FileNotFound);
				}

				return Err(SdkError::ServiceError(error).into());
			},
			Err(e) => return Err(e.into()),
		};

		Contents::from_bytestream(request.body)
			.await?
			.try_into()
			.map_err(|_| Error::DecodeContents)
	}

	async fn delete_directory(&mut self, path: &Path) -> Result<(), Self::Error> {
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
			.await?;

		Ok(())
	}

	async fn create_directory(&mut self, path: &Path) -> Result<(), Self::Error> {
		self.client
			.put_object()
			.bucket(&self.bucket)
			.key(format!("{}/", path.to_str().ok_or(Error::InvalidPath)?))
			.body(ByteStream::default())
			.send()
			.await?;

		Ok(())
	}

	/// Set the visibility of a file.
	///
	/// Note that some S3 providers (like Minio) don't implement this feature.
	async fn set_visibility(
		&mut self,
		path: &Path,
		visibility: Visibility,
	) -> Result<(), Self::Error> {
		let response = self
			.client
			.put_object_acl()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.acl(visibility.into())
			.send()
			.await;

		match response {
			Ok(_) => Ok(()),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_no_such_key() {
					return Err(Error::FileNotFound);
				}

				Err(SdkError::ServiceError(error).into())
			},
			Err(e) => Err(e.into()),
		}
	}

	async fn visibility(&self, path: &Path) -> Result<Visibility, Self::Error> {
		let response = self
			.client
			.get_object_acl()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.send()
			.await;

		match response {
			Ok(response) => Ok(response.into()),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_no_such_key() {
					return Err(Error::FileNotFound);
				}

				Err(SdkError::ServiceError(error).into())
			},
			Err(e) => Err(e.into()),
		}
	}

	async fn mime_type(&self, path: &Path) -> Result<Mime, Self::Error> {
		let response = self
			.client
			.head_object()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.send()
			.await;

		match response {
			Ok(response) => Ok(response
				.content_type()
				.map(Mime::from_str)
				.ok_or(Error::FileNotFound)??),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_not_found() {
					return Err(Error::FileNotFound);
				}

				Err(SdkError::ServiceError(error).into())
			},
			Err(e) => Err(e.into()),
		}
	}

	async fn last_modified(&self, path: &Path) -> Result<SystemTime, Self::Error> {
		let response = self
			.client
			.head_object()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.send()
			.await;

		match response {
			Ok(response) => Ok(SystemTime::try_from(
				response.last_modified.ok_or(Error::LastModifiedMissing)?,
			)?),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_not_found() {
					return Err(Error::FileNotFound);
				}

				Err(SdkError::ServiceError(error).into())
			},
			Err(e) => Err(e.into()),
		}
	}

	async fn file_size(&self, path: &Path) -> Result<u64, Self::Error> {
		let response = self
			.client
			.head_object()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.send()
			.await;

		match response {
			#[allow(clippy::cast_sign_loss)]
			Ok(response) => Ok(response.content_length.ok_or(Error::FileNotFound)? as u64),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_not_found() {
					return Err(Error::FileNotFound);
				}

				Err(SdkError::ServiceError(error).into())
			},
			Err(e) => Err(e.into()),
		}
	}

	/// Delete a file from the filesystem.
	///
	/// Note that some S3 providers will return a success response even if the file does not exist.
	async fn delete(&mut self, path: &Path) -> Result<(), Self::Error> {
		self.client
			.delete_object()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.send()
			.await?;

		Ok(())
	}

	async fn list_contents(
		&self,
		path: &Path,
		deep: bool,
	) -> Result<Vec<std::path::PathBuf>, Self::Error> {
		let mut paths = Vec::new();

		let mut request = self
			.client
			.list_objects_v2()
			.bucket(&self.bucket)
			.prefix(format!("{}/", path.to_str().ok_or(Error::InvalidPath)?));

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

					return Err(SdkError::ServiceError(error).into());
				},
				Err(e) => return Err(e.into()),
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

	async fn r#move(&mut self, source: &Path, destination: &Path) -> Result<(), Self::Error> {
		self.copy(source, destination).await?;
		self.delete(source).await?;

		Ok(())
	}

	async fn copy(&mut self, source: &Path, destination: &Path) -> Result<(), Self::Error> {
		let request = self
			.client
			.copy_object()
			.copy_source(format!(
				"{}/{}",
				self.bucket,
				source.to_str().ok_or(Error::InvalidPath)?
			))
			.bucket(&self.bucket)
			.key(destination.to_str().ok_or(Error::InvalidPath)?)
			.send()
			.await;

		match request {
			Ok(_) => Ok(()),
			Err(SdkError::ServiceError(error)) => {
				if error.err().meta().code() == Some("NoSuchKey") {
					return Err(Error::FileNotFound);
				}

				Err(SdkError::ServiceError(error).into())
			},
			Err(e) => Err(e.into()),
		}
	}

	async fn checksum(&self, path: &Path) -> Result<String, Self::Error> {
		let request = self
			.client
			.head_object()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.send()
			.await;

		match request {
			Ok(request) => request.e_tag.ok_or(Error::ETagMissing),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_not_found() {
					return Err(Error::FileNotFound);
				}

				Err(SdkError::ServiceError(error).into())
			},
			Err(e) => Err(e.into()),
		}
	}
}

impl TemporaryUrlGenerator for S3Adapter {
	type Error = Error;

	async fn temporary_url(
		&self,
		path: &Path,
		expires_in: Duration,
	) -> Result<String, Self::Error> {
		let request = self
			.client
			.get_object()
			.bucket(&self.bucket)
			.key(path.to_str().ok_or(Error::InvalidPath)?)
			.presigned(PresigningConfig::expires_in(expires_in)?)
			.await;

		match request {
			Ok(presigned_req) => Ok(presigned_req.uri().to_string()),
			Err(SdkError::ServiceError(error)) => {
				if error.err().is_no_such_key() {
					return Err(Error::FileNotFound);
				}

				Err(SdkError::ServiceError(error).into())
			},
			Err(e) => Err(e.into()),
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
				.map(|uri| uri == "http://acs.amazonaws.com/groups/global/AllUsers")
				.unwrap_or_default();

			let can_read = grant
				.permission()
				.map(|p| p == &Permission::Read)
				.unwrap_or_default();

			if affects_all_users && can_read {
				return Self::Public;
			}
		}

		Self::Private
	}
}

type ApiError<T> = aws_smithy_runtime_api::client::result::SdkError<
	T,
	aws_smithy_runtime_api::client::orchestrator::HttpResponse,
>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("S3 did not return an ETag header")]
	ETagMissing,

	#[error("S3 did not return a Last-Modified header")]
	LastModifiedMissing,

	#[error("The requested file does not exist")]
	FileNotFound,

	#[error("Unable to get file details: {0}")]
	HeadObject(#[from] ApiError<aws_sdk_s3::operation::head_object::HeadObjectError>),

	#[error("Unable to list files in directory: {0}")]
	ListObjects(#[from] ApiError<aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Error>),

	#[error("Failed to parse: {0}")]
	Parse(#[from] FromStrError),

	#[error("Unable to upload file: {0}")]
	PutObject(#[from] ApiError<aws_sdk_s3::operation::put_object::PutObjectError>),

	#[error("Unable to download file: {0}")]
	GetObject(#[from] ApiError<aws_sdk_s3::operation::get_object::GetObjectError>),

	#[error("Unable to update visibility: {0}")]
	PutObjectACL(#[from] ApiError<aws_sdk_s3::operation::put_object_acl::PutObjectAclError>),

	#[error("Unable to read visibility: {0}")]
	GetObjectACL(#[from] ApiError<aws_sdk_s3::operation::get_object_acl::GetObjectAclError>),

	#[error("Unable to copy file: {0}")]
	CopyObject(#[from] ApiError<aws_sdk_s3::operation::copy_object::CopyObjectError>),

	#[error("Unable to delete file: {0}")]
	DeleteObject(#[from] ApiError<aws_sdk_s3::operation::delete_object::DeleteObjectError>),

	#[error("Unable to delete directory: {0}")]
	DeleteObjects(#[from] ApiError<aws_sdk_s3::operation::delete_objects::DeleteObjectsError>),

	#[error("The provided path contains invalid characters")]
	InvalidPath,

	#[error("Failed to load file contents: {0}")]
	AllocateBuffer(#[from] aws_smithy_types::byte_stream::error::Error),

	#[error("Failed to decode file contents")]
	DecodeContents,

	#[error("Failed to decode updated time: {0}")]
	ConversionError(#[from] aws_smithy_types::date_time::ConversionError),

	#[error("Failed to generate temporary URL: {0}")]
	PresigningError(#[from] aws_sdk_s3::presigning::PresigningConfigError),
}

#[cfg(test)]
mod tests {
	use std::env;

	use super::*;

	async fn get_client() -> S3Adapter {
		S3Adapter::new(Config {
			region: env::var("S3_REGION").ok(),
			bucket: env::var("S3_BUCKET").unwrap(),
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
			.write(Path::new("test_file_exists.txt"), "Hello, world!")
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
			.write(Path::new("test_write.txt"), "Hello, world!")
			.await
			.unwrap();

		assert_eq!(
			client
				.read::<String>(Path::new("test_write.txt"))
				.await
				.unwrap(),
			"Hello, world!"
		);

		client.delete(Path::new("test_write.txt")).await.unwrap();
	}

	#[tokio::test]
	async fn test_read() {
		let mut client = get_client().await;

		client
			.write(Path::new("test_read.txt"), "Hello, world!")
			.await
			.unwrap();

		assert_eq!(
			client
				.read::<String>(Path::new("test_read.txt"))
				.await
				.unwrap(),
			"Hello, world!"
		);

		client.delete(Path::new("test_read.txt")).await.unwrap();
	}

	#[tokio::test]
	async fn test_delete() {
		let mut client = get_client().await;

		client
			.write(Path::new("test_delete.txt"), "Hello, world!")
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
			.write(Path::new("test_set_visibility.txt"), "")
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
			.write(Path::new("test_visibility.txt"), "")
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
			.write(Path::new("test_mime.txt"), "Hello, world!")
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
			.write(Path::new("test_last_modified.txt"), "")
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
			.write(Path::new("test_file_size.txt"), "Hello, world!")
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
				"Hello, world!",
			)
			.await
			.unwrap();
		client
			.write(
				Path::new("test_list_contents/test_recursive_dir/test_file.txt"),
				"Hello, world!",
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
			.write(Path::new("test_move.txt"), "Hello, world!")
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
			.write(Path::new("test_copy.txt"), "Hello, world!")
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
			.write(Path::new("test_checksum.txt"), "Hello, world!")
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
