//! Saorsa WebRTC CLI Application

use anyhow::Result;
use clap::{Parser, Subcommand};
use rand::Rng;
use saorsa_webrtc_core::prelude::*;
use std::sync::Arc;
use terminal_ui::{CliDisplayMode, TerminalUI};

mod terminal_ui;
#[cfg(test)]
mod terminal_ui_tests;

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    /// Four-word identity (e.g., "alice-bob-charlie-david")
    #[arg(short, long, env = "SAORSA_IDENTITY")]
    identity: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initiate a call
    Call {
        /// Peer to call (four-word address)
        peer: String,

        /// Enable video
        #[arg(long, default_value = "true")]
        video: bool,

        /// Enable audio
        #[arg(long, default_value = "true")]
        audio: bool,

        /// Video display mode
        #[arg(long, value_enum, default_value = "sixel")]
        display: CliDisplayMode,
    },

    /// Start in receive mode
    Listen {
        /// Auto-accept incoming calls
        #[arg(long)]
        auto_accept: bool,

        /// Video display mode for accepted calls
        #[arg(long, value_enum, default_value = "sixel")]
        display: CliDisplayMode,
    },

    /// Show status and available commands
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for debugging
    tracing_subscriber::fmt()
        .with_env_filter("saorsa=info")
        .init();

    let cli = Cli::parse();

    // Get or generate identity
    let identity = cli.identity.unwrap_or_else(generate_random_identity);

    println!("🔗 Using identity: {}", identity);

    match cli.command {
        Commands::Call {
            peer,
            video,
            audio,
            display,
        } => {
            handle_call(&identity, &peer, video, audio, display).await?;
        }
        Commands::Listen {
            auto_accept,
            display,
        } => {
            handle_listen(&identity, auto_accept, display).await?;
        }
        Commands::Status => {
            handle_status().await?;
        }
    }

    Ok(())
}

async fn handle_call(
    _identity: &str,
    peer: &str,
    video: bool,
    audio: bool,
    display: CliDisplayMode,
) -> Result<()> {
    println!("📞 Calling {}...", peer);
    println!(
        "   Video: {} | Audio: {} | Display: {:?}",
        video, audio, display
    );

    // Create transport configuration
    let transport_config = TransportConfig::default();

    // Create transport
    let transport = Arc::new(AntQuicTransport::new(transport_config));

    // Create signaling (simplified - would need actual DHT implementation)
    let signaling = Arc::new(SignalingHandler::new(transport.clone()));

    // Create WebRTC service
    let service = Arc::new(WebRtcService::builder(signaling).build().await?);

    // Start the service
    service.start().await?;
    println!("✅ WebRTC service started");

    // Set up media constraints
    let constraints = MediaConstraints {
        audio,
        video,
        screen_share: false,
    };

    // Initiate call
    let peer_identity = PeerIdentityString::new(peer);
    let call_id = service.initiate_call(peer_identity, constraints).await?;
    println!("📞 Call initiated with ID: {}", call_id);

    // Start terminal UI
    let mut ui = TerminalUI::new(display.into())?;
    ui.run(Arc::clone(&service), call_id).await?;

    println!("📞 Call ended");
    Ok(())
}

async fn handle_listen(_identity: &str, auto_accept: bool, display: CliDisplayMode) -> Result<()> {
    println!("👂 Listening for incoming calls...");
    if auto_accept {
        println!("   Auto-accept: enabled");
    }
    println!("   Display mode: {:?}", display);

    // Create transport configuration
    let transport_config = TransportConfig::default();

    // Create transport
    let transport = Arc::new(AntQuicTransport::new(transport_config));

    // Create signaling
    let signaling = Arc::new(SignalingHandler::new(transport.clone()));

    // Create WebRTC service
    let service = Arc::new(WebRtcService::builder(signaling).build().await?);

    // Start the service
    service.start().await?;
    println!("✅ WebRTC service started");

    // Subscribe to events
    let mut events = service.subscribe_events();

    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(WebRtcEvent::Call(CallEvent::IncomingCall { offer })) => {
                        println!("📞 Incoming call from {}", offer.caller);
                        println!("   Video: {} | Audio: {}",
                            offer.media_types.contains(&saorsa_webrtc_core::types::MediaType::Video),
                            offer.media_types.contains(&saorsa_webrtc_core::types::MediaType::Audio)
                        );

                        let should_accept = if auto_accept {
                            true
                        } else {
                            // TODO: Prompt user for acceptance
                            println!("   Press 'y' to accept, 'n' to reject");
                            // For now, auto-accept in listen mode
                            true
                        };

                        if should_accept {
                            println!("✅ Accepting call...");
                            // Convert media types back to constraints
                            let constraints = MediaConstraints {
                                audio: offer.media_types.contains(&saorsa_webrtc_core::types::MediaType::Audio),
                                video: offer.media_types.contains(&saorsa_webrtc_core::types::MediaType::Video),
                                screen_share: offer.media_types.contains(&saorsa_webrtc_core::types::MediaType::ScreenShare),
                            };
                            service.accept_call(offer.call_id, constraints).await?;

                            // Start terminal UI
                            let mut ui = TerminalUI::new(display.into())?;
                            ui.run(Arc::clone(&service), offer.call_id).await?;
                        } else {
                            println!("❌ Rejecting call...");
                            service.reject_call(offer.call_id).await?;
                        }
                    }
                    Ok(other) => {
                        tracing::debug!("Received event: {:?}", other);
                    }
                    Err(e) => {
                        tracing::error!("Event stream error: {}", e);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_status() -> Result<()> {
    println!("📊 Saorsa WebRTC CLI Status");
    println!("==========================");
    println!("✅ CLI interface: Ready");
    println!("✅ Terminal UI: Available");
    println!("✅ Video codecs: Stub implementation");
    println!("⚠️  Signaling: Needs DHT implementation");
    println!("⚠️  Real codecs: Needs OpenH264 integration");
    println!();
    println!("Available commands:");
    println!("  saorsa call <peer> [options]  - Initiate a call");
    println!("  saorsa listen [options]       - Listen for calls");
    println!("  saorsa status                 - Show this status");
    println!();
    println!("Use 'saorsa --help' for detailed options");

    Ok(())
}

fn generate_random_identity() -> String {
    const WORDS: &[&str] = &[
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliet", "kilo", "lima", "mike", "november", "oscar", "papa", "quebec", "romeo", "sierra",
        "tango", "uniform", "victor", "whiskey", "xray", "yankee", "zulu", "atlas", "beacon",
        "comet", "dragon", "eagle", "falcon", "galaxy", "harbor", "icarus", "jupiter", "knight",
        "lunar", "meteor", "nebula", "orbit", "phoenix", "quasar", "rocket", "stellar", "titan",
        "universe", "vortex",
    ];

    let mut rng = rand::thread_rng();
    let indices: Vec<usize> = (0..4).map(|_| rng.gen_range(0..WORDS.len())).collect();

    format!(
        "{}-{}-{}-{}",
        WORDS[indices[0]], WORDS[indices[1]], WORDS[indices[2]], WORDS[indices[3]]
    )
}
