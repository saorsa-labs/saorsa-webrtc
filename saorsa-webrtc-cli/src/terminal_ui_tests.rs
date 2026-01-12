//! Tests for terminal UI

#[cfg(test)]
mod tests {
    use super::super::terminal_ui::*;

    #[test]
    fn test_connection_stats_default() {
        let stats = ConnectionStats::default();
        assert!(stats.rtt_ms.is_none());
        assert!(stats.bitrate_kbps.is_none());
        assert!(stats.fps.is_none());
        assert!(stats.packets_lost.is_none());
        assert!(stats.packets_sent.is_none());
    }

    #[test]
    fn test_connection_stats_with_values() {
        let stats = ConnectionStats {
            rtt_ms: Some(25),
            bitrate_kbps: Some(1500),
            fps: Some(30),
            packets_lost: Some(10),
            packets_sent: Some(1000),
        };

        assert_eq!(stats.rtt_ms, Some(25));
        assert_eq!(stats.bitrate_kbps, Some(1500));
        assert_eq!(stats.fps, Some(30));
        assert_eq!(stats.packets_lost, Some(10));
        assert_eq!(stats.packets_sent, Some(1000));
    }

    #[test]
    fn test_display_mode_conversions() {
        let sixel = CliDisplayMode::Sixel;
        let display: DisplayMode = sixel.into();
        assert!(matches!(display, DisplayMode::Sixel));

        let ascii = CliDisplayMode::Ascii;
        let display: DisplayMode = ascii.into();
        assert!(matches!(display, DisplayMode::Ascii));

        let none = CliDisplayMode::None;
        let display: DisplayMode = none.into();
        assert!(matches!(display, DisplayMode::None));
    }

    // Integration test that terminal UI can be created and dropped
    // Note: This test won't run in CI without a TTY, but validates the structure
    #[test]
    #[ignore] // Ignore by default as it requires terminal
    fn test_terminal_ui_lifecycle() {
        // This would need a real terminal, so we just verify the types compile
        let _ = DisplayMode::Sixel;
        let _ = DisplayMode::Ascii;
        let _ = DisplayMode::None;
    }
}
