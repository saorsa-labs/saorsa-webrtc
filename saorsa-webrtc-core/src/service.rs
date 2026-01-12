//! WebRTC service orchestration

use crate::call::{CallManager, CallManagerConfig};
use crate::identity::PeerIdentity;
use crate::media::MediaStreamManager;
use crate::signaling::{SignalingHandler, SignalingTransport};
use crate::types::{CallEvent, CallId, CallState, MediaConstraints, NativeQuicConfiguration};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::broadcast;

/// Service errors
#[derive(Error, Debug)]
pub enum ServiceError {
    /// Initialization error
    #[error("Initialization error: {0}")]
    InitError(String),

    /// Call error
    #[error("Call error: {0}")]
    CallError(String),
}

/// Top-level WebRTC events
#[derive(Debug, Clone)]
pub enum WebRtcEvent<I: PeerIdentity> {
    /// Signaling event
    Signaling(SignalingEvent),
    /// Media event
    Media(crate::media::MediaEvent),
    /// Call event
    Call(CallEvent<I>),
}

/// Signaling event (placeholder)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignalingEvent {
    /// Connected
    Connected,
    /// Disconnected
    Disconnected,
}

/// WebRTC configuration
#[derive(Debug, Clone)]
pub struct WebRtcConfig {
    /// QUIC configuration
    pub quic_config: NativeQuicConfiguration,
    /// Default media constraints
    pub default_constraints: MediaConstraints,
    /// Call manager config
    pub call_config: CallManagerConfig,
}

impl Default for WebRtcConfig {
    fn default() -> Self {
        Self {
            quic_config: NativeQuicConfiguration::default(),
            default_constraints: MediaConstraints::audio_only(),
            call_config: CallManagerConfig::default(),
        }
    }
}

/// Main WebRTC service
pub struct WebRtcService<I: PeerIdentity, T: SignalingTransport> {
    _signaling: Arc<SignalingHandler<T>>,
    media: Arc<MediaStreamManager>,
    call_manager: Arc<CallManager<I>>,
    event_sender: broadcast::Sender<WebRtcEvent<I>>,
}

impl<I: PeerIdentity, T: SignalingTransport> WebRtcService<I, T> {
    /// Create new WebRTC service
    ///
    /// # Errors
    ///
    /// Returns error if service creation fails
    pub async fn new(
        signaling: Arc<SignalingHandler<T>>,
        config: WebRtcConfig,
    ) -> Result<Self, ServiceError> {
        let (event_sender, _) = broadcast::channel(1000);

        let media = Arc::new(MediaStreamManager::new());
        let call_manager = Arc::new(
            CallManager::new(config.call_config)
                .await
                .map_err(|e| ServiceError::InitError(e.to_string()))?,
        );

        Ok(Self {
            _signaling: signaling,
            media,
            call_manager,
            event_sender,
        })
    }

    /// Start the service
    ///
    /// # Errors
    ///
    /// Returns error if service cannot be started
    #[tracing::instrument(skip(self))]
    pub async fn start(&self) -> Result<(), ServiceError> {
        tracing::info!("Starting WebRTC service");

        self.media
            .initialize()
            .await
            .map_err(|e| ServiceError::InitError(e.to_string()))?;

        self.call_manager
            .start()
            .await
            .map_err(|e| ServiceError::InitError(e.to_string()))?;

        tracing::info!("WebRTC service started successfully");
        Ok(())
    }

    /// Initiate a call
    ///
    /// # Errors
    ///
    /// Returns error if call cannot be initiated
    #[tracing::instrument(skip(self), fields(peer = %callee.to_string_repr()))]
    pub async fn initiate_call(
        &self,
        callee: I,
        constraints: MediaConstraints,
    ) -> Result<CallId, ServiceError> {
        tracing::info!("Initiating call");

        let call_id = self
            .call_manager
            .initiate_call(callee, constraints)
            .await
            .map_err(|e| ServiceError::CallError(e.to_string()))?;

        tracing::info!(call_id = %call_id, "Call initiated successfully");
        Ok(call_id)
    }

    /// Accept a call
    ///
    /// # Errors
    ///
    /// Returns error if call cannot be accepted
    #[tracing::instrument(skip(self), fields(call_id = %call_id))]
    pub async fn accept_call(
        &self,
        call_id: CallId,
        constraints: MediaConstraints,
    ) -> Result<(), ServiceError> {
        tracing::info!("Accepting call");

        self.call_manager
            .accept_call(call_id, constraints)
            .await
            .map_err(|e| ServiceError::CallError(e.to_string()))?;

        tracing::info!("Call accepted");
        Ok(())
    }

    /// Reject a call
    ///
    /// # Errors
    ///
    /// Returns error if call cannot be rejected
    #[tracing::instrument(skip(self), fields(call_id = %call_id))]
    pub async fn reject_call(&self, call_id: CallId) -> Result<(), ServiceError> {
        tracing::info!("Rejecting call");

        self.call_manager
            .reject_call(call_id)
            .await
            .map_err(|e| ServiceError::CallError(e.to_string()))?;

        tracing::info!("Call rejected");
        Ok(())
    }

    /// End a call
    ///
    /// # Errors
    ///
    /// Returns error if call cannot be ended
    #[tracing::instrument(skip(self), fields(call_id = %call_id))]
    pub async fn end_call(&self, call_id: CallId) -> Result<(), ServiceError> {
        tracing::info!("Ending call");

        self.call_manager
            .end_call(call_id)
            .await
            .map_err(|e| ServiceError::CallError(e.to_string()))?;

        tracing::info!("Call ended");
        Ok(())
    }

    /// Get call state
    #[must_use]
    pub async fn get_call_state(&self, call_id: CallId) -> Option<CallState> {
        self.call_manager.get_call_state(call_id).await
    }

    /// Subscribe to events
    #[must_use]
    pub fn subscribe_events(&self) -> broadcast::Receiver<WebRtcEvent<I>> {
        self.event_sender.subscribe()
    }

    /// Create a builder
    #[must_use]
    pub fn builder(signaling: Arc<SignalingHandler<T>>) -> WebRtcServiceBuilder<I, T> {
        WebRtcServiceBuilder::new(signaling)
    }
}

/// WebRTC service builder
pub struct WebRtcServiceBuilder<I: PeerIdentity, T: SignalingTransport> {
    signaling: Arc<SignalingHandler<T>>,
    config: WebRtcConfig,
    _phantom: std::marker::PhantomData<I>,
}

impl<I: PeerIdentity, T: SignalingTransport> WebRtcServiceBuilder<I, T> {
    /// Create new builder
    #[must_use]
    pub fn new(signaling: Arc<SignalingHandler<T>>) -> Self {
        Self {
            signaling,
            config: WebRtcConfig::default(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set configuration
    #[must_use]
    pub fn with_config(mut self, config: WebRtcConfig) -> Self {
        self.config = config;
        self
    }

    /// Build the service
    ///
    /// # Errors
    ///
    /// Returns error if service creation fails
    pub async fn build(self) -> Result<WebRtcService<I, T>, ServiceError> {
        WebRtcService::new(self.signaling, self.config).await
    }
}
