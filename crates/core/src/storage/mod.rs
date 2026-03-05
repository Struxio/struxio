use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::Client;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("S3 error: {0}")]
    S3(String),
    #[error("Presign error: {0}")]
    Presign(String),
}

#[derive(Clone)]
pub struct StorageClient {
    pub client: Client,
    pub bucket: String,
}

impl StorageClient {
    pub fn new(client: Client, bucket: String) -> Self {
        Self { client, bucket }
    }

    pub async fn generate_presigned_upload_url(
        &self,
        key: &str,
        content_type: &str,
        expires_in_secs: u64,
    ) -> Result<String, StorageError> {
        let presigning_config = PresigningConfig::expires_in(Duration::from_secs(expires_in_secs))
            .map_err(|e| StorageError::Presign(e.to_string()))?;

        let presigned = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .presigned(presigning_config)
            .await
            .map_err(|e| StorageError::Presign(e.to_string()))?;

        Ok(presigned.uri().to_string())
    }

    pub async fn upload(
        &self,
        key: &str,
        bytes: &[u8],
        content_type: &str,
    ) -> Result<(), StorageError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(aws_sdk_s3::primitives::ByteStream::from(bytes.to_vec()))
            .send()
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?;

        Ok(())
    }

    pub async fn download(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?;

        let body = response
            .body
            .collect()
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?
            .into_bytes()
            .to_vec();

        Ok(body)
    }

    pub async fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?;

        Ok(())
    }
}
