// macOS-only: AVAudioEngine with Voice Processing for Acoustic Echo Cancellation (AEC)
//
// When speakers are playing system audio, the microphone picks up that audio
// as "echo". Without AEC, our dual-VAD system misattributes system audio
// heard through the mic as the local speaker ("Me").
//
// AVAudioEngine's Voice Processing mode uses Apple's built-in
// kAudioUnitSubType_VoiceProcessingIO to subtract the system audio signal
// from the microphone input, producing clean speech-only audio.

use anyhow::Result;
use log::info;

use cidre::av;

use super::super::pipeline::AudioCapture;

/// Voice-processed microphone capture using AVAudioEngine.
///
/// Enables Acoustic Echo Cancellation (AEC) to prevent system audio
/// played through speakers from being picked up by the microphone.
/// This is the same approach used by Granola and other meeting apps.
pub struct VoiceProcessingCapture {
    engine: cidre::arc::R<av::audio::Engine>,
}

// SAFETY: AVAudioEngine uses ObjC reference counting which is thread-safe
// for retain/release. The engine's internal audio processing runs on its
// own render thread. We only call stop() from the owning thread.
unsafe impl Send for VoiceProcessingCapture {}

impl VoiceProcessingCapture {
    /// Create and start a voice-processed microphone capture.
    ///
    /// Audio flows: Microphone -> VoiceProcessingIO (AEC) -> Tap -> AudioCapture -> Pipeline
    ///
    /// The tap requests 48kHz mono f32 to match the pipeline's expected format.
    pub fn start(capture: AudioCapture) -> Result<Self> {
        info!("Creating AVAudioEngine with Voice Processing (AEC)...");

        let mut engine = av::audio::Engine::new();

        // Get the input node (microphone) and enable voice processing
        let mut input_node = engine.input_node();

        input_node.set_vp_enabled(true)
            .map_err(|e| anyhow::anyhow!("Failed to enable voice processing: {:?}", e))?;

        // Optionally enable AGC for consistent mic levels
        input_node.set_vp_agc_enabled(true);

        info!("Voice Processing enabled - VP: {}, AGC: {}",
              input_node.is_vp_enabled(), input_node.is_vp_agc_enabled());

        // Log the native input format for diagnostics
        let native_format = input_node.output_format_for_bus(0);
        info!("Native input format: {:.0}Hz, {} ch, {:?}",
              native_format.absd().sample_rate,
              native_format.channel_count(),
              native_format.common_format());

        // Request 48kHz mono f32 interleaved for our tap
        // This matches what CPAL would provide after our mono conversion
        let tap_format = av::audio::Format::with_common_format_sample_rate_channels_interleaved(
            av::audio::CommonFormat::PcmF32,
            48000.0,
            1,    // mono
            true, // interleaved (irrelevant for mono, but explicit)
        ).ok_or_else(|| anyhow::anyhow!("Failed to create 48kHz mono f32 tap format"))?;

        // Install tap on input node output bus (bus 0)
        // Buffer size 4800 = 100ms at 48kHz, balancing latency vs overhead
        let capture_clone = capture;
        input_node.install_tap_on_bus(0, 4800, Some(&tap_format), move |buffer, _time| {
            // Extract f32 samples from the PCM buffer (channel 0)
            if let Some(data) = buffer.data_f32_at(0) {
                let frame_len = buffer.frame_len() as usize;
                // data_f32_at returns a slice of frame_len samples for mono
                if frame_len > 0 && data.len() >= frame_len {
                    capture_clone.process_audio_data(&data[..frame_len]);
                }
            }
        }).map_err(|e| anyhow::anyhow!("Failed to install audio tap: {:?}", e))?;

        // Prepare and start the engine
        engine.prepare();
        engine.start()
            .map_err(|e| anyhow::anyhow!("Failed to start AVAudioEngine: {:?}", e))?;

        info!("AVAudioEngine started with AEC - microphone capture active");

        Ok(Self { engine })
    }

    /// Stop the voice processing capture explicitly.
    pub fn stop(mut self) {
        info!("Stopping voice processing capture");
        self.engine.stop();
    }
}

impl Drop for VoiceProcessingCapture {
    fn drop(&mut self) {
        self.engine.stop();
    }
}
