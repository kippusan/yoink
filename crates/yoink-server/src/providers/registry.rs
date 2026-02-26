use std::sync::Arc;

use super::{DownloadSource, MetadataProvider, ProviderArtist, ProviderError};
use super::tidal::TidalProvider;

/// Central registry that holds all enabled providers and dispatches operations.
pub(crate) struct ProviderRegistry {
    metadata: Vec<Arc<dyn MetadataProvider>>,
    download: Vec<Arc<dyn DownloadSource>>,
    /// Concrete reference to the Tidal provider for Tidal-specific endpoints (e.g. instances).
    tidal: Option<Arc<TidalProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            metadata: Vec::new(),
            download: Vec::new(),
            tidal: None,
        }
    }

    /// Store a concrete reference to the Tidal provider for Tidal-specific endpoints.
    pub fn set_tidal(&mut self, tidal: Arc<TidalProvider>) {
        self.tidal = Some(tidal);
    }

    /// Get the concrete Tidal provider (for the /api/tidal/instances endpoint).
    pub fn tidal_provider(&self) -> Option<&TidalProvider> {
        self.tidal.as_deref()
    }

    /// Register a provider that implements MetadataProvider.
    pub fn register_metadata(&mut self, provider: Arc<dyn MetadataProvider>) {
        self.metadata.push(provider);
    }

    /// Register a provider that implements DownloadSource.
    pub fn register_download(&mut self, source: Arc<dyn DownloadSource>) {
        self.download.push(source);
    }

    /// Fan-out search to all metadata providers concurrently.
    /// Returns a list of (provider_id, results).
    pub async fn search_artists_all(
        &self,
        query: &str,
    ) -> Vec<(String, Vec<ProviderArtist>)> {
        let mut handles = Vec::new();

        for provider in &self.metadata {
            let p = Arc::clone(provider);
            let q = query.to_string();
            handles.push(tokio::spawn(async move {
                let id = p.id().to_string();
                match p.search_artists(&q).await {
                    Ok(artists) => (id, artists),
                    Err(_) => (id, Vec::new()),
                }
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }
        results
    }

    /// Get a specific metadata provider by ID.
    pub fn metadata_provider(&self, id: &str) -> Option<Arc<dyn MetadataProvider>> {
        self.metadata.iter().find(|p| p.id() == id).cloned()
    }

    /// Get a specific download source by ID.
    pub fn download_source(&self, id: &str) -> Option<Arc<dyn DownloadSource>> {
        self.download.iter().find(|s| s.id() == id).cloned()
    }

    /// List all enabled metadata provider IDs.
    #[allow(dead_code)]
    pub fn metadata_provider_ids(&self) -> Vec<String> {
        self.metadata.iter().map(|p| p.id().to_string()).collect()
    }

    /// List all enabled download source IDs.
    pub fn download_source_ids(&self) -> Vec<String> {
        self.download.iter().map(|s| s.id().to_string()).collect()
    }

    /// Search artists using a specific metadata provider.
    #[allow(dead_code)]
    pub async fn search_artists(
        &self,
        provider_id: &str,
        query: &str,
    ) -> Result<Vec<ProviderArtist>, ProviderError> {
        let provider = self
            .metadata_provider(provider_id)
            .ok_or_else(|| ProviderError(format!("Unknown metadata provider: {provider_id}")))?;
        provider.search_artists(query).await
    }
}
